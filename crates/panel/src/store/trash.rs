//! A person's trash: their gmails and GBPs removed and not purged yet.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::State;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{
	connections::{self, Connection},
	from_db,
	gmbs::{Unique, taken},
	to_db,
};

/// Kept this long with no row of the owner's.
const KEEP_DAYS: i64 = 30;
const DAY_US: i64 = 86_400_000_000;

const GMB: &str = "SELECT b.id, b.name, b.state, b.gmail_id, g.gmail, b.removed_at, b.place_id, b.phone, \
	b.gmail_id IS NOT NULL AND (g.removed_at IS NOT NULL) FROM gmbs b LEFT JOIN gmails g ON g.id = b.gmail_id \
	WHERE b.owner = $1 AND b.removed_at IS NOT NULL AND b.purged_at IS NULL";

type GmbRow = (i64, String, String, Option<i64>, Option<String>, i64, Option<String>, Option<String>, bool);

pub(crate) struct TrashedGmail {
	pub id: i64,
	pub gmail: String,
	pub removed_at: Timestamp,
	/// Its GBPs removed with it, which come back with it.
	pub gmbs: usize,
}

pub(crate) struct TrashedGmb {
	pub id: i64,
	pub name: String,
	pub state: State,
	pub gmail_id: Option<i64>,
	pub gmail: Option<String>,
	pub removed_at: Timestamp,
	place_id: Option<String>,
	phone: Option<String>,
	/// Its gmail is not live.
	gmail_gone: bool,
}

/// `(keep_days, never_purge)`, each `None` unsaid.
pub(crate) async fn settings(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<(i64, Option<bool>)> {
	let row: Option<(i64, Option<bool>)> = sqlx::query_as("SELECT keep_days, never_purge FROM trash_settings WHERE user_id = $1")
		.bind(owner)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading trash settings")?;
	Ok(row.map_or((KEEP_DAYS, None), |(d, n)| (d, n)))
}

/// `never_purge` `None`: left as it was.
pub(crate) async fn set_settings(conn: &mut SqliteConnection, owner: Uuid, keep_days: i64, never_purge: Option<bool>) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO trash_settings (user_id, keep_days, never_purge) VALUES ($1, $2, $3) ON CONFLICT (user_id) DO UPDATE SET \
		 keep_days = excluded.keep_days, never_purge = CASE WHEN $4 THEN excluded.never_purge ELSE trash_settings.never_purge END",
	)
	.bind(owner)
	.bind(keep_days)
	.bind(never_purge)
	.bind(never_purge.is_some())
	.execute(&mut *conn)
	.await
	.wrap_err("setting trash settings")?;
	Ok(())
}

/// The owner's gmails in the trash, newest first; `id` only that one.
pub(crate) async fn gmails(conn: &mut SqliteConnection, owner: Uuid, id: Option<i64>) -> eyre::Result<Vec<TrashedGmail>> {
	let rows: Vec<(i64, String, i64, i64)> = sqlx::query_as(
		"SELECT g.id, g.gmail, g.removed_at, (SELECT COUNT(*) FROM gmbs b WHERE b.gmail_id = g.id AND b.removed_at = g.removed_at) \
		 FROM gmails g WHERE g.owner = $1 AND g.removed_at IS NOT NULL AND g.purged_at IS NULL AND g.id IS coalesce($2, g.id) \
		 ORDER BY g.removed_at DESC, g.id",
	)
	.bind(owner)
	.bind(id)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing trashed gmails")?;
	rows.into_iter()
		.map(|(id, gmail, removed_at, gmbs)| {
			Ok(TrashedGmail {
				id,
				gmail,
				removed_at: from_db(removed_at)?,
				gmbs: usize::try_from(gmbs).expect("a count"),
			})
		})
		.collect()
}

fn gmb((id, name, state, gmail_id, gmail, removed_at, place_id, phone, gmail_gone): GmbRow) -> eyre::Result<TrashedGmb> {
	Ok(TrashedGmb {
		id,
		name,
		state: state.parse()?,
		gmail_id,
		gmail,
		removed_at: from_db(removed_at)?,
		place_id,
		phone,
		gmail_gone,
	})
}

/// The owner's GBPs in the trash on their own, not with their gmail, newest first.
pub(crate) async fn gmbs(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<TrashedGmb>> {
	let rows: Vec<GmbRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{GMB} AND NOT (g.removed_at IS NOT NULL AND g.removed_at = b.removed_at) ORDER BY b.removed_at DESC, b.id"
	)))
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing trashed GBPs")?;
	rows.into_iter().map(gmb).collect()
}

/// The GBPs removed with the owner's trashed gmail `id`, or GBP `gmb` alone.
async fn coming_back(conn: &mut SqliteConnection, owner: Uuid, gmail: Option<i64>, gmb_id: Option<i64>) -> eyre::Result<Vec<TrashedGmb>> {
	let rows: Vec<GmbRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{GMB} AND (b.id IS $3 OR (b.gmail_id IS $2 AND b.removed_at = g.removed_at)) ORDER BY b.id"
	)))
	.bind(owner)
	.bind(gmail)
	.bind(gmb_id)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing the GBPs a restore brings back")?;
	rows.into_iter().map(gmb).collect()
}

/// Why `b` cannot come back on its own: its gmail is not live, or a live GBP took its name,
/// place or phone.
pub(crate) async fn gmb_blocked(conn: &mut SqliteConnection, b: &TrashedGmb) -> eyre::Result<Option<String>> {
	if b.gmail_gone {
		return Ok(Some(format!("its gmail {} is removed: restore it first", b.gmail.as_deref().expect("joined on its gmail_id"))));
	}
	unique_blocked(conn, b).await
}

async fn unique_blocked(conn: &mut SqliteConnection, b: &TrashedGmb) -> eyre::Result<Option<String>> {
	let u = Unique {
		name: Some(&b.name),
		place_id: b.place_id.as_deref(),
		phone: b.phone.as_deref(),
	};
	taken(conn, u, Some(b.id)).await
}

/// Why the owner's trashed gmail `g` cannot come back with its GBPs.
pub(crate) async fn gmail_blocked(conn: &mut SqliteConnection, owner: Uuid, g: &TrashedGmail) -> eyre::Result<Option<String>> {
	let live: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM gmails WHERE owner = $1 AND gmail = $2 AND removed_at IS NULL)")
		.bind(owner)
		.bind(&g.gmail)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking for a live gmail of an address")?;
	if live {
		return Ok(Some(format!("{} is one of your gmails again", g.gmail)));
	}
	for b in coming_back(conn, owner, Some(g.id), None).await? {
		if let Some(why) = unique_blocked(conn, &b).await? {
			return Ok(Some(format!("{}: {why}", b.name)));
		}
	}
	Ok(None)
}

/// The owner's trashed GBP `id`.
pub(crate) async fn gmb_of(conn: &mut SqliteConnection, owner: Uuid, id: i64) -> eyre::Result<Option<TrashedGmb>> {
	Ok(coming_back(conn, owner, None, Some(id)).await?.pop())
}

/// Back from the trash: GBP `id`, else gmail `gmail` with the GBPs removed with it. The schema
/// refuses what [`gmb_blocked`] and [`gmail_blocked`] would name.
pub(crate) async fn restore(conn: &mut SqliteConnection, gmail: Option<i64>, gmb_id: Option<i64>) -> eyre::Result<()> {
	if let Some(g) = gmail {
		sqlx::query("UPDATE gmbs SET removed_at = NULL WHERE gmail_id = $1 AND removed_at = (SELECT removed_at FROM gmails WHERE id = $1)")
			.bind(g)
			.execute(&mut *conn)
			.await
			.wrap_err("restoring a gmail's GBPs")?;
		sqlx::query("UPDATE gmails SET removed_at = NULL WHERE id = $1")
			.bind(g)
			.execute(&mut *conn)
			.await
			.wrap_err("restoring a gmail")?;
	}
	if let Some(b) = gmb_id {
		sqlx::query("UPDATE gmbs SET removed_at = NULL WHERE id = $1")
			.bind(b)
			.execute(&mut *conn)
			.await
			.wrap_err("restoring a GBP")?;
	}
	Ok(())
}

/// Everything removed past its owner's kept days, unless they keep everything
/// (`keep_by_default` without their say): purged, and the purged gmails' connections deleted.
pub(crate) async fn purge(conn: &mut SqliteConnection, now: Timestamp, keep_by_default: bool) -> eyre::Result<()> {
	const DUE: &str = "removed_at IS NOT NULL AND purged_at IS NULL \
		AND NOT coalesce((SELECT never_purge FROM trash_settings t WHERE t.user_id = owner), $2) \
		AND removed_at + coalesce((SELECT keep_days FROM trash_settings t WHERE t.user_id = owner), $3) * $4 <= $1";
	let gmails: Vec<i64> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("UPDATE gmails SET purged_at = $1 WHERE {DUE} RETURNING id")))
		.bind(to_db(now))
		.bind(keep_by_default)
		.bind(KEEP_DAYS)
		.bind(DAY_US)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("purging gmails")?;
	sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE gmbs SET purged_at = $1 WHERE {DUE}")))
		.bind(to_db(now))
		.bind(keep_by_default)
		.bind(KEEP_DAYS)
		.bind(DAY_US)
		.execute(&mut *conn)
		.await
		.wrap_err("purging GBPs")?;
	for g in gmails {
		for which in [Connection::Imap, Connection::MapsKey] {
			connections::delete(conn, g, which).await?;
		}
	}
	Ok(())
}
