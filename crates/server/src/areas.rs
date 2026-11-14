//! Areas (`sa_panel::areas`): geography, everyone's, under `/api/v1`. The kit's list of the
//! caller's GBPs by one stays `kit`'s.
//!
//! ```text
//! GET    /api/v1/areas                                        {areas}
//! GET    /api/v1/areas/{slug}                                 Area
//! PUT    /api/v1/areas/{slug} {kind: city, place_id, margin_m} | {kind: circle, name, lat, lng, radius_m, margin_m}
//!                                                             → Area; 400 a reserved link, a centre in no country; 409 someone else's
//! GET    /api/v1/kit/cities?input=                            Suggestions: cities and towns
//! GET    /api/v1/kit/cities/{place_id}                        Locality
//! GET    /api/v1/kit/locality?lat=&lng=                       Locality | 404 none there
//! ```

use axum::{
	Extension, Json,
	extract::{
		Path, Query, State,
		rejection::{JsonRejection, QueryRejection},
	},
	http::Method,
};
use jiff::Timestamp;
use sa_panel::{
	Panel,
	areas::{Area, NewArea, Shape},
};
use sa_places::{LatLng, Locality, Viewport};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
	api::{ApiError, ApiResult, body},
	gmbs::{SuggestionDto, SuggestionsDto},
	http::{ApiRoute, ApiState},
	signin::{Caller, Freshness},
};

#[derive(Clone, Copy, Debug, Serialize, TS)]
#[ts(rename = "Viewport")]
pub struct ViewportDto {
	pub south: f64,
	pub west: f64,
	pub north: f64,
	pub east: f64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "AreaShape")]
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
#[ts(rename = "Area")]
pub struct AreaDto {
	pub slug: String,
	pub name: String,
	/// ISO 3166-1 alpha-2.
	pub country: String,
	#[ts(type = "{ lat: number, lng: number }")]
	pub center: LatLng,
	pub shape: ShapeDto,
	#[ts(type = "number")]
	pub margin_m: i64,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Areas")]
pub struct AreasDto {
	pub areas: Vec<AreaDto>,
}

#[derive(Deserialize, TS)]
#[ts(rename = "PutArea")]
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

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Locality")]
pub struct LocalityDto {
	pub place_id: String,
	pub name: String,
	#[ts(type = "{ lat: number, lng: number }")]
	pub location: LatLng,
	pub viewport: ViewportDto,
	pub country: String,
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
			country: a.country,
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
			country: l.country,
		}
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/areas", None, Cached, areas),
		ApiRoute::new(M::GET, "/areas/{slug}", None, Cached, area),
		ApiRoute::new(M::PUT, "/areas/{slug}", None, Cached, put_area),
		ApiRoute::new(M::GET, "/kit/cities", None, Cached, cities),
		ApiRoute::new(M::GET, "/kit/cities/{place_id}", None, Cached, city),
		ApiRoute::new(M::GET, "/kit/locality", None, Cached, locality),
	]
}

async fn areas(State(panel): State<Panel>) -> ApiResult<Json<AreasDto>> {
	Ok(Json(AreasDto {
		areas: panel.areas().await?.into_iter().map(Into::into).collect(),
	}))
}

async fn area(State(panel): State<Panel>, Path(slug): Path<String>) -> ApiResult<Json<AreaDto>> {
	Ok(Json(panel.area(&slug).await?.ok_or(ApiError::NotFound)?.into()))
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
	let area = s.panel.put_area(caller.user_id, &slug, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, slug, "area drawn");
	Ok(Json(area.into()))
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

