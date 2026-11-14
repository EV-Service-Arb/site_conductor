//! The review kit: a category's drafts and photos, offered to a reviewer of one of its GBPs, and
//! the areas an operator shows QRs in. What goes where is the offer's rule
//! (docs/ARCHITECTURE.md, Review kit): no draft and no photo goes twice to one GBP, nor is offered
//! while another view holds it; the less used go first.

use std::{
	collections::{BTreeMap, BTreeSet},
	io::Cursor,
};

use eyre::WrapErr;
use image::{DynamicImage, ImageDecoder, ImageReader, codecs::jpeg::JpegEncoder};
use jiff::{SignedDuration, Timestamp};
use sa_core::gbp::Country;
use sa_places::{LatLng, Locality, Suggestion, Viewport};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	store::{
		gmbs as gmbs_db,
		kit::{self as db, Removable},
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
/// Photos a draft selects.
const PHOTOS_PER_DRAFT: usize = 10;

/// Where a GBP's QR leads.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, strum::EnumString, strum::IntoStaticStr, serde::Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[ts(rename = "KitTarget")]
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
	/// Its photos, in its order.
	pub photos: Vec<i64>,
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

/// A category's drafts and photos, not removed.
#[derive(Clone, Debug)]
pub struct Pool {
	pub drafts: Vec<Draft>,
	pub photos: Vec<Photo>,
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
	/// Its photos, the ones that went here last.
	pub photos: Vec<RankedPhoto>,
}

#[derive(Clone, Debug)]
pub struct RankedPhoto {
	pub id: i64,
	pub width: i64,
	pub height: i64,
	pub uses: i64,
	pub went_here: bool,
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
	/// Best first, their photos not gone here; none for [`Target::Google`], which a page target
	/// becomes once none is left for the GBP.
	pub drafts: Vec<Ranked>,
}

/// A GBP's kit, as its owner sees it.
#[derive(Clone, Debug)]
pub struct Kit {
	pub target: Option<Target>,
	/// Google's form for it; `None` while the kit is off.
	pub review_uri: Option<String>,
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

/// A place an operator walks: `/<slug>/` lists their GBPs in it and next to it.
#[derive(Clone, Debug)]
pub struct Area {
	pub slug: String,
	pub name: String,
	pub center: LatLng,
	pub shape: Shape,
	/// How far outside it a GBP is still next to it.
	pub margin_m: i64,
	pub created_by: Uuid,
	pub created_at: Timestamp,
}

#[derive(Clone, Debug)]
pub enum Shape {
	/// Google's locality: inside its viewport is in.
	City {
		place_id: String,
		viewport: Viewport,
	},
	Circle {
		radius_m: i64,
	},
}

/// An area as asked for: a city by its place, or a circle drawn by hand.
#[derive(Clone, Debug)]
pub enum NewArea {
	City { place_id: String, margin_m: i64 },
	Circle { name: String, center: LatLng, radius_m: i64, margin_m: i64 },
}

/// One of the caller's GBPs by an area.
#[derive(Clone, Debug)]
pub struct Nearby {
	pub gmb: crate::store::gmbs::Gmb,
	/// From the area's centre.
	pub distance_m: f64,
	/// `false`: within the margin outside it.
	pub inside: bool,
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

	pub async fn kit_pool(&self, owner: Uuid, trade: i64, country: Country) -> eyre::Result<Result<Pool, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		if !self.category_is(&mut conn, owner, trade, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		let mut photos_of: BTreeMap<i64, Vec<i64>> = BTreeMap::new();
		for p in db::draft_photos(&mut conn, trade, country, None).await? {
			photos_of.entry(p.draft_id).or_default().push(p.photo_id);
		}
		let drafts = db::drafts(&mut conn, trade, country, None, Timestamp::now())
			.await?
			.into_iter()
			.map(|d| Draft {
				photos: photos_of.remove(&d.id).unwrap_or_default(),
				id: d.id,
				text: d.text,
				uses: d.uses,
				created_at: d.created_at,
			})
			.collect();
		let photos = db::photos(&mut conn, trade, country)
			.await?
			.into_iter()
			.map(|p| Photo {
				id: p.id,
				width: p.width,
				height: p.height,
				uses: p.uses,
				created_at: p.created_at,
			})
			.collect();
		Ok(Ok(Pool { drafts, photos }))
	}

	/// A draft of the category, selecting `photos` of it.
	pub async fn add_draft(&self, owner: Uuid, trade: i64, country: Country, text: &str, photos: &[i64], now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
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
		if let Err(e) = selection(&mut tx, trade, country, photos).await? {
			return Ok(Err(e));
		}
		let id = db::add_draft(&mut tx, trade, country, &text, now).await?;
		db::select(&mut tx, id, photos).await?;
		tx.commit().await.wrap_err("committing a draft")?;
		Ok(Ok(id))
	}

	/// Draft `id`'s text (only while no copy of it was made: a use is of the text it had) and its
	/// photos, each as given.
	pub async fn edit_draft(&self, owner: Uuid, trade: i64, country: Country, id: i64, text: Option<&str>, photos: Option<&[i64]>) -> eyre::Result<Result<(), Refused>> {
		let text = match text.map(draft_text).transpose() {
			Ok(t) => t,
			Err(e) => return Ok(Err(e)),
		};
		let mut tx = self.store.begin_write().await?;
		if !self.category_is(&mut tx, owner, trade, country).await? {
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
		if let Some(photos) = photos {
			if let Err(e) = selection(&mut tx, trade, country, photos).await? {
				return Ok(Err(e));
			}
			db::select(&mut tx, id, photos).await?;
		}
		tx.commit().await.wrap_err("committing a draft's edit")?;
		Ok(Ok(()))
	}

	/// A draft or photo of the category out of every offer; its uses stay.
	pub async fn remove_from_kit(&self, owner: Uuid, trade: i64, country: Country, what: Removable, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !self.category_is(&mut tx, owner, trade, country).await? || !db::remove(&mut tx, what, trade, country, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a removal from a kit")?;
		Ok(Ok(()))
	}

	/// A photo of the category: re-encoded as a JPEG (its EXIF, GPS with it, gone; turned upright),
	/// at most [`PHOTO_SIDE`] on its longer side. The same bytes again are the same photo, back
	/// if removed.
	pub async fn add_photo(&self, owner: Uuid, trade: i64, country: Country, upload: Vec<u8>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
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
		if !self.category_is(&mut tx, owner, trade, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		let id = db::add_photo(&mut tx, trade, country, &sha256, &jpeg, size, now).await?;
		tx.commit().await.wrap_err("committing a photo")?;
		Ok(Ok(id))
	}

	pub async fn kit_photo(&self, owner: Uuid, trade: i64, country: Country, id: i64) -> eyre::Result<Result<Vec<u8>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a photo")?;
		if !self.category_is(&mut conn, owner, trade, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		Ok(db::photo_bytes(&mut conn, trade, country, id).await?.ok_or(Refused::NotFound))
	}

	/// The owner's GBP `gmb`'s kit as of `now`.
	pub async fn kit(&self, owner: Uuid, gmb: i64, now: Timestamp) -> eyre::Result<Result<Kit, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a kit")?;
		let Some(g) = gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let target = db::target(&mut conn, gmb).await?;
		let drafts = match (g.trade_id, g.country) {
			(Some(trade), Some(c)) => ranked(&mut conn, trade, c.code, gmb, now).await?,
			_ => Vec::new(),
		};
		let [shown, scanned, opened, copied, photos_saved] = db::funnel(&mut conn, gmb, now - SignedDuration::from_hours(24 * 30)).await?;
		Ok(Ok(Kit {
			target: target.as_ref().map(|t| t.0),
			review_uri: target.map(|t| t.1),
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
	pub async fn set_kit_target(&self, owner: Uuid, gmb: i64, target: Option<Target>, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
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
				let purse = self.purse(owner);
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
			(Target::Drafts | Target::OneTap, Some(trade)) => ranked(&mut tx, trade, country, gmb, now)
				.await?
				.into_iter()
				.filter(|d| !d.went_here && !d.held)
				.take(if target == Target::OneTap { 1 } else { OFFERED })
				.map(|d| Ranked {
					photos: d.photos.into_iter().filter(|p| !p.went_here).collect(),
					..d
				})
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

	/// A photo offered to `view`, as JPEG.
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
				if !db::use_draft(&mut tx, view, gmb, draft, now).await? {
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
					if !db::use_photo(&mut tx, view, gmb, p, now).await? {
						return Ok(Err(Refused::Conflict(format!("photo {p} went to this place already"))));
					}
				}
			}
		}
		tx.commit().await.wrap_err("committing a kit's event")?;
		Ok(Ok(()))
	}

	pub async fn kit_area(&self, slug: &str) -> eyre::Result<Option<Area>> {
		db::area(&mut *self.store.pool().acquire().await.wrap_err("a connection for an area")?, slug).await
	}

	pub async fn kit_areas(&self) -> eyre::Result<Vec<Area>> {
		db::areas(&mut *self.store.pool().acquire().await.wrap_err("a connection for areas")?).await
	}

	/// Area `slug` drawn as `new` asks, over what it was if `user` drew it; a city's viewport
	/// asked of Places for them.
	pub async fn put_kit_area(&self, user: Uuid, slug: &str, new: NewArea, now: Timestamp) -> eyre::Result<Result<Area, Refused>> {
		if !sa_core::ids::is_slug(slug) || slug.contains('_') {
			return Ok(Err(Refused::Invalid(format!("{slug:?}: an area's link is lowercase letters, digits and dashes"))));
		}
		let (name, center, shape, margin_m) = match new {
			NewArea::City { place_id, margin_m } => {
				let l = match self.locality(user, &place_id).await? {
					Ok(l) => l,
					Err(e) => return Ok(Err(e)),
				};
				(
					l.name,
					l.location,
					Shape::City {
						place_id: l.place_id,
						viewport: l.viewport,
					},
					margin_m,
				)
			}
			NewArea::Circle { name, center, radius_m, margin_m } => {
				let Some(name) = Some(name.trim().to_owned()).filter(|n| !n.is_empty()) else {
					return Ok(Err(Refused::Invalid("an area has a name".to_owned())));
				};
				if !(1..=100_000).contains(&radius_m) {
					return Ok(Err(Refused::Invalid("a circle's radius is 1 m to 100 km".to_owned())));
				}
				(name, center, Shape::Circle { radius_m }, margin_m)
			}
		};
		if !(0..=50_000).contains(&margin_m) {
			return Ok(Err(Refused::Invalid("the margin is 0 to 50 km".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		if let Some(was) = db::area(&mut tx, slug).await?
			&& was.created_by != user
		{
			return Ok(Err(Refused::Conflict(format!("/{slug}/ is {} already", was.name))));
		}
		let area = Area {
			slug: slug.to_owned(),
			name,
			center,
			shape,
			margin_m,
			created_by: user,
			created_at: now,
		};
		db::put_area(&mut tx, &area).await?;
		tx.commit().await.wrap_err("committing an area")?;
		Ok(Ok(area))
	}

	/// The owner's GBPs with a point in area `slug` or within its margin, nearest its centre first.
	pub async fn area_gmbs(&self, owner: Uuid, slug: &str) -> eyre::Result<Result<Vec<Nearby>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an area")?;
		let Some(area) = db::area(&mut conn, slug).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let mut near = Vec::new();
		for g in gmbs_db::gmbs(&mut conn, owner).await? {
			let Some(at) = g.location.map(|p| p.at) else { continue };
			let outside = match &area.shape {
				Shape::City { viewport, .. } => metres(at, clamp(at, viewport)),
				Shape::Circle { radius_m } => (metres(at, area.center) - *radius_m as f64).max(0.),
			};
			if outside > area.margin_m as f64 {
				continue;
			}
			let target = db::target(&mut conn, g.id).await?.map(|t| t.0);
			let last_shown = db::last_shown(&mut conn, g.id).await?;
			near.push(Nearby {
				distance_m: metres(at, area.center),
				inside: outside == 0.,
				target,
				last_shown,
				gmb: g,
			});
		}
		near.sort_by(|a, b| a.distance_m.total_cmp(&b.distance_m));
		Ok(Ok(near))
	}

	/// Cities and towns `input` may be heading for, in the countries served; paid by `user`.
	pub async fn cities(&self, user: Uuid, input: &str) -> eyre::Result<Result<Vec<Suggestion>, Refused>> {
		let purse = self.purse(user);
		let payer = self.payer(user, None, Some(&purse)).await?;
		let regions: Vec<&str> = <Country as strum::VariantArray>::VARIANTS.iter().map(|c| c.as_str()).collect();
		match self.google.suggest(input, &regions, &["(cities)"], &payer).await {
			Ok(found) => Ok(Ok(found)),
			Err(e) => Ok(Err(crate::gmbs::places_error(e)?)),
		}
	}

	pub async fn locality(&self, user: Uuid, place_id: &str) -> eyre::Result<Result<Locality, Refused>> {
		let purse = self.purse(user);
		let payer = self.payer(user, None, Some(&purse)).await?;
		match self.google.locality(place_id, &payer).await {
			Ok(l) => Ok(Ok(l)),
			Err(e) => Ok(Err(crate::gmbs::places_error(e)?)),
		}
	}

	/// The city or town `at` lies in; `None` outside any.
	pub async fn locality_at(&self, user: Uuid, at: LatLng) -> eyre::Result<Result<Option<Locality>, Refused>> {
		let purse = self.purse(user);
		let payer = self.payer(user, None, Some(&purse)).await?;
		match self.google.locality_at(at, &payer).await {
			Ok(l) => Ok(Ok(l)),
			Err(e) => Ok(Err(crate::gmbs::places_error(e)?)),
		}
	}
}

/// The category's drafts for GBP `gmb` in the offer's order: fewest copies first, then the fewest
/// saves of its photos still offerable here, then the oldest.
async fn ranked(conn: &mut sqlx::SqliteConnection, trade: i64, country: Country, gmb: i64, now: Timestamp) -> eyre::Result<Vec<Ranked>> {
	let mut photos_of: BTreeMap<i64, Vec<RankedPhoto>> = BTreeMap::new();
	for p in db::draft_photos(conn, trade, country, Some(gmb)).await? {
		photos_of.entry(p.draft_id).or_default().push(RankedPhoto {
			id: p.photo_id,
			width: p.width,
			height: p.height,
			uses: p.uses,
			went_here: p.went_here,
		});
	}
	let mut drafts: Vec<Ranked> = db::drafts(conn, trade, country, Some(gmb), now)
		.await?
		.into_iter()
		.map(|d| {
			let mut photos = photos_of.remove(&d.id).unwrap_or_default();
			photos.sort_by_key(|p| (p.went_here, p.uses));
			Ranked {
				id: d.id,
				text: d.text,
				uses: d.uses,
				went_here: d.went_here,
				held: d.held,
				photos,
			}
		})
		.collect();
	drafts.sort_by_key(|d| {
		let photo_uses: i64 = d.photos.iter().filter(|p| !p.went_here).map(|p| p.uses).sum();
		(d.went_here || d.held, d.uses, photo_uses, d.id)
	});
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

/// `photos` are the category's, live, none twice, at most [`PHOTOS_PER_DRAFT`].
async fn selection(conn: &mut sqlx::SqliteConnection, trade: i64, country: Country, photos: &[i64]) -> eyre::Result<Result<(), Refused>> {
	if photos.len() > PHOTOS_PER_DRAFT {
		return Ok(Err(Refused::Invalid(format!("a draft selects at most {PHOTOS_PER_DRAFT} photos"))));
	}
	let mut seen = BTreeSet::new();
	if let Some(twice) = photos.iter().find(|p| !seen.insert(**p)) {
		return Ok(Err(Refused::Invalid(format!("photo {twice} twice"))));
	}
	if db::live_photos(conn, trade, country, photos).await? != photos.len() {
		return Ok(Err(Refused::Invalid("a draft selects photos of its own category".to_owned())));
	}
	Ok(Ok(()))
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

/// The point of `viewport` nearest `at`.
fn clamp(at: LatLng, v: &Viewport) -> LatLng {
	let lng = if v.west <= v.east {
		at.lng.clamp(v.west, v.east)
	} else if at.lng >= v.west || at.lng <= v.east {
		at.lng // across the antimeridian, and inside
	} else if (v.west - at.lng) < (at.lng - v.east) {
		v.west
	} else {
		v.east
	};
	LatLng {
		lat: at.lat.clamp(v.south, v.north),
		lng,
	}
}

/// Great-circle metres.
fn metres(a: LatLng, b: LatLng) -> f64 {
	let (la, lb) = (a.lat.to_radians(), b.lat.to_radians());
	let h = ((lb - la) / 2.).sin().powi(2) + la.cos() * lb.cos() * ((b.lng - a.lng).to_radians() / 2.).sin().powi(2);
	2. * 6_371_000. * h.sqrt().asin()
}
