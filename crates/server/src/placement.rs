//! Placement's studies (docs/ARCHITECTURE.md, Placement): the conductor builds a study's
//! [`Definition`] from its own areas and trades, and is placement's one client, as the person asking.
//!
//! ```text
//! GET  /placement/catalog                  → placement GET /trades.json
//! GET  /placement/study?area&category      → Definition → placement POST /studies, the payload streamed back
//! POST /placement/study/pull?area&category → held, budget → placement POST /studies/pull?budget, charged by x-billed
//! ```

use axum::{
	Extension, Json,
	body::{Body, to_bytes},
	extract::State,
	http::{HeaderMap, HeaderValue, Method, StatusCode, header},
	response::{IntoResponse, Response},
};
use ev_lib::api::{
	GenContext, JsonSchema, OperationOutput,
	openapi::{MediaType, Operation, Response as Answer, StatusCode as Status},
};
use sa_auth::{Principal, Service};
use sa_core::{
	gbp::Country,
	placement::{self, BILLED, Billed, Catalog, Definition, Gcid, NEED},
	prices::{AdsRequest, DataForSeoTask, PlacementRequest, Spends},
};
use sa_panel::gmbs::GoogleCategory;
use serde::Deserialize;

use crate::{
	api::{ApiError, ApiResult, Params, Unstudied},
	forward::{self, Services},
	http::{ApiRoute, ApiState, Gate, Section},
	signin::Caller,
};

/// The most of placement's refusal read.
const REFUSAL: usize = 64 * 1024;

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let (read, spend) = (Gate::section(Section::Study), Gate::section(Section::Pull));
	vec![
		ApiRoute::new(M::GET, "/placement/catalog", read, catalog),
		ApiRoute::new(M::GET, "/placement/study", read.slow(), study),
		// a pull spends: not on a permission revoked a minute ago
		ApiRoute::new(M::POST, "/placement/study/pull", spend.fresh().slow(), pull),
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
			content: [("application/json".into(), MediaType::default())].into_iter().collect(),
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
	let body = to_bytes(res.into_body(), REFUSAL).await.map_err(|e| ApiError::BadGateway(format!("reading placement's catalog: {e}")))?;
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
/// balance affords, nothing bought; 409 while a pull's item is unpriced. What it bought is charged,
/// and owed back when the pull failed.
async fn pull(
	State(s): State<ApiState>,
	Spends(caller, _): Spends<(PlacementRequest, (AdsRequest, DataForSeoTask)), Caller>,
	Params(at): Params<StudyIn>,
) -> ApiResult<Payload> {
	let def = define(&s, &caller, at).await?;
	let sha = def.id();
	let held = s.panel.pull(caller.user_id).await??;
	let res = ask(&s, &caller, Method::POST, &format!("/studies/pull?budget={}", held.budget()), json(&def)).await?;
	let billed: Billed = match res.headers().get(BILLED) {
		Some(v) => serde_json::from_slice(v.as_bytes()).map_err(|e| eyre::eyre!("placement's {BILLED} of {sha} is {v:?}: {e}"))?,
		// placement says it on every pull it ran: a refusal without it never reached one
		None if !res.status().is_success() => Billed::new(),
		None => return Err(eyre::eyre!("placement answered the pull of {sha} without {BILLED}").into()),
	};
	let need = match res.headers().get(NEED) {
		Some(v) => Some(v.to_str().ok().and_then(|v| v.parse::<u64>().ok()).ok_or_else(|| eyre::eyre!("placement's {NEED} of {sha} is {v:?}"))?),
		None => None,
	};
	let failed = !res.status().is_success();
	match (res.status(), need) {
		(StatusCode::OK, None) => {
			held.paid(&billed, &sha, failed).await?;
			Ok(Payload(res))
		}
		(StatusCode::PAYMENT_REQUIRED, Some(need)) => {
			let why = held.unaffordable(need);
			held.paid(&billed, &sha, failed).await?;
			Err(ApiError::Unaffordable(why))
		}
		(status, need) => {
			let refused = refused(res, "/studies/pull").await;
			if need.is_some() {
				return Err(eyre::eyre!("placement answered the pull of {sha} {status} with {NEED}").into());
			}
			held.paid(&billed, &sha, failed).await?;
			Err(refused)
		}
	}
}

/// The study of `at`, as the conductor's own areas and the caller's trades say it.
async fn define(s: &ApiState, caller: &Caller, at: StudyIn) -> ApiResult<Definition> {
	let area = s.panel.area(&at.area).await?.ok_or(ApiError::NotFound)?;
	let country: Country = area
		.country
		.parse()
		.map_err(|_| ApiError::Unstudied(Unstudied::Unstudied, format!("{} is in {}, where nothing is studied", area.name, area.country)))?; // no Country: none of placement's bases
	let category = match at.category {
		None => None,
		Some(key) => {
			let (trade, of) = key
				.split_once('.')
				.and_then(|(t, c)| Some((t.parse::<i64>().ok()?, c.parse::<Country>().ok()?)))
				.ok_or_else(|| ApiError::BadRequest(format!("category {key:?}: not <trade id>.<country>")))?;
			let trades = s.panel.trades(caller.user_id).await?;
			let (trade, c) = trades
				.iter()
				.find(|t| t.id == trade)
				.and_then(|t| t.categories.iter().find(|c| c.country == of).map(|c| (t, c)))
				.ok_or(ApiError::NotFound)?;
			let named = format!("{} · {of}", trade.name);
			let study = c.study.clone().ok_or_else(|| ApiError::Unstudied(Unstudied::NoStudy, format!("{named} names no study")))?;
			let primary = c.google_primary.ok_or_else(|| ApiError::Unstudied(Unstudied::NoGoogleCategory, format!("{named} has no Google category")))?;
			if c.keywords.is_empty() {
				return Err(ApiError::Unstudied(Unstudied::NoKeywords, format!("{named} has no keyword")));
			}
			if of != country {
				return Err(ApiError::Unstudied(Unstudied::AnotherCountry, format!("{named} is not set up for {country}, where {} is", area.name)));
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
	let services: &Services = s.placement.as_ref().ok_or_else(|| ApiError::Unavailable("placement not connected".to_owned()))?;
	let headers = HeaderMap::from_iter([(header::CONTENT_TYPE, HeaderValue::from_static("application/json"))]);
	let user = forward::user(caller.clone(), Service::Placement);
	services.send(Service::Placement, method, path, headers, Some(Principal::User(user)), body).await.map_err(|e| {
		tracing::warn!(error = %e, "placement did not answer");
		ApiError::BadGateway("placement did not answer".to_owned())
	})
}

/// Placement's refusal of `path`, as the caller is told it.
async fn refused(res: Response, path: &str) -> ApiError {
	#[derive(Deserialize)]
	#[serde(rename_all = "snake_case")]
	enum Code {
		Unstudied,
		Forbidden,
		Unaffordable,
		Internal,
		#[serde(other)]
		Other,
	}
	#[derive(Deserialize)]
	struct Theirs {
		code: Code,
		error: String,
	}
	let status = res.status();
	let body = match to_bytes(res.into_body(), REFUSAL).await {
		Ok(b) => b,
		Err(e) => return ApiError::BadGateway(format!("reading placement's answer to {path}: {e}")),
	};
	match serde_json::from_slice::<Theirs>(&body) {
		Ok(Theirs { code: Code::Unstudied, error }) => ApiError::Unstudied(Unstudied::Unstudied, error),
		Ok(Theirs { code: Code::Forbidden, .. }) => ApiError::Forbidden,
		Ok(Theirs { code: Code::Internal, error }) => ApiError::BadGateway(format!("placement failed: {error}")),
		// an unaffordable pull carries x-need, handled before; anything else is what the conductor built or signed
		Ok(Theirs { code: Code::Unaffordable | Code::Other, .. }) | Err(_) => ApiError::Internal(eyre::eyre!("placement answered {path} with {status}: {}", String::from_utf8_lossy(&body))),
	}
}
