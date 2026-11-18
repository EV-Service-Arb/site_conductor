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
	pub shows: Option<String>,
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
	let rows: Vec<(i64, Option<String>, i64, i64, i64, bool, i64)> = sqlx::query_as(
		"SELECT p.id, p.shows, p.width, p.height, (SELECT COUNT(*) FROM kit_photo_uses u WHERE u.photo_id = p.id), \
		 EXISTS (SELECT 1 FROM kit_photo_uses u WHERE u.photo_id = p.id AND u.gmb_id = $2), p.created_at \
		 FROM kit_photos p WHERE p.brand_id = $1 AND p.removed_at IS NULL ORDER BY p.id DESC",
	)
	.bind(brand)
	.bind(gmb)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a brand's photos")?;
	rows.into_iter()
		.map(|(id, shows, width, height, uses, went_here, created_at)| {
			Ok(PhotoRow {
				id,
				shows,
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

/// Since `since`: QRs shown, views from a scan, views in all, copies, photos saved, reviews from a
/// copy posted, those gone, reviews from no copy.
pub async fn funnel(conn: &mut SqliteConnection, gmb: i64, since: Timestamp) -> eyre::Result<[i64; 8]> {
	let row: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
		"SELECT (SELECT COUNT(*) FROM kit_shown WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_views WHERE gmb_id = $1 AND at >= $2 AND qr = 1), \
		 (SELECT COUNT(*) FROM kit_views WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_uses WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_photo_uses WHERE gmb_id = $1 AND at >= $2), \
		 (SELECT COUNT(*) FROM kit_posted WHERE gmb_id = $1 AND first_seen >= $2 AND draft_id IS NOT NULL), \
		 (SELECT COUNT(*) FROM kit_posted WHERE gmb_id = $1 AND gone_at >= $2 AND draft_id IS NOT NULL), \
		 (SELECT COUNT(*) FROM kit_posted WHERE gmb_id = $1 AND first_seen >= $2 AND draft_id IS NULL)",
	)
	.bind(gmb)
	.bind(to_db(since))
	.fetch_one(&mut *conn)
	.await
	.wrap_err("counting a kit's funnel")?;
	Ok([row.0, row.1, row.2, row.3, row.4, row.5, row.6, row.7])
}

/// What a reviewer's page or the archive saw on a GBP, newest first.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Happening {
	/// The operator showed the QR.
	Shown,
	/// A page opened from the QR.
	Scanned,
	/// A page opened from elsewhere.
	Opened,
	/// A draft copied: `detail` the draft.
	Copied,
	/// A photo saved: `detail` the photo.
	Saved,
	/// A review on Google from a copy: `detail` its rating.
	Posted,
	/// A review on Google from no copy: `detail` its rating.
	Reviewed,
	/// A review from a copy gone off Google: `detail` its rating.
	Gone,
}

#[derive(Clone, Debug)]
pub struct KitRow {
	pub id: i64,
	pub slug: String,
	pub name: String,
	pub area: Option<String>,
	pub members: i64,
}

const KIT: &str = "SELECT k.id, k.slug, k.name, k.area, (SELECT COUNT(*) FROM kit_members m WHERE m.kit_id = k.id) FROM kits k";

pub async fn kits(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<KitRow>> {
	let rows: Vec<(i64, String, String, Option<String>, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{KIT} WHERE k.owner = $1 ORDER BY k.name, k.slug")))
		.bind(owner)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing kits")?;
	Ok(rows.into_iter().map(|(id, slug, name, area, members)| KitRow { id, slug, name, area, members }).collect())
}

pub async fn kit(conn: &mut SqliteConnection, owner: Uuid, slug: &str) -> eyre::Result<Option<KitRow>> {
	let row: Option<(i64, String, String, Option<String>, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(format!("{KIT} WHERE k.owner = $1 AND k.slug = $2")))
		.bind(owner)
		.bind(slug)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a kit")?;
	Ok(row.map(|(id, slug, name, area, members)| KitRow { id, slug, name, area, members }))
}

/// Over what the owner's slug held; its id.
pub async fn put_kit(conn: &mut SqliteConnection, owner: Uuid, slug: &str, name: &str, area: Option<&str>, now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar(
		"INSERT INTO kits (owner, slug, name, area, created_at) VALUES ($1, $2, $3, $4, $5) \
		 ON CONFLICT (owner, slug) DO UPDATE SET name = excluded.name, area = excluded.area RETURNING id",
	)
	.bind(owner)
	.bind(slug)
	.bind(name)
	.bind(area)
	.bind(to_db(now))
	.fetch_one(&mut *conn)
	.await
	.wrap_err("writing a kit")
}

/// The kit holds `gmbs` and nothing else; one it held keeps when it joined.
pub async fn set_members(conn: &mut SqliteConnection, kit: i64, gmbs: &[i64], now: Timestamp) -> eyre::Result<()> {
	let ids = serde_json::to_string(gmbs).expect("ids serialize");
	sqlx::query("DELETE FROM kit_members WHERE kit_id = $1 AND gmb_id NOT IN (SELECT value FROM json_each($2))")
		.bind(kit)
		.bind(&ids)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping a kit's members")?;
	sqlx::query("INSERT INTO kit_members (kit_id, gmb_id, added_at) SELECT $1, value, $3 FROM json_each($2) WHERE true ON CONFLICT DO NOTHING")
		.bind(kit)
		.bind(&ids)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("adding a kit's members")?;
	Ok(())
}

/// `false`: the owner has no such kit.
pub async fn delete_kit(conn: &mut SqliteConnection, owner: Uuid, slug: &str) -> eyre::Result<bool> {
	let done = sqlx::query("DELETE FROM kits WHERE owner = $1 AND slug = $2")
		.bind(owner)
		.bind(slug)
		.execute(&mut *conn)
		.await
		.wrap_err("deleting a kit")?;
	Ok(done.rows_affected() == 1)
}

/// The kit's GBPs and when each joined, in that order.
pub async fn members(conn: &mut SqliteConnection, kit: i64) -> eyre::Result<Vec<(i64, Timestamp)>> {
	let rows: Vec<(i64, i64)> = sqlx::query_as("SELECT gmb_id, added_at FROM kit_members WHERE kit_id = $1 ORDER BY added_at, gmb_id")
		.bind(kit)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a kit's members")?;
	rows.into_iter().map(|(g, at)| Ok((g, from_db(at)?))).collect()
}

/// The newest `limit` of what happened on the kit's GBPs, each since `since` and since it joined.
pub async fn latest(conn: &mut SqliteConnection, kit: i64, since: Timestamp, limit: i64) -> eyre::Result<Vec<(Timestamp, i64, Happening, Option<i64>)>> {
	let rows: Vec<(i64, i64, String, Option<i64>)> = sqlx::query_as(
		"SELECT e.at, e.gmb_id, e.kind, e.detail FROM ( \
		 SELECT at, gmb_id, 'shown' AS kind, NULL AS detail FROM kit_shown \
		 UNION ALL SELECT at, gmb_id, CASE qr WHEN 1 THEN 'scanned' ELSE 'opened' END, NULL FROM kit_views \
		 UNION ALL SELECT at, gmb_id, 'copied', draft_id FROM kit_uses \
		 UNION ALL SELECT at, gmb_id, 'saved', photo_id FROM kit_photo_uses \
		 UNION ALL SELECT first_seen, gmb_id, CASE WHEN draft_id IS NULL THEN 'reviewed' ELSE 'posted' END, rating FROM kit_posted \
		 UNION ALL SELECT gone_at, gmb_id, 'gone', rating FROM kit_posted WHERE gone_at IS NOT NULL AND draft_id IS NOT NULL \
		 ) e JOIN kit_members m ON m.gmb_id = e.gmb_id AND m.kit_id = $1 WHERE e.at >= max($2, m.added_at) ORDER BY e.at DESC LIMIT $3",
	)
	.bind(kit)
	.bind(to_db(since))
	.bind(limit)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a kit's latest")?;
	rows.into_iter()
		.map(|(at, gmb, kind, detail)| Ok((from_db(at)?, gmb, kind.parse().wrap_err_with(|| format!("a kit event {kind:?}"))?, detail)))
		.collect()
}

/// `(draft, area)` of the category's drafts tagged with a place.
pub async fn draft_places(conn: &mut SqliteConnection, trade: i64, country: Country) -> eyre::Result<Vec<(i64, String)>> {
	sqlx::query_as("SELECT p.draft_id, p.area FROM kit_draft_places p JOIN kit_drafts d ON d.id = p.draft_id WHERE d.trade_id = $1 AND d.country = $2 ORDER BY p.area")
		.bind(trade)
		.bind(country.as_str())
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing drafts' places")
}

/// Over the places it had.
pub async fn set_draft_places(conn: &mut SqliteConnection, draft: i64, areas: &[String]) -> eyre::Result<()> {
	sqlx::query("DELETE FROM kit_draft_places WHERE draft_id = $1").bind(draft).execute(&mut *conn).await.wrap_err("clearing places")?;
	sqlx::query("INSERT INTO kit_draft_places (draft_id, area) SELECT $1, value FROM json_each($2)")
		.bind(draft)
		.bind(serde_json::to_string(areas).expect("slugs serialize"))
		.execute(&mut *conn)
		.await
		.wrap_err("tagging places")?;
	Ok(())
}

/// `false`: no such photo of the brand, not removed.
pub async fn set_shows(conn: &mut SqliteConnection, brand: i64, photo: i64, shows: Option<&str>) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE kit_photos SET shows = $3 WHERE id = $1 AND brand_id = $2 AND removed_at IS NULL")
		.bind(photo)
		.bind(brand)
		.bind(shows)
		.execute(&mut *conn)
		.await
		.wrap_err("labelling a photo")?;
	Ok(done.rows_affected() == 1)
}

/// Where review_archive's next page of reviews starts.
pub async fn posted_cursor(conn: &mut SqliteConnection) -> eyre::Result<i64> {
	sqlx::query_scalar("SELECT after FROM kit_posted_cursor WHERE id = 1")
		.fetch_one(&mut *conn)
		.await
		.wrap_err("reading the reviews' cursor")
}

/// `moved.start` → `moved.end`; `false` when the cursor is not at the start.
pub async fn move_posted_cursor(conn: &mut SqliteConnection, moved: std::ops::Range<i64>) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE kit_posted_cursor SET after = $2 WHERE id = 1 AND after = $1")
		.bind(moved.start)
		.bind(moved.end)
		.execute(&mut *conn)
		.await
		.wrap_err("moving the reviews' cursor")?;
	Ok(done.rows_affected() == 1)
}

/// `false`: a review already known; only its `gone_at` follows.
pub async fn post(conn: &mut SqliteConnection, r: &sa_core::feeds::Review) -> eyre::Result<bool> {
	let known: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM kit_posted WHERE review_id = $1)")
		.bind(&r.review_id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a review up")?;
	if known {
		sqlx::query("UPDATE kit_posted SET gone_at = $2 WHERE review_id = $1")
			.bind(&r.review_id)
			.bind(r.gone_at.map(to_db))
			.execute(&mut *conn)
			.await
			.wrap_err("following a review")?;
		return Ok(false);
	}
	sqlx::query("INSERT INTO kit_posted (review_id, gmb_id, rating, first_seen, gone_at) VALUES ($1, $2, $3, $4, $5)")
		.bind(&r.review_id)
		.bind(r.gmb_id)
		.bind(i64::from(r.rating))
		.bind(to_db(r.first_seen))
		.bind(r.gone_at.map(to_db))
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("taking review {} of GBP {}", r.review_id, r.gmb_id))?;
	Ok(true)
}

/// Copies on `gmb` made in `window` that no review matched yet, oldest first: draft and its text.
pub async fn unmatched_uses(conn: &mut SqliteConnection, gmb: i64, window: std::ops::RangeInclusive<Timestamp>) -> eyre::Result<Vec<(i64, String)>> {
	sqlx::query_as(
		"SELECT u.draft_id, d.text FROM kit_uses u JOIN kit_drafts d ON d.id = u.draft_id WHERE u.gmb_id = $1 AND u.at BETWEEN $2 AND $3 \
		 AND NOT EXISTS (SELECT 1 FROM kit_posted p WHERE p.gmb_id = u.gmb_id AND p.draft_id = u.draft_id) ORDER BY u.at, u.draft_id",
	)
	.bind(gmb)
	.bind(to_db(*window.start()))
	.bind(to_db(*window.end()))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing unmatched copies")
}

pub async fn match_posted(conn: &mut SqliteConnection, review: &str, draft: i64) -> eyre::Result<()> {
	sqlx::query("UPDATE kit_posted SET draft_id = $2 WHERE review_id = $1 AND draft_id IS NULL")
		.bind(review)
		.bind(draft)
		.execute(&mut *conn)
		.await
		.wrap_err("matching a review to its copy")?;
	Ok(())
}