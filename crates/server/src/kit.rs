//! The review kit (`sa_panel::kit`): the owner's routes under `/api/v1`, and the landings' under
//! `/api/kit/v1`, signed as ingest is and answered here.
//!
//! ```text
//! GET    /api/v1/trades/{id}/{country}/kit                    KitPool
//! POST   /api/v1/trades/{id}/{country}/kit/drafts             {text, photos} → 201 KitPool
//! PATCH  /api/v1/trades/{id}/{country}/kit/drafts/{d}         {text?, photos?} → KitPool; 409 text of a copied one
//! DELETE /api/v1/trades/{id}/{country}/kit/drafts/{d}         204, removed: out of every offer, its uses kept
//! POST   /api/v1/trades/{id}/{country}/kit/photos             an image ≤ 10 MiB → 201 KitPool
//! GET    /api/v1/trades/{id}/{country}/kit/photos/{p}.jpg     image/jpeg
//! DELETE /api/v1/trades/{id}/{country}/kit/photos/{p}         204, as a draft
//! GET    /api/v1/gmbs/{id}/kit                                GmbKit
//! PUT    /api/v1/gmbs/{id}/kit {target | null}                → GmbKit; 409 no place, or a page target without a site page
//! POST   /api/v1/gmbs/{id}/kit/shown                          204
//! GET    /api/v1/kit/areas                                    {areas}
//! GET    /api/v1/kit/areas/{slug}                             KitArea
//! PUT    /api/v1/kit/areas/{slug} {kind: city, place_id, margin_m} | {kind: circle, name, lat, lng, radius_m, margin_m}
//!                                                             → KitArea; 400 a reserved link; 409 someone else's
//! GET    /api/v1/kit/areas/{slug}/gmbs                        {area, gmbs}: the caller's, in it or next to it
//! GET    /api/v1/kit/cities?input=                            Suggestions: cities and towns
//! GET    /api/v1/kit/cities/{place_id}                        Locality
//! GET    /api/v1/kit/locality?lat=&lng=                       Locality | 404 none there
//!
//! GET    /api/kit/v1/offer?brand&slug[&qr=1]                  KitOffer | 404 the kit off
//! GET    /api/kit/v1/photos/{p}.jpg?view=                     image/jpeg, offered to that view
//! POST   /api/kit/v1/events {view, copied: d} | {view, saved: [p]}   204; 409 went there already
//! ```
//!
//! A landing signs as ingest's sources do (a GET over `GET <path?query>`); the key's brands must
//! hold the place's. (brand, slug) ↔ GBP is sites': `GET /api/conductor/places`, kept 60 s.

use std::{
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use axum::{
	Extension, Json, Router,
	body::{Body, to_bytes},
	error_handling::HandleErrorLayer,
	extract::{
		OriginalUri, Path, Query, State,
		rejection::{JsonRejection, QueryRejection},
	},
	http::{HeaderMap, HeaderValue, Method, StatusCode, header},
	response::{IntoResponse, Response},
	routing::{get, post},
};
use jiff::Timestamp;
use sa_auth::SourceKind;
use sa_core::gbp::{Country, State as GbpState};
use sa_panel::{
	Grant, Panel, SignedBatch,
	kit::{Area, Event, Kit, Nearby, NewArea, Offer, PHOTO_MAX, Pool, Ranked, Shape, Target},
	store::kit::Removable,
};
use sa_places::{LatLng, Locality, Viewport};
use serde::{Deserialize, Serialize};
use tower::{BoxError, ServiceBuilder};
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};
use ts_rs::TS;
use uuid::Uuid;

use crate::{
	api::{ApiError, ApiResult, body},
	forward::Services,
	gmbs::{SuggestionDto, SuggestionsDto, id},
	http::{ApiRoute, ApiState, Limits},
	ingest,
	signin::{Caller, Freshness},
};

/// The landings' routes sit under it.
pub const LANDING: &str = "/api/kit/v1";

pub(crate) const PHOTOS: &str = "/api/v1/trades/{id}/{country}/kit/photos";

/// How long sites' places are kept.
const PAGES_FOR: Duration = Duration::from_secs(60);

/// The landing's path a GBP's page is at, under its site key's origin.
const PAGE: &str = "/avis/";

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitDraft")]
pub struct DraftDto {
	#[ts(type = "number")]
	pub id: i64,
	pub text: String,
	#[ts(type = "number[]")]
	pub photos: Vec<i64>,
	/// Copies, every GBP.
	#[ts(type = "number")]
	pub uses: i64,
	pub created_at: String,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitPhoto")]
pub struct PhotoDto {
	#[ts(type = "number")]
	pub id: i64,
	#[ts(type = "number")]
	pub width: i64,
	#[ts(type = "number")]
	pub height: i64,
	/// Saved, every GBP.
	#[ts(type = "number")]
	pub uses: i64,
	pub created_at: String,
}

/// A category's drafts and photos.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitPool")]
pub struct PoolDto {
	pub drafts: Vec<DraftDto>,
	/// Newest first.
	pub photos: Vec<PhotoDto>,
}

#[derive(Deserialize, TS)]
#[ts(rename = "NewKitDraft")]
#[serde(deny_unknown_fields)]
pub struct NewDraftDto {
	pub text: String,
	#[ts(type = "number[]")]
	pub photos: Vec<i64>,
}

#[derive(Deserialize, TS)]
#[ts(rename = "EditKitDraft")]
#[serde(deny_unknown_fields)]
pub struct EditDraftDto {
	#[ts(optional)]
	pub text: Option<String>,
	#[ts(optional, type = "number[]")]
	pub photos: Option<Vec<i64>>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitRankedPhoto")]
pub struct RankedPhotoDto {
	#[ts(type = "number")]
	pub id: i64,
	#[ts(type = "number")]
	pub width: i64,
	#[ts(type = "number")]
	pub height: i64,
	#[ts(type = "number")]
	pub uses: i64,
	/// Saved on this GBP: never offered here again.
	pub went_here: bool,
}

/// A draft as the offer weighs it for one GBP.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitRanked")]
pub struct RankedDto {
	#[ts(type = "number")]
	pub id: i64,
	pub text: String,
	#[ts(type = "number")]
	pub uses: i64,
	/// Copied on this GBP.
	pub went_here: bool,
	/// Held by a reviewer's page open now.
	pub held: bool,
	/// Among what a reviewer is offered now.
	pub offered: bool,
	pub photos: Vec<RankedPhotoDto>,
}

/// The last 30 days.
#[derive(Clone, Copy, Debug, Serialize, TS)]
#[ts(rename = "KitFunnel")]
pub struct FunnelDto {
	#[ts(type = "number")]
	pub shown: i64,
	#[ts(type = "number")]
	pub scanned: i64,
	#[ts(type = "number")]
	pub opened: i64,
	#[ts(type = "number")]
	pub copied: i64,
	#[ts(type = "number")]
	pub photos_saved: i64,
}

/// A GBP's kit.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "GmbKit")]
pub struct GmbKitDto {
	/// `null`: off.
	pub target: Option<Target>,
	/// What its QR encodes; `null` while off.
	pub url: Option<String>,
	/// Its page on its landing: `null` without one, and then only `google` is open to it.
	pub page: Option<String>,
	/// Its category's drafts in the offer's order.
	pub drafts: Vec<RankedDto>,
	pub funnel: FunnelDto,
}

#[derive(Deserialize, TS)]
#[ts(rename = "SetKit")]
#[serde(deny_unknown_fields)]
pub struct SetKitDto {
	pub target: Option<Target>,
}

#[derive(Clone, Copy, Debug, Serialize, TS)]
#[ts(rename = "Viewport")]
pub struct ViewportDto {
	pub south: f64,
	pub west: f64,
	pub north: f64,
	pub east: f64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitShape")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ShapeDto {
	City {
		place_id: String,
		viewport: ViewportDto,
	},
	Circle {
		#[ts(type = "number")]
		radius_m: i64,
	},
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitArea")]
pub struct AreaDto {
	pub slug: String,
	pub name: String,
	#[ts(type = "{ lat: number, lng: number }")]
	pub center: LatLng,
	pub shape: ShapeDto,
	#[ts(type = "number")]
	pub margin_m: i64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitAreas")]
pub struct AreasDto {
	pub areas: Vec<AreaDto>,
}

#[derive(Deserialize, TS)]
#[ts(rename = "PutKitArea")]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PutAreaDto {
	City {
		place_id: String,
		#[ts(type = "number")]
		margin_m: i64,
	},
	Circle {
		name: String,
		lat: f64,
		lng: f64,
		#[ts(type = "number")]
		radius_m: i64,
		#[ts(type = "number")]
		margin_m: i64,
	},
}

/// One of the caller's GBPs by an area.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitNearby")]
pub struct NearbyDto {
	#[ts(type = "number")]
	pub id: i64,
	pub name: String,
	pub state: GbpState,
	pub address: Option<String>,
	#[ts(type = "{ lat: number, lng: number }")]
	pub location: LatLng,
	#[ts(type = "number")]
	pub distance_m: f64,
	/// `false`: within the margin outside it.
	pub inside: bool,
	pub target: Option<Target>,
	pub last_shown: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitAreaGmbs")]
pub struct AreaGmbsDto {
	pub area: AreaDto,
	pub gmbs: Vec<NearbyDto>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Locality")]
pub struct LocalityDto {
	pub place_id: String,
	pub name: String,
	#[ts(type = "{ lat: number, lng: number }")]
	pub location: LatLng,
	pub viewport: ViewportDto,
}

/// What a reviewer's page shows (`/api/kit/v1/offer`).
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitOffer")]
pub struct OfferDto {
	pub target: Target,
	pub place_id: String,
	pub name: String,
	pub lang: String,
	/// Google's form for the place (Places' writeAReviewUri).
	pub review_uri: String,
	pub view: Uuid,
	pub drafts: Vec<OfferedDto>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitOffered")]
pub struct OfferedDto {
	#[ts(type = "number")]
	pub id: i64,
	pub text: String,
	pub photos: Vec<OfferedPhotoDto>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "KitOfferedPhoto")]
pub struct OfferedPhotoDto {
	#[ts(type = "number")]
	pub id: i64,
	#[ts(type = "number")]
	pub width: i64,
	#[ts(type = "number")]
	pub height: i64,
}

/// Which GBP each landing page is: sites' places, kept [`PAGES_FOR`].
#[derive(Clone)]
pub(crate) struct Pages {
	services: Services,
	kept: Arc<Mutex<Option<(Instant, Arc<Vec<SitePlace>>)>>>,
}
impl Pages {
	pub(crate) fn new(services: Services) -> Self {
		Self { services, kept: Arc::default() }
	}

	async fn all(&self) -> Result<Arc<Vec<SitePlace>>, ApiError> {
		if let Some((at, places)) = self.kept.lock().expect("never poisoned: no panic while held").as_ref()
			&& at.elapsed() < PAGES_FOR
		{
			return Ok(places.clone());
		}
		let fetched: SitePlaces = self.services.conductor_get("/api/conductor/places").await.map_err(|e| {
			tracing::warn!(error = %e, "kit: sites' places");
			ApiError::Unavailable("sites did not say which page is which place".to_owned())
		})?;
		let places = Arc::new(fetched.places);
		*self.kept.lock().expect("never poisoned: no panic while held") = Some((Instant::now(), places.clone()));
		Ok(places)
	}

	async fn of_gmb(&self, gmb: i64) -> Result<Option<SitePlace>, ApiError> {
		Ok(self.all().await?.iter().find(|p| p.gmb_id == gmb).cloned())
	}

	async fn gmb_at(&self, brand: &str, slug: &str) -> Result<Option<i64>, ApiError> {
		Ok(self.all().await?.iter().find(|p| p.brand == brand && p.slug == slug).map(|p| p.gmb_id))
	}
}

#[derive(Clone, Debug, Deserialize)]
struct SitePlace {
	gmb_id: i64,
	brand: String,
	slug: String,
}

#[derive(Deserialize)]
struct SitePlaces {
	places: Vec<SitePlace>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventDto {
	view: Uuid,
	copied: Option<i64>,
	saved: Option<Vec<i64>>,
}

impl From<Pool> for PoolDto {
	fn from(p: Pool) -> Self {
		Self {
			drafts: p
				.drafts
				.into_iter()
				.map(|d| DraftDto {
					id: d.id,
					text: d.text,
					photos: d.photos,
					uses: d.uses,
					created_at: d.created_at.to_string(),
				})
				.collect(),
			photos: p
				.photos
				.into_iter()
				.map(|p| PhotoDto {
					id: p.id,
					width: p.width,
					height: p.height,
					uses: p.uses,
					created_at: p.created_at.to_string(),
				})
				.collect(),
		}
	}
}

impl From<Viewport> for ViewportDto {
	fn from(v: Viewport) -> Self {
		Self {
			south: v.south,
			west: v.west,
			north: v.north,
			east: v.east,
		}
	}
}

impl From<Area> for AreaDto {
	fn from(a: Area) -> Self {
		Self {
			slug: a.slug,
			name: a.name,
			center: a.center,
			shape: match a.shape {
				Shape::City { place_id, viewport } => ShapeDto::City {
					place_id,
					viewport: viewport.into(),
				},
				Shape::Circle { radius_m } => ShapeDto::Circle { radius_m },
			},
			margin_m: a.margin_m,
		}
	}
}

impl From<Locality> for LocalityDto {
	fn from(l: Locality) -> Self {
		Self {
			place_id: l.place_id,
			name: l.name,
			location: l.location,
			viewport: l.viewport.into(),
		}
	}
}

impl From<Nearby> for NearbyDto {
	fn from(n: Nearby) -> Self {
		Self {
			id: n.gmb.id,
			name: n.gmb.name,
			state: n.gmb.state,
			address: n.gmb.address,
			location: n.gmb.location.expect("only a GBP with a point is by an area").at,
			distance_m: n.distance_m.round(),
			inside: n.inside,
			target: n.target,
			last_shown: n.last_shown.map(|t| t.to_string()),
		}
	}
}

fn ranked(drafts: Vec<Ranked>, target: Option<Target>) -> Vec<RankedDto> {
	let shown = match target {
		Some(Target::Drafts) => sa_panel::kit::OFFERED,
		Some(Target::OneTap) => 1,
		Some(Target::Google) | None => 0,
	};
	let mut open = 0;
	drafts
		.into_iter()
		.map(|d| {
			let offered = !d.went_here && !d.held && open < shown;
			open += usize::from(!d.went_here && !d.held);
			RankedDto {
				id: d.id,
				text: d.text,
				uses: d.uses,
				went_here: d.went_here,
				held: d.held,
				offered,
				photos: d
					.photos
					.into_iter()
					.map(|p| RankedPhotoDto {
						id: p.id,
						width: p.width,
						height: p.height,
						uses: p.uses,
						went_here: p.went_here,
					})
					.collect(),
			}
		})
		.collect()
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/trades/{id}/{country}/kit", None, Cached, pool),
		ApiRoute::new(M::POST, "/trades/{id}/{country}/kit/drafts", None, Cached, add_draft),
		ApiRoute::new(M::PATCH, "/trades/{id}/{country}/kit/drafts/{draft}", None, Cached, edit_draft),
		ApiRoute::new(M::DELETE, "/trades/{id}/{country}/kit/drafts/{draft}", None, Cached, remove_draft),
		ApiRoute::new(M::GET, "/trades/{id}/{country}/kit/photos/{file}", None, Cached, photo),
		ApiRoute::new(M::DELETE, "/trades/{id}/{country}/kit/photos/{file}", None, Cached, remove_photo),
		ApiRoute::new(M::GET, "/gmbs/{id}/kit", None, Cached, gmb_kit),
		ApiRoute::new(M::PUT, "/gmbs/{id}/kit", None, Cached, set_kit),
		ApiRoute::new(M::POST, "/gmbs/{id}/kit/shown", None, Cached, shown),
		ApiRoute::new(M::GET, "/kit/areas", None, Cached, areas),
		ApiRoute::new(M::GET, "/kit/areas/{slug}", None, Cached, area),
		ApiRoute::new(M::PUT, "/kit/areas/{slug}", None, Cached, put_area),
		ApiRoute::new(M::GET, "/kit/areas/{slug}/gmbs", None, Cached, area_gmbs),
		ApiRoute::new(M::GET, "/kit/cities", None, Cached, cities),
		ApiRoute::new(M::GET, "/kit/cities/{place_id}", None, Cached, city),
		ApiRoute::new(M::GET, "/kit/locality", None, Cached, locality),
	]
}

fn category(raw: &str, country: &str) -> ApiResult<(i64, Country)> {
	Ok((id(raw)?, country.parse()?))
}

/// `<id>.jpg` → id.
fn jpg(file: &str) -> ApiResult<i64> {
	id(file.strip_suffix(".jpg").ok_or(ApiError::NotFound)?)
}

fn jpeg(bytes: Vec<u8>) -> Response {
	let mut res = bytes.into_response();
	let h = res.headers_mut();
	h.insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
	h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable"));
	res
}

async fn pool(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path((raw, country)): Path<(String, String)>) -> ApiResult<Json<PoolDto>> {
	let (trade, country) = category(&raw, &country)?;
	Ok(Json(panel.kit_pool(caller.user_id, trade, country).await??.into()))
}

async fn add_draft(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Path((raw, country)): Path<(String, String)>,
	b: Result<Json<NewDraftDto>, JsonRejection>,
) -> ApiResult<(StatusCode, Json<PoolDto>)> {
	let (trade, country) = category(&raw, &country)?;
	let b = body(b)?;
	let draft = panel.add_draft(caller.user_id, trade, country, &b.text, &b.photos, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = trade, %country, draft, "kit draft added");
	Ok((StatusCode::CREATED, Json(panel.kit_pool(caller.user_id, trade, country).await??.into())))
}

async fn edit_draft(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Path((raw, country, draft)): Path<(String, String, String)>,
	b: Result<Json<EditDraftDto>, JsonRejection>,
) -> ApiResult<Json<PoolDto>> {
	let (trade, country) = category(&raw, &country)?;
	let draft = id(&draft)?;
	let b = body(b)?;
	panel.edit_draft(caller.user_id, trade, country, draft, b.text.as_deref(), b.photos.as_deref()).await??;
	Ok(Json(panel.kit_pool(caller.user_id, trade, country).await??.into()))
}

async fn remove_draft(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path((raw, country, draft)): Path<(String, String, String)>) -> ApiResult<StatusCode> {
	let (trade, country) = category(&raw, &country)?;
	panel.remove_from_kit(caller.user_id, trade, country, Removable::Draft, id(&draft)?, Timestamp::now()).await??;
	Ok(StatusCode::NO_CONTENT)
}

/// Under the gate, outside the API's bounds (`http::app`): an upload may be 10 MiB.
pub(crate) async fn add_photo(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Path((raw, country)): Path<(String, String)>,
	upload: Body,
) -> ApiResult<(StatusCode, Json<PoolDto>)> {
	let (trade, country) = category(&raw, &country)?;
	let bytes = to_bytes(upload, PHOTO_MAX)
		.await
		.map_err(|_| ApiError::TooLarge(format!("a photo is {} MiB at most", PHOTO_MAX >> 20)))?;
	let photo = panel.add_photo(caller.user_id, trade, country, bytes.to_vec(), Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = trade, %country, photo, "kit photo added");
	Ok((StatusCode::CREATED, Json(panel.kit_pool(caller.user_id, trade, country).await??.into())))
}

async fn photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path((raw, country, file)): Path<(String, String, String)>) -> ApiResult<Response> {
	let (trade, country) = category(&raw, &country)?;
	Ok(jpeg(panel.kit_photo(caller.user_id, trade, country, jpg(&file)?).await??))
}

async fn remove_photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path((raw, country, file)): Path<(String, String, String)>) -> ApiResult<StatusCode> {
	let (trade, country) = category(&raw, &country)?;
	panel.remove_from_kit(caller.user_id, trade, country, Removable::Photo, id(&file)?, Timestamp::now()).await??;
	Ok(StatusCode::NO_CONTENT)
}

/// The landing page of the GBP, `<origin>/avis/<slug>`: the origin of the active site key of its
/// brand.
async fn page_of(s: &ApiState, gmb: i64) -> ApiResult<Option<String>> {
	let pages = s
		.pages
		.as_ref()
		.ok_or_else(|| ApiError::Unavailable("sites not connected: no landing page is known".to_owned()))?;
	let Some(place) = pages.of_gmb(gmb).await? else {
		return Ok(None);
	};
	let mut origins: Vec<String> = s
		.panel
		.store()
		.sources()
		.await?
		.into_iter()
		.filter(|k| k.revoked_at.is_none() && k.grant.kind == SourceKind::Site && k.grant.brands.iter().any(|b| b.as_str() == place.brand))
		.filter_map(|k| k.origin.map(|o| o.to_string()))
		.collect();
	origins.sort();
	origins.dedup();
	match <[String; 1]>::try_from(origins) {
		Ok([origin]) => Ok(Some(format!("{origin}{PAGE}{}", place.slug))),
		Err(none) if none.is_empty() => Ok(None),
		Err(many) => Err(ApiError::Conflict(format!("{}'s site keys have {} origins: {}", place.brand, many.len(), many.join(", ")))),
	}
}

async fn kit_dto(s: &ApiState, caller: &Caller, gmb: i64) -> ApiResult<GmbKitDto> {
	let Kit { target, review_uri, drafts, funnel } = s.panel.kit(caller.user_id, gmb, Timestamp::now()).await??;
	let page = page_of(s, gmb).await?;
	let url = match (&page, target) {
		(_, None) => None,
		(Some(page), Some(_)) => Some(format!("{page}?qr")),
		(None, Some(_)) => review_uri,
	};
	Ok(GmbKitDto {
		target,
		url,
		page,
		drafts: ranked(drafts, target),
		funnel: FunnelDto {
			shown: funnel.shown,
			scanned: funnel.scanned,
			opened: funnel.opened,
			copied: funnel.copied,
			photos_saved: funnel.photos_saved,
		},
	})
}

async fn gmb_kit(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<Json<GmbKitDto>> {
	Ok(Json(kit_dto(&s, &caller, id(&raw)?).await?))
}

async fn set_kit(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetKitDto>, JsonRejection>) -> ApiResult<Json<GmbKitDto>> {
	let gmb = id(&raw)?;
	let SetKitDto { target } = body(b)?;
	if matches!(target, Some(Target::Drafts | Target::OneTap)) && page_of(&s, gmb).await?.is_none() {
		return Err(ApiError::Conflict("no landing page of its: only Google's form".to_owned()));
	}
	s.panel.set_kit_target(caller.user_id, gmb, target, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, ?target, "kit target set");
	Ok(Json(kit_dto(&s, &caller, gmb).await?))
}

async fn shown(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	panel.kit_shown(caller.user_id, id(&raw)?, Timestamp::now()).await??;
	Ok(StatusCode::NO_CONTENT)
}

async fn areas(State(panel): State<Panel>) -> ApiResult<Json<AreasDto>> {
	Ok(Json(AreasDto {
		areas: panel.kit_areas().await?.into_iter().map(Into::into).collect(),
	}))
}

async fn area(State(panel): State<Panel>, Path(slug): Path<String>) -> ApiResult<Json<AreaDto>> {
	Ok(Json(panel.kit_area(&slug).await?.ok_or(ApiError::NotFound)?.into()))
}

async fn put_area(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Path(slug): Path<String>, b: Result<Json<PutAreaDto>, JsonRejection>) -> ApiResult<Json<AreaDto>> {
	if crate::web::taken(&slug, s.web.as_ref()) {
		return Err(ApiError::BadRequest(format!("/{slug}/ is one of the conductor's own pages")));
	}
	let new = match body(b)? {
		PutAreaDto::City { place_id, margin_m } => NewArea::City { place_id, margin_m },
		PutAreaDto::Circle { name, lat, lng, radius_m, margin_m } => NewArea::Circle {
			name,
			center: LatLng { lat, lng },
			radius_m,
			margin_m,
		},
	};
	let area = s.panel.put_kit_area(caller.user_id, &slug, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, slug, "kit area drawn");
	Ok(Json(area.into()))
}

async fn area_gmbs(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(slug): Path<String>) -> ApiResult<Json<AreaGmbsDto>> {
	let gmbs = panel.area_gmbs(caller.user_id, &slug).await??;
	let area = panel.kit_area(&slug).await?.ok_or(ApiError::NotFound)?;
	Ok(Json(AreaGmbsDto {
		area: area.into(),
		gmbs: gmbs.into_iter().map(Into::into).collect(),
	}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
	input: String,
}

async fn cities(State(panel): State<Panel>, Extension(caller): Extension<Caller>, q: Result<Query<Input>, QueryRejection>) -> ApiResult<Json<SuggestionsDto>> {
	let Query(q) = q.map_err(|e| ApiError::BadRequest(e.body_text()))?;
	let found = panel.cities(caller.user_id, &q.input).await??;
	Ok(Json(SuggestionsDto {
		suggestions: found
			.into_iter()
			.map(|s| SuggestionDto {
				place_id: s.place_id,
				main: s.main,
				secondary: s.secondary,
			})
			.collect(),
	}))
}

async fn city(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(place_id): Path<String>) -> ApiResult<Json<LocalityDto>> {
	Ok(Json(panel.locality(caller.user_id, &place_id).await??.into()))
}

async fn locality(State(panel): State<Panel>, Extension(caller): Extension<Caller>, q: Result<Query<LatLng>, QueryRejection>) -> ApiResult<Json<LocalityDto>> {
	let Query(at) = q.map_err(|e| ApiError::BadRequest(e.body_text()))?;
	Ok(Json(panel.locality_at(caller.user_id, at).await??.ok_or(ApiError::NotFound)?.into()))
}

/// What the landings' routes are served on.
#[derive(Clone)]
pub(crate) struct Landing {
	panel: Panel,
	pages: Pages,
}

/// The landings' routes, bounded as ingest's.
pub(crate) fn landing_routes(panel: Panel, pages: Pages, limits: &Limits) -> Router {
	let layers = ServiceBuilder::new()
		.layer(HandleErrorLayer::new(|_: BoxError| async { ingest::error(StatusCode::SERVICE_UNAVAILABLE, "busy, try again") }))
		.load_shed()
		.concurrency_limit(limits.max_concurrent)
		.layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, limits.request_timeout))
		.layer(RequestBodyTimeoutLayer::new(limits.body_timeout));
	Router::new()
		.route(&format!("{LANDING}/offer"), get(offer))
		.route(&format!("{LANDING}/photos/{{file}}"), get(offered_photo))
		.route(&format!("{LANDING}/events"), post(event))
		.layer(layers)
		.with_state(Landing { panel, pages })
}

/// The landing's key, its MAC over `body` (a GET's: `GET <path?query>`).
async fn landing_key(l: &Landing, headers: &HeaderMap, body: &[u8]) -> Result<Grant, Response> {
	let (key_id, timestamp, signature) = ingest::signed(headers).ok_or_else(ingest::unsigned)?;
	let batch = SignedBatch { key_id, timestamp, signature, body };
	l.panel.authenticate(batch, Timestamp::now()).await.map_err(ingest::refused)
}

fn answer(e: ApiError) -> Response {
	e.into_response()
}

/// The GBP `view` showed, its brand one of `key`'s.
async fn view_of(l: &Landing, key: &Grant, view: Uuid) -> Result<(), Response> {
	let mut conn = l.panel.store().pool().acquire().await.map_err(|e| answer(ApiError::Internal(e.into())))?;
	let gmb = sa_panel::store::kit::view_gmb(&mut conn, view)
		.await
		.map_err(|e| answer(ApiError::Internal(e)))?
		.ok_or_else(|| answer(ApiError::NotFound))?;
	let place = l.pages.of_gmb(gmb).await.map_err(answer)?.ok_or_else(|| answer(ApiError::NotFound))?;
	if !key.brands.iter().any(|b| b.as_str() == place.brand) {
		return Err(answer(ApiError::NotFound));
	}
	Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferQuery {
	brand: String,
	slug: String,
	qr: Option<u8>,
}

async fn offer(State(l): State<Landing>, OriginalUri(uri): OriginalUri, headers: HeaderMap, q: Result<Query<OfferQuery>, QueryRejection>) -> Response {
	let signed = format!("GET {}", uri.path_and_query().map_or_else(|| uri.path(), |pq| pq.as_str()));
	let key = match landing_key(&l, &headers, signed.as_bytes()).await {
		Ok(k) => k,
		Err(res) => return res,
	};
	let Ok(Query(q)) = q else {
		return answer(ApiError::BadRequest("brand, slug and qr=1 if it came from the QR".to_owned()));
	};
	if !key.brands.iter().any(|b| b.as_str() == q.brand) {
		return answer(ApiError::NotFound);
	}
	let gmb = match l.pages.gmb_at(&q.brand, &q.slug).await {
		Ok(Some(g)) => g,
		Ok(None) => return answer(ApiError::NotFound),
		Err(e) => return answer(e),
	};
	match l.panel.kit_offer(gmb, q.qr == Some(1), Timestamp::now()).await {
		Ok(Ok(o)) => {
			let mut res = Json(OfferDto::from(o)).into_response();
			res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
			res
		}
		Ok(Err(r)) => answer(r.into()),
		Err(e) => answer(ApiError::Internal(e)),
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewQuery {
	view: Uuid,
}

async fn offered_photo(State(l): State<Landing>, OriginalUri(uri): OriginalUri, headers: HeaderMap, Path(file): Path<String>, q: Result<Query<ViewQuery>, QueryRejection>) -> Response {
	let signed = format!("GET {}", uri.path_and_query().map_or_else(|| uri.path(), |pq| pq.as_str()));
	let key = match landing_key(&l, &headers, signed.as_bytes()).await {
		Ok(k) => k,
		Err(res) => return res,
	};
	let (Ok(Query(q)), Ok(photo)) = (q, jpg(&file)) else {
		return answer(ApiError::NotFound);
	};
	if let Err(res) = view_of(&l, &key, q.view).await {
		return res;
	}
	match l.panel.offered_photo(q.view, photo).await {
		Ok(Ok(bytes)) => jpeg(bytes),
		Ok(Err(r)) => answer(r.into()),
		Err(e) => answer(ApiError::Internal(e)),
	}
}

async fn event(State(l): State<Landing>, headers: HeaderMap, body: Body) -> Response {
	let body = match to_bytes(body, 64 * 1024).await {
		Ok(b) => b,
		Err(e) => return ingest::body_error(e, 64 * 1024),
	};
	let key = match landing_key(&l, &headers, &body).await {
		Ok(k) => k,
		Err(res) => return res,
	};
	let e: EventDto = match serde_json::from_slice(&body) {
		Ok(e) => e,
		Err(e) => return answer(ApiError::BadRequest(e.to_string())),
	};
	let event = match (e.copied, e.saved) {
		(Some(draft), None) => Event::Copied(draft),
		(None, Some(photos)) => Event::Saved(photos),
		_ => return answer(ApiError::BadRequest("one of copied or saved".to_owned())),
	};
	if let Err(res) = view_of(&l, &key, e.view).await {
		return res;
	}
	match l.panel.kit_event(e.view, event, Timestamp::now()).await {
		Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
		Ok(Err(r)) => answer(r.into()),
		Err(e) => answer(ApiError::Internal(e)),
	}
}

impl From<Offer> for OfferDto {
	fn from(o: Offer) -> Self {
		Self {
			target: o.target,
			place_id: o.place_id,
			name: o.name,
			lang: o.lang.to_owned(),
			review_uri: o.review_uri,
			view: o.view,
			drafts: o
				.drafts
				.into_iter()
				.map(|d| OfferedDto {
					id: d.id,
					text: d.text,
					photos: d
						.photos
						.into_iter()
						.map(|p| OfferedPhotoDto {
							id: p.id,
							width: p.width,
							height: p.height,
						})
						.collect(),
				})
				.collect(),
		}
	}
}
