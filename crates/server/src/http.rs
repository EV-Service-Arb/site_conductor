//! The HTTP surface: `/health`, the sign-in (`/auth/*`, [`crate::signin`]), the conductor's own
//! `/api/v1` ([`crate::api`], [`crate::access`], [`crate::gmbs`], [`crate::tokens`],
//! [`crate::alerts`], [`crate::assistant`]) and its live socket
//! (`/api/v1/live`, [`crate::live`]); with the forward configured, ingest (`ingest.rs`) and
//! the services behind the origin ([`crate::forward`]).

use std::{sync::Arc, time::Duration};

use axum::{
	Json, Router,
	error_handling::HandleErrorLayer,
	extract::{FromRef, RawPathParams, Request, State},
	http::{HeaderValue, Method, StatusCode, header},
	middleware,
	middleware::Next,
	response::{IntoResponse, Response},
	routing::{get, post},
};
use ev_lib::api::{Guard, OpenApi, Route};
use jiff::Timestamp;
use sa_auth::{PermissionSet, Sources, Tokens};
use sa_panel::{
	Panel,
	live::{Audience, Change},
};
use serde_json::json;
use tower::{BoxError, ServiceBuilder, limit::GlobalConcurrencyLimitLayer};
use tower_http::timeout::TimeoutLayer;

use crate::{
	access,
	alerts::{self, Telegram},
	api, areas,
	ask::Model,
	assistant, browser,
	catalog::Catalog,
	connected, connections,
	forward::{Forward, Services},
	gmbs,
	grafana::Grafana,
	ingest, kit,
	live::{self, Live, LiveLimits},
	mcp, oauth, queries_cache,
	signin::{self, Caller, Freshness, Gatekeeper, SignIn},
	tokens,
	tools::Tools,
	trash, usage,
};

/// One `/api/v1` operation.
pub(crate) type ApiRoute = Route<ApiState, Gate>;

/// A part of the conductor's API, and the permission that opens it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Section {
	/// Sources and their keys.
	Admin,
	/// Everyone's tokens.
	Tokens,
}

impl Section {
	pub fn may(self, permissions: &PermissionSet) -> bool {
		match self {
			Self::Admin => permissions.may(Sources::Manage),
			Self::Tokens => permissions.may(Tokens::Grant),
		}
	}

	fn permission(self) -> &'static str {
		match self {
			Self::Admin => Sources::Manage.as_str(),
			Self::Tokens => Tokens::Grant.as_str(),
		}
	}
}

/// Who passes an `/api/v1` route's gate, and its budget.
#[derive(Clone, Copy, Debug)]
pub struct Gate {
	/// `None`: every signed-in user.
	pub section: Option<Section>,
	pub(crate) freshness: Freshness,
	/// Waits on a browser or takes a film's bytes: 8 at once, 10 min each, not the API's bounds.
	pub(crate) slow: bool,
}

impl Gate {
	pub(crate) const USER: Self = Self {
		section: None,
		freshness: Freshness::Cached,
		slow: false,
	};

	pub(crate) const fn section(section: Section) -> Self {
		Self {
			section: Some(section),
			..Self::USER
		}
	}

	/// Asks concierge afresh: for what must not outlive a revoked permission by a minute.
	pub(crate) const fn fresh(self) -> Self {
		Self {
			freshness: Freshness::Fresh,
			..self
		}
	}

	pub(crate) const fn slow(self) -> Self {
		Self { slow: true, ..self }
	}
}

impl Guard for Gate {
	fn permission(&self) -> Option<String> {
		self.section.map(|s| s.permission().to_owned())
	}
}

/// The conductor's own `/api/v1` document, derived from its table.
pub fn document() -> OpenApi {
	ev_lib::api::document("Service-Arb conductor", env!("CARGO_PKG_VERSION"), "/api/v1", &api_routes(), ev_lib::api_docs!())
}

/// What a request may cost.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
	/// Ingest's and the queries cache's requests served at once, each; past it, `503` rather than a queue.
	pub max_concurrent: usize,
	/// How long a body may take to arrive.
	pub body_timeout: Duration,
	/// How long a request may take in all: 500 short transactions, with room to spare.
	pub request_timeout: Duration,
	/// `/auth/*` requests at once: each may wait on concierge, so few.
	pub auth_concurrent: usize,
	/// `/auth/*`: a code exchange and a session write, concierge's 5 s included.
	pub auth_timeout: Duration,
	/// `/api/v1` requests at once.
	pub api_concurrent: usize,
	/// `/api/v1`: a rotation (which may wait on another replica's) and a few queries.
	pub api_timeout: Duration,
	/// `/api/v1/live`.
	pub live: LiveLimits,
}

/// Everything the conductor answers but the front end's files; ingest, the forward and the
/// landings' kit only with `forward`, `/grafana/` only with `grafana`. `web`: the front end's
/// files, whose directories no kit area may take.
#[expect(clippy::too_many_arguments, reason = "each is one optional part of the deployment")]
pub fn app(
	sign_in: SignIn,
	forward: Option<Forward>,
	grafana: Option<&Grafana>,
	limits: Limits,
	telegram: Option<Telegram>,
	map: Option<gmbs::MapDto>,
	browsers: Option<browser::Connected>,
	web: Option<&crate::web::Files>,
	model: Option<Model>,
) -> Router {
	let auth = Router::new()
		.route("/auth/login", get(signin::login))
		.route("/auth/callback", get(signin::callback))
		.route("/auth/logout", post(signin::logout));
	let auth = bounded(auth, limits.auth_concurrent, limits.auth_timeout).with_state(sign_in.clone());
	let gate = |gate| {
		let keeper = Gatekeeper { sign_in: sign_in.clone(), gate };
		middleware::from_fn_with_state(keeper, signin::gate)
	};
	let (slow, routes): (Vec<_>, Vec<_>) = api_routes().into_iter().partition(|r| r.guard.slow);
	let panel = sign_in.panel.clone();
	let mount = |routes: Vec<ApiRoute>| {
		routes.into_iter().fold(Router::new(), |routes, r| {
			let route = Router::new().route(r.path, r.handler());
			let route = match r.method {
				Method::GET => route,
				_ => route.route_layer(middleware::from_fn_with_state(Told::of(&r, panel.clone()), tell)),
			};
			routes.merge(route.route_layer(gate(r.guard)))
		})
	};
	let catalog = forward.as_ref().map_or_else(|| Catalog::new(document()), |f| f.catalog().clone());
	let tools = Tools::new(catalog.clone());
	let agents = mcp::Mcp {
		sign_in: sign_in.clone(),
		tools: tools.clone(),
		catalog: catalog.clone(),
	};
	// A call waits as long as the slowest operation it runs.
	let agents = bounded(oauth::routes(sign_in.clone()), limits.auth_concurrent, limits.auth_timeout).merge(bounded(mcp::routes(agents), limits.api_concurrent, Duration::from_secs(600)));
	let pages = forward.as_ref().map(|f| kit::Pages::new(f.services().clone()));
	let state = ApiState {
		catalog,
		tools: tools.clone(),
		panel: sign_in.panel.clone(),
		sites: forward.as_ref().map(|f| f.services().clone()),
		pages: pages.clone(),
		web: web.cloned(),
		probe: connected::probe_client(),
		telegram,
		map,
		browsers: browsers.clone(),
		model,
	};
	let slow = bounded(mount(slow).with_state(state.clone()), 8, Duration::from_secs(600));
	if let Some(grafana) = grafana {
		let own = api_routes().into_iter().filter(|r| r.method == Method::GET).fold(Router::new(), |routes, r| {
			routes.merge(
				Router::new()
					.route(r.path, r.handler())
					.route_layer(middleware::from_fn_with_state((grafana.clone(), r.guard.section), crate::grafana::viewer_gate)),
			)
		});
		grafana.own(bounded(own.with_state(state.clone()), limits.api_concurrent, limits.api_timeout));
	}
	let api = bounded(mount(routes).with_state(state), limits.api_concurrent, limits.api_timeout);
	// Its own budget: the bound and the timeout cover the handshake only (the socket lives on
	// after the 101), and a burst of reconnecting tabs must not shed the API's requests.
	let live = Router::new().route("/api/v1/live", get(live::upgrade)).with_state(Live::new(sign_in.clone(), limits.live));
	let live = bounded(live, limits.live.upgrade_concurrent, limits.live.upgrade_timeout);
	let viewer = bounded(browser::viewer_routes(sign_in.clone(), browsers), limits.live.upgrade_concurrent, limits.live.upgrade_timeout);
	let app = Router::new()
		.route("/health", get(|| async { "ok" }))
		.merge(auth)
		.merge(live)
		.merge(viewer)
		.merge(agents)
		.nest("/api/v1", slow)
		.merge(queries_cache::routes(sign_in.panel.clone(), &limits))
		.nest("/api/v1", api);
	let app = match forward {
		Some(forward) => app
			.merge(ingest::routes(forward.clone(), &limits))
			.merge(kit::landing_routes(sign_in.panel.clone(), pages.expect("made with the forward"), &limits))
			.merge(forward.routes()),
		None => app,
	};
	let app = match grafana {
		Some(grafana) => app.merge(grafana.routes()),
		None => app,
	};
	let app = app.layer(middleware::map_response(nosniff));
	tools.serve(app.clone());
	app
}

/// Every `/api/v1` route on `ApiState`.
pub(crate) fn api_routes() -> Vec<ApiRoute> {
	[
		api::routes(),
		access::routes(),
		alerts::routes(),
		gmbs::routes(),
		connections::routes(),
		browser::routes(),
		tokens::routes(),
		trash::routes(),
		usage::routes(),
		assistant::routes(),
		kit::routes(),
		areas::routes(),
	]
	.into_iter()
	.flatten()
	.collect()
}

/// Who is told of a route's writes, over `/api/v1/live`.
#[derive(Clone)]
struct Told {
	panel: Panel,
	/// The path's first segment: `gmails` for `/gmails/{id}/imap`.
	topic: &'static str,
	/// The section's: else the caller, whose own resource it is.
	holders: Option<&'static str>,
}

impl Told {
	fn of(r: &ApiRoute, panel: Panel) -> Self {
		Self {
			panel,
			topic: r.path.split('/').nth(1).expect("an operation's path starts with /"),
			holders: r.guard.section.map(Section::permission),
		}
	}
}

/// Every write that succeeds is told as `changed`, its topic the path's first segment and its id
/// the path's first parameter: a screen re-reads what it follows, whoever wrote it.
async fn tell(State(t): State<Told>, params: RawPathParams, req: Request, next: Next) -> Response {
	let caller = req.extensions().get::<Caller>().expect("the gate lets no request through without its caller").user_id;
	let id = params.iter().next().map(|(_, v)| v.to_owned());
	let res = next.run(req).await;
	if res.status().is_success() {
		t.panel.bus().changed(Change {
			topic: t.topic.to_owned(),
			brand: None,
			id,
			at: Timestamp::now(),
			audience: t.holders.map_or(Audience::User(caller), |p| Audience::Holders(p.to_owned())),
		});
	}
	res
}

/// What the `/api/v1` routes are served on.
#[derive(Clone)]
pub(crate) struct ApiState {
	pub(crate) catalog: Catalog,
	/// What an agent reaches the operations with: the in-page assistant's loop, an MCP client.
	pub(crate) tools: Tools,
	pub(crate) panel: Panel,
	/// With the forward configured.
	pub(crate) sites: Option<Services>,
	/// What the Connected sites page asks the landings with.
	pub(crate) probe: reqwest::Client,
	/// With `TELEGRAM_BOT_TOKEN`.
	pub(crate) telegram: Option<Telegram>,
	/// With `GOOGLE_MAPS_BROWSER_KEY`.
	pub(crate) map: Option<gmbs::MapDto>,
	/// With GOLOGIN_TOKEN and PANEL_BROWSERS_URL.
	pub(crate) browsers: Option<browser::Connected>,
	/// With ANTHROPIC_API_KEY: the assistant's.
	pub(crate) model: Option<Model>,
	/// With the forward: which landing page is which GBP.
	pub(crate) pages: Option<kit::Pages>,
	/// PANEL_WEB_DIR's.
	pub(crate) web: Option<crate::web::Files>,
}

impl FromRef<ApiState> for Panel {
	fn from_ref(s: &ApiState) -> Self {
		s.panel.clone()
	}
}

impl Default for Limits {
	fn default() -> Self {
		Self {
			max_concurrent: 32,
			body_timeout: Duration::from_secs(10),
			request_timeout: Duration::from_secs(30),
			auth_concurrent: 8,
			auth_timeout: Duration::from_secs(10),
			api_concurrent: 32,
			api_timeout: Duration::from_secs(15),
			live: LiveLimits::default(),
		}
	}
}

/// At most `concurrent` at once across `routes`, past it `503` rather than a queue; at most
/// `timeout` each. The semaphore is shared: `Router::layer` wraps every route on its own, and
/// a plain concurrency limit would give each route a budget of its own.
fn bounded<S: Clone + Send + Sync + 'static>(routes: Router<S>, concurrent: usize, timeout: Duration) -> Router<S> {
	let permits = Arc::new(tokio::sync::Semaphore::new(concurrent));
	routes.layer(
		ServiceBuilder::new()
			// The only error the layers below raise is the shed: everything else is a response.
			.layer(HandleErrorLayer::new(|_: BoxError| async { error(StatusCode::SERVICE_UNAVAILABLE, "busy, try again") }))
			.load_shed()
			.layer(GlobalConcurrencyLimitLayer::with_semaphore(permits))
			.layer(TimeoutLayer::with_status_code(StatusCode::SERVICE_UNAVAILABLE, timeout)),
	)
}

/// Every answer is what its `Content-Type` says, never sniffed into something else.
async fn nosniff(mut res: Response) -> Response {
	res.headers_mut().insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
	res
}

fn error(status: StatusCode, msg: impl Into<String>) -> Response {
	(status, Json(json!({ "error": msg.into() }))).into_response()
}
