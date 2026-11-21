//! For tests only (feature `testing`): what the HTTP tests share: a stand-in for the services behind the conductor, the conductor
//! signed in through the development identity, and a browser keeping its cookies.

use std::{
	collections::HashMap,
	path::Path,
	sync::{Arc, Mutex},
};

use axum::{
	Router,
	body::{Body, Bytes, to_bytes},
	extract::Request,
	http::{HeaderMap, Method, StatusCode, Uri, header},
	middleware::{self, Next},
	response::IntoResponse,
};
use sa_auth::{Assertion, Keys, PermissionSet, Service, Signer};
use sa_panel::{Panel, session::pkce_challenge};
use tower::ServiceExt;

use crate::{
	concierge::{Concierge, DEV_CODE, DevIdentity},
	forward::{Forward, Services, Upstreams},
	grafana::Grafana,
	http,
	signin::{SignIn, SignInConfig},
};

pub const ORIGIN: &str = "http://127.0.0.1:59120";
pub const SEED: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

const MCP_VERIFIER: &str = "a-verifier-of-forty-three-or-more-characters-0123456789";

const MCP_REDIRECT: &str = "http://127.0.0.1/callback";
/// The client listens on a port of its own (RFC 8252 §7.3).
const MCP_LISTENING: &str = "http://127.0.0.1:43117/callback";
pub type Log = Arc<Mutex<Vec<Seen>>>;

/// What a service behind the conductor was sent.
#[derive(Clone, Debug)]
pub struct Seen {
	pub method: Method,
	pub path_and_query: String,
	pub headers: HeaderMap,
	pub body: Bytes,
}

impl Seen {
	/// Its assertion, verified as the service would: for `service`, this method and path, and
	/// for a source's, this body.
	pub fn assertion(&self, service: Service, body: Option<&[u8]>) -> Assertion {
		let keys: Keys = SEED.parse::<Signer>().unwrap().public().parse().unwrap();
		let token = self.headers[sa_auth::HEADER].to_str().unwrap();
		let path = self.path_and_query.split('?').next().unwrap();
		sa_auth::verify(
			&keys,
			token,
			service,
			sa_auth::Request {
				method: self.method.as_str(),
				path,
				body,
			},
			jiff::Timestamp::now().as_second(),
		)
		.expect("the conductor's assertion")
	}
}

/// A service at a loopback address: `routes`, and anything else answered with a cookie of its
/// own and a compressed, cacheable body. Every request is logged first.
//@codestyle::skip(no-tokio-spawn)
pub async fn upstream(routes: Router) -> (String, Log) {
	let seen: Log = Arc::default();
	let log = seen.clone();
	let app = routes
		.fallback(|| async {
			(
				[
					(header::SET_COOKIE, "upstream=1; Path=/"),
					(header::HeaderName::from_static("x-upstream"), "yes"),
					(header::CONTENT_ENCODING, "gzip"),
					(header::CACHE_CONTROL, "public, max-age=31536000"),
				],
				"ok",
			)
				.into_response()
		})
		.layer(middleware::from_fn(move |req: Request, next: Next| {
			let log = log.clone();
			async move {
				let (parts, body) = req.into_parts();
				let body = to_bytes(body, usize::MAX).await.unwrap();
				log.lock().unwrap().push(Seen {
					method: parts.method.clone(),
					path_and_query: parts.uri.path_and_query().unwrap().to_string(),
					headers: parts.headers.clone(),
					body: body.clone(),
				});
				next.run(Request::from_parts(parts, Body::from(body))).await
			}
		}));
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	//@codestyle::skip(no-tokio-spawn)
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(format!("http://{addr}"), seen)
}

/// The services, all at `base`.
pub fn services(base: &Uri) -> Services {
	Services::new(Upstreams {
		review_archive: base.clone(),
		playbook: base.clone(),
		sites: base.clone(),
		signer: SEED.parse().unwrap(),
	})
}

/// The conductor on `panel`, signing in whoever asks as `email` holding `permissions`, with every
/// service at `base`.
pub fn app_with(panel: Panel, permissions: PermissionSet, email: &str, base: &Uri, limits: http::Limits) -> Router {
	let (sign_in, forward) = conductor(panel, permissions, email, base);
	http::app(sign_in, Some(forward), None, limits, None, None, None, None, None)
}

/// What a viewer's browser and Grafana reach: the conductor of [`app_with`] with Grafana on `dir`,
/// Grafana's loopback listener, and Grafana itself, to run.
pub fn app_with_grafana(panel: Panel, permissions: PermissionSet, email: &str, base: &Uri, dir: &Path) -> (Router, Router, Grafana) {
	let (sign_in, forward) = conductor(panel, permissions, email, base);
	let grafana = Grafana::new(forward.clone(), dir.to_owned());
	let app = http::app(sign_in, Some(forward), Some(&grafana), http::Limits::default(), None, None, None, None, None);
	(app, grafana.loopback(), grafana)
}

pub fn app(panel: Panel, permissions: PermissionSet, base: &str) -> Router {
	app_with(
		panel,
		permissions,
		"dev@localhost",
		&base.parse().expect("a test's services are at a URL"),
		http::Limits::default(),
	)
}

#[derive(Default)]
pub struct Browser {
	pub jar: HashMap<String, String>,
}
impl Browser {
	pub async fn send_body(&mut self, app: &Router, method: Method, uri: &str, extra: &[(&str, &str)], csrf: bool, body: Body) -> (StatusCode, HeaderMap, Bytes) {
		let mut req = Request::builder().method(method).uri(uri);
		let cookie: Vec<String> = self.jar.iter().map(|(k, v)| format!("{k}={v}")).collect();
		if !cookie.is_empty() {
			req = req.header(header::COOKIE, cookie.join("; "));
		}
		if csrf && let Some(t) = self.jar.get("sa_csrf") {
			req = req.header("x-sa-csrf", t);
		}
		for (k, v) in extra {
			req = req.header(*k, *v);
		}
		let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
		for c in res.headers().get_all(header::SET_COOKIE) {
			let (pair, _) = c.to_str().unwrap().split_once(';').unwrap();
			let (k, v) = pair.split_once('=').unwrap();
			self.jar.insert(k.to_owned(), v.to_owned());
		}
		let (status, headers) = (res.status(), res.headers().clone());
		let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
		(status, headers, body)
	}

	/// `body` sent as JSON, the answer read as JSON (`Null` for an empty or non-JSON one).
	pub async fn call(&mut self, app: &Router, method: Method, uri: &str, body: Option<serde_json::Value>) -> (StatusCode, serde_json::Value) {
		let json = [("content-type", "application/json")];
		let (status, _, bytes) = self
			.send_body(
				app,
				method,
				uri,
				if body.is_some() { &json } else { &[] },
				true,
				body.map_or(Body::empty(), |b| Body::from(b.to_string())),
			)
			.await;
		(status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)) // 204s and HTML errors carry no JSON; callers assert on the status first
	}

	pub async fn send(&mut self, app: &Router, method: Method, uri: &str, extra: &[(&str, &str)], csrf: bool) -> (StatusCode, HeaderMap) {
		let (status, headers, _) = self.send_body(app, method, uri, extra, csrf, Body::empty()).await;
		(status, headers)
	}

	pub async fn sign_in(&mut self, app: &Router) {
		let (_, headers) = self.send(app, Method::GET, "/auth/login", &[], false).await;
		let location = headers[header::LOCATION].to_str().unwrap().strip_prefix(ORIGIN).unwrap().to_owned();
		assert!(location.starts_with(&format!("/auth/callback?code={DEV_CODE}")));
		self.send(app, Method::GET, &location, &[], false).await;
		assert!(self.jar.contains_key("sa_session"));
	}

	/// The `Cookie` header this browser sends.
	pub fn cookie(&self) -> String {
		self.jar.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
	}
}

/// An MCP client registered and authorized through the whole OAuth dance, its user consenting to
/// `approval`: its bearer, the browser of the user who consented, and that browser's own session.
pub async fn mcp_grant(app: &Router, approval: &str) -> (String, Browser, String) {
	let app = app.clone();
	let mut client = Browser::default();
	let (status, _, body) = client.send_body(&app, Method::GET, "/.well-known/oauth-protected-resource/mcp", &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::OK);
	let resource: serde_json::Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(resource["authorization_servers"][0], format!("{ORIGIN}/mcp"));
	let (_, _, body) = client
		.send_body(&app, Method::GET, "/.well-known/oauth-authorization-server/mcp", &[], false, Body::empty())
		.await;
	let server: serde_json::Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(server["token_endpoint"], format!("{ORIGIN}/mcp/token"));

	let registration = serde_json::json!({ "redirect_uris": [MCP_REDIRECT], "client_name": "Claude Code" }).to_string();
	let (status, _, body) = client
		.send_body(&app, Method::POST, "/mcp/register", &[("content-type", "application/json")], false, Body::from(registration))
		.await;
	assert_eq!(status, StatusCode::CREATED);
	let client_id = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["client_id"].as_str().unwrap().to_owned();

	// Signed out: the sign-in, then back to the consent page.
	let mut user = Browser::default();
	let authorize = format!(
		"/mcp/authorize?response_type=code&client_id={client_id}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state=xyz",
		url::form_urlencoded::byte_serialize(MCP_LISTENING.as_bytes()).collect::<String>(),
		pkce_challenge(MCP_VERIFIER),
	);
	let (status, headers) = user.send(&app, Method::GET, &authorize, &[], false).await;
	assert_eq!(status, StatusCode::SEE_OTHER);
	let login = headers[header::LOCATION].to_str().unwrap().to_owned();
	assert!(login.starts_with("/auth/login?return_to="), "{login}");
	let (_, headers) = user.send(&app, Method::GET, &login, &[], false).await;
	let callback = path_of(headers[header::LOCATION].to_str().unwrap());
	let (_, headers) = user.send(&app, Method::GET, &callback, &[], false).await;
	assert_eq!(headers[header::LOCATION], authorize.as_str(), "back where the sign-in began");
	let (status, _, page) = user.send_body(&app, Method::GET, &authorize, &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::OK);
	assert!(String::from_utf8_lossy(&page).contains("Claude Code wants to act as you"));

	let form = |csrf: &str| {
		url::form_urlencoded::Serializer::new(String::new())
			.extend_pairs([
				("client_id", client_id.as_str()),
				("redirect_uri", MCP_LISTENING),
				("code_challenge", &pkce_challenge(MCP_VERIFIER)),
				("state", "xyz"),
				("csrf", csrf),
				("approval", approval),
			])
			.finish()
	};
	let urlencoded = [("content-type", "application/x-www-form-urlencoded")];
	let (status, _, _) = user.send_body(&app, Method::POST, "/mcp/authorize", &urlencoded, false, Body::from(form("forged"))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "the form carries the session's CSRF token");
	let csrf = user.jar["sa_csrf"].clone();
	let session = user.jar["sa_session"].clone();
	let (status, headers, _) = user.send_body(&app, Method::POST, "/mcp/authorize", &urlencoded, false, Body::from(form(&csrf))).await;
	assert_eq!(status, StatusCode::FOUND, "to concierge's sign-in, for the client");
	let (status, headers) = user.send(&app, Method::GET, &path_of(headers[header::LOCATION].to_str().unwrap()), &[], false).await;
	assert_eq!(status, StatusCode::SEE_OTHER);
	assert_eq!(user.jar["sa_session"], session, "the browser keeps its own session");
	let back = url::Url::parse(headers[header::LOCATION].to_str().unwrap()).unwrap();
	assert_eq!(format!("{}://{}{}", back.scheme(), back.authority(), back.path()), MCP_LISTENING);
	let query: std::collections::HashMap<_, _> = back.query_pairs().into_owned().collect();
	assert_eq!(query["state"], "xyz");

	let exchange = |verifier: &str| {
		url::form_urlencoded::Serializer::new(String::new())
			.extend_pairs([
				("grant_type", "authorization_code"),
				("client_id", client_id.as_str()),
				("code", &query["code"]),
				("code_verifier", verifier),
				("redirect_uri", MCP_LISTENING),
			])
			.finish()
	};
	let (status, _, body) = client.send_body(&app, Method::POST, "/mcp/token", &urlencoded, false, Body::from(exchange(MCP_VERIFIER))).await;
	assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
	let bearer = serde_json::from_slice::<serde_json::Value>(&body).unwrap()["access_token"].as_str().unwrap().to_owned();
	let (status, _, _) = client.send_body(&app, Method::POST, "/mcp/token", &urlencoded, false, Body::from(exchange(MCP_VERIFIER))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a code redeems once");

	(bearer, user, session)
}

/// An MCP client's session at `/mcp`.
pub struct Mcp {
	pub bearer: String,
	pub session: String,
}
impl Mcp {
	/// Initializes as the client holding `bearer`, declaring `capabilities`: what `/mcp` answered, and the session.
	pub async fn initialize(app: &Router, bearer: &str, capabilities: serde_json::Value) -> (serde_json::Value, Self) {
		let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-06-18", "capabilities": capabilities, "clientInfo": { "name": "t", "version": "0" } } });
		let auth = format!("Bearer {bearer}");
		let (status, headers, body) = Browser::default()
			.send_body(
				app,
				Method::POST,
				"/mcp",
				&[("authorization", &auth), ("content-type", "application/json")],
				false,
				Body::from(body.to_string()),
			)
			.await;
		assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
		let session = headers["mcp-session-id"].to_str().unwrap().to_owned();
		(serde_json::from_slice(&body).unwrap(), Self { bearer: bearer.to_owned(), session })
	}

	fn request(&self, body: &serde_json::Value, accept: &str) -> Request<Body> {
		Request::builder()
			.method(Method::POST)
			.uri("/mcp")
			.header(header::AUTHORIZATION, format!("Bearer {}", self.bearer))
			.header("mcp-session-id", &self.session)
			.header(header::CONTENT_TYPE, "application/json")
			.header(header::ACCEPT, accept)
			.body(Body::from(body.to_string()))
			.unwrap()
	}

	/// What `/mcp` answers `method` with `params`, as plain JSON.
	pub async fn rpc(&self, app: &Router, method: &str, params: serde_json::Value) -> (StatusCode, serde_json::Value) {
		let body = serde_json::json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
		let res = app.clone().oneshot(self.request(&body, "application/json")).await.unwrap();
		let status = res.status();
		let json = res.headers().get(header::CONTENT_TYPE).is_some_and(|t| t == "application/json");
		let body = to_bytes(res.into_body(), usize::MAX).await.unwrap();
		match (json, body.is_empty()) {
			(true, _) => (status, serde_json::from_slice(&body).unwrap()),
			(false, true) => (status, serde_json::Value::Null),
			(false, false) => (status, serde_json::Value::String(String::from_utf8(body.to_vec()).unwrap())),
		}
	}

	/// A tool's answer, its text parsed, and whether it is an error.
	pub async fn tool(&self, app: &Router, name: &str, arguments: serde_json::Value) -> (serde_json::Value, bool) {
		let (status, res) = self.rpc(app, "tools/call", serde_json::json!({ "name": name, "arguments": arguments })).await;
		assert_eq!(status, StatusCode::OK, "{res}");
		tool_result(&res)
	}

	/// `tools/call`, accepting an event stream: what it sends, as it comes.
	pub async fn tool_streamed(&self, app: &Router, name: &str, arguments: serde_json::Value) -> Events {
		let body = serde_json::json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/call", "params": { "name": name, "arguments": arguments } });
		let res = app.clone().oneshot(self.request(&body, "application/json, text/event-stream")).await.unwrap();
		assert_eq!(res.status(), StatusCode::OK);
		assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
		Events {
			body: res.into_body(),
			read: String::new(),
		}
	}

	/// The client's answer to what `/mcp` asked it.
	pub async fn answer(&self, app: &Router, id: &serde_json::Value, result: serde_json::Value) -> StatusCode {
		let body = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
		app.clone().oneshot(self.request(&body, "application/json")).await.unwrap().status()
	}
}

/// A tool's result, its text parsed, and whether it is an error.
pub fn tool_result(res: &serde_json::Value) -> (serde_json::Value, bool) {
	let result = &res["result"];
	(
		serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap(),
		result["isError"].as_bool().unwrap(),
	)
}

/// An answer's SSE stream.
pub struct Events {
	body: Body,
	read: String,
}
impl Events {
	/// Its next event's data; `None` once it ended.
	pub async fn next(&mut self) -> Option<serde_json::Value> {
		use http_body_util::BodyExt;
		loop {
			if let Some(at) = self.read.find("\n\n") {
				let event: String = self.read.drain(..at + 2).collect();
				let data = event.lines().find_map(|l| l.strip_prefix("data: ")).expect("an event carries data");
				return Some(serde_json::from_str(data).unwrap());
			}
			let frame = self.body.frame().await?.unwrap();
			if let Ok(bytes) = frame.into_data() {
				self.read.push_str(std::str::from_utf8(&bytes).unwrap());
			}
		}
	}
}

pub fn conductor(panel: Panel, permissions: PermissionSet, email: &str, base: &Uri) -> (SignIn, Forward) {
	let who = DevIdentity {
		user_id: DevIdentity::named(email),
		permissions,
		email: email.to_owned(),
	};
	let config = SignInConfig {
		panel_origin: ORIGIN.to_owned(),
		concierge_origin: ORIGIN.to_owned(),
	};
	let sign_in = SignIn::new(panel, Concierge::dev(who), config);
	let forward = Forward::new(sign_in.clone(), services(base));
	(sign_in, forward)
}

fn path_of(location: &str) -> String {
	location.strip_prefix(ORIGIN).unwrap_or(location).to_owned()
}
