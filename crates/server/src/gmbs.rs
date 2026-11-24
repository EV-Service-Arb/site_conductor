//! The caller's gmails and the GBPs they manage (`sa_panel::gmbs`), open to every signed-in user:
//! each sees and changes only their own.
//!
//! Before the import from review_archive, the writes answer 503.

use std::collections::{BTreeMap, HashSet};

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, Created, Ids, JsonSchema, Params, Refusal};
use jiff::Timestamp;
use sa_core::{
	gbp::{Country, OnGoogle, Role, State as GbpState},
	prices::{PlacesRequest, Spends, StreetViewLoad},
};
use sa_panel::{
	Panel, Refused,
	categories::Mine,
	gmbs::{Edit, Existing, Gmail, Gmb, GoogleCategory, Note, Unplaced},
};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiResult, Code, Id},
	connections::CheckedDto,
	http::{ApiRoute, ApiState, USER},
	signin::Caller,
	tokens::BalanceDto,
	view::Tab,
};

/// `GET /map`: the Maps JS key this page loads on, referrer-restricted, and its account, which the
/// page's map loads and panoramas name.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MapConfig")]
pub struct MapDto {
	pub key: String,
	pub account: String,
}

/// A GBP of the caller's.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Gmb")]
pub struct GmbDto {
	pub id: i64,
	/// `null`: a draft no gmail manages yet.
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
	/// The gcid it is meant to have as its primary: its category is the caller's of it in its
	/// country. Google's own is `google_primary`.
	pub trade: Option<String>,
	/// Of its gmail and its category.
	pub brand_id: Option<i64>,
	/// Service areas, at most 20.
	pub areas: Vec<String>,
	/// Where its verification film is shot.
	pub hidden_address: Option<String>,
	/// `false`: a service-area business, its address hidden; `null` undecided.
	pub customers_visit: Option<bool>,
	/// verif_vid's film, recorded by hand.
	pub film: Option<String>,
	/// Its gmail's on it; `null` until said, and without a gmail.
	pub role: Option<Role>,
	/// A manager's: who owns it, for reference only.
	pub owner_gmail: Option<String>,
	/// RFC 3339.
	pub created_at: String,
	/// The caller's switch; review_archive inherits it.
	pub enabled: bool,
	/// `null`: none on Google (a service-area place, a draft) and none placed yet. `placed`: on
	/// the map by hand.
	pub location: Option<sa_panel::gmbs::Point>,
	/// A draft's candidate spots, in order, its location the first's; none past a draft.
	pub spots: Vec<sa_panel::gmbs::Spot>,
	/// Google's, from its last Sync, as `hours`, `google_primary` and `photos`.
	pub website: Option<String>,
	/// A line a weekday, Monday first.
	pub hours: Vec<String>,
	/// `gcid:plumber`, when the catalog knows Google's.
	pub google_primary: Option<String>,
	/// In Google's order, each at `GET /gmbs/{id}/photos/{photo}`.
	pub photos: Vec<sa_panel::gmbs::GmbPhoto>,
	/// RFC 3339: when a Sync last wrote it.
	pub synced_at: Option<String>,
	/// Its window's tabs, in order: its state's own work first.
	pub tabs: Vec<TabDto>,
	/// A draft's must-haves not ready; `null` past a draft.
	pub readiness: Option<ReadinessDto>,
}

/// What a draft needs before Google's form: the form's order, then what stands behind it.
#[derive(Clone, Copy, Debug, Eq, JsonSchema, PartialEq, Serialize, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
pub enum MustHave {
	Name,
	/// A trade, a country, and the caller's category of it there.
	Category,
	Areas,
	Phone,
	/// A landing page of sites' shows it.
	Site,
	/// The address its film is shot at, and whether customers visit.
	HiddenAddress,
	Film,
	Gmail,
	/// Its gmail's browser set up.
	Profile,
}

/// A draft's must-haves, in [`MustHave`]'s order, that are not ready.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Readiness")]
pub struct ReadinessDto {
	pub missing: Vec<MustHave>,
	/// Not known now: sites could not be read.
	pub unknown: Vec<MustHave>,
}

/// One of Google's categories.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "GoogleCategory")]
pub struct GoogleCategoryDto {
	/// `gcid:plumber`.
	pub id: String,
	/// By language: shown in the reader's, typed into Google's form in the country's.
	pub names: BTreeMap<String, String>,
}

/// `GET /google-categories/{country}`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "GoogleCategories")]
pub struct GoogleCategoriesDto {
	pub categories: Vec<GoogleCategoryDto>,
}

/// A gmail of the caller's.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Gmail")]
pub struct GmailDto {
	pub id: i64,
	pub gmail: String,
	/// RFC 3339.
	pub created_at: String,
	/// Its browser set up, its last check through: the GBPs' `profile` must-have.
	pub browser: bool,
	/// Its inbox's last check, as `Connections.imap.checked`; `null`: none set, its mail is not read.
	pub imap: Option<CheckedDto>,
	/// Its window's tabs, in order.
	pub tabs: Vec<Tab>,
}

/// `GET /gmails`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Gmails")]
pub struct GmailsDto {
	pub gmails: Vec<GmailDto>,
	pub gmbs: Vec<GmbDto>,
}

/// A place a pasted input may mean.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Place")]
pub struct PlaceDto {
	pub place_id: String,
	/// Named and addressed only with the conductor's Places API key.
	pub name: Option<String>,
	pub address: Option<String>,
	/// `null`: a service-area place, or no key.
	pub location: Option<sa_places::LatLng>,
	/// `null`: none on Google, or no key.
	pub country: Option<Country>,
}

/// `GET /gmbs/resolve`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Candidates")]
pub struct CandidatesDto {
	pub candidates: Vec<PlaceDto>,
}

/// A place what is being typed may be heading for.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Suggestion")]
pub struct SuggestionDto {
	pub place_id: String,
	/// Its name, or the address's first line.
	pub main: String,
	/// The rest of its address.
	pub secondary: Option<String>,
}

/// `GET /gmbs/suggest`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Suggestions")]
pub struct SuggestionsDto {
	pub suggestions: Vec<SuggestionDto>,
}

/// `GET /gmbs/{id}/log`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
pub struct LogDto {
	/// Newest first.
	pub events: Vec<LoggedDto>,
	/// `before` for the older page; `null` past the first event.
	pub next: Option<i64>,
}

/// A row of a GBP's log.
#[derive(Clone, Debug, JsonSchema, Serialize)]
pub struct LoggedDto {
	pub id: i64,
	/// RFC 3339.
	pub at: String,
	/// Who did it, by name; `null` for Google, or a run.
	pub by: Option<String>,
	/// The gmail it was under.
	pub gmail_id: Option<i64>,
	/// Its gmail's (a browser run for the gmail alone), not its own.
	pub of_gmail: bool,
	#[serde(flatten)]
	pub event: sa_panel::gmbs::Event,
}

/// A GBP's window's tab; a closed one is shown greyed with why.
#[derive(Clone, Debug, JsonSchema, Serialize)]
pub struct TabDto {
	pub tab: Tab,
	pub closed: Option<Closed>,
}

/// Why a tab of a GBP's window is closed in its state.
#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Closed {
	/// A draft has no place to measure yet.
	UntilVerified,
	/// Google holds it while it verifies.
	FrozenWhileVerifying,
	/// Suspended: it is off Maps.
	OffMaps,
}

/// A panorama to open, on the account the page's map loaded on.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PanoramaAt {
	lat: f64,
	lng: f64,
	account: String,
}

impl From<GoogleCategory> for GoogleCategoryDto {
	fn from(g: GoogleCategory) -> Self {
		Self {
			id: g.id.to_owned(),
			names: g.names.clone(),
		}
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/gmails", USER, gmails),
		ApiRoute::new(M::POST, "/gmails", USER, add_gmail),
		ApiRoute::new(M::DELETE, "/gmails/{id}", USER, remove_gmail),
		ApiRoute::new(M::GET, "/gmbs/resolve", USER, resolve_place),
		ApiRoute::new(M::GET, "/gmbs/suggest", USER, suggest_places),
		ApiRoute::new(M::POST, "/gmbs", USER, create_gmb),
		ApiRoute::new(M::POST, "/gmails/{id}/gmbs", USER, add_gmb),
		ApiRoute::new(M::PATCH, "/gmbs/{id}", USER, edit_gmb),
		ApiRoute::new(M::PUT, "/gmbs/{id}/state", USER, move_gmb),
		ApiRoute::new(M::POST, "/gmbs/{id}/sync", USER, sync_gmb),
		ApiRoute::new(M::GET, "/gmbs/{id}/photos/{photo}", USER, gmb_photo),
		ApiRoute::new(M::GET, "/gmbs/{id}/log", USER, gmb_log),
		ApiRoute::new(M::GET, "/gmbs/{id}/note", USER, gmb_note),
		ApiRoute::new(M::PUT, "/gmbs/{id}/note", USER, set_gmb_note),
		ApiRoute::new(M::DELETE, "/gmbs/{id}", USER, remove_gmb),
		ApiRoute::new(M::PUT, "/gmbs/enabled", USER, switch_gmbs),
		ApiRoute::new(M::PUT, "/gmbs/trade", USER, set_gmb_trades),
		ApiRoute::new(M::PUT, "/gmbs/{id}/spots", USER, set_spots),
		ApiRoute::new(M::PUT, "/gmbs/{id}/location", USER, place_gmb),
		ApiRoute::new(M::DELETE, "/gmbs/{id}/location", USER, unplace_gmb),
		ApiRoute::new(M::PUT, "/gmbs/{id}/country", USER, set_gmb_country),
		ApiRoute::new(M::GET, "/map", USER, map_key),
		ApiRoute::new(M::POST, "/map/street_view", USER, street_view),
		ApiRoute::new(M::GET, "/google-categories/{country}", USER, google_categories),
	]
}

/// What a GBP's readiness stands on beside the GBP itself, read once per answer.
struct Around {
	categories: Vec<Mine>,
	/// The caller's gmails with their browser set up.
	browsers: HashSet<i64>,
	/// The GBPs a landing page shows; `None`: sites could not be read.
	sites: Option<HashSet<i64>>,
}

impl Around {
	async fn read(s: &ApiState, caller: &Caller, gmails: Option<&[Gmail]>) -> ApiResult<Self> {
		let panel = &s.sign_in.panel;
		let browsers = match gmails {
			Some(gmails) => gmails.iter().filter(|g| g.browser).map(|g| g.id).collect(),
			None => panel.gmails(caller.user_id).await?.0.iter().filter(|g| g.browser).map(|g| g.id).collect(),
		};
		let sites = match &s.pages {
			// what sites cannot say now is unknown, not missing: `all` logs why
			Some(pages) => pages.all().await.ok().map(|places| places.iter().map(|p| p.gmb_id).collect()),
			None => None,
		};
		Ok(Self {
			categories: panel.categories(caller.user_id).await?,
			browsers,
			sites,
		})
	}

	fn dto(&self, b: Gmb) -> GmbDto {
		let readiness = (b.state == GbpState::Draft).then(|| self.readiness(&b));
		GmbDto {
			tabs: tabs(&b),
			readiness,
			id: b.id,
			gmail_id: b.gmail_id,
			place_id: b.place_id,
			name: b.name,
			address: b.address,
			state: b.state,
			next: b.state.nexts().to_vec(),
			country: b.country,
			phone: b.phone,
			trade: b.trade,
			brand_id: b.brand_id,
			areas: b.areas,
			hidden_address: b.hidden_address,
			customers_visit: b.customers_visit,
			film: b.film,
			role: b.role,
			owner_gmail: b.owner_gmail,
			created_at: b.created_at.to_string(),
			enabled: b.enabled,
			location: b.location,
			spots: b.spots,
			website: b.website,
			hours: b.hours,
			google_primary: b.google_primary,
			photos: b.photos,
			synced_at: b.synced_at.map(|t| t.to_string()),
		}
	}

	fn readiness(&self, b: &Gmb) -> ReadinessDto {
		use strum::VariantArray;
		let (mut missing, mut unknown) = (Vec::new(), Vec::new());
		for &must in MustHave::VARIANTS {
			let ready = match must {
				MustHave::Name => Some(true),
				MustHave::Category => Some(
					b.trade
						.as_deref()
						.zip(b.country)
						.is_some_and(|(trade, country)| self.categories.iter().any(|c| c.trade.id == trade && c.country == country.code)),
				),
				MustHave::Areas => Some(!b.areas.is_empty()),
				MustHave::Phone => Some(b.phone.is_some()),
				MustHave::Site => self.sites.as_ref().map(|shown| shown.contains(&b.id)),
				MustHave::HiddenAddress => Some(b.hidden_address.is_some() && b.customers_visit.is_some()),
				MustHave::Film => Some(b.film.is_some()),
				MustHave::Gmail => Some(b.gmail_id.is_some()),
				MustHave::Profile => Some(b.gmail_id.is_some_and(|g| self.browsers.contains(&g))),
			};
			match ready {
				Some(true) => {}
				Some(false) => missing.push(must),
				None => unknown.push(must),
			}
		}
		ReadinessDto { missing, unknown }
	}
}

/// A GBP's window's tabs: its state's work, its connections and its gmail's browser, then its profile and log.
fn tabs(b: &Gmb) -> Vec<TabDto> {
	let open = |tab| Some(TabDto { tab, closed: None });
	let closed = |tab, why| Some(TabDto { tab, closed: Some(why) });
	let measured = |why| [closed(Tab::Stats, why), closed(Tab::Ranking, why), closed(Tab::Reviews, why)];
	let work: [Option<TabDto>; 4] = match b.state {
		GbpState::Verified => [open(Tab::Stats), open(Tab::Ranking), open(Tab::Reviews), None],
		GbpState::Draft => {
			let [s, r, v] = measured(Closed::UntilVerified);
			[open(Tab::Launch), s, r, v]
		}
		GbpState::Verifying => {
			let [s, r, v] = measured(Closed::FrozenWhileVerifying);
			[open(Tab::Verification), s, r, v]
		}
		GbpState::Suspended => [open(Tab::Appeal), open(Tab::Stats), closed(Tab::Ranking, Closed::OffMaps), open(Tab::Reviews)],
	};
	let browser = b.gmail_id.and_then(|_| open(Tab::Browser));
	work.into_iter().chain([open(Tab::Connections), browser, open(Tab::Profile), open(Tab::Log)]).flatten().collect()
}

impl From<Gmail> for GmailDto {
	fn from(g: Gmail) -> Self {
		Self {
			id: g.id,
			gmail: g.gmail,
			created_at: g.created_at.to_string(),
			browser: g.browser,
			imap: g.imap.map(CheckedDto::from),
			tabs: vec![Tab::Connections, Tab::Browser],
		}
	}
}

/// A path's country.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct In {
	country: Country,
}

/// The caller's gmails and their GBPs, the removed left out.
///
/// A draft's `gmail_id` may be null; only a verified one's `place_id` is certain.
async fn gmails(State(s): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<Json<GmailsDto>> {
	let (gmails, gmbs) = s.sign_in.panel.gmails(caller.user_id).await?;
	let around = Around::read(&s, &caller, Some(&gmails)).await?;
	Ok(Json(GmailsDto {
		gmails: gmails.into_iter().map(GmailDto::from).collect(),
		gmbs: gmbs.into_iter().map(|b| around.dto(b)).collect(),
	}))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewGmail {
	gmail: String,
}

/// Adds a managing gmail. 409 if the caller has it already.
async fn add_gmail(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<NewGmail>) -> ApiResult<Created<GmailDto>> {
	let added = panel.add_gmail(caller.user_id, &b.gmail, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = added.id, "gmail added");
	Ok(Created(GmailDto::from(added)))
}

/// Removes the gmail to the trash, its GBPs with it. 409 while it has a browser.
async fn remove_gmail(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.remove_gmail(caller.user_id, gmail, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail removed");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Resolve {
	/// A place id, a Maps URL or a search.
	place: String,
	/// The caller's gmail whose key is tried first.
	gmail: Option<i64>,
}

/// The places a pasted id, Maps URL or search may mean (Google Places).
///
/// Paid with `gmail`'s Maps key first, then the caller's others', then the shared keys their
/// tokens pay for. 400 one in a country not served.
async fn resolve_place(State(panel): State<Panel>, Spends(_, by): Spends<PlacesRequest, Caller>, Params(q): Params<Resolve>) -> ApiResult<Json<CandidatesDto>> {
	let candidates = panel.place_candidates(by, q.gmail, &q.place).await??;
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Suggest {
	/// What is typed so far.
	input: String,
	/// The caller's gmail whose key is tried first.
	gmail: Option<i64>,
}

/// Places Autocomplete in the countries served, paid as `resolve`: each distinct input is a request.
async fn suggest_places(State(panel): State<Panel>, Spends(_, by): Spends<PlacesRequest, Caller>, Params(q): Params<Suggest>) -> ApiResult<Json<SuggestionsDto>> {
	let found = panel.suggest(by, q.gmail, &q.input).await??;
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

/// The Maps JS key this page loads on: of the accounts with a browser key, the first with free map
/// loads and panoramas left, else the first within its allowance. 404 when none has a browser key:
/// there is no map; 409 when none has room.
async fn map_key(State(s): State<ApiState>) -> ApiResult<Json<MapDto>> {
	if s.sign_in.panel.accounts().iter().all(|a| a.provides.browser.is_none()) {
		return Err(Refusal::new(Code::NotFound, "not found"));
	}
	let account = s
		.sign_in
		.panel
		.map_account(Timestamp::now())
		.await?
		.ok_or_else(|| Refusal::new(Code::Conflict, "every Google account's free map loads and allowance this month are used"))?;
	Ok(Json(MapDto {
		key: account.provides.browser.as_deref().expect("only an account with a browser key stands").to_owned(),
		account: account.name.clone(),
	}))
}

/// Buys the Street View panorama at a point on the page's account: recorded and charged
/// (`street_view.panorama`, the admin's price) in one write.
///
/// 400 an account without a browser key; 409 not priced, past the balance, or its meter there
/// stopped (with the account a reload would load on, when another has room).
async fn street_view(State(s): State<ApiState>, Spends(_, by): Spends<StreetViewLoad, Caller>, Body(at): Body<PanoramaAt>) -> ApiResult<Json<BalanceDto>> {
	let now = Timestamp::now();
	match s.sign_in.panel.open_street_view(by, &at.account, sa_places::LatLng { lat: at.lat, lng: at.lng }, now).await? {
		Ok(balance) => Ok(Json(BalanceDto { balance })),
		Err(Refused::Conflict(why)) => match s.sign_in.panel.map_account(now).await? {
			Some(other) if other.name != at.account => Err(Refusal::new(Code::Conflict, format!("{why}; reload the page: it then loads on {}, which has room", other.name))),
			_ => Err(Refusal::new(Code::Conflict, why)),
		},
		Err(refused) => Err(refused.into()),
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewUnplaced {
	name: String,
	/// A gcid Google lists in its country.
	trade: String,
	/// Draft, verifying or suspended.
	state: GbpState,
	/// Required past a draft.
	gmail_id: Option<i64>,
	/// Required past a draft; set by hand.
	country: Option<Country>,
	/// Placed by hand.
	location: Option<sa_places::LatLng>,
	/// A draft's candidate spots, 1–20, in order: its location the first's.
	#[serde(default)]
	spots: Vec<sa_panel::gmbs::Spot>,
	/// The caller's own note on it, as `PUT /gmbs/{id}/note`.
	note: Option<String>,
}

/// Creates a GBP no place is known of: a draft by its spots, or one verifying or suspended (off
/// Maps) by a location.
///
/// 409 if a live GBP has the name.
async fn create_gmb(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Body(b): Body<NewUnplaced>) -> ApiResult<Created<GmbDto>> {
	let unplaced = Unplaced {
		name: b.name,
		trade: b.trade,
		state: b.state,
		gmail_id: b.gmail_id,
		country: b.country,
		at: b.location,
		spots: b.spots,
		note: b.note.map(|text| Note { by: caller.person(), text }),
	};
	let added = s.sign_in.panel.create_gmb(caller.user_id, unplaced, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = added.id, state = %added.state, "GBP without a place created");
	Ok(Created(Around::read(&s, &caller, None).await?.dto(added)))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewGmb {
	place_id: String,
	state: OnGoogle,
	/// Fills what Google lacks.
	name: Option<String>,
	/// Fills what Google lacks.
	country: Option<Country>,
	/// A gcid Google lists in its country.
	trade: Option<String>,
}

/// Adds a GBP on Google already, by its place, to the gmail.
///
/// `name` and `country` fill what Google lacks (400 when still missing); 409 the name or the place
/// taken.
async fn add_gmb(State(s): State<ApiState>, Spends(caller, by): Spends<PlacesRequest, Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>, Body(b): Body<NewGmb>) -> ApiResult<Created<GmbDto>> {
	let NewGmb {
		place_id,
		state,
		name,
		country,
		trade,
	} = b;
	if !sa_places::parse::looks_like_place_id(&place_id) {
		return Err(Refusal::new(Code::BadRequest, format!("{place_id:?} is not a place id")));
	}
	let existing = Existing {
		gmail_id: gmail,
		place: place_id,
		state,
		name,
		country,
		trade,
	};
	let added = s.sign_in.panel.add_gmb(Some(by), existing, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, gmb_id = added.id, "GBP added");
	Ok(Created(Around::read(&s, &caller, None).await?.dto(added)))
}

/// A present `null` is `Some(None)`; an absent field `None`, by `#[serde(default)]`.
pub(crate) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<Option<T>>, D::Error> {
	Option::<T>::deserialize(d).map(Some)
}

/// Absent fields stay; `null` clears.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "GmbEdit")]
pub(crate) struct EditBody {
	/// A draft's only.
	name: Option<String>,
	/// Another of the caller's gmails: a transfer.
	gmail_id: Option<i64>,
	/// `+` and digits.
	#[serde(default, deserialize_with = "present")]
	phone: Option<Option<String>>,
	/// A gcid Google lists in its country.
	#[serde(default, deserialize_with = "present")]
	trade: Option<Option<String>>,
	#[serde(default, deserialize_with = "present")]
	areas: Option<Option<Vec<String>>>,
	#[serde(default, deserialize_with = "present")]
	hidden_address: Option<Option<String>>,
	#[serde(default, deserialize_with = "present")]
	customers_visit: Option<Option<bool>>,
	#[serde(default, deserialize_with = "present")]
	film: Option<Option<String>>,
	/// Its gmail's on it; needs a gmail, and a transfer clears it.
	#[serde(default, deserialize_with = "present")]
	role: Option<Option<Role>>,
	/// A manager's only: who owns it, an address.
	#[serde(default, deserialize_with = "present")]
	owner_gmail: Option<Option<String>>,
}

/// Edits a GBP.
///
/// 409 a name not a draft's, a name or a phone another live GBP's.
async fn edit_gmb(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Body(b): Body<EditBody>) -> ApiResult<Json<GmbDto>> {
	let EditBody {
		name,
		gmail_id,
		phone,
		trade,
		areas,
		hidden_address,
		customers_visit,
		film,
		role,
		owner_gmail,
	} = b;
	let edit = Edit {
		name,
		gmail_id,
		phone,
		trade,
		areas,
		hidden_address,
		customers_visit,
		film,
		role,
		owner_gmail,
	};
	let edited = s.sign_in.panel.edit_gmb(caller.user_id, gmb, edit).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GBP edited");
	Ok(Json(Around::read(&s, &caller, None).await?.dto(edited)))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Advance {
	/// One of the GBP's `next`.
	state: GbpState,
	/// Pasted as to `resolve`: taken while the GBP has none, needed to become verified.
	place: Option<String>,
}

/// Moves a GBP to another state. 400 a move its `next` lacks.
async fn move_gmb(
	State(s): State<ApiState>,
	Spends(caller, by): Spends<PlacesRequest, Caller>,
	Ids(Id { id: gmb }): Ids<Id<i64>>,
	Body(Advance { state: to, place }): Body<Advance>,
) -> ApiResult<Json<GmbDto>> {
	let moved = s.sign_in.panel.advance(Some(by), gmb, to, place.as_deref()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, %to, "GBP's state moved");
	Ok(Json(Around::read(&s, &caller, None).await?.dto(moved)))
}

/// Overwrites a verified GBP with its profile on Google, asked afresh, paid by the caller: name,
/// address, phone, website, hours, primary category, country, point (none on Google: one placed by
/// hand kept), photos. With no trade, it takes the caller's one trade set up there with that category.
///
/// 409 for a GBP not verified, a name or phone another live GBP has, or a place Google now names
/// otherwise.
async fn sync_gmb(State(s): State<ApiState>, Spends(caller, by): Spends<PlacesRequest, Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>) -> ApiResult<Json<GmbDto>> {
	let synced = s.sign_in.panel.sync(by, gmb).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, photos = synced.photos.len(), "GBP synced from Google");
	Ok(Json(Around::read(&s, &caller, None).await?.dto(synced)))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PhotoIn {
	id: i64,
	photo: i64,
}

/// A photo a GBP's last Sync took, never another under its id.
async fn gmb_photo(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(PhotoIn { id, photo }): Ids<PhotoIn>) -> ApiResult<crate::kit::Picture<crate::kit::Jpeg>> {
	Ok(crate::kit::Picture(panel.gmb_photo(caller.user_id, id, photo).await??, crate::kit::Jpeg))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LogQuery {
	/// The previous page's `next`.
	before: Option<i64>,
	/// Comma-separated; none, every kind.
	kinds: Option<String>,
}

/// A GBP's log, newest first, a page at a time: what changed of it (each field before and after) and what was done
/// with it, in the write that did it; and its gmails' own while it was under them.
async fn gmb_log(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Params(q): Params<LogQuery>) -> ApiResult<Json<LogDto>> {
	let kinds = q
		.kinds
		.iter()
		.flat_map(|k| k.split(','))
		.map(|k| {
			k.parse::<sa_panel::gmbs::EventKind>()
				.map_err(|_| Refusal::new(Code::BadRequest, format!("no kind of event {k:?}")))
		})
		.collect::<Result<Vec<_>, _>>()?;
	let page = panel.gmb_log(caller.user_id, gmb, q.before, &kinds).await??;
	let full = page.len() == usize::try_from(sa_panel::gmbs::LOG_PAGE).expect("a page fits");
	Ok(Json(LogDto {
		next: page.last().filter(|_| full).map(|l| l.id),
		events: page
			.into_iter()
			.map(|l| LoggedDto {
				id: l.id,
				at: l.at.to_string(),
				by: l.by,
				gmail_id: l.gmail_id,
				of_gmail: l.of_gmail,
				event: l.event,
			})
			.collect(),
	}))
}

/// What the caller noted on a GBP: theirs alone, read by nobody else.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "GmbNote")]
pub(crate) struct NoteDto {
	/// Plain text; `null`: none.
	note: Option<String>,
}

/// The caller's own note on a GBP. While acting as another, the actor's own on it, never the
/// subject's.
async fn gmb_note(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>) -> ApiResult<Json<NoteDto>> {
	Ok(Json(NoteDto {
		note: panel.gmb_note(caller.user_id, caller.person(), gmb).await??,
	}))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetNote {
	#[schemars(length(max = 4000))]
	note: Option<String>,
}

/// Writes the caller's own note on a GBP, in any state; `null` takes it away. Nobody else reads it,
/// and it is not in the GBP's log.
///
/// 400 empty or past 4000 characters.
async fn set_gmb_note(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Body(SetNote { note }): Body<SetNote>) -> ApiResult<NoContent> {
	panel.set_gmb_note(caller.user_id, caller.person(), gmb, note.as_deref()).await??;
	tracing::info!(user_id = %caller.person(), gmb_id = gmb, "note on a GBP written");
	Ok(NoContent)
}

/// Removes a GBP to the trash.
async fn remove_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.remove_gmb(caller.user_id, gmb, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GMB removed");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Switch {
	gmbs: Vec<i64>,
	enabled: bool,
}

/// Switches GBPs on or off, every one or none (404 one not the caller's); review_archive inherits it.
async fn switch_gmbs(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(Switch { gmbs, enabled }): Body<Switch>) -> ApiResult<NoContent> {
	panel.switch_gmbs(caller.user_id, &gmbs, enabled).await??;
	tracing::info!(user_id = %caller.user_id, ?gmbs, enabled, "GMBs switched");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetSpots {
	spots: Vec<sa_panel::gmbs::Spot>,
}

/// Replaces a draft's candidate spots, its location the first's. 409 not a draft.
async fn set_spots(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Body(SetSpots { spots }): Body<SetSpots>) -> ApiResult<Json<GmbDto>> {
	let set = s.sign_in.panel.set_spots(caller.user_id, gmb, spots).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, spots = set.spots.len(), "draft's spots set");
	Ok(Json(Around::read(&s, &caller, None).await?.dto(set)))
}

/// Places a GBP's point by hand. 409 Google has its point.
async fn place_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Body(at): Body<sa_places::LatLng>) -> ApiResult<NoContent> {
	panel.place_gmb(caller.user_id, gmb, at).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, ?at, "GMB placed");
	Ok(NoContent)
}

/// Takes back the point placed by hand. 409 none is.
async fn unplace_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.unplace_gmb(caller.user_id, gmb).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GMB's placed point taken back");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetCountry {
	country: Country,
}

/// Sets a GBP's country by hand. 409 Google has its country.
async fn set_gmb_country(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id: gmb }): Ids<Id<i64>>,
	Body(SetCountry { country }): Body<SetCountry>,
) -> ApiResult<NoContent> {
	panel.set_country(Some(caller.user_id), gmb, country).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, %country, "GMB's country set");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetTrades {
	gmbs: Vec<i64>,
	/// A gcid Google lists in each one's country; `null` takes their trade away.
	trade: Option<String>,
}

/// Gives GBPs a trade, every one or none (400 a gcid Google does not list in one's country, 404
/// one not the caller's).
async fn set_gmb_trades(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(SetTrades { gmbs, trade }): Body<SetTrades>) -> ApiResult<NoContent> {
	panel.set_trades(caller.user_id, &gmbs, trade.as_deref()).await??;
	tracing::info!(user_id = %caller.user_id, ?gmbs, ?trade, "GBPs given a trade");
	Ok(NoContent)
}

/// Google's categories in a country, named in its language and the front end's.
async fn google_categories(Ids(In { country }): Ids<In>) -> ApiResult<Json<GoogleCategoriesDto>> {
	Ok(Json(GoogleCategoriesDto {
		categories: GoogleCategory::all(country).map(GoogleCategoryDto::from).collect(),
	}))
}
