//! The caller's brands (`sa_panel::brands`): each under one of their gmails and categories, its
//! logo and the kit's photos; open to every signed-in user, each sees and changes only their own.

use axum::{
	Extension, Json,
	extract::{DefaultBodyLimit, State},
	http::Method,
	response::NoContent,
};
use ev_lib::api::{Body, Created, Ids, JsonSchema};
use jiff::Timestamp;
use sa_core::{gbp::Country, ids::BrandId};
use sa_panel::{
	Panel,
	brands::{Brand, NewBrand},
	kit::PHOTO_MAX,
};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiResult, Id},
	http::{ApiRoute, SLOW, USER},
	kit::{Format, Image, Jpeg, PhotoDto, Picture, jpg},
	signin::Caller,
};

/// A brand of the caller's: under one gmail and one category, the name its GBPs share.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Brand")]
pub(crate) struct BrandDto {
	pub id: i64,
	pub gmail_id: i64,
	pub trade_id: i64,
	pub country: Country,
	/// What its GBPs' names start with: "Aquafix" → "Aquafix · Guillotière".
	pub name: String,
	/// The site (sites' brand) its pages are on; brands of other gmails may share it.
	pub site: BrandId,
	/// The logo's version, for its URL; `null` until one is uploaded.
	pub logo: Option<String>,
	/// RFC 3339.
	pub created_at: String,
	/// Live GBPs holding it.
	pub gmbs: i64,
	/// Live photos.
	pub photos: i64,
}

/// `GET /brands`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Brands")]
pub(crate) struct BrandsDto {
	pub brands: Vec<BrandDto>,
}

/// A brand's photos, newest first.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "BrandPhotos")]
pub(crate) struct PhotosDto {
	pub photos: Vec<PhotoDto>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "NewBrand")]
#[serde(deny_unknown_fields)]
pub(crate) struct NewBrandDto {
	gmail: i64,
	trade: i64,
	country: Country,
	name: String,
	site: BrandId,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "EditBrand")]
#[serde(deny_unknown_fields)]
pub(crate) struct EditBrandDto {
	name: Option<String>,
	site: Option<BrandId>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetBrands {
	gmbs: Vec<i64>,
	/// `null` takes their brand away.
	brand: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "LabelPhoto")]
#[serde(deny_unknown_fields)]
pub(crate) struct LabelDto {
	/// What it shows, ≤ 60 characters: its alt and caption on the landing; empty clears it.
	shows: String,
}

#[derive(Deserialize, JsonSchema)]
struct PhotoIn {
	id: i64,
	/// `<photo>.jpg`.
	file: String,
}

impl From<Brand> for BrandDto {
	fn from(b: Brand) -> Self {
		Self {
			id: b.id,
			gmail_id: b.gmail_id,
			trade_id: b.trade_id,
			country: b.country,
			name: b.name,
			site: b.site,
			logo: b.logo,
			created_at: b.created_at.to_string(),
			gmbs: b.gmbs,
			photos: b.photos,
		}
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/brands", USER, brands),
		ApiRoute::new(M::POST, "/brands", USER, add_brand),
		ApiRoute::new(M::PATCH, "/brands/{id}", USER, edit_brand),
		ApiRoute::new(M::DELETE, "/brands/{id}", USER, remove_brand),
		ApiRoute::new(M::PUT, "/brands/{id}/logo", SLOW, set_logo).layer(|h| h.layer(DefaultBodyLimit::max(PHOTO_MAX))),
		ApiRoute::new(M::GET, "/brands/{id}/logo", USER, logo),
		ApiRoute::new(M::GET, "/brands/{id}/photos", USER, photos),
		ApiRoute::new(M::POST, "/brands/{id}/photos", SLOW, add_photo).layer(|h| h.layer(DefaultBodyLimit::max(PHOTO_MAX))),
		ApiRoute::new(M::GET, "/brands/{id}/photos/{file}", USER, photo),
		ApiRoute::new(M::PATCH, "/brands/{id}/photos/{file}", USER, label_photo),
		ApiRoute::new(M::DELETE, "/brands/{id}/photos/{file}", USER, remove_photo),
		ApiRoute::new(M::PUT, "/gmbs/brand", USER, set_brands),
	]
}

/// The caller's brands, each with how many GBPs hold it and its photos.
async fn brands(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<BrandsDto>> {
	Ok(Json(BrandsDto {
		brands: panel.brands(caller.user_id).await?.into_iter().map(Into::into).collect(),
	}))
}

/// Adds a brand under one of the caller's gmails and categories (set up there). 409 a live one
/// of that gmail and category is so named already, whatever its case.
async fn add_brand(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<NewBrandDto>) -> ApiResult<Created<BrandDto>> {
	let new = NewBrand {
		gmail_id: b.gmail,
		trade_id: b.trade,
		country: b.country,
		name: b.name,
		site: b.site,
	};
	let added = panel.add_brand(caller.user_id, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, brand_id = added.id, "brand added");
	Ok(Created(added.into()))
}

/// Renames a brand or moves it to another site.
async fn edit_brand(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>, Body(b): Body<EditBrandDto>) -> ApiResult<Json<BrandDto>> {
	Ok(Json(panel.edit_brand(caller.user_id, id, b.name.as_deref(), b.site).await??.into()))
}

/// Removes a brand; its photos' uses are kept. 409 while a live GBP holds it.
async fn remove_brand(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.remove_brand(caller.user_id, id, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, brand_id = id, "brand removed");
	Ok(NoContent)
}

/// Sets a brand's logo, ≤ 10 MiB: re-encoded as a PNG, at most 512 px on its longer side.
async fn set_logo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>, Image(bytes): Image) -> ApiResult<Json<BrandDto>> {
	Ok(Json(panel.set_logo(caller.user_id, id, bytes.to_vec()).await??.into()))
}

/// A brand's logo. 404 none yet.
async fn logo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Picture<Png>> {
	Ok(Picture(panel.logo(caller.user_id, id).await??, Png))
}

/// A brand's photos, newest first.
async fn photos(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<PhotosDto>> {
	Ok(Json(photos_of(&panel, &caller, id).await?))
}

/// Adds a photo to a brand, ≤ 10 MiB: re-encoded as a JPEG, its EXIF (GPS with it) gone, turned
/// upright; the same bytes again are the same photo, back if removed. Answers its photos.
async fn add_photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>, Image(bytes): Image) -> ApiResult<Created<PhotosDto>> {
	let photo = panel.add_photo(caller.user_id, id, bytes.to_vec(), Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, brand_id = id, photo, "brand photo added");
	Ok(Created(photos_of(&panel, &caller, id).await?))
}

/// A photo of a brand, `{file}` being `<photo>.jpg`.
async fn photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(PhotoIn { id, file }): Ids<PhotoIn>) -> ApiResult<Picture<Jpeg>> {
	Ok(Picture(panel.kit_photo(caller.user_id, id, jpg(&file)?).await??, Jpeg))
}

/// Labels a photo of the brand with what it shows; answers the brand's photos.
async fn label_photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(PhotoIn { id, file }): Ids<PhotoIn>, Body(b): Body<LabelDto>) -> ApiResult<Json<PhotosDto>> {
	panel.label_photo(caller.user_id, id, jpg(&file)?, &b.shows).await??;
	Ok(Json(photos_of(&panel, &caller, id).await?))
}

/// Removes a photo: out of every offer, its uses kept.
async fn remove_photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(PhotoIn { id, file }): Ids<PhotoIn>) -> ApiResult<NoContent> {
	panel.remove_photo(caller.user_id, id, jpg(&file)?, Timestamp::now()).await??;
	Ok(NoContent)
}

/// Gives GBPs a brand, every one or none: 404 one not the caller's, 409 one not of the brand's
/// gmail and category.
async fn set_brands(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(SetBrands { gmbs, brand }): Body<SetBrands>) -> ApiResult<NoContent> {
	panel.set_brands(caller.user_id, &gmbs, brand).await??;
	tracing::info!(user_id = %caller.user_id, ?gmbs, ?brand, "GBPs given a brand");
	Ok(NoContent)
}

async fn photos_of(panel: &Panel, caller: &Caller, brand: i64) -> ApiResult<PhotosDto> {
	Ok(PhotosDto {
		photos: panel
			.brand_photos(caller.user_id, brand)
			.await??
			.into_iter()
			.map(|p| PhotoDto {
				id: p.id,
				shows: p.shows,
				width: p.width,
				height: p.height,
				uses: p.uses,
				created_at: p.created_at.to_string(),
			})
			.collect(),
	})
}

/// A logo; its URL stays as it changes.
struct Png;

impl Format for Png {
	const CACHE: &'static str = "private, no-cache";
	const DESCRIPTION: &'static str = "A PNG.";
	const MIME: &'static str = "image/png";
}
