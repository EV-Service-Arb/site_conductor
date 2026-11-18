//! What the vendors count of each meter ([`sa_core::meters`]) on each billing account, read off
//! their own records, and a meter's use there: their count, plus ours on its keys since it.
//!
//! ```text
//! serve: at start, then daily ─ each account's Reader::read(its vendor's meters) ─ one write: a reading each
//! a buy of a metered item on an account ─ in its write: the account's latest reading of this period?
//!                                           its units, ours on the account after its `through`
//!                                           else  0, ours on the account after the period's start
//!                          ─ refused past the free units if the meter stops there, or with no reading at all
//! which account buys ─ its standing: 0 free units left, 1 its net spend this period under its allowance
//! ```

pub mod google;

use std::{future::Future, sync::Arc};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	meters::{ALL, Meter, Stop, Vendor},
	prices::{Item, MapLoad, PlacesRequest, StreetViewLoad},
};
use sa_places::{Account, Accounts, Pending};
use sqlx::SqliteConnection;

pub use crate::store::meters::{Reading, Spend, Synced};
use crate::{
	Panel,
	store::{Store, meters as db},
};

/// A vendor's own record of what it bills on one account.
pub trait Reader {
	fn vendor(&self) -> Vendor;

	/// The billing account's name.
	fn account(&self) -> &str;

	/// Each of `meters` as the vendor counts it in its period around `now`.
	fn read(&self, meters: &[&'static Meter], now: Timestamp) -> impl Future<Output = eyre::Result<Vec<Reading>>> + Send;
}

/// A meter's use on an account, now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gauge {
	pub account: String,
	pub meter: &'static Meter,
	/// `[start, end)` of the period counted.
	pub period: (Timestamp, Timestamp),
	/// The vendor's latest reading, of whichever period; none before a first sync.
	pub synced: Option<Synced>,
	/// The vendor's count this period: the reading's, 0 when it is of one before.
	pub baseline: i64,
	/// Ours are counted after it.
	pub since: Timestamp,
	/// Ours after `since`.
	pub delta: i64,
}

impl Gauge {
	pub fn used(&self) -> i64 {
		self.baseline + self.delta
	}

	/// Free units left; below 0 once past them.
	pub fn left(&self) -> i64 {
		self.meter.free - self.used()
	}

	/// Why `n` more are refused, by the meter's stop.
	pub fn refuses(&self, n: i64) -> Option<String> {
		let m = self.meter;
		match m.stop {
			Stop::Never => None,
			Stop::AtFree if self.synced.is_none() => Some(format!(
				"{} on {} has no reading of {:?}'s count yet, so none is bought: an admin sees why on the Tokens page",
				m.name, self.account, m.vendor
			)),
			Stop::AtFree if self.used() + n > m.free => Some(format!(
				"{}'s free {} this period on {} are used ({:?}'s count {} as of {}, ours {} since): none until {}",
				m.name,
				m.free,
				self.account,
				m.vendor,
				self.baseline,
				self.since.strftime("%d %b %H:%M UTC"),
				self.delta,
				self.period.1.strftime("%d %b %H:%M UTC")
			)),
			Stop::AtFree => None,
		}
	}
}

/// What `account` may spend a month past its free units and credits on what is metered here: all
/// of it falls in accounting's `maps` slice.
pub fn allowance(account: &Account) -> f64 {
	account.allow.get("maps").copied().unwrap_or(0.0) // a slice not named is allowed none, as accounting's guard reads it
}

/// The conductor's [`sa_places::Rank`]: each account by its `standing` on the meter of the
/// SKU bought.
#[derive(Debug)]
pub struct ByStanding {
	pub store: Store,
	pub accounts: Arc<Accounts>,
}

/// `meter`'s use on `account` at `now`, read in the caller's transaction.
pub(crate) async fn gauge(conn: &mut SqliteConnection, account: &str, meter: &'static Meter, now: Timestamp) -> eyre::Result<Gauge> {
	let period = meter.vendor.period(now);
	let synced = db::latest(conn, account, meter).await?;
	let (baseline, since) = match &synced {
		Some(s) if s.reading.period_start == period.0 => (s.reading.units, s.reading.through),
		_ => (0, period.0),
	};
	let delta = db::ours_after(conn, account, meter, since).await?;
	Ok(Gauge {
		account: account.to_owned(),
		meter,
		period,
		synced,
		baseline,
		since,
		delta,
	})
}

/// `account`'s standing for one buy on each of `meters` at `now`: 0 with free units of each left
/// (a meter never read counts ours alone), 1 with its net spend this period, as read, under its
/// allowance, `None` past both.
pub(crate) async fn standing(conn: &mut SqliteConnection, account: &Account, meters: &[&'static Meter], now: Timestamp) -> eyre::Result<Option<u8>> {
	let mut free = true;
	for m in meters {
		free &= gauge(conn, &account.name, m, now).await?.left() > 0;
	}
	if free {
		return Ok(Some(0));
	}
	let vendor = meters[0].vendor;
	assert!(meters.iter().all(|m| m.vendor == vendor), "an account bills one vendor");
	let spend = db::spend(conn, &account.name, vendor.period(now).0).await?;
	Ok(spend.is_some_and(|s| (s.net_micros as f64) < allowance(account) * 1e6).then_some(1))
}

impl sa_places::Rank for ByStanding {
	fn rank<'a>(&'a self, sku: PlacesRequest, accounts: &'a [String]) -> Pending<'a, Vec<Option<u8>>> {
		Box::pin(async move {
			let meter = Item::from(sku).meter().expect("every Places SKU is on Google's meter");
			let ranks = async {
				let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the accounts' standing")?;
				let mut ranks = Vec::with_capacity(accounts.len());
				for name in accounts {
					let account = self.accounts.get(name).expect("the rotation is built off the accounts");
					ranks.push(standing(&mut conn, account, &[meter], jiff::Timestamp::now()).await?);
				}
				eyre::Ok(ranks)
			};
			ranks.await.map_err(|e| sa_places::Error::Rank(format!("{e:#}")))
		})
	}
}

impl Panel {
	/// Every meter of `reader`'s vendor read on its account, and kept in one write. How many.
	pub async fn sync_meters(&self, reader: &impl Reader, now: Timestamp) -> eyre::Result<usize> {
		let (vendor, account) = (reader.vendor(), reader.account());
		let meters: Vec<&'static Meter> = ALL.iter().filter(|m| m.vendor == vendor).collect();
		let readings = reader.read(&meters, now).await?;
		let start = vendor.period(now).0;
		for m in &meters {
			eyre::ensure!(readings.iter().filter(|r| r.meter == *m).count() == 1, "{vendor:?} answered {m} other than once");
		}
		for r in &readings {
			eyre::ensure!(r.account == account, "{account}'s reader answered for {}", r.account);
			eyre::ensure!(meters.contains(&r.meter), "{vendor:?} answered {}, not asked", r.meter);
			eyre::ensure!(r.period_start == start, "{vendor:?}'s reading of {} is of a period from {}, not {start}", r.meter, r.period_start);
			eyre::ensure!(r.units >= 0, "{vendor:?} counts {} of {}", r.units, r.meter);
		}
		let mut tx = self.store.begin_write().await?;
		for r in &readings {
			db::append(&mut tx, r, now).await?;
		}
		tx.commit().await.wrap_err("committing the meters' readings")?;
		Ok(readings.len())
	}

	/// Every meter's use on each account at `now`, in [`ALL`]'s order, each in the accounts'.
	pub async fn meters(&self, now: Timestamp) -> eyre::Result<Vec<Vec<Gauge>>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the meters")?;
		let mut gauges = Vec::with_capacity(ALL.len());
		for m in ALL {
			let mut each = Vec::with_capacity(self.accounts.len());
			for a in self.accounts.iter() {
				each.push(gauge(&mut conn, &a.name, m, now).await?);
			}
			gauges.push(each);
		}
		Ok(gauges)
	}

	/// What `account` spent with `vendor` this period, as its latest readings tell; `None` before a
	/// reading of this period.
	pub async fn spend(&self, account: &str, vendor: Vendor, now: Timestamp) -> eyre::Result<Option<Spend>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an account's spend")?;
		db::spend(&mut conn, account, vendor.period(now).0).await
	}

	/// The account the browser's map loads on: of those with a browser key, the first standing
	/// lowest for a map and a panorama; `None`, none stands.
	pub async fn map_account(&self, now: Timestamp) -> eyre::Result<Option<&Account>> {
		let meters = [Item::from(MapLoad::Map), Item::from(StreetViewLoad::Panorama)].map(|i| i.meter().expect("the browser's loads are on Google's meters"));
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the accounts' standing")?;
		let mut best: Option<(u8, &Account)> = None;
		for a in self.accounts.iter().filter(|a| a.provides.browser.is_some()) {
			if let Some(r) = standing(&mut conn, a, &meters, now).await?
				&& best.is_none_or(|(b, _)| r < b)
			{
				best = Some((r, a));
			}
		}
		Ok(best.map(|(_, a)| a))
	}

	/// Every reading synced in `[from, to)`, oldest first.
	pub async fn meter_readings(&self, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Synced>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the meters' readings")?;
		db::between(&mut conn, from, to).await
	}
}
