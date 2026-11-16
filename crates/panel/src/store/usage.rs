//! Every costly action, as it was done or told ([`sa_core::prices::Item`]), only ever added; where
//! each service's feed of them is read up to; and what a unit of each costs us.

use std::collections::BTreeMap;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{prices::Item, usage::Bill};
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};

/// The `source` of what the conductor does itself.
pub const CONDUCTOR: &str = "conductor";

/// One action done, or several of a unit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Use {
	pub at: Timestamp,
	pub item: Item,
	pub units: i64,
	pub bill: Bill,
	/// Who told: `conductor`, a queries cache scheme, or a feed's service.
	pub source: String,
	/// Whom it was done for, when the conductor knows.
	pub user: Option<Uuid>,
	/// What it was about, as its tool names it.
	pub key: Option<String>,
}

/// What one unit costs us, and the admin who set it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Cost {
	/// Millionths of a dollar.
	pub usd_micros: i64,
	pub set_by: String,
	pub set_at: Timestamp,
}

/// A UTC day's units of one item, by whose bill and who told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Day {
	/// Days since the epoch, UTC.
	pub day: i64,
	pub item: Item,
	pub bill: Bill,
	pub source: String,
	pub units: i64,
	pub times: i64,
	/// Millionths of a dollar, each use at the cost in effect when it was done; `None`, none had one.
	pub usd_micros: Option<i64>,
	/// Units done while the item had no cost.
	pub uncosted: i64,
}

pub(crate) async fn record(conn: &mut SqliteConnection, u: &Use) -> eyre::Result<()> {
	assert!(u.units > 0, "a use is of 1 unit or more: {u:?}");
	sqlx::query("INSERT INTO usage (at, item, units, bill, source, user_id, key) VALUES ($1, $2, $3, $4, $5, $6, $7)")
		.bind(to_db(u.at))
		.bind(u.item.to_string())
		.bind(u.units)
		.bind(<&str>::from(u.bill))
		.bind(&u.source)
		.bind(u.user)
		.bind(&u.key)
		.execute(conn)
		.await
		.wrap_err_with(|| format!("recording {} × {}", u.units, u.item))?;
	Ok(())
}

/// Where `source`'s feed is read up to; 0, nothing taken from it yet.
pub(crate) async fn cursor(conn: &mut SqliteConnection, source: &str) -> eyre::Result<i64> {
	let at: Option<i64> = sqlx::query_scalar("SELECT after FROM usage_cursors WHERE source = $1")
		.bind(source)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err_with(|| format!("reading {source}'s usage cursor"))?;
	// a feed's ids start past 0: before its first page, nothing of it is taken
	Ok(at.unwrap_or(0))
}

/// The cursor moved from `from` to `to`, in a write transaction; false when it no longer stood at `from`.
pub(crate) async fn move_cursor(conn: &mut SqliteConnection, source: &str, from: i64, to: i64) -> eyre::Result<bool> {
	if cursor(conn, source).await? != from {
		return Ok(false);
	}
	sqlx::query("INSERT INTO usage_cursors (source, after) VALUES ($1, $2) ON CONFLICT (source) DO UPDATE SET after = excluded.after")
		.bind(source)
		.bind(to)
		.execute(conn)
		.await
		.wrap_err_with(|| format!("moving {source}'s usage cursor"))?;
	Ok(true)
}

/// Each item's cost in effect now; an item last set back to unknown has none.
pub(crate) async fn costs(conn: &mut SqliteConnection) -> eyre::Result<BTreeMap<Item, Cost>> {
	let rows: Vec<(String, i64, String, i64)> = sqlx::query_as(
		"SELECT item, usd_micros, set_by, set_at FROM unit_costs c \
		 WHERE id = (SELECT MAX(id) FROM unit_costs WHERE item = c.item) AND usd_micros IS NOT NULL",
	)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading the costs")?;
	rows.into_iter()
		.map(|(item, usd_micros, set_by, set_at)| {
			Ok((
				item.parse().wrap_err("a costed row names an item the catalog lacks: a migration drops it")?,
				Cost {
					usd_micros,
					set_by,
					set_at: from_db(set_at)?,
				},
			))
		})
		.collect()
}

/// `item`'s unit costs `usd_micros` from `now` on, 0 or more; `None`, unknown from now on.
pub(crate) async fn set_cost(conn: &mut SqliteConnection, item: Item, usd_micros: Option<i64>, by: &str, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO unit_costs (item, usd_micros, set_by, set_at) VALUES ($1, $2, $3, $4)")
		.bind(item.to_string())
		.bind(usd_micros)
		.bind(by)
		.bind(to_db(now))
		.execute(conn)
		.await
		.wrap_err_with(|| format!("costing {item}"))?;
	Ok(())
}

/// Every day's usage between `from` and `to`, oldest first. A use before its item's first cost
/// takes that first one: the vendor charged it all along, nobody had typed it in yet.
pub(crate) async fn by_day(conn: &mut SqliteConnection, from: Timestamp, to: Timestamp) -> eyre::Result<Vec<Day>> {
	let rows: Vec<(i64, String, String, String, i64, i64, Option<i64>, i64)> = sqlx::query_as(
		"WITH costed AS ( \
		   SELECT u.at, u.item, u.bill, u.source, u.units, CASE \
		     WHEN EXISTS (SELECT 1 FROM unit_costs c WHERE c.item = u.item AND c.set_at <= u.at) \
		     THEN (SELECT c.usd_micros FROM unit_costs c WHERE c.item = u.item AND c.set_at <= u.at ORDER BY c.id DESC LIMIT 1) \
		     ELSE (SELECT c.usd_micros FROM unit_costs c WHERE c.item = u.item ORDER BY c.id LIMIT 1) \
		   END AS cost FROM usage u WHERE u.at >= $1 AND u.at < $2 \
		 ) \
		 SELECT at / 86400000000 AS day, item, bill, source, SUM(units), COUNT(*), SUM(units * cost), \
		   SUM(CASE WHEN cost IS NULL THEN units ELSE 0 END) \
		 FROM costed GROUP BY day, item, bill, source ORDER BY day",
	)
	.bind(to_db(from))
	.bind(to_db(to))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("summing usage by day")?;
	rows.into_iter()
		.map(|(day, item, bill, source, units, times, usd_micros, uncosted)| {
			Ok(Day {
				day,
				item: item.parse().wrap_err("a usage row names an item the catalog lacks: a migration renames it")?,
				bill: bill.parse().wrap_err_with(|| format!("a usage row billed to {bill:?}"))?,
				source,
				units,
				times,
				usd_micros,
				uncosted,
			})
		})
		.collect()
}
