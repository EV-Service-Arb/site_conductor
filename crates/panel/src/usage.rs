//! What our tools spend on vendors ([`sa_core::prices::Item`]): done here, told by the queries
//! cache's buyers, or read from each service's usage feed.
//!
//! ```text
//! a Google answer bought ─ kept and counted in one write (queries cache)
//! a map built in a browser, a panorama opened ─ bought first: counted and charged in one write
//! GET <service> usage feed?after=<cursor> ─ a page ─ one write: its rows, each charged to whom it
//!                                                    names, the cursor moved
//! ```
//!
//! **A use is counted and charged before the cursor passes it**, and a page is taken only from
//! where its service's cursor stands.

use std::collections::{BTreeMap, BTreeSet};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	prices::{Item, MapLoad, ModelTokens, Scope, Spender},
	usage::{Bill, UsagePage},
};
use strum::VariantArray;

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
	/// The user's browser builds a map on `account`'s browser key: bought, or refused before it
	/// loads. Their balance then.
	pub async fn buy_map(&self, by: Spender<MapLoad>, account: &str, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let user = by.user();
		if self.accounts.get(account).is_none_or(|a| a.provides.browser.is_none()) {
			return Ok(Err(Refused::Invalid(format!("{account:?} is no Google account with a browser key"))));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let item = Item::from(MapLoad::Map);
		let Some(price) = ledger::price(&mut tx, item).await? else {
			return Ok(Err(Refused::Conflict("the map is not priced yet: ask an admin to price it".to_owned())));
		};
		let balance = ledger::renewed(&mut tx, user, now, &self.renewal).await?;
		if balance < price {
			return Ok(Err(Refused::Conflict(format!("the map costs {price} tokens, and you hold {balance}"))));
		}
		let used = Use {
			at: now,
			item,
			units: 1,
			bill: Bill::Ours,
			account: Some(account.to_owned()),
			source: CONDUCTOR.to_owned(),
			user: Some(user),
			key: None,
		};
		db::record(&mut tx, &used).await?;
		if price > 0 {
			let entry = Entry {
				at: now,
				delta: -price,
				kind: Kind::Charge,
				charged: Some(Charged {
					service: item.tool().to_owned(),
					key: now.to_string(),
					label: Some("The map".to_owned()),
				}),
				by: None,
				note: None,
			};
			ledger::append(&mut tx, user, &entry).await?;
		}
		tx.commit().await.wrap_err("committing a map bought")?;
		if price > 0 {
			tracing::info!(user_id = %user, tokens = price, "a map bought");
			self.snapshot_due.notify_one();
		}
		Ok(Ok(balance - price))
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
		let due = (tab.owed + per - 1).div_euclid(per) - tab.billed;
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

	/// A page of `service`'s usage feed, asked for after `after`, counted, each use naming whom it
	/// was made for charged to them at its price up to their balance, and the cursor moved, in one
	/// write; a page that does not start at the cursor, names an item another service does, names a
	/// person for a use charged otherwise or none for one charged here, or charges an unpriced item,
	/// fails whole. How many uses were taken.
	pub async fn take_usage(&self, service: &str, items: &BTreeSet<Item>, page: UsagePage, after: i64) -> eyre::Result<usize> {
		let last = page.used.last().map_or(after, |u| u.id);
		eyre::ensure!(page.next == last, "{service} answered next {} for a page ending at {last}", page.next);
		let mut prev = after;
		for u in &page.used {
			eyre::ensure!(u.id > prev, "{service}'s use {} after {prev}: ids ascend past the cursor", u.id);
			eyre::ensure!(u.units > 0, "{service}'s use {} of {} units", u.id, u.units);
			eyre::ensure!(items.contains(&u.item), "{service}'s use {} is of {}, not its", u.id, u.item);
			eyre::ensure!(
				u.user.is_some() == (u.item.scope() == Scope::Browsers),
				"{service}'s use {} of {} names {:?}: a use is for a person exactly when it is charged as it is taken",
				u.id,
				u.item,
				u.user
			);
			prev = u.id;
		}
		if page.used.is_empty() {
			return Ok(0);
		}
		let mut tx = self.store.begin_write().await?;
		let mut charged = false;
		for u in &page.used {
			let used = Use {
				at: u.at,
				item: u.item,
				units: u.units,
				bill: Bill::Ours,
				account: None,
				source: service.to_owned(),
				user: u.user,
				key: None,
			};
			db::record(&mut tx, &used).await?;
			let Some(user) = u.user else { continue };
			let Some(price) = ledger::price(&mut tx, u.item).await? else {
				eyre::bail!(
					"{service}'s use {} of {} for {user} is unpriced: an admin took its price off after it was let start; price it again",
					u.id,
					u.item
				);
			};
			let per = u.item.per();
			let due = (u.units * price + per - 1).div_euclid(per);
			let balance = ledger::renewed(&mut tx, user, Timestamp::now(), &self.renewal).await?;
			// its cost is known only once it is done: what it costs past the balance is let go
			let charge = due.min(balance.max(0));
			if charge > 0 {
				let entry = Entry {
					at: u.at,
					delta: -charge,
					kind: Kind::Charge,
					charged: Some(Charged {
						service: u.item.tool().to_owned(),
						key: format!("{service}:{}", u.id),
						label: Some(format!("{} · {} {}", u.item.title(), u.units, u.item.unit())),
					}),
					by: None,
					note: None,
				};
				ledger::append(&mut tx, user, &entry).await?;
				charged = true;
			}
		}
		eyre::ensure!(db::move_cursor(&mut tx, service, after..page.next).await?, "{service}'s usage cursor moved meanwhile");
		tx.commit().await.wrap_err("committing a page of usage")?;
		if charged {
			self.snapshot_due.notify_one();
		}
		Ok(page.used.len())
	}

	/// Every day's usage between `from` and `to`, oldest first.
	pub async fn used(&self, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Day>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for usage")?;
		db::by_day(&mut conn, from, to).await
	}
}
