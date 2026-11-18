//! The review kit: a category's drafts and its GBPs' brands' photos, offered to a reviewer of one
//! of its GBPs; an owner's kits, named selections of their GBPs, and what each collected. What goes
//! where is the offer's rule (docs/ARCHITECTURE.md, Review kit): no draft and no photo goes twice to
//! one GBP, no draft outside its places, none is offered while another view holds it; the less used
//! go first.

use std::{
	collections::{BTreeMap, BTreeSet},
	io::Cursor,
};

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
	areas::Area,
	brands::Brand,
	store::{
		areas as areas_db, brands as brands_db,
		gmbs::{self as gmbs_db, Gmb, GmbId, TradeId},
		kit::{self as db, Happening},
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
/// How long after a copy a review on its GBP may be of it.
const POSTED_WITHIN: SignedDuration = SignedDuration::from_hours(24 * 14);
/// The share of a draft's words a review carries to be of it.
const POSTED_OVERLAP: f64 = 0.5;
/// What a kit's latest lists.
const LATEST: i64 = 20;

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
	/// Areas it is offered in; none: everywhere.
	pub places: Vec<String>,
	/// The category's keywords its text carries.
	pub keywords: Vec<String>,
	/// Areas of the category's country its text names and `places` lacks.
	pub names: Vec<String>,
	/// Copies, every GBP.
	pub uses: i64,
	pub created_at: Timestamp,
}

#[derive(Clone, Debug)]
pub struct Photo {
	pub id: i64,
	/// What it shows, for the reviewer; `None`: not labelled yet.
	pub shows: Option<String>,
	pub width: i64,
	pub height: i64,
	/// Saved, every GBP.
	pub uses: i64,
	pub created_at: Timestamp,
}

/// A category's drafts, not removed, with what tags them.
#[derive(Clone, Debug)]
pub struct Pool {
	pub drafts: Vec<Draft>,
	/// The category's keywords no draft carries.
	pub unmentioned: Vec<String>,
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
	pub shows: Option<String>,
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

/// What a GBP collected over a period; zero over none.
#[derive(Clone, Copy, Debug, Default)]
pub struct Funnel {
	pub shown: i64,
	pub scanned: i64,
	pub opened: i64,
	pub copied: i64,
	pub photos_saved: i64,
	/// Reviews from a copy on Google.
	pub on_google: i64,
	/// Of those, gone off it.
	pub gone: i64,
	/// Reviews from no copy.
	pub other_reviews: i64,
}

impl From<[i64; 8]> for Funnel {
	fn from([shown, scanned, opened, copied, photos_saved, on_google, gone, other_reviews]: [i64; 8]) -> Self {
		Self {
			shown,
			scanned,
			opened,
			copied,
			photos_saved,
			on_google,
			gone,
			other_reviews,
		}
	}
}

impl std::ops::AddAssign for Funnel {
	fn add_assign(&mut self, o: Self) {
		self.shown += o.shown;
		self.scanned += o.scanned;
		self.opened += o.opened;
		self.copied += o.copied;
		self.photos_saved += o.photos_saved;
		self.on_google += o.on_google;
		self.gone += o.gone;
		self.other_reviews += o.other_reviews;
	}
}

/// An owner's kit as listed.
#[derive(Clone, Debug)]
pub struct KitSummary {
	pub slug: String,
	pub name: String,
	pub area: Option<String>,
	pub members: i64,
}

/// An owner's kit, over a period.
#[derive(Clone, Debug)]
pub struct Scope {
	pub slug: String,
	pub name: String,
	pub area: Option<String>,
	/// In the order they joined.
	pub members: Vec<Member>,
	/// The members' categories.
	pub pools: Vec<PoolSummary>,
	/// Newest first.
	pub latest: Vec<Happened>,
}

#[derive(Clone, Debug)]
pub struct Member {
	pub gmb: Gmb,
	/// Whose photos its drafts carry.
	pub brand: Option<Brand>,
	pub target: Option<Target>,
	pub review_uri: Option<String>,
	/// Since the period's start or its joining, the later.
	pub tally: Funnel,
	pub left: Left,
}

/// What the offer still has for a GBP: drafts not gone there, in its places; its brand's photos not saved there.
#[derive(Clone, Copy, Debug)]
pub struct Left {
	pub drafts: i64,
	pub photos: i64,
}

#[derive(Clone, Debug)]
pub struct PoolSummary {
	pub trade: i64,
	pub country: Country,
	pub name: String,
	pub drafts: i64,
	/// What keeps part of it from being offered as one may expect.
	pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
pub struct Happened {
	pub at: Timestamp,
	pub gmb: i64,
	pub kind: Happening,
	/// The draft, the photo, or the review's rating.
	pub detail: Option<i64>,
}

/// What a kit's editor sets.
#[derive(Clone, Debug)]
pub struct PutKit {
	pub name: String,
	pub area: Option<String>,
	pub gmbs: Vec<i64>,
}

/// What a reviewer's page did.
#[derive(Clone, Debug)]
pub enum Event {
	Copied(i64),
	Saved(Vec<i64>),
}

/// One of the caller's GBPs in or next to an area.
#[derive(Clone, Debug)]
pub struct Nearby {
	pub gmb: Gmb,
	/// From the area's centre.
	pub distance_m: f64,
	/// Within its radius; else within twice it.
	pub inside: bool,
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

	/// The owner's category's keywords; `None`: not theirs, or not set up.
	async fn keywords_of(&self, conn: &mut sqlx::SqliteConnection, owner: Uuid, trade: i64, country: Country) -> eyre::Result<Option<Vec<String>>> {
		Ok(gmbs_db::trades(conn, owner)
			.await?
			.into_iter()
			.filter(|t| t.id == trade)
			.flat_map(|t| t.categories)
			.find(|c| c.country == country)
			.map(|c| c.keywords))
	}

	/// The category's drafts, not removed, with their tags.
	pub async fn kit_pool(&self, owner: Uuid, trade: i64, country: Country) -> eyre::Result<Result<Pool, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		let Some(keywords) = self.keywords_of(&mut conn, owner, trade, country).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let areas = areas_db::areas(&mut conn).await?;
		Ok(Ok(pool(&mut conn, trade, country, &keywords, &areas).await?))
	}

	/// A draft of the category, offered only in `places` (none: everywhere).
	pub async fn add_draft(&self, owner: Uuid, trade: i64, country: Country, text: &str, places: &[String], now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
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
		if let Err(e) = known_areas(&mut tx, places).await? {
			return Ok(Err(e));
		}
		let id = db::add_draft(&mut tx, trade, country, &text, now).await?;
		db::set_draft_places(&mut tx, id, places).await?;
		tx.commit().await.wrap_err("committing a draft")?;
		Ok(Ok(id))
	}

	/// Draft `id`'s text (only while no copy of it was made: a use is of the text it had) and its
	/// places, each as given.
	pub async fn edit_draft(&self, owner: Uuid, trade: TradeId, country: Country, id: i64, text: Option<&str>, places: Option<&[String]>) -> eyre::Result<Result<(), Refused>> {
		let text = match text.map(draft_text).transpose() {
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
		if let Some(text) = text {
			if uses > 0 {
				return Ok(Err(Refused::Conflict(format!("copied {uses} times: its text stays; add another draft"))));
			}
			db::set_text(&mut tx, id, &text).await?;
		}
		if let Some(places) = places {
			if let Err(e) = known_areas(&mut tx, places).await? {
				return Ok(Err(e));
			}
			db::set_draft_places(&mut tx, id, places).await?;
		}
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
				shows: p.shows,
				width: p.width,
				height: p.height,
				uses: p.uses,
				created_at: p.created_at,
			})
			.collect()))
	}

	/// What a photo of the owner's brand shows, its alt and caption on the landing; empty clears it.
	pub async fn label_photo(&self, owner: Uuid, brand: i64, id: i64, shows: &str) -> eyre::Result<Result<(), Refused>> {
		let shows = match shows.trim() {
			"" => None,
			t if t.chars().count() > 60 => return Ok(Err(Refused::Invalid("what a photo shows is at most 60 characters".to_owned()))),
			t => Some(t),
		};
		let mut tx = self.store.begin_write().await?;
		if brands_db::brand_of(&mut tx, owner, brand).await?.is_none() || !db::set_shows(&mut tx, brand, id, shows).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a photo's label")?;
		Ok(Ok(()))
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

	/// The owner's GBP `gmb`'s kit as of `now`, its funnel since `since`.
	pub async fn kit(&self, owner: Uuid, gmb: i64, since: Timestamp, now: Timestamp) -> eyre::Result<Result<Kit, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		let Some(g) = gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let target = db::target(&mut conn, gmb).await?;
		let areas = areas_db::areas(&mut conn).await?;
		let drafts = ranked(&mut conn, &g, &areas, now).await?;
		let brand = match g.brand_id {
			Some(b) => Some(brands_db::brand_of(&mut conn, owner, b).await?.expect("a GBP's brand is live while it is")),
			None => None,
		};
		Ok(Ok(Kit {
			target: target.as_ref().map(|t| t.0),
			review_uri: target.map(|t| t.1),
			brand,
			drafts,
			funnel: db::funnel(&mut conn, gmb, since).await?.into(),
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
		let areas = areas_db::areas(&mut tx).await?;
		let drafts = match target {
			Target::Google => Vec::new(),
			Target::Drafts | Target::OneTap => ranked(&mut tx, &g, &areas, now)
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

	/// The owner's GBPs with a point in area `slug` or within twice its radius, nearest its centre first.
	pub async fn area_gmbs(&self, owner: Uuid, slug: &str) -> eyre::Result<Result<Vec<Nearby>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an area")?;
		let Some(area) = areas_db::area(&mut conn, slug).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let r = area.radius_m as f64;
		let mut near: Vec<Nearby> = gmbs_db::gmbs(&mut conn, owner)
			.await?
			.into_iter()
			.filter_map(|g| {
				let distance_m = metres(g.location.as_ref()?.at, area.center);
				(distance_m <= 2. * r).then_some(Nearby {
					distance_m,
					inside: distance_m <= r,
					gmb: g,
				})
			})
			.collect();
		near.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
		Ok(Ok(near))
	}

	pub async fn kits(&self, owner: Uuid) -> eyre::Result<Vec<KitSummary>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for kits")?;
		Ok(db::kits(&mut conn, owner)
			.await?
			.into_iter()
			.map(|k| KitSummary {
				slug: k.slug,
				name: k.name,
				area: k.area,
				members: k.members,
			})
			.collect())
	}

	/// The owner's kit `slug`, made or over what it was. `Conflict`: a GBP not theirs.
	pub async fn put_kit(&self, owner: Uuid, slug: &str, kit: PutKit, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if !sa_core::ids::is_slug(slug) || slug.contains('_') || slug == "new" {
			return Ok(Err(Refused::Invalid(format!("{slug:?}: a kit's link is 1–64 of a-z, 0-9 and -, not \"new\""))));
		}
		let name = kit.name.trim();
		if name.is_empty() || name.chars().count() > 80 {
			return Ok(Err(Refused::Invalid("a kit's name is 1–80 characters".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		if let Some(area) = &kit.area
			&& areas_db::area(&mut tx, area).await?.is_none()
		{
			return Ok(Err(Refused::Invalid(format!("no area {area}"))));
		}
		for g in &kit.gmbs {
			if gmbs_db::gmb_of(&mut tx, Some(owner), *g).await?.is_none() {
				return Ok(Err(Refused::Conflict(format!("GBP {g} is not yours"))));
			}
		}
		let id = db::put_kit(&mut tx, owner, slug, name, kit.area.as_deref(), now).await?;
		db::set_members(&mut tx, id, &kit.gmbs, now).await?;
		tx.commit().await.wrap_err("committing a kit")?;
		Ok(Ok(()))
	}

	/// Gone with its selection; no draft, photo or use goes with it.
	pub async fn delete_kit(&self, owner: Uuid, slug: &str) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !db::delete_kit(&mut tx, owner, slug).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a kit's deletion")?;
		Ok(Ok(()))
	}

	/// The owner's kit `slug` as of `now`, over the period from `since`.
	pub async fn kit_scope(&self, owner: Uuid, slug: &str, since: Timestamp, now: Timestamp) -> eyre::Result<Result<Scope, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		let Some(k) = db::kit(&mut conn, owner, slug).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let areas = areas_db::areas(&mut conn).await?;
		let trades = gmbs_db::trades(&mut conn, owner).await?;
		let mut members = Vec::new();
		let mut categories: Vec<(i64, Country)> = Vec::new();
		for (gmb, added_at) in db::members(&mut conn, k.id).await? {
			let g = gmbs_db::gmb_of(&mut conn, Some(owner), gmb)
				.await?
				.ok_or_else(|| eyre::eyre!("kit {slug}'s GBP {gmb} is not its owner's"))?;
			if let (Some(t), Some(c)) = (g.trade_id, &g.country) {
				if !categories.contains(&(t, c.code)) {
					categories.push((t, c.code));
				}
			}
			let target = db::target(&mut conn, gmb).await?;
			let ranked = ranked(&mut conn, &g, &areas, now).await?;
			let photos = match g.brand_id {
				Some(brand) => db::photos(&mut conn, brand, Some(gmb)).await?.iter().filter(|p| !p.went_here).count(),
				None => 0,
			};
			let brand = match g.brand_id {
				Some(b) => Some(brands_db::brand_of(&mut conn, owner, b).await?.expect("a GBP's brand is live while it is")),
				None => None,
			};
			members.push(Member {
				brand,
				left: Left {
					drafts: ranked.iter().filter(|d| !d.went_here).count() as i64,
					photos: photos as i64,
				},
				tally: db::funnel(&mut conn, gmb, since.max(added_at)).await?.into(),
				target: target.as_ref().map(|t| t.0),
				review_uri: target.map(|t| t.1),
				gmb: g,
			});
		}
		let mut pools = Vec::new();
		for (trade, country) in categories {
			let t = trades.iter().find(|t| t.id == trade).ok_or_else(|| eyre::eyre!("trade {trade} of a member is not its owner's"))?;
			let Some(c) = t.categories.iter().find(|c| c.country == country) else { continue }; // a GBP's category not set up: nothing to offer
			let pool = pool(&mut conn, trade, country, &c.keywords, &areas).await?;
			let mut warnings = Vec::new();
			let named: BTreeSet<&str> = pool.drafts.iter().flat_map(|d| d.names.iter().map(String::as_str)).collect();
			for n in named {
				warnings.push(format!("names {n}: offered everywhere"));
			}
			if !pool.unmentioned.is_empty() {
				warnings.push(format!("no draft mentions: {}", pool.unmentioned.join(", ")));
			}
			pools.push(PoolSummary {
				trade,
				country,
				name: t.name.clone(),
				drafts: pool.drafts.len() as i64,
				warnings,
			});
		}
		let latest = db::latest(&mut conn, k.id, since, LATEST)
			.await?
			.into_iter()
			.map(|(at, gmb, kind, detail)| Happened { at, gmb, kind, detail })
			.collect();
		Ok(Ok(Scope {
			slug: k.slug,
			name: k.name,
			area: k.area,
			members,
			pools,
			latest,
		}))
	}

	/// Where review_archive's next page of reviews starts.
	pub async fn reviews_after(&self) -> eyre::Result<i64> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the reviews' cursor")?;
		db::posted_cursor(&mut conn).await
	}

	/// A page of review_archive's reviews, asked for after `after`, and the cursor moved to its
	/// `next`, in one write transaction. A review first seen is matched once, greedily by time, to
	/// the oldest copy on its GBP within [`POSTED_WITHIN`] before it whose words it carries; after
	/// that only its `gone_at` follows. How many were taken.
	pub async fn take_reviews(&self, page: sa_core::feeds::ReviewPage, after: i64) -> eyre::Result<usize> {
		let last = page.reviews.last().map_or(after, |r| r.seq);
		eyre::ensure!(page.next == last, "review_archive answered next {} for a page ending at {last}", page.next);
		let mut prev = after;
		for r in &page.reviews {
			eyre::ensure!(r.seq > prev, "review {} after {prev}: seqs ascend past the cursor", r.seq);
			eyre::ensure!((1..=5).contains(&r.rating), "review {} rated {}", r.seq, r.rating);
			prev = r.seq;
		}
		if page.reviews.is_empty() {
			return Ok(0);
		}
		let mut tx = self.store.begin_write().await?;
		eyre::ensure!(db::move_posted_cursor(&mut tx, after..last).await?, "a page of reviews after {after}, the cursor elsewhere");
		for r in &page.reviews {
			if !db::post(&mut tx, r).await? {
				continue;
			}
			let review = words(&r.text);
			let copies = db::unmatched_uses(&mut tx, r.gmb_id, (r.first_seen - POSTED_WITHIN)..=r.first_seen).await?;
			if let Some((draft, _)) = copies.into_iter().find(|(_, text)| overlap(&words(text), &review) >= POSTED_OVERLAP) {
				db::match_posted(&mut tx, &r.review_id, draft).await?;
			}
		}
		tx.commit().await.wrap_err("committing a page of reviews")?;
		Ok(page.reviews.len())
	}
}

/// `places` let a GBP at `at` have it: none, or one whose circle holds `at`.
fn allowed(places: &[String], at: Option<LatLng>, areas: &[Area]) -> bool {
	places.is_empty() || at.is_some_and(|at| areas.iter().any(|a| places.contains(&a.slug) && metres(at, a.center) <= a.radius_m as f64))
}

/// GBP `g`'s category's drafts in its places, in the offer's order: fewest copies first, then the
/// oldest; none without a category. The open ones, first in that order, are each given
/// [`PHOTOS_PER_DRAFT`] of its brand's photos not saved on it, the less used first, on round them.
async fn ranked(conn: &mut sqlx::SqliteConnection, g: &Gmb, areas: &[Area], now: Timestamp) -> eyre::Result<Vec<Ranked>> {
	let (Some(trade), Some(country)) = (g.trade_id, g.country.as_ref().map(|c| c.code)) else {
		return Ok(Vec::new());
	};
	let at = g.location.as_ref().map(|p| p.at);
	let mut places: BTreeMap<i64, Vec<String>> = BTreeMap::new();
	for (d, a) in db::draft_places(conn, trade, country).await? {
		places.entry(d).or_default().push(a);
	}
	let mut photos: Vec<RankedPhoto> = match g.brand_id {
		Some(brand) => db::photos(conn, brand, Some(g.id))
			.await?
			.into_iter()
			.filter(|p| !p.went_here)
			.map(|p| RankedPhoto {
				id: p.id,
				shows: p.shows,
				width: p.width,
				height: p.height,
				uses: p.uses,
			})
			.collect(),
		None => Vec::new(),
	};
	photos.sort_by_key(|p| (p.uses, p.id));
	let mut drafts: Vec<Ranked> = db::drafts(conn, trade, country, Some(g.id), now)
		.await?
		.into_iter()
		.filter(|d| allowed(places.get(&d.id).map_or(&[], Vec::as_slice), at, areas))
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

/// The category's drafts, not removed, with what tags them.
async fn pool(conn: &mut sqlx::SqliteConnection, trade: i64, country: Country, keywords: &[String], areas: &[Area]) -> eyre::Result<Pool> {
	let mut places: BTreeMap<i64, Vec<String>> = BTreeMap::new();
	for (d, a) in db::draft_places(conn, trade, country).await? {
		places.entry(d).or_default().push(a);
	}
	let local: Vec<&Area> = areas.iter().filter(|a| a.country == country.as_str()).collect();
	let drafts: Vec<Draft> = db::drafts(conn, trade, country, None, Timestamp::now())
		.await?
		.into_iter()
		.map(|d| {
			let text = fold(&d.text);
			let places = places.remove(&d.id).unwrap_or_default();
			Draft {
				keywords: keywords.iter().filter(|k| mentions(&text, &fold(k))).cloned().collect(),
				names: local.iter().filter(|a| !places.contains(&a.slug) && mentions(&text, &fold(&a.name))).map(|a| a.name.clone()).collect(),
				places,
				id: d.id,
				text: d.text,
				uses: d.uses,
				created_at: d.created_at,
			}
		})
		.collect();
	let unmentioned = keywords.iter().filter(|k| !drafts.iter().any(|d| d.keywords.contains(k))).cloned().collect();
	Ok(Pool { drafts, unmentioned })
}

/// `Invalid`: one of `slugs` is no area.
async fn known_areas(conn: &mut sqlx::SqliteConnection, slugs: &[String]) -> eyre::Result<Result<(), Refused>> {
	for s in slugs {
		if areas_db::area(conn, s).await?.is_none() {
			return Ok(Err(Refused::Invalid(format!("no area {s}"))));
		}
	}
	Ok(Ok(()))
}

/// Lowercase, the Latin accents off, every run of non-alphanumerics one space, padded by one.
// shortcut: Latin-1 and œ/æ only, enough for the countries served; unicode-normalization once another script is.
fn fold(s: &str) -> String {
	let mut out = String::from(" ");
	for c in s.chars().flat_map(char::to_lowercase) {
		let c = match c {
			'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
			'ç' => "c",
			'è' | 'é' | 'ê' | 'ë' => "e",
			'ì' | 'í' | 'î' | 'ï' => "i",
			'ñ' => "n",
			'ò' | 'ó' | 'ô' | 'õ' | 'ö' => "o",
			'ù' | 'ú' | 'û' | 'ü' => "u",
			'ý' | 'ÿ' => "y",
			'œ' => "oe",
			'æ' => "ae",
			c if c.is_alphanumeric() => {
				out.push(c);
				continue;
			}
			_ => " ",
		};
		if !(c == " " && out.ends_with(' ')) {
			out.push_str(c);
		}
	}
	if !out.ends_with(' ') {
		out.push(' ');
	}
	out
}

/// Folded `text` carries folded `phrase` as whole words.
fn mentions(text: &str, phrase: &str) -> bool {
	phrase.trim() != "" && text.contains(phrase)
}

/// The folded words of at least 3 letters.
fn words(text: &str) -> BTreeSet<String> {
	fold(text).split(' ').filter(|w| w.chars().count() >= 3).map(str::to_owned).collect()
}

/// The share of `draft`'s words `review` carries.
fn overlap(draft: &BTreeSet<String>, review: &BTreeSet<String>) -> f64 {
	if draft.is_empty() {
		return 0.;
	}
	draft.intersection(review).count() as f64 / draft.len() as f64
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