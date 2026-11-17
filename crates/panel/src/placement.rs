//! A study's pull, sold per Places search at the admin's price of [`PlacementRequest::Search`], and
//! per Ads request and DataForSEO task at theirs.
//!
//! ```text
//! pull(user): the person's turn held, every item priced — budget = balance ÷ a search's price
//!   ─ placement buys no more searches than the budget ─ paid(billed, failed): each item × its price
//!   in the ledger (the Ads and DataForSEO ones, known only after, up to the balance), owed back
//!   whole when the pull failed, as a failed walk's is; the turn let go
//! ```

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	placement::Billed,
	prices::{AdsRequest, DataForSeoTask, Item, PlacementRequest, PlacesRequest, Tool},
};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	store::tokens::{self as ledger, Charged, Entry, Kind, Owed},
	tokens::is_open,
};

/// A pull held for its person: no other buy of theirs on the shared keys passes on this balance
/// until it is paid.
pub struct Pull {
	panel: Panel,
	user: Uuid,
	/// A search's.
	price: i64,
	/// The Ads requests' and DataForSEO tasks'.
	after: Vec<(Item, i64)>,
	balance: i64,
	turn: tokio::sync::OwnedMutexGuard<()>,
}
impl Pull {
	/// The searches the balance affords.
	pub fn budget(&self) -> u64 {
		match self.price {
			0 => u64::MAX,
			price => u64::try_from(self.balance / price).unwrap_or(0), // a balance below zero affords none
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

	/// What study `sha`'s pull bought, its searches no more than [`Self::budget`], charged; a pull
	/// that `failed` is the house's, so all of it is owed back.
	pub async fn paid(self, billed: &Billed, sha: &str, failed: bool) -> eyre::Result<()> {
		let budget = self.budget();
		let search = Item::from(PlacesRequest::Search);
		let bought = billed.get(&search).copied().unwrap_or(0); // a pull that bought no search names none
		eyre::ensure!(bought <= budget, "placement bought {bought} searches of {sha} on a budget of {budget}");
		let Self {
			panel,
			user,
			price,
			after,
			balance,
			turn,
		} = self;
		if let Some(other) = billed.keys().find(|i| **i != search && !after.iter().any(|(a, _)| a == *i)) {
			eyre::bail!("placement billed {other} for {sha}: no pull buys it");
		}
		let searches = price * i64::try_from(bought).wrap_err("searches bought")?;
		let rest = after
			.iter()
			.map(|(item, p)| p * i64::try_from(billed.get(item).copied().unwrap_or(0)).expect("a count of requests"))
			.sum::<i64>(); // an item not billed was not bought
		// known only once the pull is done: what they cost past the balance is let go
		let tokens = searches + rest.min((balance - searches).max(0));
		if tokens == 0 {
			drop(turn);
			return Ok(());
		}
		let entry = Entry {
			at: Timestamp::now(),
			delta: -tokens,
			kind: Kind::Charge,
			charged: Some(Charged {
				service: Item::from(PlacementRequest::Search).tool().to_owned(),
				key: sha.to_owned(),
				label: Some(format!(
					"Study pull · {}",
					billed.iter().map(|(i, n)| format!("{n} × {}: {}", i.tool_title(), i.title())).collect::<Vec<_>>().join(", ")
				)),
			}),
			by: None,
			note: None,
		};
		let mut tx = panel.store.begin_write().await?;
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

impl Panel {
	pub async fn pull(&self, user: Uuid) -> eyre::Result<Result<Pull, Refused>> {
		let turn = self.purses.lock().expect("never poisoned: no panic while held").entry(user).or_default().clone();
		let turn = turn.lock_owned().await;
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(price) = ledger::price(&mut tx, PlacementRequest::Search.into()).await? else {
			return Ok(Err(Refused::Conflict("a study's pull is not priced yet: ask an admin to price it".to_owned())));
		};
		let mut after = Vec::new();
		for item in <(AdsRequest, DataForSeoTask)>::items() {
			let Some(p) = ledger::price(&mut tx, item).await? else {
				return Ok(Err(Refused::Conflict(format!(
					"{} ({}) is not priced yet: ask an admin to price it",
					item.title(),
					item.tool_title()
				))));
			};
			after.push((item, p));
		}
		let balance = ledger::renewed(&mut tx, user, Timestamp::now(), &self.renewal).await?;
		tx.commit().await.wrap_err("committing a renewal")?;
		Ok(Ok(Pull {
			panel: self.clone(),
			user,
			price,
			after,
			balance,
			turn,
		}))
	}
}
