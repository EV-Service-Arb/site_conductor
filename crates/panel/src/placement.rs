//! A study's pull, sold per Places search at the admin's price of [`PlacementRequest::Search`].
//!
//! ```text
//! pull(user): the person's turn held, price set — budget = balance ÷ price
//!   ─ placement buys no more than the budget ─ paid(bought): bought × price in the ledger, the turn let go
//! ```

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::prices::{Item, PlacementRequest};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	store::tokens::{self as ledger, Charged, Entry, Kind},
	tokens::is_open,
};

/// A pull held for its person: no other buy of theirs on the shared keys passes on this balance
/// until it is paid.
pub struct Pull {
	panel: Panel,
	user: Uuid,
	price: i64,
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

	/// `bought` searches of study `sha`, no more than [`Self::budget`], charged.
	pub async fn paid(self, bought: u64, sha: &str) -> eyre::Result<()> {
		let budget = self.budget();
		eyre::ensure!(bought <= budget, "placement bought {bought} searches of {sha} on a budget of {budget}");
		let Self { panel, user, price, turn, .. } = self;
		let tokens = price * i64::try_from(bought).wrap_err("searches bought")?;
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
				label: Some(format!("Study pull · {bought} searches")),
			}),
			by: None,
			note: None,
		};
		let mut tx = panel.store.begin_write().await?;
		ledger::append(&mut tx, user, &entry).await?;
		tx.commit().await.wrap_err("committing a pull's charge")?;
		drop(turn);
		tracing::info!(user_id = %user, tokens, bought, "a study's pull charged");
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
		let balance = ledger::renewed(&mut tx, user, Timestamp::now(), &self.renewal).await?;
		tx.commit().await.wrap_err("committing a renewal")?;
		Ok(Ok(Pull {
			panel: self.clone(),
			user,
			price,
			balance,
			turn,
		}))
	}
}
