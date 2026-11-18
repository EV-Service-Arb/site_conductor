//! The review kit: a category's drafts and its GBPs' brands' photos, offered to a reviewer of one
//! of its GBPs, and a way to list the caller's GBPs by an area. What goes where is the offer's rule
//! (docs/ARCHITECTURE.md, Review kit): no draft and no photo goes twice to one GBP, nor is offered
//! while another view holds it; the less used go first.

use std::io::Cursor;

use eyre::WrapErr;
use image::{DynamicImage, ImageDecoder, ImageReader, codecs::jpeg::JpegEncoder};
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	gbp::Country,
	prices::{PlacesRequest, Spender},
};
use sa_places::LatLng;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	brands::Brand,
	store::{
		areas as areas_db, brands as brands_db,
		gmbs::{self as gmbs_db, GmbId, TradeId},
		kit as db,
	},
	tokens::is_open,
};

/// How long a view holds the drafts it was offered.
pub const LEASE: SignedDuration = SignedDuration::from_mins(15);
/// Drafts one view is offered.
pub const OFFERED: usize = 6;
/// An upload's bytes.
pub const PHOTO_MAX: usize = 10 * 1024 * 1024;
/// The longer side of a photo as kept.
const PHOTO_SIDE: u32 = 2048;
/// Photos a draft is offered with.
const PHOTOS_PER_DRAFT: usize = 2;

/// Where a GBP's QR leads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "KitTarget")]
pub enum Target {
	/// Google's form, no draft.
	Google,
	/// Our page: the drafts to pick from.
	Drafts,
	/// Our page: the top draft, one tap.
	OneTap,
}

#[derive(Clone, Debug)]
pub struct Draft {
	pub id: i64,
	pub text: String,
	/// Copies, every GBP.
	pub uses: i64,
	pub created_at: Timestamp,
}

#[derive(Clone, Debug)]
pub struct Photo {
	pub id: i64,
	pub width: i64,
	pub height: i64,
	/// Saved, every GBP.
	pub uses: i64,
	pub created_at: Timestamp,
}

/// A draft as the offer weighs it for one GBP.
#[derive(Clone, Debug)]
pub struct Ranked {
	pub id: i64,
	pub text: String,
	pub uses: i64,
	/// Copied on this GBP: never offered there again.
	pub went_here: bool,
	/// Held by a live view of this GBP.
	pub held: bool,
	/// Its GBP's brand's photos not saved on it, the less used first.
	pub photos: Vec<RankedPhoto>,
}

#[derive(Clone, Debug)]
pub struct RankedPhoto {
	pub id: i64,
	pub width: i64,
	pub height: i64,
	pub uses: i64,
}

/// What a reviewer's page shows.
#[derive(Clone, Debug)]
pub struct Offer {
	pub target: Target,
	pub place_id: String,
	pub name: String,
	pub lang: &'static str,
	pub review_uri: String,
	pub view: Uuid,
	/// Best first; none for [`Target::Google`], which a page target
	/// becomes once none is left for the GBP.
	pub drafts: Vec<Ranked>,
}

/// A GBP's kit, as its owner sees it.
#[derive(Clone, Debug)]
pub struct Kit {
	pub target: Option<Target>,
	/// Google's form for it; `None` while the kit is off.
	pub review_uri: Option<String>,
	/// Whose photos its drafts are offered with; none without one.
	pub brand: Option<Brand>,
	/// Its category's drafts in the offer's order: the first [`OFFERED`] not gone here nor held
	/// are what a reviewer is shown now.
	pub drafts: Vec<Ranked>,
	pub funnel: Funnel,
}

/// The last 30 days.
#[derive(Clone, Copy, Debug)]
pub struct Funnel {
	pub shown: i64,
	pub scanned: i64,
	pub opened: i64,
	pub copied: i64,
	pub photos_saved: i64,
}

/// What a reviewer's page did.
#[derive(Clone, Debug)]
pub enum Event {
	Copied(i64),
	Saved(Vec<i64>),
}

/// One of the caller's GBPs by an area.
#[derive(Clone, Debug)]
pub struct Nearby {
	pub gmb: crate::store::gmbs::Gmb,
	/// From the area's centre.
	pub distance_m: f64,
	pub target: Option<Target>,
	pub last_shown: Option<Timestamp>,
}

impl Panel {
	/// The owner's category `trade` · `country`, set up.
	async fn category_is(&self, conn: &mut sqlx::SqliteConnection, owner: Uuid, trade: i64, country: Country) -> eyre::Result<bool> {
		Ok(gmbs_db::trade_owner(conn, trade).await? == Some(owner)
			&& gmbs_db::trades(conn, owner)
				.await?
				.into_iter()
				.any(|t| t.id == trade && t.categories.iter().any(|c| c.country == country)))
	}

	/// The category's drafts, not removed.
	pub async fn kit_pool(&self, owner: Uuid, trade: i64, country: Country) -> eyre::Result<Result<Vec<Draft>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		if !self.category_is(&mut conn, owner, trade, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		Ok(Ok(db::drafts(&mut conn, trade, country, None, Timestamp::now())
			.await?
			.into_iter()
			.map(|d| Draft {
				id: d.id,
				text: d.text,
				uses: d.uses,
				created_at: d.created_at,
			})
			.collect()))
	}

	/// A draft of the category.
	pub async fn add_draft(&self, owner: Uuid, trade: i64, country: Country, text: &str, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let text = match draft_text(text) {
			Ok(t) => t,
			Err(e) => return Ok(Err(e)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !self.category_is(&mut tx, owner, trade, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		let id = db::add_draft(&mut tx, trade, country, &text, now).await?;
		tx.commit().await.wrap_err("committing a draft")?;
		Ok(Ok(id))
	}

	/// Draft `id`'s text, only while no copy of it was made: a use is of the text it had.
	pub async fn edit_draft(&self, owner: Uuid, trade: TradeId, country: Country, id: i64, text: &str) -> eyre::Result<Result<(), Refused>> {
		let text = match draft_text(text) {
			Ok(t) => t,
			Err(e) => return Ok(Err(e)),
		};
		let mut tx = self.store.begin_write().await?;
		if !self.category_is(&mut tx, owner, trade.v, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		let Some(uses) = db::draft_uses(&mut tx, trade, country, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if uses > 0 {
			return Ok(Err(Refused::Conflict(format!("copied {uses} times: its text stays; add another draft"))));
		}
		db::set_text(&mut tx, id, &text).await?;
		tx.commit().await.wrap_err("committing a draft's edit")?;
		Ok(Ok(()))
	}

	/// A draft of the category out of every offer; its uses stay.
	pub async fn remove_draft(&self, owner: Uuid, trade: TradeId, country: Country, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !self.category_is(&mut tx, owner, trade.v, country).await? || !db::remove_draft(&mut tx, trade, country, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a draft's removal")?;
		Ok(Ok(()))
	}

	/// The owner's brand `brand`'s photos, not removed, newest first.
	pub async fn brand_photos(&self, owner: Uuid, brand: i64) -> eyre::Result<Result<Vec<Photo>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for photos")?;
		if brands_db::brand_of(&mut conn, owner, brand).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		Ok(Ok(db::photos(&mut conn, brand, None)
			.await?
			.into_iter()
			.map(|p| Photo {
				id: p.id,
				width: p.width,
				height: p.height,
				uses: p.uses,
				created_at: p.created_at,
			})
			.collect()))
	}

	/// A photo of the owner's brand out of every offer; its uses stay.
	pub async fn remove_photo(&self, owner: Uuid, brand: i64, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if brands_db::brand_of(&mut tx, owner, brand).await?.is_none() || !db::remove_photo(&mut tx, brand, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a photo's removal")?;
		Ok(Ok(()))
	}

	/// A photo of the owner's brand: re-encoded as a JPEG (its EXIF, GPS with it, gone; turned
	/// upright), at most `PHOTO_SIDE` on its longer side. The same bytes again are the same photo,
	/// back if removed.
	pub async fn add_photo(&self, owner: Uuid, brand: i64, upload: Vec<u8>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		if upload.len() > PHOTO_MAX {
			return Ok(Err(Refused::TooLarge(format!("a photo is at most {} MiB", PHOTO_MAX >> 20))));
		}
		let sha256: [u8; 32] = Sha256::digest(&upload).into();
		let encoded = tokio::task::spawn_blocking(move || reencode(&upload)).await.wrap_err("re-encoding a photo")?;
		let (jpeg, size) = match encoded {
			Ok(e) => e,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if brands_db::brand_of(&mut tx, owner, brand).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		let id = db::add_photo(&mut tx, brand, &sha256, &jpeg, size, now).await?;
		tx.commit().await.wrap_err("committing a photo")?;
		Ok(Ok(id))
	}

	pub async fn kit_photo(&self, owner: Uuid, brand: i64, id: i64) -> eyre::Result<Result<Vec<u8>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a photo")?;
		if brands_db::brand_of(&mut conn, owner, brand).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		Ok(db::photo_bytes(&mut conn, brand, id).await?.ok_or(Refused::NotFound))
	}

	/// The owner's GBP `gmb`'s kit as of `now`.
	pub async fn kit(&self, owner: Uuid, gmb: i64, now: Timestamp) -> eyre::Result<Result<Kit, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		let Some(g) = gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let target = db::target(&mut conn, gmb).await?;
		let drafts = match (g.trade_id, g.country) {
			(Some(trade), Some(c)) => ranked(&mut conn, TradeId { v: trade }, c.code, &g, now).await?,
			_ => Vec::new(),
		};
		let brand = match g.brand_id {
			Some(b) => Some(brands_db::brand_of(&mut conn, owner, b).await?.expect("a GBP's brand is live while it is")),
			None => None,
		};
		let [shown, scanned, opened, copied, photos_saved] = db::funnel(&mut conn, gmb, now - SignedDuration::from_hours(24 * 30)).await?;
		Ok(Ok(Kit {
			target: target.as_ref().map(|t| t.0),
			review_uri: target.map(|t| t.1),
			brand,
			drafts,
			funnel: Funnel {
				shown,
				scanned,
				opened,
				copied,
				photos_saved,
			},
		}))
	}

	/// The owner's GBP `gmb`'s QR leads to `target`; `None` turns the kit off. On, it needs a
	/// place on Google: its review link is asked once (a Place Details, paid as a lookup).
	pub async fn set_kit_target(&self, by: Spender<PlacesRequest>, gmb: i64, target: Option<Target>, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let owner = by.user();
		let g = {
			let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
			match gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await? {
				Some(g) => g,
				None => return Ok(Err(Refused::NotFound)),
			}
		};
		let link = match (target, &g.place_id) {
			(None, _) => None,
			(Some(_), None) => return Ok(Err(Refused::Conflict(format!("{} has no place on Google yet", g.name)))),
			(Some(t), Some(place)) => {
				let purse = self.purse(by);
				let payer = self.payer(owner, g.gmail_id, Some(&purse)).await?;
				match self.google.review_link(place, &payer).await {
					Ok(uri) => Some((t, uri)),
					Err(e) => return Ok(Err(crate::gmbs::places_error(e)?)),
				}
			}
		};
		let mut tx = self.store.begin_write().await?;
		db::set_target(&mut tx, gmb, link.as_ref().map(|(t, uri)| (*t, uri.as_str())), now).await?;
		tx.commit().await.wrap_err("committing a kit's target")?;
		Ok(Ok(()))
	}

	/// The operator showed the owner's GBP `gmb`'s QR.
	pub async fn kit_shown(&self, owner: Uuid, gmb: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmb_of(&mut tx, Some(owner), gmb).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		db::shown(&mut tx, gmb, now).await?;
		tx.commit().await.wrap_err("committing a QR shown")?;
		Ok(Ok(()))
	}

	/// A reviewer opened GBP `gmb`'s page: a view, holding the drafts it is offered for
	/// [`LEASE`]. `NotFound` while the kit is off.
	pub async fn kit_offer(&self, gmb: i64, qr: bool, now: Timestamp) -> eyre::Result<Result<Offer, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(g) = gmbs_db::gmb_of(&mut tx, None, gmb).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let Some((target, review_uri)) = db::target(&mut tx, gmb).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let place_id = g.place_id.clone().expect("a kit is on only for a GBP with a place");
		let country = g.country.ok_or_else(|| eyre::eyre!("GBP {gmb} has a place and no country"))?.code;
		let drafts = match (target, g.trade_id) {
			(Target::Google, _) | (_, None) => Vec::new(),
			(Target::Drafts | Target::OneTap, Some(trade)) => ranked(&mut tx, TradeId { v: trade }, country, &g, now)
				.await?
				.into_iter()
				.filter(|d| !d.went_here && !d.held)
				.take(if target == Target::OneTap { 1 } else { OFFERED })
				.collect(),
		};
		// none left for this GBP: nothing to copy, so straight to Google's form
		let target = if drafts.is_empty() { Target::Google } else { target };
		let view = Uuid::now_v7();
		db::add_view(&mut tx, view, gmb, qr, now, now + LEASE, &drafts.iter().map(|d| d.id).collect::<Vec<_>>()).await?;
		tx.commit().await.wrap_err("committing a view")?;
		Ok(Ok(Offer {
			target,
			place_id,
			name: g.name,
			lang: country.lang(),
			review_uri,
			view,
			drafts,
		}))
	}

	/// A photo of the brand of the GBP `view` showed, as JPEG.
	pub async fn offered_photo(&self, view: Uuid, photo: i64) -> eyre::Result<Result<Vec<u8>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a photo")?;
		Ok(db::offered_photo(&mut conn, view, photo).await?.ok_or(Refused::NotFound))
	}

	/// What `view`'s page did: a draft copied or photos saved, each a use on its GBP, once.
	/// `Conflict`, nothing recorded, when one went there already.
	pub async fn kit_event(&self, view: Uuid, event: Event, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(gmb) = db::view_gmb(&mut tx, view).await? else {
			return Ok(Err(Refused::NotFound));
		};
		match event {
			Event::Copied(draft) => {
				if !db::offered(&mut tx, view, draft).await? {
					return Ok(Err(Refused::NotFound));
				}
				if !db::use_draft(&mut tx, view, GmbId { v: gmb }, draft, now).await? {
					return Ok(Err(Refused::Conflict(format!("draft {draft} went to this place already"))));
				}
			}
			Event::Saved(photos) => {
				if photos.is_empty() {
					return Ok(Err(Refused::Invalid("no photo saved".to_owned())));
				}
				for p in photos {
					if db::offered_photo(&mut tx, view, p).await?.is_none() {
						return Ok(Err(Refused::NotFound));
					}
					if !db::use_photo(&mut tx, view, GmbId { v: gmb }, p, now).await? {
						return Ok(Err(Refused::Conflict(format!("photo {p} went to this place already"))));
					}
				}
			}
		}
		tx.commit().await.wrap_err("committing a kit's event")?;
		Ok(Ok(()))
	}

	/// The owner's GBPs with a point in area `slug`, nearest its centre first.
	pub async fn area_gmbs(&self, owner: Uuid, slug: &str) -> eyre::Result<Result<Vec<Nearby>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an area")?;
		let Some(area) = areas_db::area(&mut conn, slug).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let mut near = Vec::new();
		for g in gmbs_db::gmbs(&mut conn, owner).await? {
			let Some(at) = g.location.map(|p| p.at) else { continue };
			let distance_m = metres(at, area.center);
			if distance_m > area.radius_m as f64 {
				continue;
			}
			let target = db::target(&mut conn, g.id).await?.map(|t| t.0);
			let last_shown = db::last_shown(&mut conn, g.id).await?;
			near.push(Nearby {
				distance_m,
				target,
				last_shown,
				gmb: g,
			});
		}
		near.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
		Ok(Ok(near))
	}
}

/// The category's drafts for GBP `g` in the offer's order: fewest copies first, then the oldest.
/// The open ones, first in that order, are each given [`PHOTOS_PER_DRAFT`] of its brand's photos
/// not saved on it, the less used first, on round them.
async fn ranked(conn: &mut sqlx::SqliteConnection, trade: TradeId, country: Country, g: &gmbs_db::Gmb, now: Timestamp) -> eyre::Result<Vec<Ranked>> {
	let mut photos: Vec<RankedPhoto> = match g.brand_id {
		Some(brand) => db::photos(conn, brand, Some(g.id))
			.await?
			.into_iter()
			.filter(|p| !p.went_here)
			.map(|p| RankedPhoto {
				id: p.id,
				width: p.width,
				height: p.height,
				uses: p.uses,
			})
			.collect(),
		None => Vec::new(),
	};
	photos.sort_by_key(|p| (p.uses, p.id));
	let mut drafts: Vec<Ranked> = db::drafts(conn, trade.v, country, Some(g.id), now)
		.await?
		.into_iter()
		.map(|d| Ranked {
			id: d.id,
			text: d.text,
			uses: d.uses,
			went_here: d.went_here,
			held: d.held,
			photos: Vec::new(),
		})
		.collect();
	drafts.sort_by_key(|d| (d.went_here || d.held, d.uses, d.id));
	let each = PHOTOS_PER_DRAFT.min(photos.len());
	for (i, d) in drafts.iter_mut().filter(|d| !d.went_here && !d.held).enumerate() {
		d.photos = (0..each).map(|j| photos[(i * PHOTOS_PER_DRAFT + j) % photos.len()].clone()).collect();
	}
	Ok(drafts)
}

fn draft_text(raw: &str) -> Result<String, Refused> {
	let t = raw.trim();
	match t.chars().count() {
		0 => Err(Refused::Invalid("a draft is not empty".to_owned())),
		n if n > 4000 => Err(Refused::Invalid("a draft is at most 4000 characters".to_owned())),
		_ => Ok(t.to_owned()),
	}
}

/// `upload` as a JPEG at quality 90, upright, its longer side at most [`PHOTO_SIDE`]; its size.
/// `Err`: why it is no photo.
fn reencode(upload: &[u8]) -> Result<(Vec<u8>, (u32, u32)), String> {
	let refused = |e: image::ImageError| format!("not a photo this reads (JPEG, PNG or WebP): {e}");
	let mut decoder = ImageReader::new(Cursor::new(upload))
		.with_guessed_format()
		.map_err(|e| e.to_string())?
		.into_decoder()
		.map_err(refused)?;
	let orientation = decoder.orientation().map_err(refused)?;
	let mut img = DynamicImage::from_decoder(decoder).map_err(refused)?;
	img.apply_orientation(orientation);
	if img.width().max(img.height()) > PHOTO_SIDE {
		img = img.resize(PHOTO_SIDE, PHOTO_SIDE, image::imageops::FilterType::Lanczos3);
	}
	let img = DynamicImage::ImageRgb8(img.into_rgb8());
	let mut jpeg = Vec::new();
	img.write_with_encoder(JpegEncoder::new_with_quality(&mut jpeg, 90)).map_err(refused)?;
	Ok((jpeg, (img.width(), img.height())))
}

/// Great-circle metres.
fn metres(a: LatLng, b: LatLng) -> f64 {
	let (la, lb) = (a.lat.to_radians(), b.lat.to_radians());
	let h = ((lb - la) / 2.).sin().powi(2) + la.cos() * lb.cos() * ((b.lng - a.lng).to_radians() / 2.).sin().powi(2);
	2. * 6_371_000. * h.sqrt().asin()
}
