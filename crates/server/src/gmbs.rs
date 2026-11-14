//! The caller's gmails and the GBPs they manage (`sa_panel::gmbs`), open to every signed-in user:
//! each sees and changes only their own.
//!
//! ```text
//! GET    /api/v1/gmails               Gmails: theirs, and their GBPs; the removed left out
//! POST   /api/v1/gmails {gmail}       → 201 Gmail; 409 theirs already
//! PATCH  /api/v1/gmails/{id} {role}   → 200 Gmail
//! DELETE /api/v1/gmails/{id}          → 204 (removed; its GBPs with it)
//! GET    /api/v1/gmbs/resolve?place=&gmail=  Candidates; the caller's keys first, `gmail`'s
//!                                     before their others', then the shared ones they pay for
//! GET    /api/v1/gmbs/suggest?input=&gmail=  Suggestions: Places Autocomplete in the countries
//!                                     served, paid for as resolve
//! POST   /api/v1/gmbs {name, trade, state, gmail_id?, country?, location?, spots?}  → 201 Gmb, no
//!                                     place known (not verified): a draft by its spots, the others
//!                                     by a location; 409 the name taken
//! POST   /api/v1/gmails/{id}/gmbs {place_id, state, name?, country?, trade}  → 201 Gmb, on
//!                                     Google already; 409 the name or the place taken
//! PATCH  /api/v1/gmbs/{id} {name?, gmail_id?, phone?, trade?, areas?, hidden_address?,
//!        customers_visit?, film?}  → 200 Gmb; null clears; 409 a name not a draft's,
//!                                     a name or a phone another live GBP's
//! PUT    /api/v1/gmbs/{id}/state {state, place?}  → 200 Gmb; 400 a move State::next refuses
//! DELETE /api/v1/gmbs/{id}            → 204
//! PUT    /api/v1/gmbs/enabled {gmbs, enabled}  → 204; 404 one is not theirs, and none switched
//! PUT    /api/v1/gmbs/trade {gmbs, trade}  → 204; 404 one is not theirs, and none given it
//! GET    /api/v1/trades               Trades: theirs, each with its categories
//! POST   /api/v1/trades {name}        → 201 Trade; 409 the name theirs already, whatever its case
//! PATCH  /api/v1/trades/{id} {name}   → 200 Trade; 409
//! DELETE /api/v1/trades/{id}          → 204; 409 while a live GBP holds it
//! PUT    /api/v1/trades/{id}/{country} {keywords, google_primary, google_additional, study}
//!                                     → 200 Trade; the gcids Google's in that country
//! DELETE /api/v1/trades/{id}/{country}  → 204
//! GET    /api/v1/google-categories/{country}  GoogleCategories: Google's list there
//! PUT    /api/v1/gmbs/{id}/spots {spots}  → 200 Gmb; 409 not a draft
//! PUT    /api/v1/gmbs/{id}/location {lat, lng}  → 204; 409 Google has its point
//! DELETE /api/v1/gmbs/{id}/location   → 204; 409 no point placed by hand
//! PUT    /api/v1/gmbs/{id}/country {country}   → 204; 409 Google has its country
//! GET    /api/v1/map                  Map: what the browser loads Maps JS with; 404 without it
//! ```
//!
//! Before the import from review_archive, the writes answer 503.

use std::collections::BTreeMap;

use axum::{
	Extension, Json,
	extract::{
		Path, Query, State,
		rejection::{JsonRejection, QueryRejection},
	},
	http::{Method, StatusCode},
	response::{IntoResponse, Response},
};
use jiff::Timestamp;
use sa_core::gbp::{Country, GmailRole, State as GbpState};
use sa_panel::{
	Panel,
	gmbs::{Category, Edit, Existing, Gmail, Gmb, GoogleCategory, Trade, Unplaced},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
	api::{ApiError, ApiResult, body},
	http::{ApiRoute, ApiState},
	signin::{Caller, Freshness},
};

/// `GET /map`: the browser's Maps JS key, referrer-restricted.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "MapConfig")]
pub struct MapDto {
	pub key: String,
}

/// A GBP of the caller's.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Gmb")]
pub struct GmbDto {
	#[ts(type = "number")]
	pub id: i64,
	/// `null`: a draft no gmail manages yet.
	#[ts(type = "number | null")]
	pub gmail_id: Option<i64>,
	/// Google's place id; `null` while a draft, and maybe while verifying or suspended: off Maps.
	pub place_id: Option<String>,
	pub name: String,
	pub address: Option<String>,
	pub state: GbpState,
	/// The states it may move to.
	pub next: Vec<GbpState>,
	/// `by_hand`: set by the caller, Google having none.
	pub country: Option<sa_panel::gmbs::GbpCountry>,
	/// `+` and digits; one live GBP's.
	pub phone: Option<String>,
	/// Its category is this trade in its country.
	#[ts(type = "number | null")]
	pub trade_id: Option<i64>,
	/// Service areas, at most 20.
	pub areas: Vec<String>,
	/// Where its verification film is shot.
	pub hidden_address: Option<String>,
	/// `false`: a service-area business, its address hidden; `null` undecided.
	pub customers_visit: Option<bool>,
	/// verif_vid's film, recorded by hand.
	pub film: Option<String>,
	/// RFC 3339.
	pub created_at: String,
	/// The caller's switch; review_archive inherits it.
	pub enabled: bool,
	/// `null`: none on Google (a service-area place, a draft) and none placed yet. `placed`: on
	/// the map by hand.
	#[ts(type = "{ lat: number, lng: number, placed: boolean } | null")]
	pub location: Option<sa_panel::gmbs::Point>,
	/// A draft's candidate spots, in order, its location the first's; none past a draft.
	#[ts(type = "{ lat: number, lng: number, label: string | null, note: string | null }[]")]
	pub spots: Vec<sa_panel::gmbs::Spot>,
}

/// A trade of the caller's: Plumbing, set up in some countries.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Trade")]
pub struct TradeDto {
	#[ts(type = "number")]
	pub id: i64,
	pub name: String,
	/// RFC 3339.
	pub created_at: String,
	/// By country.
	pub categories: Vec<CategoryDto>,
}

/// A trade set up for a country: Plumbing · FR.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Category")]
pub struct CategoryDto {
	pub country: Country,
	/// The country's language, its keywords' and Google's form's: `fr`.
	pub lang: String,
	/// Lowercase, in the country's language, none naming a city: the terms every GBP of the
	/// category is compared on.
	pub keywords: Vec<String>,
	/// What Google's form is given as the category.
	pub google_primary: Option<GoogleCategoryDto>,
	/// At most 9, in the order they are entered into Google's form.
	pub google_additional: Vec<GoogleCategoryDto>,
	/// Placement's trade that studies it.
	pub study: Option<String>,
}

/// One of Google's categories.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "GoogleCategory")]
pub struct GoogleCategoryDto {
	/// `gcid:plumber`.
	pub id: String,
	/// By language: shown in the reader's, typed into Google's form in the country's.
	pub names: BTreeMap<String, String>,
}

/// `GET /google-categories/{country}`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "GoogleCategories")]
pub struct GoogleCategoriesDto {
	pub categories: Vec<GoogleCategoryDto>,
}

impl From<GoogleCategory> for GoogleCategoryDto {
	fn from(g: GoogleCategory) -> Self {
		Self {
			id: g.id.to_owned(),
			names: g.names.clone(),
		}
	}
}

/// `GET /trades`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Trades")]
pub struct TradesDto {
	pub trades: Vec<TradeDto>,
}

/// A gmail of the caller's.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Gmail")]
pub struct GmailDto {
	#[ts(type = "number")]
	pub id: i64,
	pub gmail: String,
	/// Optional, the caller's say: nothing here judges it.
	pub role: Option<GmailRole>,
	/// RFC 3339.
	pub created_at: String,
	/// Its browser set up, its last check through: the GBPs' `profile` must-have.
	pub browser: bool,
}

/// `GET /gmails`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Gmails")]
pub struct GmailsDto {
	pub gmails: Vec<GmailDto>,
	pub gmbs: Vec<GmbDto>,
}

/// A place a pasted input may mean.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Place")]
pub struct PlaceDto {
	pub place_id: String,
	/// Named and addressed only with the conductor's Places API key.
	pub name: Option<String>,
	pub address: Option<String>,
	/// `null`: a service-area place, or no key.
	#[ts(type = "{ lat: number, lng: number } | null")]
	pub location: Option<sa_places::LatLng>,
	/// `null`: none on Google, or no key.
	pub country: Option<Country>,
}

/// `GET /gmbs/resolve`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Candidates")]
pub struct CandidatesDto {
	pub candidates: Vec<PlaceDto>,
}

/// A place what is being typed may be heading for.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Suggestion")]
pub struct SuggestionDto {
	pub place_id: String,
	/// Its name, or the address's first line.
	pub main: String,
	/// The rest of its address.
	pub secondary: Option<String>,
}

/// `GET /gmbs/suggest`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Suggestions")]
pub struct SuggestionsDto {
	pub suggestions: Vec<SuggestionDto>,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/gmails", None, Cached, gmails),
		ApiRoute::new(M::POST, "/gmails", None, Cached, add_gmail),
		ApiRoute::new(M::PATCH, "/gmails/{id}", None, Cached, set_role),
		ApiRoute::new(M::DELETE, "/gmails/{id}", None, Cached, remove_gmail),
		ApiRoute::new(M::GET, "/gmbs/resolve", None, Cached, resolve),
		ApiRoute::new(M::GET, "/gmbs/suggest", None, Cached, suggest),
		ApiRoute::new(M::POST, "/gmbs", None, Cached, create_gmb),
		ApiRoute::new(M::POST, "/gmails/{id}/gmbs", None, Cached, add_gmb),
		ApiRoute::new(M::PATCH, "/gmbs/{id}", None, Cached, edit),
		ApiRoute::new(M::PUT, "/gmbs/{id}/state", None, Cached, advance),
		ApiRoute::new(M::DELETE, "/gmbs/{id}", None, Cached, remove_gmb),
		ApiRoute::new(M::PUT, "/gmbs/enabled", None, Cached, switch),
		ApiRoute::new(M::PUT, "/gmbs/trade", None, Cached, set_trades),
		ApiRoute::new(M::PUT, "/gmbs/{id}/spots", None, Cached, set_spots),
		ApiRoute::new(M::PUT, "/gmbs/{id}/location", None, Cached, place),
		ApiRoute::new(M::DELETE, "/gmbs/{id}/location", None, Cached, unplace),
		ApiRoute::new(M::PUT, "/gmbs/{id}/country", None, Cached, set_country),
		ApiRoute::new(M::GET, "/map", None, Cached, map),
		ApiRoute::new(M::GET, "/trades", None, Cached, trades),
		ApiRoute::new(M::POST, "/trades", None, Cached, add_trade),
		ApiRoute::new(M::PATCH, "/trades/{id}", None, Cached, rename_trade),
		ApiRoute::new(M::DELETE, "/trades/{id}", None, Cached, delete_trade),
		ApiRoute::new(M::PUT, "/trades/{id}/{country}", None, Cached, set_up),
		ApiRoute::new(M::DELETE, "/trades/{id}/{country}", None, Cached, remove_category),
		ApiRoute::new(M::GET, "/google-categories/{country}", None, Cached, google_categories),
	]
}

impl From<Gmb> for GmbDto {
	fn from(b: Gmb) -> Self {
		Self {
			id: b.id,
			gmail_id: b.gmail_id,
			place_id: b.place_id,
			name: b.name,
			address: b.address,
			state: b.state,
			next: b.state.nexts().to_vec(),
			country: b.country,
			phone: b.phone,
			trade_id: b.trade_id,
			areas: b.areas,
			hidden_address: b.hidden_address,
			customers_visit: b.customers_visit,
			film: b.film,
			created_at: b.created_at.to_string(),
			enabled: b.enabled,
			location: b.location,
			spots: b.spots,
		}
	}
}

impl From<Gmail> for GmailDto {
	fn from(g: Gmail) -> Self {
		Self {
			id: g.id,
			gmail: g.gmail,
			role: g.role,
			created_at: g.created_at.to_string(),
			browser: g.browser,
		}
	}
}

impl From<Trade> for TradeDto {
	fn from(t: Trade) -> Self {
		Self {
			id: t.id,
			name: t.name,
			created_at: t.created_at.to_string(),
			categories: t
				.categories
				.into_iter()
				.map(|c| CategoryDto {
					country: c.country,
					lang: c.country.lang().to_owned(),
					keywords: c.keywords,
					google_primary: c.google_primary.map(GoogleCategoryDto::from),
					google_additional: c.google_additional.into_iter().map(GoogleCategoryDto::from).collect(),
					study: c.study,
				})
				.collect(),
		}
	}
}

/// A path's id: one that is no number is no id of the caller's.
pub(crate) fn id(raw: &str) -> ApiResult<i64> {
	raw.parse().map_err(|_| ApiError::NotFound)
}

async fn gmails(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<GmailsDto>> {
	let (gmails, gmbs) = panel.gmails(caller.user_id).await?;
	Ok(Json(GmailsDto {
		gmails: gmails.into_iter().map(GmailDto::from).collect(),
		gmbs: gmbs.into_iter().map(GmbDto::from).collect(),
	}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGmail {
	gmail: String,
}

async fn add_gmail(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<NewGmail>, JsonRejection>) -> ApiResult<Response> {
	let added = panel.add_gmail(caller.user_id, &body(b)?.gmail, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = added.id, "gmail added");
	Ok((StatusCode::CREATED, Json(GmailDto::from(added))).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetRole {
	#[serde(deserialize_with = "Option::deserialize")] // required: `null` clears, a missing field is refused
	role: Option<GmailRole>,
}

async fn set_role(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetRole>, JsonRejection>) -> ApiResult<Json<GmailDto>> {
	let gmail = id(&raw)?;
	let role = body(b)?.role;
	let set = panel.set_gmail_role(caller.user_id, gmail, role).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, role = role.map(GmailRole::as_str), "gmail's role set");
	Ok(Json(GmailDto::from(set)))
}

async fn remove_gmail(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	panel.remove_gmail(caller.user_id, gmail, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail removed");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resolve {
	place: String,
	/// The caller's gmail whose key is tried first.
	gmail: Option<i64>,
}

async fn resolve(State(panel): State<Panel>, Extension(caller): Extension<Caller>, q: Result<Query<Resolve>, QueryRejection>) -> ApiResult<Json<CandidatesDto>> {
	let Query(q) = q.map_err(|e| ApiError::BadRequest(e.body_text()))?;
	let candidates = panel.place_candidates(caller.user_id, q.gmail, &q.place).await??;
	Ok(Json(CandidatesDto {
		candidates: candidates
			.into_iter()
			.map(|(place_id, p, country)| PlaceDto {
				place_id,
				name: p.as_ref().and_then(|p| p.name.clone()),
				address: p.as_ref().and_then(|p| p.address.clone()),
				location: p.and_then(|p| p.location),
				country,
			})
			.collect(),
	}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suggest {
	input: String,
	/// The caller's gmail whose key is tried first.
	gmail: Option<i64>,
}

async fn suggest(State(panel): State<Panel>, Extension(caller): Extension<Caller>, q: Result<Query<Suggest>, QueryRejection>) -> ApiResult<Json<SuggestionsDto>> {
	let Query(q) = q.map_err(|e| ApiError::BadRequest(e.body_text()))?;
	let found = panel.suggest(caller.user_id, q.gmail, &q.input).await??;
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

async fn map(State(s): State<ApiState>) -> ApiResult<Json<MapDto>> {
	s.map.map(Json).ok_or(ApiError::NotFound)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewUnplaced {
	name: String,
	trade: i64,
	state: String,
	gmail_id: Option<i64>,
	country: Option<String>,
	location: Option<sa_places::LatLng>,
	#[serde(default)]
	spots: Vec<sa_panel::gmbs::Spot>,
}

async fn create_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<NewUnplaced>, JsonRejection>) -> ApiResult<Response> {
	let NewUnplaced {
		name,
		trade,
		state,
		gmail_id,
		country,
		location,
		spots,
	} = body(b)?;
	let unplaced = Unplaced {
		name,
		trade,
		state: state.parse()?,
		gmail_id,
		country: country.as_deref().map(str::parse).transpose()?,
		at: location,
		spots,
	};
	let added = panel.create_gmb(caller.user_id, unplaced, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = added.id, state = %added.state, "GBP without a place created");
	Ok((StatusCode::CREATED, Json(GmbDto::from(added))).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewGmb {
	place_id: String,
	state: String,
	name: Option<String>,
	country: Option<String>,
	trade: i64,
}

async fn add_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<NewGmb>, JsonRejection>) -> ApiResult<Response> {
	let gmail = id(&raw)?;
	let NewGmb {
		place_id,
		state,
		name,
		country,
		trade,
	} = body(b)?;
	if !sa_places::parse::looks_like_place_id(&place_id) {
		return Err(ApiError::BadRequest(format!("{place_id:?} is not a place id")));
	}
	let existing = Existing {
		gmail_id: gmail,
		place: place_id,
		state: state.parse()?,
		name,
		country: country.as_deref().map(str::parse).transpose()?,
		trade,
	};
	let added = panel.add_gmb(Some(caller.user_id), existing, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, gmb_id = added.id, "GBP added");
	Ok((StatusCode::CREATED, Json(GmbDto::from(added))).into_response())
}

/// A present `null` is `Some(None)`; an absent field `None`, by `#[serde(default)]`.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
	Option::<T>::deserialize(d).map(Some)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditBody {
	name: Option<String>,
	gmail_id: Option<i64>,
	#[serde(default, deserialize_with = "present")]
	phone: Option<Option<String>>,
	#[serde(default, deserialize_with = "present")]
	trade: Option<Option<i64>>,
	#[serde(default, deserialize_with = "present")]
	areas: Option<Option<Vec<String>>>,
	#[serde(default, deserialize_with = "present")]
	hidden_address: Option<Option<String>>,
	#[serde(default, deserialize_with = "present")]
	customers_visit: Option<Option<bool>>,
	#[serde(default, deserialize_with = "present")]
	film: Option<Option<String>>,
}

async fn edit(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<EditBody>, JsonRejection>) -> ApiResult<Json<GmbDto>> {
	let gmb = id(&raw)?;
	let EditBody {
		name,
		gmail_id,
		phone,
		trade,
		areas,
		hidden_address,
		customers_visit,
		film,
	} = body(b)?;
	let edit = Edit {
		name,
		gmail_id,
		phone,
		trade,
		areas,
		hidden_address,
		customers_visit,
		film,
	};
	let edited = panel.edit_gmb(caller.user_id, gmb, edit).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GBP edited");
	Ok(Json(GmbDto::from(edited)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Advance {
	state: String,
	place: Option<String>,
}

async fn advance(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<Advance>, JsonRejection>) -> ApiResult<Json<GmbDto>> {
	let gmb = id(&raw)?;
	let Advance { state, place } = body(b)?;
	let to: GbpState = state.parse()?;
	let moved = panel.advance(Some(caller.user_id), gmb, to, place.as_deref()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, %to, "GBP's state moved");
	Ok(Json(GmbDto::from(moved)))
}

async fn remove_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmb = id(&raw)?;
	panel.remove_gmb(caller.user_id, gmb, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GMB removed");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Switch {
	gmbs: Vec<i64>,
	enabled: bool,
}

async fn switch(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<Switch>, JsonRejection>) -> ApiResult<StatusCode> {
	let Switch { gmbs, enabled } = body(b)?;
	panel.switch_gmbs(caller.user_id, &gmbs, enabled).await??;
	tracing::info!(user_id = %caller.user_id, ?gmbs, enabled, "GMBs switched");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetSpots {
	spots: Vec<sa_panel::gmbs::Spot>,
}

async fn set_spots(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetSpots>, JsonRejection>) -> ApiResult<Json<GmbDto>> {
	let gmb = id(&raw)?;
	let SetSpots { spots } = body(b)?;
	let set = panel.set_spots(caller.user_id, gmb, spots).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, spots = set.spots.len(), "draft's spots set");
	Ok(Json(GmbDto::from(set)))
}

async fn place(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<sa_places::LatLng>, JsonRejection>) -> ApiResult<StatusCode> {
	let gmb = id(&raw)?;
	let at = body(b)?;
	panel.place_gmb(caller.user_id, gmb, at).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, ?at, "GMB placed");
	Ok(StatusCode::NO_CONTENT)
}

async fn unplace(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmb = id(&raw)?;
	panel.unplace_gmb(caller.user_id, gmb).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GMB's placed point taken back");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetCountry {
	country: String,
}

async fn set_country(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetCountry>, JsonRejection>) -> ApiResult<StatusCode> {
	let gmb = id(&raw)?;
	let country: Country = body(b)?.country.parse()?;
	panel.set_country(Some(caller.user_id), gmb, country).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, %country, "GMB's country set");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetTrades {
	gmbs: Vec<i64>,
	trade: Option<i64>,
}

async fn set_trades(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<SetTrades>, JsonRejection>) -> ApiResult<StatusCode> {
	let SetTrades { gmbs, trade } = body(b)?;
	panel.set_trades(caller.user_id, &gmbs, trade).await??;
	tracing::info!(user_id = %caller.user_id, ?gmbs, ?trade, "GBPs given a trade");
	Ok(StatusCode::NO_CONTENT)
}

async fn trades(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<TradesDto>> {
	let trades = panel.trades(caller.user_id).await?;
	Ok(Json(TradesDto {
		trades: trades.into_iter().map(TradeDto::from).collect(),
	}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TradeName {
	name: String,
}

async fn add_trade(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<TradeName>, JsonRejection>) -> ApiResult<Response> {
	let added = panel.add_trade(caller.user_id, &body(b)?.name, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = added.id, "trade added");
	Ok((StatusCode::CREATED, Json(TradeDto::from(added))).into_response())
}

async fn rename_trade(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<TradeName>, JsonRejection>) -> ApiResult<Json<TradeDto>> {
	let trade = id(&raw)?;
	let renamed = panel.rename_trade(caller.user_id, trade, &body(b)?.name).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = trade, "trade renamed");
	Ok(Json(TradeDto::from(renamed)))
}

async fn delete_trade(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let trade = id(&raw)?;
	panel.delete_trade(caller.user_id, trade).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = trade, "trade deleted");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetUp {
	keywords: Vec<String>,
	google_primary: Option<String>,
	google_additional: Vec<String>,
	study: Option<String>,
}

async fn set_up(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Path((raw, country)): Path<(String, String)>,
	b: Result<Json<SetUp>, JsonRejection>,
) -> ApiResult<Json<TradeDto>> {
	let trade = id(&raw)?;
	let country: Country = country.parse()?;
	let SetUp {
		keywords,
		google_primary,
		google_additional,
		study,
	} = body(b)?;
	let google = |id: &str| GoogleCategory::find(country, id);
	let category = Category {
		country,
		keywords,
		google_primary: google_primary.as_deref().map(google).transpose()?,
		google_additional: google_additional.iter().map(|id| google(id)).collect::<Result<_, _>>()?,
		study,
	};
	let set = panel.set_up(caller.user_id, trade, category).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = trade, %country, "category set up");
	Ok(Json(TradeDto::from(set)))
}

async fn remove_category(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path((raw, country)): Path<(String, String)>) -> ApiResult<StatusCode> {
	let trade = id(&raw)?;
	let country: Country = country.parse()?;
	panel.remove_category(caller.user_id, trade, country).await??;
	tracing::info!(user_id = %caller.user_id, trade_id = trade, %country, "category removed");
	Ok(StatusCode::NO_CONTENT)
}

async fn google_categories(Path(country): Path<String>) -> ApiResult<Json<GoogleCategoriesDto>> {
	let country: Country = country.parse()?;
	Ok(Json(GoogleCategoriesDto {
		categories: GoogleCategory::all(country).map(GoogleCategoryDto::from).collect(),
	}))
}
