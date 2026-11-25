//! Placement's studies (docs/ARCHITECTURE.md, Placement): the conductor builds a study's
//! [`Definition`] from one of its areas and a category version, and placement's library builds it,
//! in process, out of the work dir and this conductor's queries cache. A published study is a
//! definition kept frozen, built the same way.
//!
//! ```text
//! GET  /placement/catalog                   → the countries placement holds a base for
//! GET  /placement/study?area&version        → Definition → its payload, built once and kept
//! POST /placement/study/pull?area&version   → held: what the study lacks, each search bought, asked
//!                                             again until it lacks none → built again, charged what was bought
//! GET  /placement/studies/{sha}             a published one, as the study above
//! POST /placement/studies/{sha}/pull        as the pull above, charged to whoever pulls
//! POST /placement/pull                      a Definition handed in whole, as the pull above; its end
//!                                             an event, the stream kept alive until it
//! POST /library/studies                     publish: on a released version, or releasing it
//! GET  /library/studies?trade&country&area
//! POST /library/studies/{sha}/withdraw
//! ```

use std::{
	collections::HashMap,
	convert::Infallible,
	path::PathBuf,
	sync::{Arc, Mutex},
	time::{Duration, SystemTime},
};

use axum::{
	Extension, Json,
	body::Bytes,
	extract::State,
	http::{Method, header},
	response::{
		IntoResponse, NoContent, Response,
		sse::{Event, KeepAlive, Sse},
	},
};
use ev_lib::api::{
	Body, Created, GenContext, Ids, JsonSchema, OperationOutput, Params, Refusal,
	openapi::{MediaType, Operation, Response as Answer, SchemaObject, StatusCode as Status},
};
use gmaps_optimal_placement::{Held, Work};
use gmaps_optimal_placement_core::{Area, Ask, Country, Definition, LatLng, SEARCH_TEXT};
use jiff::Timestamp;
use sa_auth::Study;
use sa_core::{
	gbp,
	prices::{PlacementRequest, Spends},
	queries_cache::{Key, Scheme, gop},
};
use sa_panel::{
	areas::MAX_RADIUS_M,
	library::{NewListing, Release},
	store::Store,
};
use serde::{Deserialize, Serialize};
use strum::VariantArray;

use crate::{
	api::{ApiResult, Code},
	http::{ApiRoute, ApiState, Flags, Gate, SLOW, USER},
	library::PublishedStudyDto,
	signin::{Caller, Freshness},
};

/// A pull spends: not on a permission revoked a minute ago.
const PULL: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Fresh,
	slow: true,
});

/// As every slow route's: a stream outlives the bound on its answer.
const PULL_BOUND: Duration = Duration::from_secs(600);

type Slot = Arc<tokio::sync::Mutex<Option<Bytes>>>;

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

/// The answers this conductor's queries cache keeps, read from the blocking thread a build runs on.
struct Cached {
	store: Store,
	rt: tokio::runtime::Handle,
}
impl Held for Cached {
	fn held(&self, ask: &Ask) -> eyre::Result<Option<(serde_json::Value, SystemTime)>> {
		let key = Key::of(&gop(SEARCH_TEXT, &ask.body));
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
		ApiRoute::new(M::GET, "/placement/studies/{sha}", SLOW.needs(Study::Read), published_study),
		ApiRoute::new(M::POST, "/placement/studies/{sha}/pull", PULL.needs(Study::Spend), pull_published_study),
		ApiRoute::new(M::POST, "/placement/pull", PULL.needs(Study::Spend), pull_definition),
		ApiRoute::new(M::POST, "/library/studies", USER, publish_study),
		ApiRoute::new(M::GET, "/library/studies", USER, published_studies),
		ApiRoute::new(M::POST, "/library/studies/{sha}/withdraw", USER, withdraw_study),
	]
}

#[derive(Deserialize, JsonSchema)]
struct StudyIn {
	/// The area's slug.
	area: String,
	/// A category version the caller may read: one of theirs, or a released one. None: the area's
	/// grid alone, nothing billed.
	version: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
struct ShaIn {
	/// A published study's definition id.
	sha: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PublishStudy")]
struct PublishIn {
	/// The area's slug: its geometry is frozen into the study.
	area: String,
	/// The category version it is a study of.
	version: i64,
	/// 1 to 100 characters.
	title: String,
	/// Releases `version` in the same write; without it, the version must be released already.
	release: Option<ReleaseIn>,
}

/// How a version is released: a new listing, or the next version of one of the caller's.
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "as", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "StudyRelease")]
enum ReleaseIn {
	New {
		title: String,
		about: String,
		notes: String,
		forked_from: Option<i64>,
	},
	Next {
		listing: i64,
		notes: String,
	},
}

#[derive(Deserialize, JsonSchema)]
struct StudiesIn {
	/// A gcid.
	trade: Option<String>,
	country: Option<gbp::Country>,
	/// An area's slug.
	area: Option<String>,
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "PublishedStudies")]
struct StudiesDto {
	studies: Vec<PublishedStudyDto>,
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

/// A pull under way. The edge drops a request it hears nothing of for 100 s, and a pull takes minutes.
struct Pulling(Response);

impl IntoResponse for Pulling {
	fn into_response(self) -> Response {
		self.0
	}
}

impl OperationOutput for Pulling {
	type Inner = Self;

	fn operation_response(_: &mut GenContext, _: &mut Operation) -> Option<Answer> {
		Some(Answer {
			description: "`pulled` or `refused`, once it ends.".to_owned(),
			content: [("text/event-stream".into(), MediaType::default())].into_iter().collect(),
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

/// A study of an area: under a category version the caller may read, or the area's grid alone;
/// never spends. Built on the first read and kept, out of what is held: a study whose searches are
/// not all there opens on what is, and its notice says how many it lacks.
///
/// 404 no such area, or a version the caller may not read; 422 a version of `another_country`, or
/// an area where placement holds no base (`unstudied`); 503 placement not connected.
async fn study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Params(at): Params<StudyIn>) -> ApiResult<Payload> {
	let p = connected(&s)?.clone();
	let def = define(&s, &caller, &at.area, at.version).await?;
	opened(&s, &p, def).await
}

/// A published study, built as a study is: of what is held, nothing bought.
///
/// 404 one the caller may not see; 503 placement not connected.
async fn published_study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(ShaIn { sha }): Ids<ShaIn>) -> ApiResult<Payload> {
	let p = connected(&s)?.clone();
	let def = s.sign_in.panel.published_study(caller.user_id, &sha, Timestamp::now()).await??.definition;
	opened(&s, &p, def).await
}

/// `def`'s payload: the one kept, else built of what is held and kept.
async fn opened(s: &ApiState, p: &Placement, def: Definition) -> ApiResult<Payload> {
	let slot = p.slot(&def.id());
	let mut slot = slot.lock().await;
	if let Some(built) = &*slot {
		return Ok(Payload(built.clone()));
	}
	let built = built(s, p, def).await?;
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
	let def = define(&s, &caller, &at.area, at.version).await?;
	pulled(&s, &caller, &p, def).await
}

/// Builds a published study again, buying what it lacks at the caller's tokens, as a pull does.
async fn pull_published_study(State(s): State<ApiState>, Spends(caller, _): Spends<PlacementRequest, Caller>, Ids(ShaIn { sha }): Ids<ShaIn>) -> ApiResult<Payload> {
	let p = connected(&s)?.clone();
	let def = s.sign_in.panel.published_study(caller.user_id, &sha, Timestamp::now()).await??.definition;
	pulled(&s, &caller, &p, def).await
}

/// Builds a definition again, its area and category kept here or not, buying what it lacks at the
/// caller's tokens, as a pull does.
///
/// 400 a definition placement refuses, or an area not a circle of an area's radius, its margin
/// included; 503 placement not connected. Its end is the answer's one event: `pulled`, or `refused`
/// with the refusal a pull answers.
async fn pull_definition(State(s): State<ApiState>, Spends(caller, _): Spends<PlacementRequest, Caller>, Body(def): Body<Definition>) -> ApiResult<Pulling> {
	let p = connected(&s)?.clone();
	def.check().map_err(|e| Refusal::new(Code::BadRequest, e.0))?;
	match &def.area {
		Area::Circle { radius_m, margin_m, .. } if radius_m + margin_m <= MAX_RADIUS_M as f64 => {}
		_ => return Err(Refusal::new(Code::BadRequest, format!("a study is of a circle of at most {MAX_RADIUS_M} m"))),
	}
	opened(&s, &p, def.clone()).await?; // what does not build buys nothing
	let end = async move {
		let ended = tokio::time::timeout(PULL_BOUND, pulled(&s, &caller, &p, def)).await;
		let ended = ended.unwrap_or_else(|_| Err(Refusal::new(Code::Unavailable, "stopped part-way, out of time: pulled again, it buys the rest")));
		Ok::<_, Infallible>(match ended {
			Ok(_) => Event::default().event("pulled").data("{}"),
			Err(refused) => Event::default().event("refused").json_data(refused).expect("a refusal serialises"),
		})
	};
	Ok(Pulling(Sse::new(futures::stream::once(end)).keep_alive(KeepAlive::default()).into_response()))
}

/// `def` built again, what it lacks bought at `caller`'s tokens.
async fn pulled(s: &ApiState, caller: &Caller, p: &Placement, def: Definition) -> ApiResult<Payload> {
	let sha = def.id();
	let slot = p.slot(&sha);
	let mut slot = slot.lock().await;
	let held = s.sign_in.panel.pull(caller.user_id).await??;
	let pulled = match bought(s, p, &def, &held).await {
		Ok(()) => built(s, p, def).await,
		Err(e) => Err(e),
	};
	held.paid(&sha, pulled.is_err()).await?;
	let built = pulled?;
	*slot = Some(built.clone());
	Ok(Payload(built))
}

/// Every search `def` lacks, bought: it is asked again after each round, since whether another page
/// is behind one is known only once it is answered.
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
		then(&gmaps_optimal_placement::load(&def)?, &work)
	})
	.await
	.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
	Ok(done?)
}

fn connected(s: &ApiState) -> ApiResult<&Placement> {
	s.placement.as_ref().ok_or_else(|| Refusal::new(Code::Unavailable, "placement not connected".to_owned()))
}

/// Publishes a study of an area under a category version: on the version's release, or releasing it
/// in the same write. Readable by everyone, for good.
///
/// As a study is refused, and: 400 a title empty or past 100 characters; 409 published already;
/// 422 `unreleased_category`, a version no listing released and no `release`.
async fn publish_study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Body(b): Body<PublishIn>) -> ApiResult<Created<PublishedStudyDto>> {
	let def = define(&s, &caller, &b.area, Some(b.version)).await?;
	let release = b.release.map(|r| match r {
		ReleaseIn::New { title, about, notes, forked_from } => Release::New(NewListing { title, about, notes, forked_from }),
		ReleaseIn::Next { listing, notes } => Release::Next { listing, notes },
	});
	let published = s.sign_in.panel.publish_study(caller.user_id, &def, b.version, &b.title, release, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, sha = %published.sha, listing = published.listing, "study published");
	Ok(Created(PublishedStudyDto::of(published)?))
}

/// The published studies the caller may see, of a trade, a country and an area when given.
async fn published_studies(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Params(at): Params<StudiesIn>) -> ApiResult<Json<StudiesDto>> {
	let studies = s.sign_in.panel.published_studies(caller.user_id, at.trade.as_deref(), at.country, at.area.as_deref()).await?;
	Ok(Json(StudiesDto {
		studies: studies.into_iter().map(PublishedStudyDto::of).collect::<eyre::Result<_>>()?,
	}))
}

/// Withdraws a study the caller published: hidden from whoever never used its listing.
async fn withdraw_study(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(ShaIn { sha }): Ids<ShaIn>) -> ApiResult<NoContent> {
	s.sign_in.panel.withdraw_study(caller.user_id, &sha, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %sha, "published study withdrawn");
	Ok(NoContent)
}

/// The study of `area` under `version`, as the conductor's own areas and the caller's reading of
/// that version say it: the one function every study's definition is built by.
async fn define(s: &ApiState, caller: &Caller, area: &str, version: Option<i64>) -> ApiResult<Definition> {
	let area = s.sign_in.panel.area(area).await?.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
	let unstudied = || Refusal::new(Code::Unstudied, format!("{} is in {}, where nothing is studied", area.name, area.country));
	let of_area: gbp::Country = area.country.parse().map_err(|_| unstudied())?;
	let category = match version {
		None => None,
		Some(id) => {
			let v = s.sign_in.panel.version(caller.user_id, id).await??;
			if v.country != of_area {
				return Err(Refusal::new(
					Code::AnotherCountry,
					format!("version {id} is of {}, not {of_area}, where {} is", v.country, area.name),
				));
			}
			Some(v)
		}
	};
	let country: Country = of_area.as_str().parse().map_err(|_| unstudied())?; // no Country: none of placement's bases
	let category = category.map(|v| v.category(country)).transpose()?;
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
