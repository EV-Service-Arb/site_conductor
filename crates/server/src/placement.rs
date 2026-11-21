//! Placement's studies (docs/ARCHITECTURE.md, Placement): the conductor builds a study's
//! [`Definition`] from its own areas and trades, and placement's library builds it, in process, out
//! of the work dir and this conductor's queries cache.
//!
//! ```text
//! GET  /placement/catalog                  → the countries placement holds a base for
//! GET  /placement/study?area&category      → Definition → its payload, built once and kept
//! POST /placement/study/pull?area&category → held: what the study lacks, each search bought, asked
//!                                            again until it lacks none → built again, charged what was bought
//! ```

use std::{
	collections::HashMap,
	path::PathBuf,
	sync::{Arc, Mutex},
	time::SystemTime,
};

use axum::{
	Extension, Json,
	body::Bytes,
	extract::State,
	http::{Method, header},
	response::{IntoResponse, Response},
};
use ev_lib::api::{
	GenContext, JsonSchema, OperationOutput, Params, Refusal,
	openapi::{MediaType, Operation, Response as Answer, SchemaObject, StatusCode as Status},
};
use gmaps_optimal_placement::{Held, Work};
use gmaps_optimal_placement_core::{Additional, Area, Ask, Category, Country, Definition, Gcid, LatLng, SEARCH_TEXT};
use sa_auth::Study;
use sa_core::{
	gbp,
	prices::{PlacementRequest, Spends},
	queries_cache::{Key, Scheme, gop},
};
use sa_panel::{gmbs::GoogleCategory, store::Store};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use strum::VariantArray;

use crate::{
	api::{ApiResult, Code},
	http::{ApiRoute, ApiState, Flags, Gate, SLOW, USER},
	signin::{Caller, Freshness},
};

/// A pull spends: not on a permission revoked a minute ago.
const PULL: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Fresh,
	slow: true,
});

/// Placement's work dir, and every payload built over it.
#[derive(Clone)]
pub struct Placement {
	dir: PathBuf,
	// shortcut: unbounded, one payload per area × category any operator opened; an LRU when the process grows
	built: Arc<Mutex<HashMap<String, Slot>>>,
}
impl Placement {
	/// Over the work dir at `dir` (`PANEL_PLACEMENT_DIR`).
	pub fn new(dir: PathBuf) -> Self {
		Self { dir, built: Arc::default() }
	}

	/// One study's: a pull and a read of it take turns.
	fn slot(&self, sha: &str) -> Slot {
		self.built.lock().expect("no holder panics").entry(sha.to_owned()).or_default().clone()
	}
}

type Slot = Arc<tokio::sync::Mutex<Option<Bytes>>>;

/// The answers this conductor's queries cache keeps, read from the blocking thread a build runs on.
struct Cached {
	store: Store,
	rt: tokio::runtime::Handle,
}
impl Held for Cached {
	fn held(&self, ask: &Ask) -> eyre::Result<Option<(serde_json::Value, SystemTime)>> {
		let key = Key(Sha256::digest(gop(SEARCH_TEXT, &ask.body)).into());
		Ok(self
			.rt
			.block_on(self.store.cached_query(Scheme::Gop, key))?
			.map(|kept| (kept.answer, SystemTime::from(kept.fetched_at))))
	}
}

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

/// The countries placement holds a base for.
#[derive(JsonSchema, Serialize)]
struct Catalog {
	countries: Vec<Country>,
}

/// A study's payload as placement serialised it: the home map's overlay reads it.
pub(crate) struct Payload(Bytes);

impl IntoResponse for Payload {
	fn into_response(self) -> Response {
		([(header::CONTENT_TYPE, "application/json")], self.0).into_response()
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

/// The countries placement holds a base for.
///
/// 503 placement not connected.
async fn catalog(State(s): State<ApiState>) -> ApiResult<Json<Catalog>> {
	connected(&s)?;
	Ok(Json(Catalog {
		countries: Country::VARIANTS.to_vec(),
	}))
}

/// A study of an area: under one of the caller's categories, or the area's grid alone; never spends.
/// Built on the first read and kept, out of what is held: a study whose searches are not all there
/// opens on what is, and its notice says how many it lacks.
///
/// 404 no such area or category of the caller's; 422 a category with `no_google_category`, set up
/// for `another_country`, or an area where placement holds no base (`unstudied`); 503 placement not
/// connected.
async fn study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Params(at): Params<StudyIn>) -> ApiResult<Payload> {
	let p = connected(&s)?.clone();
	let def = define(&s, &caller, at).await?;
	let slot = p.slot(&def.id());
	let mut slot = slot.lock().await;
	if let Some(built) = &*slot {
		return Ok(Payload(built.clone()));
	}
	let built = built(&s, &p, def).await?;
	*slot = Some(built.clone());
	Ok(Payload(built))
}

/// Builds a study again, buying the Places searches it lacks at the caller's tokens.
///
/// As the study is refused, and: 402 `unaffordable` when the sweep needs more searches than the
/// balance affords, nothing bought when that shows before the first; 409 while a search is
/// unpriced. What it bought is charged, and owed back when the pull failed.
async fn pull(State(s): State<ApiState>, Spends(caller, _): Spends<PlacementRequest, Caller>, Params(at): Params<StudyIn>) -> ApiResult<Payload> {
	let p = connected(&s)?.clone();
	let def = define(&s, &caller, at).await?;
	let sha = def.id();
	let slot = p.slot(&sha);
	let mut slot = slot.lock().await;
	let held = s.sign_in.panel.pull(caller.user_id).await??;
	let pulled = match bought(&s, &p, &def, &held).await {
		Ok(()) => built(&s, &p, def).await,
		Err(e) => Err(e),
	};
	held.paid(&sha, pulled.is_err()).await?;
	let built = pulled?;
	*slot = Some(built.clone());
	Ok(Payload(built))
}

/// Every search `def` lacks, bought: it is asked again after each round, since what lies behind a
/// page (the next one, a tile's quarters) is known only once it is answered.
async fn bought(s: &ApiState, p: &Placement, def: &Definition, held: &sa_panel::placement::Pull) -> ApiResult<()> {
	let since = held.since(&def.id()).await?;
	let mut asked = std::collections::HashSet::new();
	//LOOP: a repeated ask errors, so it ends once placement asks for nothing
	loop {
		let asks = blocking(s, p, Some(since), def.clone(), |study, work| study.asks(work)).await?;
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

/// `def` built, serialised, out of what is held.
async fn built(s: &ApiState, p: &Placement, def: Definition) -> ApiResult<Bytes> {
	blocking(s, p, None, def, |study, work| Ok(Bytes::from(serde_json::to_vec(&study.build(work)?)?))).await
}

/// `then` over `def`'s study, on a blocking thread: a build reads archives and the queries cache
/// synchronously. An answer fetched before `since` is lacking.
async fn blocking<T: Send + 'static>(
	s: &ApiState,
	p: &Placement,
	since: Option<jiff::Timestamp>,
	def: Definition,
	then: impl FnOnce(&gmaps_optimal_placement::Study, &Work) -> eyre::Result<T> + Send + 'static,
) -> ApiResult<T> {
	let held = Cached {
		store: s.sign_in.panel.store().clone(),
		rt: tokio::runtime::Handle::current(),
	};
	let dir = p.dir.clone();
	let done = tokio::task::spawn_blocking(move || {
		let work = Work::at(dir, Box::new(held)).since(since.map(SystemTime::from));
		then(&gmaps_optimal_placement::load(&def, Vec::new())?, &work)
	})
	.await
	.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
	Ok(done?)
}

fn connected(s: &ApiState) -> ApiResult<&Placement> {
	s.placement.as_ref().ok_or_else(|| Refusal::new(Code::Unavailable, "placement not connected".to_owned()))
}

/// The study of `at`, as the conductor's own areas and the caller's trades say it.
async fn define(s: &ApiState, caller: &Caller, at: StudyIn) -> ApiResult<Definition> {
	let area = s.sign_in.panel.area(&at.area).await?.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
	let unstudied = || Refusal::new(Code::Unstudied, format!("{} is in {}, where nothing is studied", area.name, area.country));
	let of_area: gbp::Country = area.country.parse().map_err(|_| unstudied())?;
	let country: Country = of_area.as_str().parse().map_err(|_| unstudied())?; // no Country: none of placement's bases
	let category = match at.category {
		None => None,
		Some(key) => {
			let (trade, of) = key
				.split_once('.')
				.and_then(|(t, c)| Some((t.parse::<i64>().ok()?, c.parse::<gbp::Country>().ok()?)))
				.ok_or_else(|| Refusal::new(Code::BadRequest, format!("category {key:?}: not <trade id>.<country>")))?;
			let trades = s.sign_in.panel.trades(caller.user_id).await?;
			let (trade, c) = trades
				.iter()
				.find(|t| t.id == trade)
				.and_then(|t| t.categories.iter().find(|c| c.country == of).map(|c| (t, c)))
				.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
			let named = format!("{} · {of}", trade.name);
			let primary = c.google_primary.ok_or_else(|| Refusal::new(Code::NoGoogleCategory, format!("{named} has no Google category")))?;
			if of != of_area {
				return Err(Refusal::new(Code::AnotherCountry, format!("{named} is not set up for {of_area}, where {} is", area.name)));
			}
			let gcid = |g: GoogleCategory| Gcid {
				id: g.id.to_owned(),
				name: g.names.get(of.lang()).expect("google_categories.json names each in its countries' languages").clone(),
			};
			Some(Category {
				trade: u64::try_from(trade.id).expect("a trade's id is a rowid"),
				name: trade.name.clone(),
				country,
				primary: gcid(primary),
				additional: c.google_additional.iter().map(|&(g, weight)| Additional { google: gcid(g), weight }).collect(),
				keywords: c.keywords.clone(),
				excluded: c.excluded.clone(),
				demand: c.demand.clone(),
			})
		}
	};
	let def = Definition {
		area: Area::Circle {
			slug: area.slug,
			country,
			center: LatLng {
				lat: area.center.lat,
				lng: area.center.lng,
			},
			radius_m: area.radius_m as f64,
			margin_m: 0.,
		},
		category,
	};
	def.check().map_err(|e| eyre::eyre!("the conductor built a definition placement refuses: {e}"))?;
	Ok(def)
}
