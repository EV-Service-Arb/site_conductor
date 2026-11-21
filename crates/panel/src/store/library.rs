//! The library's rows: listings, the versions each releases, published studies, and who has seen
//! which listing (`migrations/…_trades_are_google.up.sql`).

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::Country;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::library::{Listing, Published, Released};

/// A listing `$1` (the reader) may see: not withdrawn, theirs, or one they have used.
const VISIBLE: &str = "(l.withdrawn_at IS NULL OR l.author = $1 OR EXISTS (SELECT 1 FROM library_seen s WHERE s.user = $1 AND s.listing = l.id))";

const LISTING: &str = "SELECT l.id, l.trade, l.country, l.author, u.name AS author_name, l.title, l.about, l.forked_from, l.standard_rank, \
	l.created_at, l.withdrawn_at, v.id AS vid, v.n, v.version, v.notes, v.published_at, \
	(SELECT COUNT(*) FROM category_lineage c JOIN listing_versions x ON x.id = c.listing_version WHERE x.listing = l.id) AS used_by \
	FROM listings l JOIN users u ON u.user_id = l.author \
	JOIN listing_versions v ON v.listing = l.id AND v.n = (SELECT max(n) FROM listing_versions WHERE listing = l.id)";

const STUDY: &str = "SELECT p.sha, p.definition, p.title, u.name, p.at, p.withdrawn_at, l.id, l.title, l.trade, l.country, \
	v.id, v.listing, v.n, v.version, v.notes, v.published_at \
	FROM published_studies p JOIN users u ON u.user_id = p.author JOIN listing_versions v ON v.id = p.listing_version JOIN listings l ON l.id = v.listing";

/// A study `$1` (the reader) may see: not withdrawn, theirs, or on a listing they have used.
const STUDY_VISIBLE: &str = "(p.withdrawn_at IS NULL OR p.author = $1 OR EXISTS (SELECT 1 FROM library_seen s WHERE s.user = $1 AND s.listing = l.id))";

type ReleasedRow = (i64, i64, i64, i64, String, i64);

type StudyRow = (String, String, String, String, i64, Option<i64>, i64, String, String, String, i64, i64, i64, i64, String, i64);

#[derive(sqlx::FromRow)]
struct ListingRow {
	id: i64,
	trade: String,
	country: String,
	author: Uuid,
	author_name: String,
	title: String,
	about: String,
	forked_from: Option<i64>,
	standard_rank: Option<i64>,
	created_at: i64,
	withdrawn_at: Option<i64>,
	vid: i64,
	n: i64,
	version: i64,
	notes: String,
	published_at: i64,
	used_by: i64,
}

fn listing(
	ListingRow {
		id,
		trade,
		country,
		author,
		author_name,
		title,
		about,
		forked_from,
		standard_rank,
		created_at,
		withdrawn_at,
		vid,
		n,
		version,
		notes,
		published_at,
		used_by,
	}: ListingRow,
) -> eyre::Result<Listing> {
	Ok(Listing {
		id,
		trade,
		country: country.parse()?,
		author,
		author_name,
		title,
		about,
		forked_from,
		standard_rank,
		created_at: from_db(created_at)?,
		withdrawn_at: withdrawn_at.map(from_db).transpose()?,
		latest: Released {
			id: vid,
			listing: id,
			n,
			version,
			notes,
			published_at: from_db(published_at)?,
		},
		used_by,
	})
}

/// The listings `reader` may see, of `trade` and `country` when given, newest first.
pub(crate) async fn listings(conn: &mut SqliteConnection, reader: Uuid, trade: Option<&str>, country: Option<Country>) -> eyre::Result<Vec<Listing>> {
	let rows: Vec<ListingRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{LISTING} WHERE {VISIBLE} AND ($2 IS NULL OR l.trade = $2) AND ($3 IS NULL OR l.country = $3) ORDER BY l.id DESC"
	)))
	.bind(reader)
	.bind(trade)
	.bind(country.map(|c| c.as_str()))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing listings")?;
	rows.into_iter().map(listing).collect()
}

/// Listing `id`, if `reader` may see it.
pub(crate) async fn listing_of(conn: &mut SqliteConnection, reader: Uuid, id: i64) -> eyre::Result<Option<Listing>> {
	let row: Option<ListingRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{LISTING} WHERE {VISIBLE} AND l.id = $2")))
		.bind(reader)
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a listing")?;
	row.map(listing).transpose()
}

fn released((id, listing, n, version, notes, published_at): ReleasedRow) -> eyre::Result<Released> {
	Ok(Released {
		id,
		listing,
		n,
		version,
		notes,
		published_at: from_db(published_at)?,
	})
}

/// Listing `id`'s versions, oldest first.
pub(crate) async fn versions(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Vec<Released>> {
	let rows: Vec<ReleasedRow> = sqlx::query_as("SELECT id, listing, n, version, notes, published_at FROM listing_versions WHERE listing = $1 ORDER BY n")
		.bind(id)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a listing's versions")?;
	rows.into_iter().map(released).collect()
}

/// Listing version `id`.
pub(crate) async fn release(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Option<Released>> {
	let row: Option<ReleasedRow> = sqlx::query_as("SELECT id, listing, n, version, notes, published_at FROM listing_versions WHERE id = $1")
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a listing version")?;
	row.map(released).transpose()
}

/// The latest listing version releasing category version `version`; none: unreleased.
pub(crate) async fn released_as(conn: &mut SqliteConnection, version: i64) -> eyre::Result<Option<Released>> {
	let row: Option<ReleasedRow> = sqlx::query_as("SELECT id, listing, n, version, notes, published_at FROM listing_versions WHERE version = $1 ORDER BY id DESC LIMIT 1")
		.bind(version)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("finding a version's release")?;
	row.map(released).transpose()
}

/// A new listing, released by [`add_release`] in the same transaction.
#[allow(clippy::too_many_arguments)] // a listing's own columns
pub(crate) async fn add_listing(
	conn: &mut SqliteConnection,
	author: Uuid,
	trade: &str,
	country: Country,
	title: &str,
	about: &str,
	forked_from: Option<i64>,
	now: Timestamp,
) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO listings (trade, country, author, title, about, forked_from, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id")
		.bind(trade)
		.bind(country.as_str())
		.bind(author)
		.bind(title)
		.bind(about)
		.bind(forked_from)
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("adding a listing")
}

/// Listing `listing`'s next version, releasing category version `version`; the schema refuses one of
/// another trade or country, or on a withdrawn listing.
pub(crate) async fn add_release(conn: &mut SqliteConnection, listing: i64, version: i64, notes: &str, now: Timestamp) -> eyre::Result<Released> {
	let row: ReleasedRow = sqlx::query_as(
		"INSERT INTO listing_versions (listing, n, version, notes, published_at) \
		 VALUES ($1, (SELECT COALESCE(max(n), 0) + 1 FROM listing_versions WHERE listing = $1), $2, $3, $4) \
		 RETURNING id, listing, n, version, notes, published_at",
	)
	.bind(listing)
	.bind(version)
	.bind(notes)
	.bind(to_db(now))
	.fetch_one(&mut *conn)
	.await
	.wrap_err("releasing a version")?;
	released(row)
}

/// The author's listing `id` withdrawn now. `false`: none of theirs, or withdrawn already.
pub(crate) async fn withdraw(conn: &mut SqliteConnection, author: Uuid, id: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE listings SET withdrawn_at = $3 WHERE id = $1 AND author = $2 AND withdrawn_at IS NULL")
		.bind(id)
		.bind(author)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("withdrawing a listing")?;
	Ok(done.rows_affected() == 1)
}

/// Listing `id` in the standard library at `rank`, or out of it. `false`: no such listing.
pub(crate) async fn rank(conn: &mut SqliteConnection, id: i64, rank: Option<i64>) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE listings SET standard_rank = $2 WHERE id = $1")
		.bind(id)
		.bind(rank)
		.execute(&mut *conn)
		.await
		.wrap_err("ranking a listing")?;
	Ok(done.rows_affected() == 1)
}

/// `user` used listing `listing` now.
pub(crate) async fn seen(conn: &mut SqliteConnection, user: Uuid, listing: i64, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO library_seen (user, listing, last_at) VALUES ($1, $2, $3) ON CONFLICT (user, listing) DO UPDATE SET last_at = excluded.last_at")
		.bind(user)
		.bind(listing)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording a listing used")?;
	Ok(())
}

/// The listings `user` used.
pub(crate) async fn seen_by(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Vec<i64>> {
	sqlx::query_scalar("SELECT listing FROM library_seen WHERE user = $1")
		.bind(user)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing the listings used")
}

/// A study published on listing version `on`; `false`: published already.
pub(crate) async fn add_study(conn: &mut SqliteConnection, sha: &str, definition: &str, on: i64, author: Uuid, title: &str, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("INSERT INTO published_studies (sha, definition, listing_version, author, title, at) VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (sha) DO NOTHING")
		.bind(sha)
		.bind(definition)
		.bind(on)
		.bind(author)
		.bind(title)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("publishing a study")?;
	Ok(done.rows_affected() == 1)
}

fn study((sha, definition, title, author_name, at, withdrawn_at, listing, listing_title, trade, country, id, l, n, version, notes, published_at): StudyRow) -> eyre::Result<Published> {
	Ok(Published {
		definition: serde_json::from_str(&definition).wrap_err_with(|| format!("published study {sha}'s definition"))?,
		sha,
		title,
		author_name,
		at: from_db(at)?,
		withdrawn_at: withdrawn_at.map(from_db).transpose()?,
		listing_title,
		trade,
		country: country.parse()?,
		on: released((id, l, n, version, notes, published_at))?,
		listing,
	})
}

/// Published study `sha`, if `reader` may see it.
pub(crate) async fn study_of(conn: &mut SqliteConnection, reader: Uuid, sha: &str) -> eyre::Result<Option<Published>> {
	let row: Option<StudyRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{STUDY} WHERE {STUDY_VISIBLE} AND p.sha = $2")))
		.bind(reader)
		.bind(sha)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a published study")?;
	row.map(study).transpose()
}

/// The published studies `reader` may see, of `trade`, `country`, `area` (its slug) and `listing`
/// when given, newest first.
pub(crate) async fn studies(
	conn: &mut SqliteConnection,
	reader: Uuid,
	trade: Option<&str>,
	country: Option<Country>,
	area: Option<&str>,
	listing: Option<i64>,
) -> eyre::Result<Vec<Published>> {
	let rows: Vec<StudyRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{STUDY} WHERE {STUDY_VISIBLE} AND ($2 IS NULL OR l.trade = $2) AND ($3 IS NULL OR l.country = $3) \
		 AND ($4 IS NULL OR p.definition ->> '$.area.slug' = $4) AND ($5 IS NULL OR l.id = $5) ORDER BY p.at DESC"
	)))
	.bind(reader)
	.bind(trade)
	.bind(country.map(|c| c.as_str()))
	.bind(area)
	.bind(listing)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing published studies")?;
	rows.into_iter().map(study).collect()
}

/// The author's published study `sha` withdrawn now. `false`: none of theirs, or withdrawn already.
pub(crate) async fn withdraw_study(conn: &mut SqliteConnection, author: Uuid, sha: &str, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE published_studies SET withdrawn_at = $3 WHERE sha = $1 AND author = $2 AND withdrawn_at IS NULL")
		.bind(sha)
		.bind(author)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("withdrawing a published study")?;
	Ok(done.rows_affected() == 1)
}
