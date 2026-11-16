//! The vendors' readings of each meter ([`sa_core::meters`]), only ever added, and our own uses of
//! its items after one.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::meters::Meter;
use sqlx::SqliteConnection;

use super::{from_db, to_db};

type Row = (String, i64, i64, i64, String, i64, i64);

/// A meter as its vendor counts it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reading {
	pub meter: &'static Meter,
	/// The period it counts from.
	pub period_start: Timestamp,
	pub units: i64,
	/// Gross, before credits, in millionths of `currency`.
	pub cost_micros: i64,
	pub currency: String,
	/// How far the vendor's record reaches.
	pub through: Timestamp,
}

/// A reading, and when it was taken.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Synced {
	pub reading: Reading,
	pub synced_at: Timestamp,
}

fn synced((meter, period_start, units, cost_micros, currency, through, synced_at): Row) -> eyre::Result<Synced> {
	Ok(Synced {
		reading: Reading {
			meter: meter.parse().wrap_err("a reading of a meter the catalog lacks: a migration drops it")?,
			period_start: from_db(period_start)?,
			units,
			cost_micros,
			currency,
			through: from_db(through)?,
		},
		synced_at: from_db(synced_at)?,
	})
}

pub(crate) async fn append(conn: &mut SqliteConnection, r: &Reading, synced_at: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO meter_readings (meter, period_start, units, cost_micros, currency, through, synced_at) VALUES ($1, $2, $3, $4, $5, $6, $7)")
		.bind(r.meter.to_string())
		.bind(to_db(r.period_start))
		.bind(r.units)
		.bind(r.cost_micros)
		.bind(&r.currency)
		.bind(to_db(r.through))
		.bind(to_db(synced_at))
		.execute(conn)
		.await
		.wrap_err_with(|| format!("recording a reading of {}", r.meter))?;
	Ok(())
}

pub(crate) async fn latest(conn: &mut SqliteConnection, meter: &'static Meter) -> eyre::Result<Option<Synced>> {
	let row: Option<Row> = sqlx::query_as("SELECT meter, period_start, units, cost_micros, currency, through, synced_at FROM meter_readings WHERE meter = $1 ORDER BY id DESC LIMIT 1")
		.bind(meter.to_string())
		.fetch_optional(conn)
		.await
		.wrap_err_with(|| format!("reading {meter}'s latest reading"))?;
	row.map(synced).transpose()
}

/// Every reading synced in `[from, to)`, oldest first.
pub(crate) async fn between(conn: &mut SqliteConnection, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Synced>> {
	let rows: Vec<Row> =
		sqlx::query_as("SELECT meter, period_start, units, cost_micros, currency, through, synced_at FROM meter_readings WHERE synced_at >= $1 AND synced_at < $2 ORDER BY id")
			.bind(to_db(from))
			.bind(to_db(to))
			.fetch_all(conn)
			.await
			.wrap_err("reading the meters' readings")?;
	rows.into_iter().map(synced).collect()
}

/// Units of `meter`'s items on our bill after `since`.
pub(crate) async fn ours_after(conn: &mut SqliteConnection, meter: &'static Meter, since: Timestamp) -> eyre::Result<i64> {
	let items = serde_json::to_string(&meter.items().map(|i| i.to_string()).collect::<Vec<_>>()).expect("strings serialize");
	sqlx::query_scalar("SELECT COALESCE(SUM(units), 0) FROM usage WHERE item IN (SELECT value FROM json_each($1)) AND bill = 'ours' AND at > $2")
		.bind(items)
		.bind(to_db(since))
		.fetch_one(conn)
		.await
		.wrap_err_with(|| format!("summing {meter} after {since}"))
}
