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
use ev_lib::api::{Guard, OpenApi, Route, document, openapi::ReferenceOr};
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
	api, assistant, browser, connected, connections,
	forward::{Forward, Services},
	gmbs,
	grafana::Grafana,
	ingest,
	live::{self, Live, LiveLimits},
	queries_cache,
	signin::{self, Caller, Freshness, Gatekeeper, SignIn},
	tokens, trash,
};

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

/// One `/api/v1` operation.
pub(crate) type ApiRoute = Route<ApiState, Gate>;

/// The `/api/v1` document: what `/openapi.json` narrows to its caller.
#[derive(Clone)]
pub struct Catalog(Arc<OpenApi>);

impl Catalog {
	pub fn new() -> Self {
		Self(Arc::new(document(
			"Service-Arb conductor",
			env!("CARGO_PKG_VERSION"),
			"/api/v1",
			&api_routes(),
			ev_lib::api_docs!(),
		)))
	}

	pub fn document(&self) -> &OpenApi {
		&self.0
	}

	/// Only the operations `permissions` open.
	pub(crate) fn opened(&self, permissions: &PermissionSet) -> OpenApi {
		let mut doc = (*self.0).clone();
		for item in doc.paths.iter_mut().flat_map(|p| p.paths.values_mut()) {
			let ReferenceOr::Item(item) = item else { unreachable!("the document holds items only") };
			for op in [&mut item.get, &mut item.post, &mut item.put, &mut item.patch, &mut item.delete] {
				let closed = op
					.as_ref()
					.and_then(|o| o.extensions.get("x-permission"))
					.is_some_and(|p| !permissions.iter().any(|held| p == held));
				if closed {
					*op = None;
				}
			}
		}
		doc
	}
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
		assistant::routes(),
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

/// Everything the conductor answers but the front end's files; ingest and the forward only with
/// `forward`, `/grafana/` only with `grafana`.
pub fn app(
	sign_in: SignIn,
	forward: Option<Forward>,
	grafana: Option<&Grafana>,
	limits: Limits,
	telegram: Option<Telegram>,
	map: Option<gmbs::MapDto>,
	browsers: Option<browser::Connected>,
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
	let state = ApiState {
		catalog: Catalog::new(),
		panel: sign_in.panel.clone(),
		sites: forward.as_ref().map(|f| f.services().clone()),
		probe: connected::probe_client(),
		telegram,
		map,
		browsers: browsers.clone(),
	};
	let slow = bounded(mount(slow).with_state(state.clone()), 8, Duration::from_secs(600));
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
		.nest("/api/v1", slow)
		.merge(queries_cache::routes(sign_in.panel.clone(), &limits))
		.nest("/api/v1", api);
	let app = match forward {
		Some(forward) => app.merge(ingest::routes(forward.clone(), &limits)).merge(forward.routes()),
		None => app,
	};
	let app = match grafana {
		Some(grafana) => app.merge(grafana.routes()),
		None => app,
	};
	app.layer(middleware::map_response(nosniff))
}

/// What the `/api/v1` routes are served on.
#[derive(Clone)]
pub(crate) struct ApiState {
	pub(crate) catalog: Catalog,
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
