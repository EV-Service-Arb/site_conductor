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
use ev_lib::api::Refusal;
use hyper_util::client::legacy::{Client, connect::HttpConnector};
use sa_auth::{Assertion, Principal, Service, Signer, User};

use crate::{
	api::Code,
	catalog::Catalog,
	cookies,
	signin::{self, Caller, Freshness, SignIn},
};

/// The services' paths on the conductor's origin, longest prefixes first: the first match wins.
/// With [`OWN`], every prefix of the origin (docs/ARCHITECTURE.md, Forward).
const ROUTES: [Route; 6] = [
	Route {
		prefix: "/api/sites",
		upstream: "/api/v1",
		service: Service::Sites,
		rate_limited: false,
		published: true,
		gate: Gate::Session,
	},
	Route {
		prefix: "/api/hooks/booking",
		upstream: "/api/hooks/booking",
		service: Service::Sites,
		rate_limited: true,
		published: true,
		gate: Gate::Open,
	},
	Route {
		prefix: "/mfe/sites",
		upstream: "/mfe",
		service: Service::Sites,
		rate_limited: false,
		published: true,
		gate: Gate::Open,
	},
	Route {
		prefix: "/api/review_archive",
		upstream: "",
		service: Service::ReviewArchive,
		rate_limited: false,
		published: true,
		gate: Gate::Session,
	},
	Route {
		prefix: "/mfe/review_archive",
		upstream: "/mfe",
		service: Service::ReviewArchive,
		rate_limited: false,
		published: true,
		gate: Gate::Open,
	},
	Route {
		prefix: "/api/playbook",
		upstream: "",
		service: Service::Playbook,
		rate_limited: false,
		published: true,
		gate: Gate::Session,
	},
];

/// The conductor's own prefixes: never a page's, nor a kit area's.
const OWN: [Path; 15] = [
	Path::new("/api", false, true),
	Path::new("/api/v1", false, true),
	// open to everyone: the edge's limit is its only guard
	Path::new("/api/queries_cache", true, true),
	// in-cluster only: the landings' and bots' ingest, the landings' review kit, the sandbox's runs
	Path::new("/api/ingest", false, false),
	Path::new("/api/kit", false, false),
	Path::new(crate::execute::PREFIX, false, false),
	Path::new("/auth", true, true),
	Path::new("/mcp", false, true),
	// open (RFC 7591), or a sign-in's every step
	Path::new("/mcp/register", true, true),
	Path::new("/mcp/authorize", true, true),
	Path::new("/mcp/token", true, true),
	Path::new("/grafana", false, true),
	Path::new("/mfe", false, true),
	Path::new("/health", false, true),
	// once the cabinet's account chip: a tab on an earlier build may still ask for its script
	Path::new("/cabinet", false, true),
];

/// A prefix of the conductor's origin, and what the edge does with it.
#[derive(Clone, Copy, Debug)]
pub struct Path {
	pub prefix: &'static str,
	/// Per client IP at the edge.
	pub rate_limited: bool,
	/// The public ingress may route it; else in-cluster only.
	pub published: bool,
}

impl Path {
	const fn new(prefix: &'static str, rate_limited: bool, published: bool) -> Self {
		Self { prefix, rate_limited, published }
	}
}

/// Every prefix of the origin: the services' and the conductor's own.
pub fn paths() -> impl Iterator<Item = Path> {
	ROUTES.iter().map(|r| Path::new(r.prefix, r.rate_limited, r.published)).chain(OWN)
}

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

	/// Whether `service` is configured: placement may not be.
	pub(crate) fn serves(&self, service: Service) -> bool {
		service != Service::Placement || self.upstreams.placement.is_some()
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
			Service::Placement => self.upstreams.placement.as_ref().expect("sent to only where `serves` says so"),
			Service::Browsers => panic!("the runner is not forwarded: `browser` calls it"),
			Service::Sandbox => panic!("the runner is not forwarded: `execute` calls it"),
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
			None => Refusal::new(Code::NotFound, "not found").into_response(),
		}
	}

	async fn pass(&self, route: &Route, req: Request, principal: Option<Principal>) -> Response {
		assert!(self.services.serves(route.service), "every forwarded service is configured");
		let rest = &req.uri().path()[route.prefix.len()..];
		// decoded as a service might, so no route reaches above its upstream prefix
		let decoded = rest.to_ascii_lowercase().replace("%2e", ".").replace("%2f", "/").replace("%5c", "/").replace('\\', "/");
		if decoded.split('/').any(|s| s == "." || s == "..") {
			return Refusal::new(Code::BadRequest, "not a forwardable path").into_response();
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
				Refusal::new(Code::BadGateway, "the service did not answer").into_response()
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
	rate_limited: bool,
	published: bool,
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
	let principal = match route.gate {
		Gate::Open => None,
		Gate::Session => {
			let write = !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS);
			// a write must not outlive a revoked grant by even the cached GetMe's minute
			let freshness = if write { Freshness::Fresh } else { Freshness::Cached };
			let caller = signin::caller_of(&f.sign_in, req.method(), req.headers(), freshness).await;
			// an MCP client's bearer is ours: it never goes up
			req.headers_mut().remove(header::AUTHORIZATION);
			match caller {
				Ok(caller) => Some(Principal::User(user(caller, route.service))),
				Err(r) => return signin::refused(&f.sign_in, r),
			}
		}
	};
	f.pass(route, req, principal).await
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
		assert_eq!(route("/mfe/review_archive/mfe-review-archive-dashboard.js"), Some(("/mfe/review_archive", Gate::Open)));
		assert_eq!(route("/mfe/banking/wallet.js"), None, "banking's bundle is not forwarded");
		assert_eq!(route("/api/placement/payload/cleaning/Lyon"), None, "placement is not forwarded");
	}
}
