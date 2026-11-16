//! What our tools spend on vendors ([`sa_core::prices::Item`]): done here, told by the queries
//! cache's buyers, or read from each service's usage feed.
//!
//! ```text
//! a Google answer bought ─ kept and counted in one write (queries cache)
//! a map built in a browser, a panorama opened ─ counted as it is
//! GET <service> usage feed?after=<cursor> ─ a page ─ one write: its rows, the cursor moved
//! ```
//!
//! **A use is counted before the cursor passes it**, and a page is taken only from where its
//! service's cursor stands.

use std::collections::{BTreeMap, BTreeSet};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	prices::{Item, MapLoad, ModelTokens, Spender},
	usage::{Bill, UsagePage},
};
use strum::VariantArray;
use uuid::Uuid;

pub use crate::store::usage::{Cost, Day};
use crate::{
	Panel, Refused,
	store::{
		tokens::{self as ledger, Charged, Entry, Kind},
		usage::{self as db, CONDUCTOR, Use},
	},
	tokens::is_open,
};

/// The ledger's name for what the panel's Claude charges.
const CLAUDE: &str = "claude";

/// What an answer on our Claude has run up for its asker.
#[derive(Debug)]
pub struct Tab {
	/// The answer's turn id: its charges' key.
	answer: String,
	prices: BTreeMap<ModelTokens, i64>,
	/// Tokens × price, in [`Item::per`]ths of a token.
	owed: i64,
	/// `owed` rounded up, to tokens: what was charged, and let go past a spent balance.
	billed: i64,
	pub charged: i64,
}

fn spent(balance: i64) -> String {
	format!("you hold {balance} tokens: connect your own Claude and answers cost nothing here, or ask for tokens")
}

impl Panel {
	/// The user's browser built a map on `account`'s browser key.
	pub async fn map_loaded(&self, user: Uuid, account: &str, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if self.accounts.get(account).is_none_or(|a| a.provides.browser.is_none()) {
			return Ok(Err(Refused::Invalid(format!("{account:?} is no Google account with a browser key"))));
		}
		let mut tx = self.store.begin_write().await?;
		let used = Use {
			at: now,
			item: MapLoad::Map.into(),
			units: 1,
			bill: Bill::Ours,
			account: Some(account.to_owned()),
			source: CONDUCTOR.to_owned(),
			user: Some(user),
			key: None,
		};
		db::record(&mut tx, &used).await?;
		tx.commit().await.wrap_err("committing a map load")?;
		Ok(Ok(()))
	}

	/// The tab `by`'s answer `answer` runs up on our Claude, at the prices in effect now; refused
	/// while one of its items is unpriced or their balance is spent.
	pub async fn model_tab(&self, by: Spender<ModelTokens>, answer: &str, now: Timestamp) -> eyre::Result<Result<Tab, String>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed.to_string()));
		}
		let mut prices = BTreeMap::new();
		for &tokens in ModelTokens::VARIANTS {
			let Some(price) = ledger::price(&mut tx, tokens.into()).await? else {
				return Ok(Err("the panel's Claude is not priced yet: connect your own Claude, or ask an admin to price it".to_owned()));
			};
			prices.insert(tokens, price);
		}
		let balance = ledger::renewed(&mut tx, by.user(), now, &self.renewal).await?;
		tx.commit().await.wrap_err("committing a renewal")?;
		if balance <= 0 {
			return Ok(Err(spent(balance)));
		}
		Ok(Ok(Tab {
			answer: answer.to_owned(),
			prices,
			owed: 0,
			billed: 0,
			charged: 0,
		}))
	}

	/// The assistant's `model` answered `by`, reading and writing `used`: on our Claude (`tab`),
	/// charged in the same write. Why no further round runs on ours, once their balance is spent.
	pub async fn model_used(&self, by: Spender<ModelTokens>, model: &str, used: &[(ModelTokens, i64)], tab: Option<&mut Tab>, now: Timestamp) -> eyre::Result<Option<String>> {
		let mut tx = self.store.begin_write().await?;
		for &(tokens, units) in used.iter().filter(|(_, n)| *n > 0) {
			let used = Use {
				at: now,
				item: tokens.into(),
				units,
				bill: if tab.is_some() { Bill::Ours } else { Bill::Theirs },
				account: None,
				source: CONDUCTOR.to_owned(),
				user: Some(by.user()),
				key: Some(model.to_owned()),
			};
			db::record(&mut tx, &used).await?;
		}
		let Some(tab) = tab else {
			tx.commit().await.wrap_err("committing a model's tokens")?;
			return Ok(None);
		};
		tab.owed += used.iter().map(|(tokens, units)| tab.prices[tokens] * units).sum::<i64>();
		let per = Item::from(ModelTokens::Input).per();
		let due = (tab.owed + per - 1) / per - tab.billed;
		let balance = ledger::renewed(&mut tx, by.user(), now, &self.renewal).await?;
		// a round's cost is known only once it is done: what it costs past the balance is let go
		let charge = due.min(balance.max(0));
		if charge > 0 {
			let entry = Entry {
				at: now,
				delta: -charge,
				kind: Kind::Charge,
				charged: Some(Charged {
					service: CLAUDE.to_owned(),
					key: tab.answer.clone(),
					label: Some("The assistant's answer".to_owned()),
				}),
				by: None,
				note: None,
			};
			ledger::append(&mut tx, by.user(), &entry).await?;
		}
		tx.commit().await.wrap_err("committing a model's tokens and their charge")?;
		tab.billed += due;
		tab.charged += charge;
		if charge > 0 {
			tracing::info!(user_id = %by.user(), tokens = charge, "the assistant's answer charged");
			self.snapshot_due.notify_one();
		}
		Ok((balance - charge <= 0).then(|| spent(balance - charge)))
	}

	/// Where `service`'s usage feed is read up to.
	pub async fn usage_after(&self, service: &str) -> eyre::Result<i64> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a usage cursor")?;
		db::cursor(&mut conn, service).await
	}

	/// A page of `service`'s usage feed, asked for after `after`, counted and the cursor moved in one
	/// write; a page that does not start at the cursor, or names an item another service does, fails
	/// whole. How many uses were taken.
	pub async fn take_usage(&self, service: &str, items: &BTreeSet<Item>, page: UsagePage, after: i64) -> eyre::Result<usize> {
		let last = page.used.last().map_or(after, |u| u.id);
		eyre::ensure!(page.next == last, "{service} answered next {} for a page ending at {last}", page.next);
		let mut prev = after;
		for u in &page.used {
			eyre::ensure!(u.id > prev, "{service}'s use {} after {prev}: ids ascend past the cursor", u.id);
			eyre::ensure!(u.units > 0, "{service}'s use {} of {} units", u.id, u.units);
			eyre::ensure!(items.contains(&u.item), "{service}'s use {} is of {}, not its", u.id, u.item);
			prev = u.id;
		}
		if page.used.is_empty() {
			return Ok(0);
		}
		let mut tx = self.store.begin_write().await?;
		for u in &page.used {
			let used = Use {
				at: u.at,
				item: u.item,
				units: u.units,
				bill: Bill::Ours,
				account: None,
				source: service.to_owned(),
				user: None,
				key: None,
			};
			db::record(&mut tx, &used).await?;
		}
		eyre::ensure!(db::move_cursor(&mut tx, service, after, page.next).await?, "{service}'s usage cursor moved meanwhile");
		tx.commit().await.wrap_err("committing a page of usage")?;
		Ok(page.used.len())
	}

	/// Every day's usage between `from` and `to`, oldest first.
	pub async fn used(&self, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Day>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for usage")?;
		db::by_day(&mut conn, from, to).await
	}
}
