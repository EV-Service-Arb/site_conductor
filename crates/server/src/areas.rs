//! Areas (`sa_panel::areas`): geography, everyone's, under `/api/v1`. The kit's list of the
//! caller's GBPs by one stays `kit`'s.

use axum::{
	Extension, Json,
	body::{Body as RawBody, to_bytes},
	extract::{Request, State},
	http::{Method, StatusCode, header},
};
use ev_lib::api::{Body, Ids, JsonSchema, Params, Refusal};
use jiff::Timestamp;
use sa_auth::Study;
use sa_core::{
	gbp::Country,
	placement::{self, Definition, Gcid},
	prices::{PlacesRequest, Spends},
};
use sa_panel::{
	Panel,
	areas::{Area, NewArea, Shape},
	gmbs::GoogleCategory,
};
use sa_places::{LatLng, Locality, Viewport};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
	api::{ApiResult, Code},
	forward,
	gmbs::{SuggestionDto, SuggestionsDto},
	http::{ApiRoute, ApiState, USER},
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
	vec![
		ApiRoute::new(M::GET, "/areas", USER, areas),
		ApiRoute::new(M::GET, "/areas/{slug}", USER, area),
		ApiRoute::new(M::PUT, "/areas/{slug}", USER, put_area),
		ApiRoute::new(M::POST, "/areas/{slug}/studies", USER.needs(Study::Read), define_study),
		ApiRoute::new(M::GET, "/kit/cities", USER, cities),
		ApiRoute::new(M::GET, "/kit/cities/{place_id}", USER, city),
		ApiRoute::new(M::GET, "/kit/locality", USER, locality),
	]
}

/// A path's area.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct SlugIn {
	#[schemars(pattern(sa_core::ids::SLUG_PATTERN))]
	pub(crate) slug: String,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "StudyOf")]
#[serde(deny_unknown_fields)]
struct StudyIn {
	/// One of the caller's; none: the area's grid alone, nothing billed.
	category: Option<CategoryIn>,
}

/// A trade of the caller's, as set up for a country.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CategoryIn {
	trade: i64,
	country: Country,
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "Study")]
struct StudyDto {
	/// The definition's id at placement: what its overlay shows.
	sha: String,
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
	let area = s.sign_in.panel.put_area(by, &slug, new, Timestamp::now()).await??;
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

/// Defines placement's study of the area at `/<slug>/` under one of the caller's categories, or
/// of the area alone; buys nothing.
///
/// 404 no such area or category of the caller's; 422 a category not set up to be studied
/// (`no_study`, `no_google_category`, `no_keywords`), set up for `another_country`, or one
/// placement does not study (`unstudied`); 503 placement not connected.
async fn define_study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(SlugIn { slug }): Ids<SlugIn>, Body(b): Body<StudyIn>) -> ApiResult<Json<StudyDto>> {
	let panel = &s.sign_in.panel;
	let area = panel.area(&slug).await?.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
	let category = match b.category {
		None => None,
		Some(CategoryIn { trade, country }) => {
			let trades = panel.trades(caller.user_id).await?;
			let (trade, c) = trades
				.iter()
				.find(|t| t.id == trade)
				.and_then(|t| t.categories.iter().find(|c| c.country == country).map(|c| (t, c)))
				.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
			let named = format!("{} · {country}", trade.name);
			let study = c.study.clone().ok_or_else(|| Refusal::new(Code::NoStudy, format!("{named} names no study")))?;
			let primary = c.google_primary.ok_or_else(|| Refusal::new(Code::NoGoogleCategory, format!("{named} has no Google category")))?;
			if c.keywords.is_empty() {
				return Err(Refusal::new(Code::NoKeywords, format!("{named} has no keyword")));
			}
			if country.as_str() != area.country {
				return Err(Refusal::new(Code::AnotherCountry, format!("{named} is not set up for {}, where {} is", area.country, area.name)));
			}
			let gcid = |g: GoogleCategory| Gcid {
				id: g.id.to_owned(),
				name: g.names.get(country.lang()).expect("google_categories.json names each in its countries' languages").clone(),
			};
			Some(placement::Category {
				trade: u64::try_from(trade.id).expect("a trade's id is a rowid"),
				country,
				study,
				keywords: c.keywords.clone(),
				google_primary: gcid(primary),
				google_additional: c.google_additional.iter().copied().map(gcid).collect(),
			})
		}
	};
	let country: Country = area
		.country
		.parse()
		.map_err(|_| Refusal::new(Code::Unstudied, format!("{} is in {}, where nothing is studied", area.name, area.country)))?; // no Country: none of placement's bases
	let margin_m = area.margin_m as f64;
	let def = Definition {
		area: match area.shape {
			Shape::City { viewport, .. } => placement::Area::City { slug, country, viewport, margin_m },
			Shape::Circle { radius_m } => placement::Area::Circle {
				slug,
				country,
				center: area.center,
				radius_m: radius_m as f64,
				margin_m,
			},
		},
		category,
	};
	def.check()?;
	let sha = def.id();
	let unconnected = || Refusal::new(Code::Unavailable, "placement is not connected, or its operations are not known yet");
	let forward = s.forward.as_ref().ok_or_else(unconnected)?;
	let (method, path) = s
		.catalog
		.resolve("placement.define", &Map::from_iter([("sha".to_owned(), Value::String(sha.clone()))]))
		.map_err(|_| unconnected())?;
	let req = Request::builder()
		.method(method)
		.uri(path)
		.header(header::CONTENT_TYPE, "application/json")
		.body(RawBody::from(serde_json::to_vec(&def).expect("a definition serializes")))
		.expect("a path the catalog resolved");
	let res = forward.as_user(req, |service| forward::user(caller, service)).await;
	let status = res.status();
	if status == StatusCode::NO_CONTENT {
		return Ok(Json(StudyDto { sha }));
	}
	#[derive(Deserialize)]
	struct Theirs {
		code: Code,
		error: String,
	}
	let body = to_bytes(res.into_body(), 64 * 1024).await.map_err(|e| eyre::eyre!("reading placement's answer to {sha}: {e}"))?;
	match serde_json::from_slice(&body) {
		Ok(Theirs {
			code: code @ (Code::Unstudied | Code::Unavailable | Code::BadGateway),
			error,
		}) => Err(Refusal::new(code, error)),
		// anything else placement refuses is what the conductor built or signed
		_ => Err(eyre::eyre!("placement answered the definition of {sha} with {status}: {}", String::from_utf8_lossy(&body)).into()),
	}
}
