//! Brands' rows (`migrations/…_brands.up.sql`).

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{gbp::Country, ids::BrandId};
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::brands::Brand;

const BRAND: &str = "SELECT b.id, b.gmail_id, b.trade, b.country, b.name, b.site, b.logo_sha256, b.created_at, \
	(SELECT COUNT(*) FROM gmbs g WHERE g.brand_id = b.id AND g.removed_at IS NULL), \
	(SELECT COUNT(*) FROM kit_photos p WHERE p.brand_id = b.id AND p.removed_at IS NULL) \
	FROM brands b JOIN gmails m ON m.id = b.gmail_id";

type Row = (i64, i64, String, String, String, String, Option<Vec<u8>>, i64, i64, i64);

/// The owner's brands not removed, on gmails not removed, by name.
pub async fn brands(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<Brand>> {
	let rows: Vec<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{BRAND} WHERE b.owner = $1 AND b.removed_at IS NULL AND m.removed_at IS NULL ORDER BY lower(b.name), b.id"
	)))
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing brands")?;
	rows.into_iter().map(brand).collect()
}

/// The owner's brand `id`, not removed, on a gmail not removed.
pub async fn brand_of(conn: &mut SqliteConnection, owner: Uuid, id: i64) -> eyre::Result<Option<Brand>> {
	let row: Option<Row> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{BRAND} WHERE b.id = $1 AND b.owner = $2 AND b.removed_at IS NULL AND m.removed_at IS NULL"
	)))
	.bind(id)
	.bind(owner)
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("loading a brand")?;
	row.map(brand).transpose()
}

/// A live brand of the gmail × category named `name` whatever its case, but `except`.
pub async fn named(conn: &mut SqliteConnection, gmail: i64, trade: &str, country: Country, name: &str, except: Option<i64>) -> eyre::Result<bool> {
	let names: Vec<String> = sqlx::query_scalar("SELECT name FROM brands WHERE gmail_id = $1 AND trade = $2 AND country = $3 AND removed_at IS NULL AND id IS NOT $4")
		.bind(gmail)
		.bind(trade)
		.bind(country.as_str())
		.bind(except)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing brands' names")?;
	let folded = name.to_lowercase();
	Ok(names.iter().any(|n| n.to_lowercase() == folded))
}

/// Its name free ([`named`]); the schema refuses a gmail or category not the owner's.
#[expect(clippy::too_many_arguments, reason = "the brand's columns, each named at the call site")]
pub async fn add(conn: &mut SqliteConnection, owner: Uuid, gmail: i64, trade: &str, country: Country, name: &str, site: &BrandId, now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO brands (owner, gmail_id, trade, country, name, site, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id")
		.bind(owner)
		.bind(gmail)
		.bind(trade)
		.bind(country.as_str())
		.bind(name)
		.bind(site.as_str())
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("adding a brand")
}

/// Its name free ([`named`]).
pub async fn edit(conn: &mut SqliteConnection, id: i64, name: Option<&str>, site: Option<&BrandId>) -> eyre::Result<()> {
	sqlx::query("UPDATE brands SET name = coalesce($2, name), site = coalesce($3, site) WHERE id = $1")
		.bind(id)
		.bind(name)
		.bind(site.map(BrandId::as_str))
		.execute(&mut *conn)
		.await
		.wrap_err("editing a brand")?;
	Ok(())
}

/// Brand `id` removed, the removed GBPs giving it up; while no live GBP holds it.
pub async fn remove(conn: &mut SqliteConnection, id: i64, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET brand_id = NULL WHERE brand_id = $1 AND removed_at IS NOT NULL")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("giving a brand up")?;
	sqlx::query("UPDATE brands SET removed_at = $2 WHERE id = $1")
		.bind(id)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("removing a brand")?;
	Ok(())
}

pub async fn set_logo(conn: &mut SqliteConnection, id: i64, png: &[u8], sha256: &[u8; 32]) -> eyre::Result<()> {
	sqlx::query("UPDATE brands SET logo = $2, logo_sha256 = $3 WHERE id = $1")
		.bind(id)
		.bind(png)
		.bind(sha256.as_slice())
		.execute(&mut *conn)
		.await
		.wrap_err("setting a brand's logo")?;
	Ok(())
}

/// Brand `id`'s logo, a PNG; `None` until one is uploaded.
pub async fn logo(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Option<Vec<u8>>> {
	sqlx::query_scalar("SELECT logo FROM brands WHERE id = $1")
		.bind(id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("loading a brand's logo")
}

/// How many of `ids` are the owner's live GBPs of `brand`'s gmail and category (any live GBP of
/// theirs, `None` asked).
pub async fn fitting(conn: &mut SqliteConnection, owner: Uuid, ids: &BTreeSet<i64>, brand: Option<i64>) -> eyre::Result<usize> {
	let n: i64 = sqlx::query_scalar(
		"SELECT COUNT(*) FROM gmbs g WHERE g.id IN (SELECT value FROM json_each($1)) AND g.owner = $2 AND g.removed_at IS NULL \
		 AND ($3 IS NULL OR EXISTS (SELECT 1 FROM brands b WHERE b.id = $3 AND b.gmail_id IS g.gmail_id AND b.trade IS g.trade AND b.country IS g.country))",
	)
	.bind(serde_json::to_string(ids).expect("ids serialize"))
	.bind(owner)
	.bind(brand)
	.fetch_one(&mut *conn)
	.await
	.wrap_err("checking GBPs against a brand")?;
	Ok(usize::try_from(n).expect("a count"))
}

/// Each of `ids` given `brand`; each [`fitting`] it.
pub async fn set_brands(conn: &mut SqliteConnection, ids: &BTreeSet<i64>, brand: Option<i64>) -> eyre::Result<()> {
	sqlx::query("UPDATE gmbs SET brand_id = $2 WHERE id IN (SELECT value FROM json_each($1))")
		.bind(serde_json::to_string(ids).expect("ids serialize"))
		.bind(brand)
		.execute(&mut *conn)
		.await
		.wrap_err("setting GBPs' brand")?;
	Ok(())
}

fn brand((id, gmail_id, trade, country, name, site, logo, created_at, gmbs, photos): Row) -> eyre::Result<Brand> {
	Ok(Brand {
		id,
		gmail_id,
		trade,
		country: country.parse()?,
		name,
		site: BrandId::parse(&site).wrap_err_with(|| format!("brand {id}'s site"))?,
		logo: logo.map(|sha| hex::encode(&sha[..8])),
		created_at: from_db(created_at)?,
		gmbs,
		photos,
	})
}
