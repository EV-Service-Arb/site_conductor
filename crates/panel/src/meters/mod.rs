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
//! the same read ─ the account's bill from its `since`, and the guard's last tick: replaced whole
//! ```

pub mod google;

use std::{future::Future, sync::Arc};

use eyre::WrapErr;
use jiff::{Timestamp, civil::Date};
use sa_core::{
	google::{Definition, Hold},
	meters::{ALL, Meter, Stop, Vendor},
	prices::{Item, MapLoad, PlacesRequest, StreetViewLoad},
};
use sa_places::{Account, Accounts, Pending};
use serde::{Deserialize, Serialize};
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

	/// The account's bill from `since`, whole.
	fn bill(&self, since: Date) -> impl Future<Output = eyre::Result<Bill>> + Send;

	/// The digest of the definition last published beside the export; `None`, none was.
	fn published(&self, now: Timestamp) -> impl Future<Output = eyre::Result<Option<String>>> + Send;

	/// `definition` beside the export, where the guard reads what to hold the account to.
	fn publish(&self, definition: &Definition, now: Timestamp) -> impl Future<Output = eyre::Result<()>> + Send;
}

/// What the vendor's record tells of an account's bill from its `since`: read whole each time, as
/// Google revises the days it exported. Amounts in millionths of their currency.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Bill {
	pub days: Vec<Day>,
	pub draws: Vec<Draw>,
	pub tiers: Vec<Tier>,
	/// The guard's last tick on the account; `None`, it never ticked there.
	pub guard: Option<Vec<Lever>>,
}

/// A day's use of one SKU.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Day {
	pub day: Date,
	pub service: String,
	pub sku: String,
	pub currency: String,
	/// At list price.
	pub cost_micros: i64,
	/// Drawn from a credit's balance ([`sa_core::google::BALANCES`]); negative.
	pub credit_micros: i64,
	/// Every other credit; negative.
	pub discount_micros: i64,
}

/// A credit's balance drawn, by the hour Google exported it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Draw {
	pub credit: String,
	pub hour: Timestamp,
	/// Negative.
	pub amount_micros: i64,
}

/// An SKU's units an invoice month, and where its first priced tier starts.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Tier {
	/// `YYYYMM`.
	pub month: String,
	pub service: String,
	pub sku: String,
	pub unit: Option<String>,
	pub units: f64,
	/// Billed at a price above nothing.
	pub paid_units: f64,
	/// Where its first priced tier starts; `None`, none was reached.
	pub free_to: Option<f64>,
	/// At list price.
	pub cost_micros: i64,
	/// What every credit but a balance's took off; negative.
	pub free_micros: i64,
}

/// One lever the guard set, or a slice's real money it read, at its tick (crates/guard).
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Lever {
	pub tick: Timestamp,
	/// `None` for an API no slice holds: free, unguarded, disabled, or held at no request.
	pub slice: Option<String>,
	pub project: Option<String>,
	/// `cap: <service>`, `quota: <api> <id>`, `free: <api>`, `unguarded: <api>`, `disable: <api>`, or `real money`.
	pub lever: String,
	pub allow_day: Option<f64>,
	pub allow_month: Option<f64>,
	pub real_day: Option<f64>,
	pub real_month: Option<f64>,
	pub setting: Option<f64>,
	pub state: Option<String>,
	/// Requests since the quota day began.
	pub today: Option<i64>,
	pub action: Option<String>,
	/// The digest of the definition it held ([`sa_core::google::digest`]).
	pub list: Option<String>,
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
/// month's allowance of [`sa_core::meters::SLICE`], `None` past both, and for an account with no export or no hold:
/// one the guard does not hold is not used.
pub(crate) async fn standing(conn: &mut SqliteConnection, account: &Account, meters: &[&'static Meter], now: Timestamp) -> eyre::Result<Option<u8>> {
	if account.export.is_none() {
		return Ok(None);
	}
	let Some(hold) = db::hold(conn, &account.name).await? else {
		return Ok(None);
	};
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
	let allowance = hold.allow[sa_core::meters::SLICE].month; // Hold::check refuses one short of a slice, or with no meters' slice
	Ok(spend.is_some_and(|s| (s.net_micros as f64) < allowance * 1e6).then_some(1))
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
	/// Every meter of `reader`'s vendor read on its account, and its bill, kept in one write. How
	/// many meters.
	pub async fn sync_meters(&self, reader: &impl Reader, now: Timestamp) -> eyre::Result<usize> {
		let (vendor, account) = (reader.vendor(), reader.account());
		let meters: Vec<&'static Meter> = ALL.iter().filter(|m| m.vendor == vendor).collect();
		let readings = reader.read(&meters, now).await?;
		let since = (self.accounts.get(account).ok_or_else(|| eyre::eyre!("{account} is read but not configured"))?.export.as_ref())
			.ok_or_else(|| eyre::eyre!("{account} is read but has no export"))?
			.since;
		let bill = reader.bill(since).await.wrap_err("the bill")?;
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
		db::put_bill(&mut tx, account, &bill, now).await?;
		let mut gauges = Vec::with_capacity(meters.len());
		for m in meters {
			gauges.push(gauge(&mut tx, account, m, now).await?);
		}
		self.tell_meters(&mut tx, account, &gauges, bill.guard.as_deref().unwrap_or_default(), now).await?; // the guard never ticked there: it did nothing
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

	/// What `account` is held to now; `None`, never set.
	pub async fn hold(&self, account: &str) -> eyre::Result<Option<Hold>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a hold")?;
		db::hold(&mut conn, account).await
	}

	/// Holds `account` to `hold` from `now`: the guard reads it once published beside the export.
	pub async fn set_hold(&self, account: &str, hold: &Hold, by: &str, now: Timestamp) -> eyre::Result<Result<(), crate::Refused>> {
		let Some(a) = self.accounts.get(account) else {
			return Ok(Err(crate::Refused::Invalid(format!("no Google account is {account:?}"))));
		};
		if a.export.is_none() {
			return Ok(Err(crate::Refused::Invalid(format!("{account} has no export: nothing holds it"))));
		}
		if let Err(e) = hold.check(self.accounts.slices()) {
			return Ok(Err(crate::Refused::Invalid(format!("{account}: {}", e.0))));
		}
		let mut tx = self.store.begin_write().await?;
		db::put_hold(&mut tx, account, hold, by, now).await?;
		tx.commit().await.wrap_err("committing a hold")?;
		tracing::info!(account, by, ?hold, "hold set");
		Ok(Ok(()))
	}

	/// What the guard is to hold `account` to; `None` without an export or a hold.
	pub async fn definition(&self, account: &Account) -> eyre::Result<Option<Definition>> {
		Ok(self.hold(&account.name).await?.and_then(|h| account.held(h)).map(|held| Definition {
			held,
			slices: self.accounts.defined_slices().to_vec(),
		}))
	}

	/// `reader`'s account's definition published beside its export, unless `last`, what was, already is.
	pub async fn publish_definition(&self, reader: &impl Reader, last: &mut Option<String>, now: Timestamp) -> eyre::Result<()> {
		let account = self
			.accounts
			.get(reader.account())
			.ok_or_else(|| eyre::eyre!("{} is read but not configured", reader.account()))?;
		let Some(definition) = self.definition(account).await? else {
			return Ok(()); // no hold set: the admin's meters card says so, and the guard refuses the account
		};
		let digest = definition.digest();
		if last.as_ref() == Some(&digest) {
			return Ok(());
		}
		if reader.published(now).await? != Some(digest.clone()) {
			reader.publish(&definition, now).await.wrap_err_with(|| format!("publishing {}'s definition", account.name))?;
			tracing::info!(account = account.name, digest, "definition published");
		}
		*last = Some(digest);
		Ok(())
	}

	/// Each account's bill as last read, and when, in the config's order; `None`, never read.
	pub async fn bills(&self) -> eyre::Result<Vec<(&Account, Option<(Bill, Timestamp)>)>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the bills")?;
		let mut bills = Vec::with_capacity(self.accounts.len());
		for a in self.accounts.iter() {
			bills.push((a, db::bill(&mut conn, &a.name).await?));
		}
		Ok(bills)
	}

	/// Every reading synced in `[from, to)`, oldest first.
	pub async fn meter_readings(&self, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Synced>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the meters' readings")?;
		db::between(&mut conn, from, to).await
	}
}
