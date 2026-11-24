//! What the conductor's own rules read: whether something was told lately, who is running low, who
//! holds what a listing releases, what the trash is about to lose.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_alerts::Rule;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{
	from_db, to_db,
	trash::{DAY_US, KEEP_DAYS},
};

/// Whether `(rule, event)` was told at or after `since`.
pub(crate) async fn told_since(conn: &mut SqliteConnection, rule: Rule, event: Uuid, since: Timestamp) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM alert_notes WHERE rule = $1 AND event_id = $2 AND noted_at >= $3)")
		.bind(rule.as_ref())
		.bind(event)
		.bind(to_db(since))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a note up")
}

/// What each user was charged since `since`, and the last entry of their ledger that topped it up but
/// its renewal.
pub(crate) async fn paces(conn: &mut SqliteConnection, since: Timestamp) -> eyre::Result<Vec<(Uuid, i64, Option<i64>)>> {
	sqlx::query_as(
		"SELECT user_id, coalesce(sum(iif(kind = 'charge' AND at >= $1, -delta, 0)), 0), max(iif(kind IN ('add', 'set', 'kickback'), id, NULL)) \
		 FROM token_ledger GROUP BY user_id",
	)
	.bind(to_db(since))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading everyone's pace")
}

/// Whose categories were last taken from listing `id`, but `author`'s: their trade and country.
pub(crate) async fn adopters(conn: &mut SqliteConnection, id: i64, author: Uuid) -> eyre::Result<Vec<(Uuid, String, String)>> {
	sqlx::query_as(
		"SELECT c.owner, c.trade, c.country FROM category_lineage c JOIN listing_versions v ON v.id = c.listing_version \
		 WHERE v.listing = $1 AND c.owner <> $2 ORDER BY c.owner",
	)
	.bind(id)
	.bind(author)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a listing's adopters")
}

/// What is purged of one person's trash in one UTC day: how many, one's name, and the first's moment.
pub(crate) struct Purging {
	pub owner: Uuid,
	pub day: i64,
	pub count: i64,
	pub name: String,
	pub first: Timestamp,
}

/// What is purged after `now` and by `by`, of trash kept `kept` days or more.
pub(crate) async fn purging(conn: &mut SqliteConnection, now: Timestamp, by: Timestamp, kept: i64, keep_by_default: bool) -> eyre::Result<Vec<Purging>> {
	type Row = (Uuid, i64, i64, String, i64);
	let rows: Vec<Row> = sqlx::query_as(
		"WITH gone AS ( \
		   SELECT owner, gmail AS name, removed_at FROM gmails WHERE removed_at IS NOT NULL AND purged_at IS NULL \
		   UNION ALL SELECT owner, name, removed_at FROM gmbs WHERE removed_at IS NOT NULL AND purged_at IS NULL \
		 ), due AS ( \
		   SELECT g.owner, g.name, max(g.removed_at, coalesce(t.counted_from, g.removed_at)) + coalesce(t.keep_days, $4) * $5 AS at \
		   FROM gone g LEFT JOIN trash_settings t ON t.user_id = g.owner \
		   WHERE NOT coalesce(t.never_purge, $6) AND coalesce(t.keep_days, $4) >= $3 \
		 ) SELECT owner, at / $5, count(*), min(name), min(at) FROM due WHERE at > $1 AND at <= $2 GROUP BY owner, at / $5",
	)
	.bind(to_db(now))
	.bind(to_db(by))
	.bind(kept)
	.bind(KEEP_DAYS)
	.bind(DAY_US)
	.bind(keep_by_default)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing what the trash is about to lose")?;
	rows.into_iter()
		.map(|(owner, day, count, name, first)| {
			Ok(Purging {
				owner,
				day,
				count,
				name,
				first: from_db(first)?,
			})
		})
		.collect()
}
