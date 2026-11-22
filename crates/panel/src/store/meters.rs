//! The vendors' readings of each meter ([`sa_core::meters`]) on each account, only ever added, and
//! our own uses of its items on that account after one; and each account's bill as last read.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::meters::Meter;
use sqlx::SqliteConnection;

use super::{from_db, to_db};

type Row = (String, String, i64, i64, i64, i64, String, i64, i64);

macro_rules! columns {
	() => {
		"account, meter, period_start, units, cost_micros, net_micros, currency, through, synced_at"
	};
}

/// A meter as its vendor counts it on one account.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reading {
	/// The billing account's name, as configured.
	pub account: String,
	pub meter: &'static Meter,
	/// The period it counts from.
	pub period_start: Timestamp,
	pub units: i64,
	/// Gross, before credits, in millionths of `currency`.
	pub cost_micros: i64,
	/// After credits (which are negative), in millionths of `currency`.
	pub net_micros: i64,
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

/// An account's spend with a vendor in a period, as its latest reading of each meter tells.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Spend {
	/// Gross, in millionths of `currency`.
	pub cost_micros: i64,
	/// After credits.
	pub net_micros: i64,
	pub currency: String,
	/// The earliest the readings reach.
	pub through: Timestamp,
}

fn synced((account, meter, period_start, units, cost_micros, net_micros, currency, through, synced_at): Row) -> eyre::Result<Synced> {
	Ok(Synced {
		reading: Reading {
			account,
			meter: meter.parse().wrap_err("a reading of a meter the catalog lacks: a migration drops it")?,
			period_start: from_db(period_start)?,
			units,
			cost_micros,
			net_micros,
			currency,
			through: from_db(through)?,
		},
		synced_at: from_db(synced_at)?,
	})
}

pub(crate) async fn append(conn: &mut SqliteConnection, r: &Reading, synced_at: Timestamp) -> eyre::Result<()> {
	sqlx::query(concat!("INSERT INTO meter_readings (", columns!(), ") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"))
		.bind(&r.account)
		.bind(r.meter.to_string())
		.bind(to_db(r.period_start))
		.bind(r.units)
		.bind(r.cost_micros)
		.bind(r.net_micros)
		.bind(&r.currency)
		.bind(to_db(r.through))
		.bind(to_db(synced_at))
		.execute(conn)
		.await
		.wrap_err_with(|| format!("recording a reading of {} on {}", r.meter, r.account))?;
	Ok(())
}

pub(crate) async fn latest(conn: &mut SqliteConnection, account: &str, meter: &'static Meter) -> eyre::Result<Option<Synced>> {
	let row: Option<Row> = sqlx::query_as(concat!("SELECT ", columns!(), " FROM meter_readings WHERE account = $1 AND meter = $2 ORDER BY id DESC LIMIT 1"))
		.bind(account)
		.bind(meter.to_string())
		.fetch_optional(conn)
		.await
		.wrap_err_with(|| format!("reading {meter}'s latest reading on {account}"))?;
	row.map(synced).transpose()
}

/// Every reading synced in `[from, to)`, oldest first.
pub(crate) async fn between(conn: &mut SqliteConnection, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Synced>> {
	let rows: Vec<Row> = sqlx::query_as(concat!("SELECT ", columns!(), " FROM meter_readings WHERE synced_at >= $1 AND synced_at < $2 ORDER BY id"))
		.bind(to_db(from))
		.bind(to_db(to))
		.fetch_all(conn)
		.await
		.wrap_err("reading the meters' readings")?;
	rows.into_iter().map(synced).collect()
}

/// Units of `meter`'s items on `account` after `since`.
pub(crate) async fn ours_after(conn: &mut SqliteConnection, account: &str, meter: &'static Meter, since: Timestamp) -> eyre::Result<i64> {
	let items = serde_json::to_string(&meter.items().map(|i| i.to_string()).collect::<Vec<_>>()).expect("strings serialize");
	sqlx::query_scalar("SELECT COALESCE(SUM(units), 0) FROM usage WHERE item IN (SELECT value FROM json_each($1)) AND account = $2 AND at > $3")
		.bind(items)
		.bind(account)
		.bind(to_db(since))
		.fetch_one(conn)
		.await
		.wrap_err_with(|| format!("summing {meter} on {account} after {since}"))
}

/// `account`'s spend in the period from `period_start`; `None` before a reading of it.
pub(crate) async fn spend(conn: &mut SqliteConnection, account: &str, period_start: Timestamp) -> eyre::Result<Option<Spend>> {
	let row: (i64, i64, i64, Option<String>, Option<i64>) = sqlx::query_as(
		"SELECT COUNT(*), COALESCE(SUM(cost_micros), 0), COALESCE(SUM(net_micros), 0), MAX(currency), MIN(through) FROM meter_readings r \
		 WHERE account = $1 AND period_start = $2 AND id = (SELECT MAX(id) FROM meter_readings WHERE account = r.account AND meter = r.meter)",
	)
	.bind(account)
	.bind(to_db(period_start))
	.fetch_one(conn)
	.await
	.wrap_err_with(|| format!("summing {account}'s spend"))?;
	match row {
		(0, ..) => Ok(None),
		(_, cost_micros, net_micros, Some(currency), Some(through)) => Ok(Some(Spend {
			cost_micros,
			net_micros,
			currency,
			through: from_db(through)?,
		})),
		_ => unreachable!("a reading has its currency and its through"),
	}
}

/// `account`'s bill, replacing the one read before.
pub(crate) async fn put_bill(conn: &mut SqliteConnection, account: &str, bill: &crate::meters::Bill, read_at: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO google_bills (account, bill, read_at) VALUES ($1, $2, $3) ON CONFLICT (account) DO UPDATE SET bill = excluded.bill, read_at = excluded.read_at")
		.bind(account)
		.bind(serde_json::to_string(bill).expect("a bill serializes"))
		.bind(to_db(read_at))
		.execute(conn)
		.await
		.wrap_err_with(|| format!("keeping {account}'s bill"))?;
	Ok(())
}

/// `account`'s bill as last read, and when; `None`, never.
pub(crate) async fn bill(conn: &mut SqliteConnection, account: &str) -> eyre::Result<Option<(crate::meters::Bill, Timestamp)>> {
	let row: Option<(String, i64)> = sqlx::query_as("SELECT bill, read_at FROM google_bills WHERE account = $1")
		.bind(account)
		.fetch_optional(conn)
		.await
		.wrap_err_with(|| format!("reading {account}'s bill"))?;
	row.map(|(bill, at)| Ok((serde_json::from_str(&bill).wrap_err_with(|| format!("{account}'s bill as kept"))?, from_db(at)?)))
		.transpose()
}
