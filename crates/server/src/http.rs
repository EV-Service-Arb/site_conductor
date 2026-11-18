//! The HTTP surface: `/health`, the sign-in (`/auth/*`, [`crate::signin`]), the conductor's own
//! `/api/v1` ([`crate::api`], [`crate::access`], [`crate::gmbs`], [`crate::tokens`],
//! [`crate::alerts`], [`crate::assistant`]) and its live socket
//! (`/api/v1/live`, [`crate::live`]); with the forward configured, ingest (`ingest.rs`) and
//! the services behind the origin ([`crate::forward`]).

use std::{sync::Arc, time::Duration};

use axum::{
	Router,
	error_handling::HandleErrorLayer,
	extract::FromRef,
	http::{HeaderValue, Method, StatusCode, header},
	middleware,
	response::{IntoResponse, Response},
	routing::{get, post},
};
use ev_lib::api::{OpenApi, Refusal, Route};
use sa_panel::Panel;
use tower::{BoxError, ServiceBuilder, limit::GlobalConcurrencyLimitLayer};
use tower_http::timeout::TimeoutLayer;

use crate::{
	access, admin,
	alerts::{self, Telegram},
	api::{self, Code},
	areas, assistant, brands, browser,
	catalog::Catalog,
	claude::{self, Model},
	connected, connections,
	execute::Sandbox,
	forward::{Forward, Services},
	gmbs,
	grafana::Grafana,
	ingest, kit,
	live::{self, Live, LiveLimits},
	mcp, meters, oauth, placement, queries_cache,
	signin::{self, Freshness, SignIn},
	tokens,
	tools::Tools,
	trash, usage,
};

pub(crate) const USER: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Cached,
	slow: false,
});

/// Asks concierge afresh: for what must not outlive a revoked permission by a minute.
pub(crate) const FRESH: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Fresh,
	slow: false,
});
pub(crate) const SLOW: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Cached,
	slow: true,
});
/// One `/api/v1` operation.
pub(crate) type ApiRoute = Route<ApiState, Gate>;

/// Who passes an `/api/v1` row: a signed-in person holding its permission, if it names one.
pub(crate) type Gate = sa_auth::Gate<Flags>;

/// The conductor's own `/api/v1` document, derived from its table.
pub fn document() -> OpenApi {
	let mut doc = ev_lib::api::document(
		"Service-Arb conductor",
		env!("CARGO_PKG_VERSION"),
		"/api/v1",
		&api_routes(),
		ev_lib::api_docs!(),
		Some(<crate::view::View as ev_lib::views::Views>::table()),
	);
	doc.info.description =
		Some("The Service-Arb panel, as the signed-in user: `search` for an operation, then `call` it. What removes or spends runs only once the user agreed to it.".into());
	doc
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
/// landings' kit only with `forward`, `/grafana/` only with `grafana`.
#[expect(clippy::too_many_arguments, reason = "each is one optional part of the deployment")]
pub fn app(
	sign_in: SignIn,
	forward: Option<Forward>,
	grafana: Option<&Grafana>,
	limits: Limits,
	telegram: Option<Telegram>,
	browsers: Option<browser::Connected>,
	model: Option<Model>,
	sandbox: Option<Sandbox>,
) -> Router {
	let auth = Router::new()
		.route("/auth/login", get(signin::login))
		.route("/auth/callback", get(signin::callback))
		.route("/auth/logout", post(signin::logout));
	let auth = bounded(auth, limits.auth_concurrent, limits.auth_timeout).with_state(sign_in.clone());
	let catalog = forward.as_ref().map_or_else(|| Catalog::new(document()), |f| f.catalog().clone());
	let runs = sandbox.is_some();
	let tools = Tools::new(catalog.clone(), sandbox);
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
		sign_in: sign_in.clone(),
		live: Live::new(sign_in.clone(), limits.live),
		sites: forward.as_ref().map(|f| f.services().clone()),
		placement: forward.as_ref().map(|f| f.services().clone()).filter(|s| s.serves(sa_auth::Service::Placement)),
		pages: pages.clone(),
		probe: connected::probe_client(),
		telegram,
		browsers,
		model,
	};
	// a socket's handshake has its own bound: the socket lives on after the 101, and a burst of
	// reconnecting tabs must not shed the API's requests
	let mount = |socket: bool, slow: bool| {
		api_routes()
			.into_iter()
			.filter(|r| r.socket() == socket && r.guard.flags.slow == slow)
			.fold(Router::new(), |routes, r| routes.merge(Router::new().route(r.path, r.handler())))
			.with_state(state.clone())
	};
	if let Some(grafana) = grafana {
		let own = api_routes()
			.into_iter()
			.filter(|r| r.method == Method::GET && !r.socket())
			.fold(Router::new(), |routes, r| routes.merge(Router::new().route(r.path, r.handler())));
		grafana.own(bounded(own.with_state(state.clone()), limits.api_concurrent, limits.api_timeout));
	}
	let session = sign_in.clone();
	let v1 = bounded(mount(false, false), limits.api_concurrent, limits.api_timeout)
		.merge(bounded(mount(false, true), 8, Duration::from_secs(600)))
		.merge(bounded(mount(true, false), limits.live.upgrade_concurrent, limits.live.upgrade_timeout))
		.layer(middleware::map_response(move |mut res: Response| {
			let session = session.clone();
			async move {
				// PII and secrets pass through here: nothing is cached.
				res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
				// only `unauthenticated` is a 401: the session is over
				if res.status() == StatusCode::UNAUTHORIZED { signin::signed_out(&session, res) } else { res }
			}
		}));
	let app = Router::new()
		.route("/health", get(|| async { "ok" }))
		.merge(auth)
		.merge(agents)
		.merge(queries_cache::routes(sign_in.panel.clone(), &limits))
		.nest("/api/v1", v1);
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
	let app = if runs {
		app.merge(bounded(Sandbox::routes(tools.clone()), limits.api_concurrent, limits.api_timeout))
	} else {
		app
	};
	let app = app.layer(middleware::map_response(nosniff));
	tools.serve(app.clone());
	app
}

/// What a row's gate decides beside its permission.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Flags {
	pub(crate) freshness: Freshness,
	/// Waits on a browser or takes a film's bytes: 8 at once, 10 min each.
	pub(crate) slow: bool,
}

/// Every `/api/v1` route on `ApiState`.
pub(crate) fn api_routes() -> Vec<ApiRoute> {
	[
		api::routes(),
		admin::routes(),
		access::routes(),
		alerts::routes(),
		gmbs::routes(),
		connections::routes(),
		browser::routes(),
		tokens::routes(),
		trash::routes(),
		usage::routes(),
		assistant::routes(),
		claude::routes(),
		kit::routes(),
		brands::routes(),
		areas::routes(),
		placement::routes(),
		meters::routes(),
		live::routes(),
	]
	.into_iter()
	.flatten()
	.collect()
}

/// What the `/api/v1` routes are served on.
#[derive(Clone)]
pub(crate) struct ApiState {
	pub(crate) catalog: Catalog,
	/// What an agent reaches the operations with: the in-page assistant's loop, an MCP client.
	pub(crate) tools: Tools,
	/// Its `panel` is the engine.
	pub(crate) sign_in: SignIn,
	/// `/live`'s sockets.
	pub(crate) live: Live,
	/// With the forward configured.
	pub(crate) sites: Option<Services>,
	/// With PANEL_PLACEMENT_URL too.
	pub(crate) placement: Option<Services>,
	/// What the Connected sites page asks the landings with.
	pub(crate) probe: reqwest::Client,
	/// With `TELEGRAM_BOT_TOKEN`.
	pub(crate) telegram: Option<Telegram>,
	/// With GOLOGIN_TOKEN and PANEL_BROWSERS_URL.
	pub(crate) browsers: Option<browser::Connected>,
	/// The assistant's Claude.
	pub(crate) model: Option<Model>,
	/// With the forward: which landing page is which GBP.
	pub(crate) pages: Option<kit::Pages>,
}

impl FromRef<ApiState> for Panel {
	fn from_ref(s: &ApiState) -> Self {
		s.sign_in.panel.clone()
	}
}

impl FromRef<ApiState> for Live {
	fn from_ref(s: &ApiState) -> Self {
		s.live.clone()
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
			.layer(HandleErrorLayer::new(|_: BoxError| async { Refusal::new(Code::Unavailable, "busy, try again").into_response() }))
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
