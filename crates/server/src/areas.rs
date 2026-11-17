//! Areas (`sa_panel::areas`): geography, everyone's, under `/api/v1`. The kit's list of the
//! caller's GBPs by one stays `kit`'s.

use axum::{Json, extract::State, http::Method};
use ev_lib::api::{Body, Ids, JsonSchema, Params, Refusal};
use jiff::Timestamp;
use sa_core::prices::{PlacesRequest, Spends};
use sa_panel::{
	Panel,
	areas::{Area, NewArea, Shape},
};
use sa_places::{LatLng, Locality, Viewport};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiResult, Code},
	gmbs::{SuggestionDto, SuggestionsDto},
	http::{ApiRoute, ApiState, Gate},
	signin::Caller,
};

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Viewport")]
pub(crate) struct ViewportDto {
	pub south: f64,
	pub west: f64,
	pub north: f64,
	pub east: f64,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AreaShape")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum ShapeDto {
	City { place_id: String, viewport: ViewportDto },
	Circle { radius_m: i64 },
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Area")]
pub(crate) struct AreaDto {
	pub slug: String,
	pub name: String,
	/// ISO 3166-1 alpha-2.
	pub country: String,
	pub center: LatLng,
	pub shape: ShapeDto,
	pub margin_m: i64,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Areas")]
pub(crate) struct AreasDto {
	pub areas: Vec<AreaDto>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "PutArea")]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PutAreaDto {
	City { place_id: String, margin_m: i64 },
	Circle { name: String, lat: f64, lng: f64, radius_m: i64, margin_m: i64 },
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Locality")]
pub(crate) struct LocalityDto {
	pub place_id: String,
	pub name: String,
	pub location: LatLng,
	pub viewport: ViewportDto,
	/// ISO 3166-1 alpha-2.
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
	use Method as M;
	let user = Gate::USER;
	vec![
		ApiRoute::new(M::GET, "/areas", user, areas),
		ApiRoute::new(M::GET, "/areas/{slug}", user, area),
		ApiRoute::new(M::PUT, "/areas/{slug}", user, put_area),
		ApiRoute::new(M::GET, "/kit/cities", user, cities),
		ApiRoute::new(M::GET, "/kit/cities/{place_id}", user, city),
		ApiRoute::new(M::GET, "/kit/locality", user, locality),
	]
}

/// A path's area.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SlugIn {
	pub(crate) slug: String,
}

#[derive(Deserialize, JsonSchema)]
struct PlaceIn {
	place_id: String,
}

/// Every area, everyone's.
async fn areas(State(panel): State<Panel>) -> ApiResult<Json<AreasDto>> {
	Ok(Json(AreasDto {
		areas: panel.areas().await?.into_iter().map(Into::into).collect(),
	}))
}

/// The area at `/<slug>/`.
async fn area(State(panel): State<Panel>, Ids(SlugIn { slug }): Ids<SlugIn>) -> ApiResult<Json<AreaDto>> {
	Ok(Json(panel.area(&slug).await?.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?.into()))
}

/// Draws the area at `/<slug>/`: a city (its outline and country asked of Places, paid by the
/// caller) or a circle (its centre's country reverse-geocoded), with a margin around it that is "next to it"; over what it was if the caller drew it.
///
/// 400 a link the conductor's own pages hold, or a centre in no country; 409 someone else's area.
async fn put_area(State(s): State<ApiState>, Spends(caller, by): Spends<PlacesRequest, Caller>, Ids(SlugIn { slug }): Ids<SlugIn>, Body(b): Body<PutAreaDto>) -> ApiResult<Json<AreaDto>> {
	if crate::web::taken(&slug, s.web.as_ref()) {
		return Err(Refusal::new(Code::BadRequest, format!("/{slug}/ is one of the conductor's own pages")));
	}
	let new = match b {
		PutAreaDto::City { place_id, margin_m } => NewArea::City { place_id, margin_m },
		PutAreaDto::Circle { name, lat, lng, radius_m, margin_m } => NewArea::Circle {
			name,
			center: LatLng { lat, lng },
			radius_m,
			margin_m,
		},
	};
	let area = s.panel.put_area(by, &slug, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, slug, "area drawn");
	Ok(Json(area.into()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Typed {
	/// What is typed so far.
	input: String,
}

/// Cities and towns what is typed may be heading for, in the countries served (Places
/// Autocomplete, paid by the caller).
async fn cities(State(panel): State<Panel>, Spends(_, by): Spends<PlacesRequest, Caller>, Params(q): Params<Typed>) -> ApiResult<Json<SuggestionsDto>> {
	let found = panel.cities(by, &q.input).await??;
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

/// The city or town a suggestion is: its point and the rectangle that frames it (Places, paid by
/// the caller).
async fn city(State(panel): State<Panel>, Spends(_, by): Spends<PlacesRequest, Caller>, Ids(PlaceIn { place_id }): Ids<PlaceIn>) -> ApiResult<Json<LocalityDto>> {
	Ok(Json(panel.locality(by, &place_id).await??.into()))
}

/// The city or town a point lies in (Geocoding, paid by the caller). 404 outside any.
async fn locality(State(panel): State<Panel>, Spends(_, by): Spends<PlacesRequest, Caller>, Params(at): Params<LatLng>) -> ApiResult<Json<LocalityDto>> {
	Ok(Json(panel.locality_at(by, at).await??.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?.into()))
}
