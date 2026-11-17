//! The services behind the conductor's origin (docs/ARCHITECTURE.md, Forward): a prefix of ours
//! is streamed to a service both ways, and who is calling is told to it by an assertion the
//! conductor signs for that one request ([`sa_auth`]), never by the browser's cookies.
//!
//! ```text
//! /api/sites/*                     → sites /api/v1/*            session + assertion; CSRF on writes
//! /api/hooks/booking/*             → sites (same path)          open: the providers' webhooks
//! /mfe/sites/*                     → sites /mfe/*               open: the sites page's bundle
//! /api/review_archive/*            → review_archive /*          session + assertion; CSRF on writes
//! /mfe/review_archive/*            → review_archive /mfe/*      open: the dashboard's bundle
//! /api/playbook/*                  → playbook /*                session + assertion; CSRF on writes
//! /api/placement/*                 → placement /*               session + assertion; CSRF on writes
//! /mfe/placement/*                 → placement /mfe/*           open: the map overlay's module
//! ```
//!
//! [`Services`] is the one client and signing key every call to a service goes through: the
//! forward's, ingest's, Grafana's ([`crate::grafana`]), and the hubs' own reads of sites.

use std::{sync::Arc, time::Duration};

use axum::{
	Router,
	body::{Body, to_bytes},
	extract::{Request, State},
	http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header},
	response::{IntoResponse, Response},
	routing::any,
};
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use sa_auth::{Assertion, Principal, Service, Signer, User};
use sa_core::placement::{BILLED, Billed};

use crate::{
	api::ApiError,
	catalog::Catalog,
	cookies,
	signin::{self, Caller, Freshness, SignIn},
};

/// Longest prefixes first: the first match wins.
const ROUTES: [Route; 8] = [
	Route {
		prefix: "/api/sites",
		upstream: "/api/v1",
		service: Service::Sites,
		gate: Gate::Session,
	},
	Route {
		prefix: "/api/hooks/booking",
		upstream: "/api/hooks/booking",
		service: Service::Sites,
		gate: Gate::Open,
	},
	Route {
		prefix: "/mfe/sites",
		upstream: "/mfe",
		service: Service::Sites,
		gate: Gate::Open,
	},
	Route {
		prefix: "/api/review_archive",
		upstream: "",
		service: Service::ReviewArchive,
		gate: Gate::Session,
	},
	Route {
		prefix: "/mfe/review_archive",
		upstream: "/mfe",
		service: Service::ReviewArchive,
		gate: Gate::Open,
	},
	Route {
		prefix: "/api/playbook",
		upstream: "",
		service: Service::Playbook,
		gate: Gate::Session,
	},
	Route {
		prefix: "/api/placement",
		upstream: "",
		service: Service::Placement,
		gate: Gate::Session,
	},
	Route {
		prefix: "/mfe/placement",
		upstream: "/mfe",
		service: Service::Placement,
		gate: Gate::Open,
	},
];

/// The services that serve their Grafana dashboards at `GET /dashboards` (docs/ARCHITECTURE.md,
/// Grafana).
pub(crate) const DASHBOARDS: [Service; 1] = [Service::ReviewArchive];

/// Hop-by-hop headers (RFC 9110 §7.6.1) and what only the panel may say: never passed on.
pub(crate) const DROPPED_UP: [&str; 11] = [
	"connection",
	"keep-alive",
	"proxy-connection",
	"te",
	"trailer",
	"transfer-encoding",
	"upgrade",
	"cookie",
	sa_auth::HEADER,
	cookies::CSRF_HEADER,
	crate::grafana::VIEWER_HEADER,
];

pub(crate) const DROPPED_DOWN: [&str; 8] = ["connection", "keep-alive", "proxy-connection", "te", "trailer", "transfer-encoding", "upgrade", "set-cookie"];
/// How long a call of the conductor's own to a service may take.
pub(crate) const SITES_TIMEOUT: Duration = Duration::from_secs(10);

/// The most of a service's answer the conductor reads: a page of notes or a photo, with room.
const ANSWER: usize = 16 * 1024 * 1024;

/// Every path the forward owns: never the front end's.
pub fn prefixes() -> impl Iterator<Item = &'static str> {
	ROUTES.iter().map(|r| r.prefix)
}

/// Whether `path` is `prefix` or below it, by whole segments.
pub fn under(path: &str, prefix: &str) -> bool {
	path.strip_prefix(prefix).is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// Where each service is, and the key the conductor vouches with.
pub struct Upstreams {
	pub review_archive: Uri,
	pub playbook: Uri,
	pub sites: Uri,
	/// gmaps_optimal_placement: optional, apart from the other three.
	pub placement: Option<Uri>,
	pub signer: Signer,
}

/// The services, and how the conductor calls them.
#[derive(Clone)]
pub struct Services {
	upstreams: Arc<Upstreams>,
	client: Client<HttpConnector, Body>,
}
impl Services {
	pub fn new(upstreams: Upstreams) -> Self {
		let mut connector = HttpConnector::new();
		connector.set_connect_timeout(Some(Duration::from_secs(5)));
		Self {
			upstreams: Arc::new(upstreams),
			client: Client::builder(hyper_util::rt::TokioExecutor::new()).build(connector),
		}
	}

	/// `method path[?query]` to `service`, `principal` vouched for by an assertion bound to the
	/// path; what only the conductor may say, and hop-by-hop headers, are not passed either way.
	/// The answer streams.
	pub(crate) async fn send(
		&self,
		service: Service,
		method: Method,
		path_and_query: &str,
		mut headers: HeaderMap,
		principal: Option<Principal>,
		body: Body,
	) -> Result<Response, hyper_util::client::legacy::Error> {
		let path = path_and_query.split_once('?').map_or(path_and_query, |(path, _)| path);
		let base = match service {
			Service::ReviewArchive => &self.upstreams.review_archive,
			Service::Playbook => &self.upstreams.playbook,
			Service::Sites => &self.upstreams.sites,
			Service::Placement => self.upstreams.placement.as_ref().expect("`pass` answers placement's routes itself without it"),
			Service::Browsers => panic!("the runner is not forwarded: `browser` calls it"),
		};
		let uri: Uri = format!("{}{path_and_query}", base.to_string().trim_end_matches('/'))
			.parse()
			.expect("a path and query that arrived in a URI stay one under a bare origin");
		strip(&mut headers, &DROPPED_UP);
		headers.remove(header::HOST);
		if let Some(principal) = principal {
			let token = self.upstreams.signer.sign(&Assertion {
				aud: service,
				principal,
				method: method.to_string(),
				path: path.to_owned(),
				exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
			});
			headers.insert(HeaderName::from_static(sa_auth::HEADER), HeaderValue::from_str(&token).expect("a JWS is base64url and dots"));
		}
		let mut req = Request::new(body);
		*req.method_mut() = method;
		*req.uri_mut() = uri;
		*req.headers_mut() = headers;
		let res = self.client.request(req).await?;
		let (mut parts, body) = res.into_parts();
		strip(&mut parts.headers, &DROPPED_DOWN);
		Ok(Response::from_parts(parts, Body::new(body)))
	}

	/// `GET path?query` of sites as the conductor, its JSON answer.
	pub(crate) async fn conductor_get<T: serde::de::DeserializeOwned>(&self, path: &str) -> eyre::Result<T> {
		let (status, body) = self.conductor_fetch(Service::Sites, path).await?;
		eyre::ensure!(status == StatusCode::OK, "sites answered {path} with {status}");
		serde_json::from_slice(&body).map_err(|e| eyre::eyre!("sites' answer to {path}: {e}"))
	}

	/// `GET path?query` of `service` as the conductor: its status and body.
	pub(crate) async fn conductor_fetch(&self, service: Service, path: &str) -> eyre::Result<(StatusCode, axum::body::Bytes)> {
		let call = self.send(service, Method::GET, path, HeaderMap::new(), Some(Principal::Conductor), Body::empty());
		let res = tokio::time::timeout(SITES_TIMEOUT, call)
			.await
			.map_err(|_| eyre::eyre!("{service:?} did not answer {path} in time"))??;
		let status = res.status();
		let body = to_bytes(res.into_body(), ANSWER).await.map_err(|e| eyre::eyre!("reading {service:?}'s answer to {path}: {e}"))?;
		Ok((status, body))
	}
}

#[derive(Clone)]
pub struct Forward {
	sign_in: SignIn,
	services: Services,
	catalog: Catalog,
}
impl Forward {
	pub fn new(sign_in: SignIn, services: Services) -> Self {
		Self {
			sign_in,
			services,
			catalog: Catalog::new(crate::http::document()),
		}
	}

	/// The conductor's operations and the services' behind it.
	pub fn catalog(&self) -> &Catalog {
		&self.catalog
	}

	pub fn routes(self) -> Router {
		let mut router = Router::new();
		for r in &ROUTES {
			router = router.route(r.prefix, any(forward)).route(&format!("{}/{{*rest}}", r.prefix), any(forward));
		}
		router.with_state(self)
	}

	pub(crate) fn panel(&self) -> &sa_panel::Panel {
		&self.sign_in.panel
	}

	pub(crate) fn sign_in(&self) -> &SignIn {
		&self.sign_in
	}

	pub(crate) fn services(&self) -> &Services {
		&self.services
	}

	/// A GET Grafana makes for `viewer` to a service's API: the forward's, as that viewer.
	/// `req`, to a service's API under its prefix on this origin, as `user` tells that service of them.
	pub(crate) async fn as_user(&self, req: Request, user: impl FnOnce(Service) -> User) -> Response {
		match apis().find(|(_, prefix)| under(req.uri().path(), prefix)) {
			Some((service, prefix)) => {
				let route = ROUTES.iter().find(|r| r.prefix == prefix).expect("apis() are ROUTES");
				self.pass(route, req, Some(Principal::User(user(service)))).await
			}
			None => ApiError::NotFound.into_response(),
		}
	}

	/// A study's pull, on what `user` affords, charged what placement says it bought; owed back when it failed.
	async fn pull(&self, route: &Route, mut req: Request, principal: Option<Principal>, user: uuid::Uuid, sha: &str) -> Response {
		let held = match self.panel().pull(user).await {
			Ok(Ok(held)) => held,
			Ok(Err(refused)) => return ApiError::from(refused).into_response(),
			Err(e) => return ApiError::from(e).into_response(),
		};
		*req.uri_mut() = format!("{}?budget={}", req.uri().path(), held.budget())
			.parse()
			.expect("a path that arrived in a URI, and a number");
		let res = self.pass(route, req, principal).await;
		let billed: Billed = match res.headers().get(BILLED) {
			Some(v) => match serde_json::from_slice(v.as_bytes()) {
				Ok(b) => b,
				Err(e) => return ApiError::Internal(eyre::eyre!("placement's {BILLED} of {sha} is {v:?}: {e}")).into_response(),
			},
			// placement says it on every pull it ran: a refusal without it never reached one
			None if !res.status().is_success() => Billed::new(),
			None => return ApiError::Internal(eyre::eyre!("placement answered the pull of {sha} without {BILLED}")).into_response(),
		};
		let failed = !res.status().is_success();
		if res.status() != StatusCode::PAYMENT_REQUIRED {
			return match held.paid(&billed, sha, failed).await {
				Ok(()) => res,
				Err(e) => ApiError::from(e).into_response(),
			};
		}
		#[derive(serde::Deserialize)]
		struct Unaffordable {
			need: u64,
		}
		let body = match to_bytes(res.into_body(), ANSWER).await {
			Ok(body) => body,
			Err(e) => return ApiError::BadGateway(format!("reading placement's refusal: {e}")).into_response(),
		};
		let need = match serde_json::from_slice::<Unaffordable>(&body) {
			Ok(u) => u.need,
			Err(e) => return ApiError::Internal(eyre::eyre!("placement's 402 for {sha}, {:?}: {e}", String::from_utf8_lossy(&body))).into_response(),
		};
		let why = held.unaffordable(need);
		if let Err(e) = held.paid(&billed, sha, failed).await {
			return ApiError::from(e).into_response();
		}
		(StatusCode::PAYMENT_REQUIRED, why).into_response()
	}

	async fn pass(&self, route: &Route, req: Request, principal: Option<Principal>) -> Response {
		if route.service == Service::Placement && self.services.upstreams.placement.is_none() {
			return ApiError::Unavailable("placement not connected".to_owned()).into_response();
		}
		let rest = &req.uri().path()[route.prefix.len()..];
		// decoded as a service might, so no route reaches above its upstream prefix
		let decoded = rest.to_ascii_lowercase().replace("%2e", ".").replace("%2f", "/").replace("%5c", "/").replace('\\', "/");
		if decoded.split('/').any(|s| s == "." || s == "..") {
			return ApiError::BadRequest("not a forwardable path".to_owned()).into_response();
		}
		let upstream_path = format!("{}{rest}", route.upstream);
		let upstream_path = if upstream_path.is_empty() { "/".to_owned() } else { upstream_path };
		let upstream = match req.uri().query() {
			Some(q) => format!("{upstream_path}?{q}"),
			None => upstream_path,
		};
		let (parts, body) = req.into_parts();
		match self.services.send(route.service, parts.method, &upstream, parts.headers, principal, body).await {
			Ok(res) => res,
			Err(e) => {
				tracing::warn!(error = %e, service = ?route.service, "a forward failed");
				ApiError::BadGateway("the service did not answer".to_owned()).into_response()
			}
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Gate {
	/// Signed in, and the assertion added; a write with the panel's CSRF header (or a bearer).
	Session,
	Open,
}

struct Route {
	/// Ours, a whole path segment or more.
	prefix: &'static str,
	/// What replaces `prefix` upstream.
	upstream: &'static str,
	service: Service,
	gate: Gate,
}

/// Each service's API, `/api/<service>`, with its prefix: what a viewer's token reaches.
pub(crate) fn apis() -> impl Iterator<Item = (Service, &'static str)> {
	ROUTES.iter().filter(|r| r.prefix == api_prefix(r.service)).map(|r| (r.service, r.prefix))
}

fn api_prefix(service: Service) -> String {
	format!("/api/{service}")
}

/// Where `service`'s API under `upstream` is on this origin.
pub(crate) fn prefix(service: Service, upstream: &str) -> Option<&'static str> {
	ROUTES
		.iter()
		.find(|r| r.service == service && r.upstream == upstream && r.gate == Gate::Session)
		.map(|r| r.prefix)
}

async fn forward(State(f): State<Forward>, mut req: Request) -> Response {
	let route = ROUTES.iter().find(|r| under(req.uri().path(), r.prefix)).expect("mounted under one of ROUTES");
	let mut puller = None;
	let principal = match route.gate {
		Gate::Open => None,
		Gate::Session => {
			let write = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
			// a write must not outlive a revoked grant by even the cached GetMe's minute
			let freshness = if write { Freshness::Fresh } else { Freshness::Cached };
			// an MCP client's bearer: no cookie for a cross-site request to ride on, and ours, so it never goes up
			let caller = if req.headers().contains_key(header::AUTHORIZATION) {
				let caller = signin::bearer(&f.sign_in, req.headers(), freshness).await;
				req.headers_mut().remove(header::AUTHORIZATION);
				caller
			} else {
				if write && !f.sign_in.cookies().csrf_ok(req.headers()) {
					return ApiError::Csrf.into_response();
				}
				signin::check(&f.sign_in, req.headers(), freshness).await
			};
			match caller {
				Ok(caller) => {
					puller = Some(caller.user_id);
					Some(Principal::User(user(caller, route.service)))
				}
				Err(denied) => return denied.into_response(&f.sign_in),
			}
		}
	};
	if *req.method() == Method::POST
		&& let Some(user) = puller
		&& let Some(sha) = pulled(req.uri().path())
	{
		let sha = sha.to_owned();
		return f.pull(route, req, principal, user, &sha).await;
	}
	f.pass(route, req, principal).await
}

/// The study of `POST /api/placement/studies/{sha}/pull`.
fn pulled(path: &str) -> Option<&str> {
	path.strip_prefix("/api/placement/studies/")?
		.strip_suffix("/pull")
		.filter(|sha| !sha.is_empty() && !sha.contains('/'))
}

/// `caller`, as `service` is told of them: its slice of their permissions only.
pub(crate) fn user(caller: Caller, service: Service) -> User {
	User {
		sub: caller.user_id.to_string(),
		email: caller.email,
		email_verified: caller.email_verified,
		name: caller.preferred_name,
		permissions: caller.permissions.iter().filter(|p| p.starts_with(service.prefix())).collect(),
	}
}

pub(crate) fn strip(headers: &mut HeaderMap, names: &[&str]) {
	// a header `Connection` names is hop-by-hop too
	let named: Vec<String> = headers
		.get_all(header::CONNECTION)
		.iter()
		.filter_map(|v| v.to_str().ok())
		.flat_map(|v| v.split(','))
		.map(|n| n.trim().to_ascii_lowercase())
		.filter(|n| !n.is_empty())
		.collect();
	for n in names.iter().copied().map(str::to_owned).chain(named) {
		headers.remove(n.as_str());
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_first_route_matching_by_whole_segments_wins() {
		let route = |p: &str| ROUTES.iter().find(|r| under(p, r.prefix)).map(|r| (r.prefix, r.gate));
		assert_eq!(route("/api/playbook/search"), Some(("/api/playbook", Gate::Session)));
		assert_eq!(route("/playbook_mcp/token"), None, "playbook is reached at /mcp, not through a forward of its own");
		assert_eq!(route("/api/review_archive/me"), Some(("/api/review_archive", Gate::Session)));
		assert_eq!(route("/api/review_archive/grafana/api/ds/query"), Some(("/api/review_archive", Gate::Session)));
		assert_eq!(route("/api/review_archived"), None);
		assert_eq!(route("/review_archive"), None, "the page is the front end's");
		assert_eq!(route("/api/sites/leads"), Some(("/api/sites", Gate::Session)));
		assert_eq!(route("/mfe/sites/mfe-sites.js"), Some(("/mfe/sites", Gate::Open)));
		assert_eq!(route("/sites/leads"), None, "the page is the front end's");
		assert_eq!(route("/api/placement/payload/cleaning/Lyon"), Some(("/api/placement", Gate::Session)));
		assert_eq!(route("/mfe/placement/overlay.js"), Some(("/mfe/placement", Gate::Open)));
		assert_eq!(route("/mfe/review_archive/mfe-review-archive-dashboard.js"), Some(("/mfe/review_archive", Gate::Open)));
		assert_eq!(route("/mfe/banking/wallet.js"), None, "banking's bundle is not forwarded");
		assert_eq!(route("/placement"), None, "no page of its own");
	}
}
