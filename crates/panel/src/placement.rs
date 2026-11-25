//! A study's pull, sold per Places search at the admin's price of [`PlacementRequest::Search`].
//!
//! ```text
//! pull(user): the person's turn held, the search priced — budget = balance ÷ its price
//!   ─ since(study), where everything is asked again: when its first pull since its last whole one began
//!   ─ buy(ask, since) for each search placement says the study lacks, on the house's rotation,
//!     refused past the budget ─ paid(study, failed): the searches bought × the price in the ledger,
//!   owed back whole when the pull failed, as a failed walk's is; a whole one forgets its `since`; the
//!   turn let go
//! ```

use std::sync::{
	Arc,
	atomic::{AtomicU64, Ordering},
};

use eyre::WrapErr;
use gmaps_optimal_placement_core::Ask;
use jiff::Timestamp;
use sa_core::prices::{Item, PlacementRequest, PlacesRequest};
use sa_places::{Bought, Hold, Pending, Purse};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	store::{
		self,
		tokens::{self as ledger, Charged, Entry, Kind, Owed},
	},
	tokens::is_open,
};

/// A pull held for its person: no other buy of theirs on the shared keys passes on this balance
/// until it is paid.
pub struct Pull {
	panel: Panel,
	user: Uuid,
	/// A search's.
	price: i64,
	/// A search's cost to us, in millionths of a dollar.
	cost: i64,
	balance: i64,
	/// Searches bought on the rotation so far.
	bought: Arc<AtomicU64>,
	/// Everything is asked again: [`Self::since`] is this pull's to forget.
	every: bool,
	turn: tokio::sync::OwnedMutexGuard<()>,
}
impl Pull {
	/// The searches the balance affords.
	pub fn budget(&self) -> u64 {
		match self.price {
			0 => u64::MAX,
			price => u64::try_from(self.balance.div_euclid(price)).unwrap_or(0), // a balance below zero affords none
		}
	}

	/// What a person is told when the sweep needs more than [`Self::budget`].
	pub fn unaffordable(&self, need: u64) -> String {
		format!(
			"this pull needs at least {} tokens ({need} searches at {}), and you hold {}",
			need.saturating_mul(self.price.unsigned_abs()),
			self.price,
			self.balance
		)
	}

	/// When the first pull of `study` since its last whole one began: what is fetched before it is
	/// bought again, and what is fetched since is what a pull that stopped part-way already paid for.
	pub async fn since(&mut self, study: &str) -> eyre::Result<Timestamp> {
		self.every = true;
		let mut tx = self.panel.store.begin_write().await?;
		let since = store::placement::began(&mut tx, study, Timestamp::now()).await?;
		tx.commit().await.wrap_err("committing a pull's beginning")?;
		Ok(since)
	}

	/// `ask` bought on the caller's own Maps keys, then on the house's rotation, unless the queries
	/// cache holds an answer fetched since `since`; refused [`sa_places::Error::Unpaid`] past
	/// [`Self::budget`] of the rotation's.
	pub async fn buy(&self, ask: &Ask, since: Timestamp) -> Result<(), sa_places::Error> {
		let payer = self.panel.payer(self.user, None, Some(self)).await.map_err(|e| sa_places::Error::Purse(format!("{e:#}")))?;
		self.panel.google.asked(&ask.mask, &ask.body, since, &payer).await
	}

	/// What study `sha`'s pull bought, charged; a pull that `failed` is the house's, so all of it is
	/// owed back, and one that did not ended whole.
	pub async fn paid(self, sha: &str, failed: bool) -> eyre::Result<()> {
		let bought = self.bought.load(Ordering::SeqCst);
		let budget = self.budget();
		let Self {
			panel,
			user,
			price,
			cost,
			every,
			turn,
			..
		} = self;
		assert!(bought <= budget, "the purse holds no search past the budget");
		let bought = i64::try_from(bought).wrap_err("searches bought")?;
		let tokens = price * bought;
		let mut tx = panel.store.begin_write().await?;
		if every && !failed {
			store::placement::ended(&mut tx, sha).await?;
		}
		if tokens == 0 {
			tx.commit().await.wrap_err("committing a pull's end")?;
			drop(turn);
			return Ok(());
		}
		let search = Item::from(PlacesRequest::Search);
		let entry = Entry {
			at: Timestamp::now(),
			delta: -tokens,
			kind: Kind::Charge,
			charged: Some(Charged {
				service: Item::from(PlacementRequest::Search).tool().to_owned(),
				key: sha.to_owned(),
				label: Some(format!("Study pull · {bought} × {}: {}", search.tool_title(), search.title())),
			}),
			cost: Some(cost * bought),
			by: None,
			note: None,
		};
		ledger::append(&mut tx, user, &entry).await?;
		if failed {
			let charged = entry.charged.expect("built with its charge above");
			ledger::owe(&mut tx, user, entry.at, tokens, &Owed::Charge(charged)).await?;
		}
		tx.commit().await.wrap_err("committing a pull's charge")?;
		drop(turn);
		tracing::info!(user_id = %user, tokens, bought, failed, "a study's pull charged");
		panel.snapshot_due.notify_one();
		Ok(())
	}
}

/// The pull already holds the person's turn, and charges once, when paid.
impl Purse for Pull {
	fn hold(&self, sku: PlacesRequest) -> Pending<'_, Box<dyn Hold>> {
		assert_eq!(sku, PlacesRequest::Search, "a pull buys searches only");
		let budget = self.budget();
		let held: Result<Box<dyn Hold>, _> = match self.bought.load(Ordering::SeqCst) < budget {
			true => Ok(Box::new(Counted(self.bought.clone()))),
			false => Err(sa_places::Error::Unpaid(self.unaffordable(budget + 1))),
		};
		Box::pin(std::future::ready(held))
	}
}

struct Counted(Arc<AtomicU64>);

impl Hold for Counted {
	fn paid(self: Box<Self>, _: Bought) -> Pending<'static, ()> {
		self.0.fetch_add(1, Ordering::SeqCst);
		Box::pin(std::future::ready(Ok(())))
	}
}

impl Panel {
	pub async fn pull(&self, user: Uuid) -> eyre::Result<Result<Pull, Refused>> {
		let turn = self.purses.lock().expect("never poisoned: no panic while held").entry(user).or_default().clone();
		let turn = turn.lock_owned().await;
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let rate = ledger::rate(&mut tx, PlacementRequest::Search.into()).await?;
		let balance = ledger::renewed(&mut tx, user, Timestamp::now(), &self.renewal).await?;
		tx.commit().await.wrap_err("committing a renewal")?;
		Ok(Ok(Pull {
			panel: self.clone(),
			user,
			price: rate.price(),
			cost: rate.cost,
			balance,
			bought: Arc::default(),
			every: false,
			turn,
		}))
	}
}
