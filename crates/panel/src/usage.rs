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

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	prices::{Item, MapLoad},
	usage::{Bill, UsagePage},
};
use uuid::Uuid;

pub use crate::store::usage::{Cost, Day};
use crate::{
	Panel, Refused,
	store::usage::{self as db, CONDUCTOR, Use},
};

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
