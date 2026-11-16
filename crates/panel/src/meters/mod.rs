//! What the vendors count of each meter ([`sa_core::meters`]), read off their own records, and a
//! meter's use: their count, plus ours since it.
//!
//! ```text
//! serve: at start, then daily ─ Reader::read(its vendor's meters) ─ one write: a reading each
//! a buy of a metered item ─ in its write: the latest reading of this period? its units, ours after its `through`
//!                                                                  else  0, ours after the period's start
//!                          ─ refused past the free units if the meter stops there, or with no reading at all
//! ```

pub mod google;

use std::future::Future;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::meters::{ALL, Meter, Stop, Vendor};
use sqlx::SqliteConnection;

pub use crate::store::meters::{Reading, Synced};
use crate::{Panel, store::meters as db};

/// A vendor's own record of what it bills.
pub trait Reader {
	fn vendor(&self) -> Vendor;

	/// Each of `meters` as the vendor counts it in its period around `now`.
	fn read(&self, meters: &[&'static Meter], now: Timestamp) -> impl Future<Output = eyre::Result<Vec<Reading>>> + Send;
}

/// A meter's use, now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Gauge {
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
				"{} has no reading of {:?}'s count yet, so none is bought: an admin sees why on the Tokens page",
				m.name, m.vendor
			)),
			Stop::AtFree if self.used() + n > m.free => Some(format!(
				"{}'s free {} this period are used ({:?}'s count {} as of {}, ours {} since): none until {}",
				m.name,
				m.free,
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

/// `meter`'s use at `now`, read in the caller's transaction.
pub(crate) async fn gauge(conn: &mut SqliteConnection, meter: &'static Meter, now: Timestamp) -> eyre::Result<Gauge> {
	let period = meter.vendor.period(now);
	let synced = db::latest(conn, meter).await?;
	let (baseline, since) = match &synced {
		Some(s) if s.reading.period_start == period.0 => (s.reading.units, s.reading.through),
		_ => (0, period.0),
	};
	let delta = db::ours_after(conn, meter, since).await?;
	Ok(Gauge {
		meter,
		period,
		synced,
		baseline,
		since,
		delta,
	})
}

impl Panel {
	/// Every meter of `reader`'s vendor read, and kept in one write. How many.
	pub async fn sync_meters(&self, reader: &impl Reader, now: Timestamp) -> eyre::Result<usize> {
		let vendor = reader.vendor();
		let meters: Vec<&'static Meter> = ALL.iter().filter(|m| m.vendor == vendor).collect();
		let readings = reader.read(&meters, now).await?;
		let start = vendor.period(now).0;
		for m in &meters {
			eyre::ensure!(readings.iter().filter(|r| r.meter == *m).count() == 1, "{vendor:?} answered {m} other than once");
		}
		for r in &readings {
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

	/// Every meter's use at `now`, in [`ALL`]'s order.
	pub async fn meters(&self, now: Timestamp) -> eyre::Result<Vec<Gauge>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the meters")?;
		let mut gauges = Vec::with_capacity(ALL.len());
		for m in ALL {
			gauges.push(gauge(&mut conn, m, now).await?);
		}
		Ok(gauges)
	}

	/// Every reading synced in `[from, to)`, oldest first.
	pub async fn meter_readings(&self, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Synced>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the meters' readings")?;
		db::between(&mut conn, from, to).await
	}
}
