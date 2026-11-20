//! Brands (docs/ARCHITECTURE.md, A GBP's brand): under one gmail and one category of their owner's,
//! the name their GBPs share, a logo and the kit's photos. A GBP holds at most one, of its own
//! gmail and category.

use std::{collections::BTreeSet, io::Cursor};

use eyre::WrapErr;
use image::{ImageFormat, ImageReader};
use jiff::Timestamp;
use sa_core::{gbp::Country, ids::BrandId};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	kit::PHOTO_MAX,
	store::{brands as db, gmbs as gmbs_db},
	tokens::is_open,
};

/// The longer side of a logo as kept.
const LOGO_SIDE: u32 = 512;

#[derive(Clone, Debug, PartialEq)]
pub struct Brand {
	pub id: i64,
	pub gmail_id: i64,
	pub trade_id: i64,
	pub country: Country,
	/// What its GBPs' names start with: "Aquafix" → "Aquafix · Guillotière".
	pub name: String,
	/// The site its pages are on; many brands may share one.
	pub site: BrandId,
	/// The logo's version; `None` until one is uploaded.
	pub logo: Option<String>,
	pub created_at: Timestamp,
	/// Live GBPs holding it.
	pub gmbs: i64,
	/// Live photos.
	pub photos: i64,
}

pub struct NewBrand {
	pub gmail_id: i64,
	pub trade_id: i64,
	pub country: Country,
	pub name: String,
	pub site: BrandId,
}

impl Panel {
	pub async fn brands(&self, owner: Uuid) -> eyre::Result<Vec<Brand>> {
		db::brands(&mut *self.store.pool().acquire().await.wrap_err("a connection for brands")?, owner).await
	}

	/// A brand under the owner's gmail and category (set up there), its name no other live one's
	/// there whatever its case.
	pub async fn add_brand(&self, owner: Uuid, new: NewBrand, now: Timestamp) -> eyre::Result<Result<Brand, Refused>> {
		let Some(name) = brand_name(&new.name) else {
			return Ok(Err(Refused::Invalid("a brand's name is not empty, at most 100 characters".to_owned())));
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let category = gmbs_db::trades(&mut tx, owner)
			.await?
			.iter()
			.any(|t| t.id == new.trade_id && t.categories.iter().any(|c| c.country == new.country));
		if !category || gmbs_db::gmail_owner(&mut tx, new.gmail_id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		if db::named(&mut tx, new.gmail_id, new.trade_id, new.country, &name, None).await? {
			return Ok(Err(Refused::Conflict(format!("{name} is a brand of that gmail and category already"))));
		}
		let id = db::add(&mut tx, owner, new.gmail_id, new.trade_id, new.country, &name, &new.site, now).await?;
		let added = db::brand_of(&mut tx, owner, id).await?.expect("added in this transaction");
		tx.commit().await.wrap_err("committing a brand")?;
		Ok(Ok(added))
	}

	/// The owner's brand `id` renamed or moved to another site.
	pub async fn edit_brand(&self, owner: Uuid, id: i64, name: Option<&str>, site: Option<BrandId>) -> eyre::Result<Result<Brand, Refused>> {
		let name = match name.map(brand_name) {
			Some(None) => return Ok(Err(Refused::Invalid("a brand's name is not empty, at most 100 characters".to_owned()))),
			n => n.flatten(),
		};
		let mut tx = self.store.begin_write().await?;
		let Some(b) = db::brand_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Some(n) = &name
			&& db::named(&mut tx, b.gmail_id, b.trade_id, b.country, n, Some(id)).await?
		{
			return Ok(Err(Refused::Conflict(format!("{n} is a brand of that gmail and category already"))));
		}
		db::edit(&mut tx, id, name.as_deref(), site.as_ref()).await?;
		let edited = db::brand_of(&mut tx, owner, id).await?.expect("read in this transaction");
		tx.commit().await.wrap_err("committing a brand's edit")?;
		Ok(Ok(edited))
	}

	/// The owner's brand `id` removed, while no live GBP holds it; its photos' uses stay.
	pub async fn remove_brand(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(b) = db::brand_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if b.gmbs > 0 {
			return Ok(Err(Refused::Conflict(format!("{} GBPs hold {}: give them another first", b.gmbs, b.name))));
		}
		db::remove(&mut tx, id, now).await?;
		tx.commit().await.wrap_err("committing a brand's removal")?;
		Ok(Ok(()))
	}

	/// The owner's brand `id`'s logo: re-encoded as a PNG, at most `LOGO_SIDE` on its longer side.
	pub async fn set_logo(&self, owner: Uuid, id: i64, upload: Vec<u8>) -> eyre::Result<Result<Brand, Refused>> {
		if upload.len() > PHOTO_MAX {
			return Ok(Err(Refused::TooLarge(format!("a logo is at most {} MiB", PHOTO_MAX >> 20))));
		}
		let encoded = tokio::task::spawn_blocking(move || reencode(&upload)).await.wrap_err("re-encoding a logo")?;
		let png = match encoded {
			Ok(p) => p,
			Err(why) => return Ok(Err(Refused::Invalid(why))),
		};
		let sha256: [u8; 32] = Sha256::digest(&png).into();
		let mut tx = self.store.begin_write().await?;
		if db::brand_of(&mut tx, owner, id).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		db::set_logo(&mut tx, id, &png, &sha256).await?;
		let set = db::brand_of(&mut tx, owner, id).await?.expect("read in this transaction");
		tx.commit().await.wrap_err("committing a logo")?;
		Ok(Ok(set))
	}

	/// The owner's brand `id`'s logo, a PNG.
	pub async fn logo(&self, owner: Uuid, id: i64) -> eyre::Result<Result<Vec<u8>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a logo")?;
		if db::brand_of(&mut conn, owner, id).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		Ok(db::logo(&mut conn, id).await?.ok_or(Refused::NotFound))
	}

	/// Every one of `ids` given the owner's `brand` (`None`: none), or none of them: each the
	/// owner's, not removed, and of the brand's gmail and category.
	pub async fn set_brands(&self, owner: Uuid, ids: &[i64], brand: Option<i64>) -> eyre::Result<Result<(), Refused>> {
		let ids: BTreeSet<i64> = ids.iter().copied().collect();
		if ids.is_empty() {
			return Ok(Err(Refused::Invalid("no GBP to give a brand".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if let Some(b) = brand
			&& db::brand_of(&mut tx, owner, b).await?.is_none()
		{
			return Ok(Err(Refused::NotFound));
		}
		if db::fitting(&mut tx, owner, &ids, None).await? != ids.len() {
			return Ok(Err(Refused::NotFound));
		}
		if db::fitting(&mut tx, owner, &ids, brand).await? != ids.len() {
			return Ok(Err(Refused::Conflict("a brand is given only to GBPs of its gmail and its category".to_owned())));
		}
		let befores = crate::store::gmbs::of_ids(&mut tx, &ids).await?;
		db::set_brands(&mut tx, &ids, brand).await?;
		crate::store::gmbs::record_since(&mut tx, Some(owner), Timestamp::now(), &befores).await?;
		tx.commit().await.wrap_err("committing GBPs' brand")?;
		Ok(Ok(()))
	}
}

fn brand_name(raw: &str) -> Option<String> {
	Some(raw.trim()).filter(|n| !n.is_empty() && n.chars().count() <= 100).map(str::to_owned)
}

/// `upload` as a PNG, its longer side at most [`LOGO_SIDE`]. `Err`: why it is no image.
fn reencode(upload: &[u8]) -> Result<Vec<u8>, String> {
	let refused = |e: image::ImageError| format!("not an image this reads (PNG, JPEG or WebP): {e}");
	let mut img = ImageReader::new(Cursor::new(upload))
		.with_guessed_format()
		.map_err(|e| e.to_string())?
		.decode()
		.map_err(refused)?;
	if img.width().max(img.height()) > LOGO_SIDE {
		img = img.resize(LOGO_SIDE, LOGO_SIDE, image::imageops::FilterType::Lanczos3);
	}
	let mut png = Vec::new();
	img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).map_err(refused)?;
	Ok(png)
}
