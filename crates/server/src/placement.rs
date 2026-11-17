//! Placement's studies (docs/ARCHITECTURE.md, Placement): the conductor builds a study's
//! [`Definition`] from its own areas and trades, and is placement's one client, as the person asking.
//!
//! ```text
//! GET  /placement/catalog                  → placement GET /trades.json
//! GET  /placement/study?area&category      → Definition → placement POST /studies, the payload streamed back
//! POST /placement/study/pull?area&category → held: placement POST /studies/asks, each search bought,
//!                                            until it lacks none → POST /studies/pull, charged what was bought
//! ```

use axum::{
	Extension, Json,
	body::{Body, to_bytes},
	extract::State,
	http::{HeaderMap, HeaderValue, Method, StatusCode, header},
	response::{IntoResponse, Response},
};
use ev_lib::api::{
	GenContext, JsonSchema, OperationOutput, Params, Refusal,
	openapi::{MediaType, Operation, Response as Answer, SchemaObject, StatusCode as Status},
};
use sa_auth::{Principal, Service, Study};
use sa_core::{
	gbp::Country,
	placement::{self, Asks, Catalog, Definition, Gcid},
	prices::{PlacementRequest, Spends},
};
use sa_panel::gmbs::GoogleCategory;
use serde::Deserialize;

use crate::{
	api::{ApiResult, Code},
	forward::{self, Services},
	http::{ApiRoute, ApiState, Flags, Gate, SLOW, USER},
	signin::{Caller, Freshness},
};

/// The most of placement's refusal read.
const REFUSAL: usize = 64 * 1024;
/// The most of placement's asks read: a round of a sweep is a few hundred searches.
const ASKS: usize = 4 * 1024 * 1024;

/// A pull spends: not on a permission revoked a minute ago.
const PULL: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Fresh,
	slow: true,
});

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/placement/catalog", USER.needs(Study::Read), catalog),
		ApiRoute::new(M::GET, "/placement/study", SLOW.needs(Study::Read), study),
		ApiRoute::new(M::POST, "/placement/study/pull", PULL.needs(Study::Spend), pull),
	]
}

#[derive(Deserialize, JsonSchema)]
struct StudyIn {
	/// The area's slug.
	area: String,
	/// `<trade id>.<country>`: one of the caller's trades as set up for a country. None: the area's
	/// grid alone, nothing billed.
	category: Option<String>,
}

/// A study's payload as placement wrote it, streamed through unread: the home map's overlay reads it.
pub(crate) struct Payload(Response);

impl IntoResponse for Payload {
	fn into_response(self) -> Response {
		self.0
	}
}

impl OperationOutput for Payload {
	type Inner = Self;

	fn operation_response(_: &mut GenContext, _: &mut Operation) -> Option<Answer> {
		Some(Answer {
			description: "placement's payload, for the map's overlay.".to_owned(),
			// any JSON: the overlay reads it, against the core it was built with
			content: [(
				"application/json".into(),
				MediaType {
					schema: Some(SchemaObject {
						json_schema: ev_lib::api::schemars::json_schema!({}),
						example: None,
						external_docs: None,
					}),
					..MediaType::default()
				},
			)]
			.into_iter()
			.collect(),
			..Answer::default()
		})
	}

	fn inferred_responses(ctx: &mut GenContext, op: &mut Operation) -> Vec<(Option<Status>, Answer)> {
		Self::operation_response(ctx, op).map(|r| (Some(Status::Code(200)), r)).into_iter().collect()
	}
}

/// What a category may be studied by, and where: placement's trade files and the countries it has
/// a base for.
async fn catalog(State(s): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<Json<Catalog>> {
	let res = ask(&s, &caller, Method::GET, "/trades.json", Body::empty()).await?;
	if res.status() != StatusCode::OK {
		return Err(refused(res, "/trades.json").await);
	}
	let body = to_bytes(res.into_body(), REFUSAL).await.map_err(|e| Refusal::new(Code::BadGateway, format!("reading placement's catalog: {e}")))?;
	Ok(Json(serde_json::from_slice(&body).map_err(|e| eyre::eyre!("placement's catalog: {e}"))?))
}

/// A study of an area: under one of the caller's categories, or the area's grid alone; never spends.
///
/// 404 no such area or category of the caller's; 422 a category not set up to be studied
/// (`no_study`, `no_google_category`, `no_keywords`), set up for `another_country`, or one
/// placement does not study (`unstudied`); 503 placement not connected.
async fn study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Params(at): Params<StudyIn>) -> ApiResult<Payload> {
	let def = define(&s, &caller, at).await?;
	let res = ask(&s, &caller, Method::POST, "/studies", json(&def)).await?;
	match res.status() {
		StatusCode::OK => Ok(Payload(res)),
		_ => Err(refused(res, "/studies").await),
	}
}

/// Builds a study again, buying the Places searches it lacks at the caller's tokens.
///
/// As the study is refused, and: 402 `unaffordable` when the sweep needs more searches than the
/// balance affords, nothing bought when that shows before the first; 409 while a search is
/// unpriced. What it bought is charged, and owed back when the pull failed.
async fn pull(State(s): State<ApiState>, Spends(caller, _): Spends<PlacementRequest, Caller>, Params(at): Params<StudyIn>) -> ApiResult<Payload> {
	let def = define(&s, &caller, at).await?;
	let held = s.sign_in.panel.pull(caller.user_id).await??;
	let pulled = match bought(&s, &caller, &def, &held).await {
		Ok(()) => ask(&s, &caller, Method::POST, "/studies/pull", json(&def)).await,
		Err(e) => Err(e),
	};
	let (failed, answer) = match pulled {
		Ok(res) if res.status() == StatusCode::OK => (false, Ok(Payload(res))),
		Ok(res) => (true, Err(refused(res, "/studies/pull").await)),
		Err(e) => (true, Err(e)),
	};
	held.paid(&def.id(), failed).await?;
	answer
}

/// Every search placement says `def` lacks, bought: it is asked again after each round, since what
/// lies behind a page (the next one, a tile's quarters) is known only once it is answered.
async fn bought(s: &ApiState, caller: &Caller, def: &Definition, held: &sa_panel::placement::Pull) -> ApiResult<()> {
	let mut asked = std::collections::HashSet::new();
	loop {
		let res = ask(s, caller, Method::POST, "/studies/asks", json(def)).await?;
		if res.status() != StatusCode::OK {
			return Err(refused(res, "/studies/asks").await);
		}
		let body = to_bytes(res.into_body(), ASKS).await.map_err(|e| Refusal::new(Code::BadGateway, format!("reading placement's asks: {e}")))?;
		let Asks { since, asks } = serde_json::from_slice(&body).map_err(|e| eyre::eyre!("placement's asks of {}: {e}", def.id()))?;
		if asks.is_empty() {
			return Ok(());
		}
		let need = u64::try_from(asks.len()).expect("a count of searches");
		if asked.is_empty() && need > held.budget() {
			return Err(Refusal::new(Code::Unaffordable, held.unaffordable(need)));
		}
		for a in &asks {
			// what was bought is answered, so asking it again would never end
			if !asked.insert(a.body.clone()) {
				return Err(eyre::eyre!("placement asked again for {} once it was bought", a.body).into());
			}
			held.buy(a, since).await.map_err(|e| match e {
				sa_places::Error::Unpaid(why) => Refusal::new(Code::Unaffordable, why),
				e @ sa_places::Error::Unbought => Refusal::new(Code::Conflict, e.to_string()),
				e @ (sa_places::Error::Api { .. } | sa_places::Error::Exhausted(_)) => Refusal::new(Code::BadGateway, format!("{e}")),
				e => eyre::eyre!(e).wrap_err("buying a study's search").into(),
			})?;
		}
	}
}

/// The study of `at`, as the conductor's own areas and the caller's trades say it.
async fn define(s: &ApiState, caller: &Caller, at: StudyIn) -> ApiResult<Definition> {
	let area = s.sign_in.panel.area(&at.area).await?.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
	let country: Country = area
		.country
		.parse()
		.map_err(|_| Refusal::new(Code::Unstudied, format!("{} is in {}, where nothing is studied", area.name, area.country)))?; // no Country: none of placement's bases
	let category = match at.category {
		None => None,
		Some(key) => {
			let (trade, of) = key
				.split_once('.')
				.and_then(|(t, c)| Some((t.parse::<i64>().ok()?, c.parse::<Country>().ok()?)))
				.ok_or_else(|| Refusal::new(Code::BadRequest, format!("category {key:?}: not <trade id>.<country>")))?;
			let trades = s.sign_in.panel.trades(caller.user_id).await?;
			let (trade, c) = trades
				.iter()
				.find(|t| t.id == trade)
				.and_then(|t| t.categories.iter().find(|c| c.country == of).map(|c| (t, c)))
				.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
			let named = format!("{} · {of}", trade.name);
			let study = c.study.clone().ok_or_else(|| Refusal::new(Code::NoStudy, format!("{named} names no study")))?;
			let primary = c.google_primary.ok_or_else(|| Refusal::new(Code::NoGoogleCategory, format!("{named} has no Google category")))?;
			if c.keywords.is_empty() {
				return Err(Refusal::new(Code::NoKeywords, format!("{named} has no keyword")));
			}
			if of != country {
				return Err(Refusal::new(Code::AnotherCountry, format!("{named} is not set up for {country}, where {} is", area.name)));
			}
			let gcid = |g: GoogleCategory| Gcid {
				id: g.id.to_owned(),
				name: g.names.get(of.lang()).expect("google_categories.json names each in its countries' languages").clone(),
			};
			Some(placement::Category {
				trade: u64::try_from(trade.id).expect("a trade's id is a rowid"),
				country: of,
				study,
				keywords: c.keywords.clone(),
				google_primary: gcid(primary),
				google_additional: c.google_additional.iter().copied().map(gcid).collect(),
			})
		}
	};
	let def = Definition {
		area: placement::Area::Circle {
			slug: area.slug,
			country,
			center: area.center,
			radius_m: area.radius_m as f64,
			margin_m: 0.,
		},
		category,
	};
	def.check().map_err(|e| eyre::eyre!("the conductor built a definition placement refuses: {e}"))?;
	Ok(def)
}

fn json(def: &Definition) -> Body {
	Body::from(serde_json::to_vec(def).expect("a definition serialises"))
}

/// `method path` of placement, as `caller`.
async fn ask(s: &ApiState, caller: &Caller, method: Method, path: &str, body: Body) -> ApiResult<Response> {
	let services: &Services = s.placement.as_ref().ok_or_else(|| Refusal::new(Code::Unavailable, "placement not connected".to_owned()))?;
	let headers = HeaderMap::from_iter([(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))]);
	let user = forward::user(caller.clone(), Service::Placement);
	services.send(Service::Placement, method, path, headers, Some(Principal::User(user)), body).await.map_err(|e| {
		tracing::warn!(error = %e, "placement did not answer");
		Refusal::new(Code::BadGateway, "placement did not answer".to_owned())
	})
}

/// Placement's refusal of `path`, as the caller is told it.
async fn refused(res: Response, path: &str) -> Refusal<Code> {
	#[derive(Deserialize)]
	#[serde(rename_all = "snake_case")]
	enum Theirs {
		Unstudied,
		Forbidden,
		Internal,
		#[serde(other)]
		Other,
	}
	#[derive(Deserialize)]
	struct Refused {
		code: Theirs,
		error: String,
	}
	let status = res.status();
	let body = match to_bytes(res.into_body(), REFUSAL).await {
		Ok(b) => b,
		Err(e) => return Refusal::new(Code::BadGateway, format!("reading placement's answer to {path}: {e}")),
	};
	match serde_json::from_slice::<Refused>(&body) {
		Ok(Refused { code: Theirs::Unstudied, error }) => Refusal::new(Code::Unstudied, error),
		Ok(Refused { code: Theirs::Forbidden, .. }) => Refusal::new(Code::Forbidden, "your permissions do not allow this"),
		Ok(Refused { code: Theirs::Internal, error }) => Refusal::new(Code::BadGateway, format!("placement failed: {error}")),
		// anything else is what the conductor built or signed
		Ok(Refused { code: Theirs::Other, .. }) | Err(_) => Refusal::from(eyre::eyre!("placement answered {path} with {status}: {}", String::from_utf8_lossy(&body))),
	}
}
