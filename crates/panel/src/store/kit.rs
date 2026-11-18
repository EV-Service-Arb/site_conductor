//! The review kit's rows (`migrations/…_kit.up.sql`).

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::Country;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{
	from_db,
	gmbs::{GmbId, TradeId},
	to_db,
};
use crate::kit::Target;

/// A draft of a category, not removed, with what the offer ranks it by.
#[derive(Clone, Debug)]
pub struct DraftRow {
	pub id: i64,
	pub text: String,
	pub created_at: Timestamp,
	/// Copies, every GBP.
	pub uses: i64,
	/// Copied on the GBP asked about.
	pub went_here: bool,
	/// Offered to another view of that GBP whose lease runs.
	pub held: bool,
}

#[derive(Clone, Debug)]
pub struct PhotoRow {
	pub id: i64,
	pub width: i64,
	pub height: i64,
	/// Saved, every GBP.
	pub uses: i64,
	/// Saved on the GBP asked about.
	pub went_here: bool,
	pub created_at: Timestamp,
}

/// The category's drafts not removed; `gmb`'s use and leases as of `now` (none asked: all false).
pub async fn drafts(conn: &mut SqliteConnection, trade: i64, country: Country, gmb: Option<i64>, now: Timestamp) -> eyre::Result<Vec<DraftRow>> {
	let rows: Vec<(i64, String, i64, i64, bool, bool)> = sqlx::query_as(
		"SELECT d.id, d.text, d.created_at, \
		 (SELECT COUNT(*) FROM kit_uses u WHERE u.draft_id = d.id), \
		 EXISTS (SELECT 1 FROM kit_uses u WHERE u.draft_id = d.id AND u.gmb_id = $3), \
		 EXISTS (SELECT 1 FROM kit_offered o JOIN kit_views v ON v.id = o.view_id WHERE o.draft_id = d.id AND v.gmb_id = $3 AND v.leased_until > $4) \
		 FROM kit_drafts d WHERE d.trade_id = $1 AND d.country = $2 AND d.removed_at IS NULL ORDER BY d.id",
	)
	.bind(trade)
	.bind(country.as_str())
	.bind(gmb)
	.bind(to_db(now))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a category's drafts")?;
	rows.into_iter()
		.map(|(id, text, created_at, uses, went_here, held)| {
			Ok(DraftRow {
				id,
				text,
				created_at: from_db(created_at)?,
				uses,
				went_here,
				held,
			})
		})
		.collect()
}

/// The brand's photos not removed, newest first; whether each was saved on `gmb` (none asked: false).
pub async fn photos(conn: &mut SqliteConnection, brand: i64, gmb: Option<i64>) -> eyre::Result<Vec<PhotoRow>> {
	let rows: Vec<(i64, i64, i64, i64, bool, i64)> = sqlx::query_as(
		"SELECT p.id, p.width, p.height, (SELECT COUNT(*) FROM kit_photo_uses u WHERE u.photo_id = p.id), \
		 EXISTS (SELECT 1 FROM kit_photo_uses u WHERE u.photo_id = p.id AND u.gmb_id = $2), p.created_at \
		 FROM kit_photos p WHERE p.brand_id = $1 AND p.removed_at IS NULL ORDER BY p.id DESC",
	)
	.bind(brand)
	.bind(gmb)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a brand's photos")?;
	rows.into_iter()
		.map(|(id, width, height, uses, went_here, created_at)| {
			Ok(PhotoRow {
				id,
				width,
				height,
				uses,
				went_here,
				created_at: from_db(created_at)?,
			})
		})
		.collect()
}

/// Drafts the category ever had, removed ones too.
pub async fn held(conn: &mut SqliteConnection, trade: i64, country: Option<Country>) -> eyre::Result<i64> {
	sqlx::query_scalar("SELECT COUNT(*) FROM kit_drafts WHERE trade_id = $1 AND ($2 IS NULL OR country = $2)")
		.bind(trade)
		.bind(country.map(Country::as_str))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("counting a category's kit")
}

pub async fn add_draft(conn: &mut SqliteConnection, trade: i64, country: Country, text: &str, now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO kit_drafts (trade_id, country, text, created_at) VALUES ($1, $2, $3, $4) RETURNING id")
		.bind(trade)
		.bind(country.as_str())
		.bind(text)
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("adding a draft")
}

/// Draft `id` of the category, not removed: its copies so far.
pub async fn draft_uses(conn: &mut SqliteConnection, trade: TradeId, country: Country, id: i64) -> eyre::Result<Option<i64>> {
	sqlx::query_scalar(
		"SELECT (SELECT COUNT(*) FROM kit_uses u WHERE u.draft_id = d.id) FROM kit_drafts d \
		 WHERE d.id = $1 AND d.trade_id = $2 AND d.country = $3 AND d.removed_at IS NULL",
	)
	.bind(id)
	.bind(trade.v)
	.bind(country.as_str())
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("loading a draft")
}

pub async fn set_text(conn: &mut SqliteConnection, id: i64, text: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE kit_drafts SET text = $2 WHERE id = $1")
		.bind(id)
		.bind(text)
		.execute(&mut *conn)
		.await
		.wrap_err("editing a draft")?;
	Ok(())
}

/// `false`: no such draft of the category, not removed.
pub async fn remove_draft(conn: &mut SqliteConnection, trade: TradeId, country: Country, id: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE kit_drafts SET removed_at = $4 WHERE id = $1 AND trade_id = $2 AND country = $3 AND removed_at IS NULL")
		.bind(id)
		.bind(trade.v)
		.bind(country.as_str())
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("removing a draft")?;
	Ok(done.rows_affected() == 1)
}

/// `false`: no such photo of the brand, not removed.
pub async fn remove_photo(conn: &mut SqliteConnection, brand: i64, id: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE kit_photos SET removed_at = $3 WHERE id = $1 AND brand_id = $2 AND removed_at IS NULL")
		.bind(id)
		.bind(brand)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("removing a photo")?;
	Ok(done.rows_affected() == 1)
}

/// The photo, or the one of the same bytes back from removal: its id.
pub async fn add_photo(conn: &mut SqliteConnection, brand: i64, sha256: &[u8; 32], jpeg: &[u8], (width, height): (u32, u32), now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar(
		"INSERT INTO kit_photos (brand_id, sha256, bytes, width, height, created_at) VALUES ($1, $2, $3, $4, $5, $6) \
		 ON CONFLICT (brand_id, sha256) DO UPDATE SET removed_at = NULL RETURNING id",
	)
	.bind(brand)
	.bind(sha256.as_slice())
	.bind(jpeg)
	.bind(i64::from(width))
	.bind(i64::from(height))
	.bind(to_db(now))
	.fetch_one(&mut *conn)
	.await
	.wrap_err("adding a photo")
}

/// Photo `id`'s JPEG, of the brand, not removed.
pub async fn photo_bytes(conn: &mut SqliteConnection, brand: i64, id: i64) -> eyre::Result<Option<Vec<u8>>> {
	sqlx::query_scalar("SELECT bytes FROM kit_photos WHERE id = $1 AND brand_id = $2 AND removed_at IS NULL")
		.bind(id)
		.bind(brand)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a photo")
}

/// GBP `gmb`'s target and review link; `None`: the kit is off.
pub async fn target(conn: &mut SqliteConnection, gmb: i64) -> eyre::Result<Option<(Target, String)>> {
	let row: Option<(String, String)> = sqlx::query_as("SELECT target, review_uri FROM kit_targets WHERE gmb_id = $1")
		.bind(gmb)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a kit's target")?;
	row.map(|(t, uri)| Ok((t.parse().wrap_err_with(|| format!("GBP {gmb}'s kit target"))?, uri))).transpose()
}

pub async fn set_target(conn: &mut SqliteConnection, gmb: i64, target: Option<(Target, &str)>, now: Timestamp) -> eyre::Result<()> {
	match target {
		None => sqlx::query("DELETE FROM kit_targets WHERE gmb_id = $1").bind(gmb).execute(&mut *conn).await,
		Some((target, uri)) =>
			sqlx::query(
				"INSERT INTO kit_targets (gmb_id, target, review_uri, set_at) VALUES ($1, $2, $3, $4) \
				 ON CONFLICT (gmb_id) DO UPDATE SET target = excluded.target, review_uri = excluded.review_uri, set_at = excluded.set_at",
			)
			.bind(gmb)
			.bind(<&str>::from(target))
			.bind(uri)
			.bind(to_db(now))
			.execute(&mut *conn)
			.await,
	}
	.wrap_err("setting a kit's target")?;
	Ok(())
}

pub async fn add_view(conn: &mut SqliteConnection, view: Uuid, gmb: i64, qr: bool, now: Timestamp, leased_until: Timestamp, drafts: &[i64]) -> eyre::Result<()> {
	sqlx::query("INSERT INTO kit_views (id, gmb_id, qr, at, leased_until) VALUES ($1, $2, $3, $4, $5)")
		.bind(view)
		.bind(gmb)
		.bind(qr)
		.bind(to_db(now))
		.bind(to_db(leased_until))
		.execute(&mut *conn)
		.await
		.wrap_err("adding a view")?;
	for d in drafts {
		sqlx::query("INSERT INTO kit_offered (view_id, draft_id) VALUES ($1, $2)")
			.bind(view)
			.bind(d)
			.execute(&mut *conn)
			.await
			.wrap_err("offering a draft")?;
	}
	Ok(())
}

/// The GBP view `view` showed.
pub async fn view_gmb(conn: &mut SqliteConnection, view: Uuid) -> eyre::Result<Option<i64>> {
	sqlx::query_scalar("SELECT gmb_id FROM kit_views WHERE id = $1")
		.bind(view)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a view")
}

pub async fn offered(conn: &mut SqliteConnection, view: Uuid, draft: i64) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM kit_offered WHERE view_id = $1 AND draft_id = $2)")
		.bind(view)
		.bind(draft)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("checking an offer")
}

/// Photo `photo`, not removed, of the brand of the GBP `view` showed.
pub async fn offered_photo(conn: &mut SqliteConnection, view: Uuid, photo: i64) -> eyre::Result<Option<Vec<u8>>> {
	sqlx::query_scalar(
		"SELECT p.bytes FROM kit_photos p JOIN gmbs g ON g.brand_id = p.brand_id JOIN kit_views v ON v.gmb_id = g.id \
		 WHERE v.id = $1 AND p.id = $2 AND p.removed_at IS NULL",
	)
	.bind(view)
	.bind(photo)
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("loading an offered photo")
}

/// `false`: that draft or photo went to that GBP already.
pub async fn use_draft(conn: &mut SqliteConnection, view: Uuid, gmb: GmbId, draft: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("INSERT INTO kit_uses (draft_id, gmb_id, view_id, at) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING")
		.bind(draft)
		.bind(gmb.v)
		.bind(view)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording a copy")?;
	Ok(done.rows_affected() == 1)
}

pub async fn use_photo(conn: &mut SqliteConnection, view: Uuid, gmb: GmbId, photo: i64, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("INSERT INTO kit_photo_uses (photo_id, gmb_id, view_id, at) VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING")
		.bind(photo)
		.bind(gmb.v)
		.bind(view)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording a photo saved")?;
	Ok(done.rows_affected() == 1)
}

pub async fn shown(conn: &mut SqliteConnection, gmb: i64, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO kit_shown (gmb_id, at) VALUES ($1, $2)")
		.bind(gmb)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording a QR shown")?;
	Ok(())
}

/// Since `since`: QRs shown, views from a scan, views in all, copies, photos saved.
pub async fn funnel(conn: &mut SqliteConnection, gmb: i64, since: Timestamp) -> eyre::Result<[i64; 5]> {
	let (shown, scanned, opened, copied, photos): (i64, i64, i64, i64, i64) = sqlx::query_as(
		"SELECT (SELECT COUNT(*) FROM kit_shown WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_views WHERE gmb_id = $1 AND at >= $2 AND qr = 1), \
		 (SELECT COUNT(*) FROM kit_views WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_uses WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_photo_uses WHERE gmb_id = $1 AND at >= $2)",
	)
	.bind(gmb)
	.bind(to_db(since))
	.fetch_one(&mut *conn)
	.await
	.wrap_err("counting a kit's funnel")?;
	Ok([shown, scanned, opened, copied, photos])
}

/// When the GBP's QR was last shown.
pub async fn last_shown(conn: &mut SqliteConnection, gmb: i64) -> eyre::Result<Option<Timestamp>> {
	let at: Option<i64> = sqlx::query_scalar("SELECT MAX(at) FROM kit_shown WHERE gmb_id = $1")
		.bind(gmb)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("loading a QR's last showing")?;
	at.map(from_db).transpose()
}
