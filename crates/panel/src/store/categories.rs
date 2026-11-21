//! A person's categories: the versions they are written in, never changed, and the moves that point
//! each category's head at one (`migrations/…_trades_are_google.up.sql`).

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::Country;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::categories::{Body, Key, Move, Version, Why};

/// A category's head, and the listing version its lineage last took it from.
pub(crate) struct Row {
	pub trade: String,
	pub country: Country,
	pub head: i64,
	pub listing_version: Option<i64>,
}

/// Version `id`.
pub(crate) async fn version(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Option<Version>> {
	let row: Option<(i64, String, String, String, Uuid, i64)> = sqlx::query_as("SELECT id, trade, country, body, author, at FROM category_versions WHERE id = $1")
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a category version")?;
	let Some((id, trade, country, body, author, at)) = row else { return Ok(None) };
	Ok(Some(Version {
		id,
		trade,
		country: country.parse()?,
		body: serde_json::from_str(&body).wrap_err_with(|| format!("category version {id}'s body"))?,
		author,
		at: from_db(at)?,
	}))
}

/// A new version of `key`'s trade and country, by its owner.
pub(crate) async fn add_version(conn: &mut SqliteConnection, key: Key, body: &Body, now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO category_versions (trade, country, body, author, at) VALUES ($1, $2, $3, $4, $5) RETURNING id")
		.bind(key.trade.id)
		.bind(key.country.as_str())
		.bind(serde_json::to_string(body).expect("a body serializes"))
		.bind(key.owner)
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("adding a category version")
}

/// The owner's categories, by trade and country.
pub(crate) async fn rows(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Row>> {
	let rows: Vec<(String, String, i64, Option<i64>)> = sqlx::query_as("SELECT trade, country, head, listing_version FROM category_lineage WHERE owner = $1 ORDER BY trade, country")
		.bind(owner)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing categories")?;
	rows.into_iter()
		.map(|(trade, country, head, listing_version)| {
			Ok(Row {
				trade,
				country: country.parse()?,
				head,
				listing_version,
			})
		})
		.collect()
}

/// `key`'s head; none while it is not set up.
pub(crate) async fn head(conn: &mut SqliteConnection, key: Key) -> eyre::Result<Option<i64>> {
	sqlx::query_scalar("SELECT head FROM categories WHERE owner = $1 AND trade = $2 AND country = $3")
		.bind(key.owner)
		.bind(key.trade.id)
		.bind(key.country.as_str())
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a category's head")
}

/// `key`'s head moved from `from` (none: created) to `to`, recorded first: the schema refuses a
/// head that moves by anything but its history's last move.
pub(crate) async fn moved(conn: &mut SqliteConnection, key: Key, why: Why, from: Option<i64>, to: i64, listing_version: Option<i64>, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO category_moves (owner, trade, country, why, from_version, to_version, listing_version, at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)")
		.bind(key.owner)
		.bind(key.trade.id)
		.bind(key.country.as_str())
		.bind(<&str>::from(why))
		.bind(from)
		.bind(to)
		.bind(listing_version)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording a category's move")?;
	let done = match from {
		None => sqlx::query("INSERT INTO categories (owner, trade, country, head) VALUES ($1, $2, $3, $4)"),
		Some(_) => sqlx::query("UPDATE categories SET head = $4 WHERE owner = $1 AND trade = $2 AND country = $3"),
	}
	.bind(key.owner)
	.bind(key.trade.id)
	.bind(key.country.as_str())
	.bind(to)
	.execute(&mut *conn)
	.await
	.wrap_err("moving a category's head")?;
	assert_eq!(done.rows_affected(), 1, "a move is of one category");
	Ok(())
}

/// `key`'s moves since it was last created, oldest first.
pub(crate) async fn history(conn: &mut SqliteConnection, key: Key) -> eyre::Result<Vec<Move>> {
	let rows: Vec<(i64, String, Option<i64>, i64, Option<i64>, i64)> = sqlx::query_as(
		"SELECT m.id, m.why, m.from_version, m.to_version, m.listing_version, m.at FROM category_moves m \
		 WHERE m.owner = $1 AND m.trade = $2 AND m.country = $3 AND m.id >= (SELECT max(s.id) FROM category_moves s \
		 WHERE s.owner = $1 AND s.trade = $2 AND s.country = $3 AND s.from_version IS NULL) ORDER BY m.id",
	)
	.bind(key.owner)
	.bind(key.trade.id)
	.bind(key.country.as_str())
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a category's moves")?;
	rows.into_iter()
		.map(|(id, why, from, to, listing_version, at)| {
			Ok(Move {
				id,
				why: why.parse().wrap_err_with(|| format!("move {id}'s why"))?,
				from,
				to,
				listing_version,
				at: from_db(at)?,
			})
		})
		.collect()
}

/// One of `owner`'s moves went to version `id`.
pub(crate) async fn ours(conn: &mut SqliteConnection, owner: Uuid, id: i64) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM category_moves WHERE owner = $1 AND to_version = $2)")
		.bind(owner)
		.bind(id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("reading whose a version is")
}

/// `reader` may read version `id`: one of their moves went to it, or a listing released it.
pub(crate) async fn readable(conn: &mut SqliteConnection, reader: Uuid, id: i64) -> eyre::Result<bool> {
	sqlx::query_scalar(
		"SELECT EXISTS (SELECT 1 FROM category_moves WHERE owner = $1 AND to_version = $2) \
		 OR EXISTS (SELECT 1 FROM listing_versions WHERE version = $2)",
	)
	.bind(reader)
	.bind(id)
	.fetch_one(&mut *conn)
	.await
	.wrap_err("reading who may read a version")
}

/// `key` taken away; its history stays. `false`: not set up.
pub(crate) async fn delete(conn: &mut SqliteConnection, key: Key) -> eyre::Result<bool> {
	let done = sqlx::query("DELETE FROM categories WHERE owner = $1 AND trade = $2 AND country = $3")
		.bind(key.owner)
		.bind(key.trade.id)
		.bind(key.country.as_str())
		.execute(&mut *conn)
		.await
		.wrap_err("deleting a category")?;
	Ok(done.rows_affected() == 1)
}

/// What holds `key`: its live GBPs, its review drafts (removed ones too: their uses are kept), its
/// brands (removed ones too: their photos' uses are).
pub(crate) async fn holders(conn: &mut SqliteConnection, key: Key) -> eyre::Result<(i64, i64, i64)> {
	sqlx::query_as(
		"SELECT (SELECT COUNT(*) FROM gmbs WHERE owner = $1 AND trade = $2 AND country = $3 AND removed_at IS NULL), \
		 (SELECT COUNT(*) FROM kit_drafts WHERE owner = $1 AND trade = $2 AND country = $3), \
		 (SELECT COUNT(*) FROM brands WHERE owner = $1 AND trade = $2 AND country = $3)",
	)
	.bind(key.owner)
	.bind(key.trade.id)
	.bind(key.country.as_str())
	.fetch_one(&mut *conn)
	.await
	.wrap_err("counting what holds a category")
}
