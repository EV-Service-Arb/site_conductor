//! The authorization server MCP clients sign in through (OAuth 2.1: RFC 8414 and 9728 metadata,
//! RFC 7591 registration, the code with PKCE S256). A consent sends the browser through
//! concierge's sign-in once more: the session it opens is the client's, its id the bearer.
//!
//! ```text
//! GET  /mcp/authorize   client and redirect_uri registered, else 400 page (nothing redirected)
//!                       decision=deny → redirect_uri?error=access_denied
//!                       signed out → /auth/login?return_to=<this> → back
//!                       → the consent page: a form carrying the request and the CSRF token
//! POST /mcp/authorize   CSRF (the form's), the session → concierge's sign-in, the grant sealed in
//!                       the pre-login → /auth/callback: a session for the client
//!                       → redirect_uri?code&state (the code: 60 s, once)
//! POST /mcp/token       code + verifier + client_id + redirect_uri → {access_token}: the
//!                       session's id, as long as the session lives
//! ```

use axum::{
	Form, Json, Router,
	extract::{Query, State, rejection::QueryRejection},
	http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
	response::{Html, IntoResponse, Redirect, Response},
	routing::{get, post},
};
use jiff::Timestamp;
use sa_panel::oauth::{Client, Grant, TokenRequest};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
	api::Code,
	concierge::Issued,
	cookies::{self, CSRF_HEADER},
	signin::{self, Freshness, Page, SignIn, private},
};

/// Where the MCP endpoint is: the resource, and the issuer under it.
pub const PATH: &str = "/mcp";
const RESOURCE_METADATA: &str = "/.well-known/oauth-protected-resource/mcp";
const SERVER_METADATA: &str = "/.well-known/oauth-authorization-server/mcp";

const MAX_REDIRECT_URIS: usize = 8;
const MAX_URI: usize = 512;
const MAX_NAME: usize = 200;

/// What the consent page may load: nothing. Its form leaves for concierge and the client, so no
/// `form-action`.
const CONSENT_CSP: &str = "default-src 'self'; frame-ancestors 'none'; base-uri 'none'";

pub(crate) fn routes(sign_in: SignIn) -> Router {
	Router::new()
		.route(RESOURCE_METADATA, get(resource_metadata))
		.route(SERVER_METADATA, get(server_metadata))
		.route(&format!("{PATH}/register"), post(register))
		.route(&format!("{PATH}/authorize"), get(authorize).post(consent))
		.route(&format!("{PATH}/token"), post(token))
		.with_state(sign_in)
}

/// The `WWW-Authenticate` of a 401 at the MCP endpoint: where to start signing in.
pub(crate) fn challenge(s: &SignIn) -> String {
	format!("Bearer resource_metadata=\"{}{RESOURCE_METADATA}\"", s.panel_origin())
}

async fn resource_metadata(State(s): State<SignIn>) -> Json<Value> {
	let base = format!("{}{PATH}", s.panel_origin());
	Json(json!({ "resource": base, "authorization_servers": [base], "bearer_methods_supported": ["header"] }))
}

async fn server_metadata(State(s): State<SignIn>) -> Json<Value> {
	let base = format!("{}{PATH}", s.panel_origin());
	Json(json!({
		"issuer": base,
		"authorization_endpoint": format!("{base}/authorize"),
		"token_endpoint": format!("{base}/token"),
		"registration_endpoint": format!("{base}/register"),
		"response_types_supported": ["code"],
		"grant_types_supported": ["authorization_code"],
		"code_challenge_methods_supported": ["S256"],
		"token_endpoint_auth_methods_supported": ["none"],
	}))
}

#[derive(Deserialize)]
struct Registration {
	redirect_uris: Vec<String>,
	client_name: Option<String>,
}

async fn register(State(s): State<SignIn>, Json(reg): Json<Registration>) -> Response {
	if reg.redirect_uris.is_empty() || reg.redirect_uris.len() > MAX_REDIRECT_URIS {
		return oauth_error(OauthError::InvalidRedirectUri, &format!("1 to {MAX_REDIRECT_URIS} redirect_uris"));
	}
	if let Some(bad) = reg.redirect_uris.iter().find(|u| !redirect_allowed(u)) {
		return oauth_error(OauthError::InvalidRedirectUri, &format!("{bad}: https, or http on loopback, ≤ {MAX_URI} bytes"));
	}
	if reg.client_name.as_ref().is_some_and(|n| n.chars().count() > MAX_NAME) {
		return oauth_error(OauthError::InvalidClientMetadata, &format!("client_name: ≤ {MAX_NAME} characters"));
	}
	let now = Timestamp::now();
	let id = match s.panel.register_client(&reg.redirect_uris, reg.client_name.as_deref(), now).await {
		Ok(id) => id,
		Err(e) => return internal(&e, "registering an MCP client"),
	};
	let body = json!({
		"client_id": id,
		"client_id_issued_at": now.as_second(),
		"client_name": reg.client_name,
		"redirect_uris": reg.redirect_uris,
		"token_endpoint_auth_method": "none",
		"grant_types": ["authorization_code"],
		"response_types": ["code"],
	});
	(StatusCode::CREATED, Json(body)).into_response()
}

fn redirect_allowed(uri: &str) -> bool {
	uri.len() <= MAX_URI && url::Url::parse(uri).is_ok_and(|u| u.scheme() == "https" || (u.scheme() == "http" && matches!(u.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"))))
}

#[derive(Deserialize)]
struct AuthorizeQuery {
	response_type: String,
	#[serde(flatten)]
	link: Link,
	code_challenge_method: String,
	state: Option<String>,
	resource: Option<String>,
	decision: Option<Decision>,
}

/// The consent page's Cancel; allowing is the form's POST.
#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Decision {
	Deny,
}

/// Asks the user to confirm; no code is issued here. The client is checked first, so that
/// Cancel and every refusal know where they may send the browser.
async fn authorize(State(s): State<SignIn>, uri: Uri, headers: HeaderMap, q: Result<Query<AuthorizeQuery>, QueryRejection>) -> Response {
	let Ok(Query(q)) = q else {
		return bad_link();
	};
	if q.response_type != "code" || q.code_challenge_method != "S256" || q.resource.as_ref().is_some_and(|r| r.trim_end_matches('/') != format!("{}{PATH}", s.panel_origin())) {
		return bad_link();
	}
	let client = match registered(&s, &q.link).await {
		Ok(c) => c,
		Err(res) => return *res,
	};
	if let Some(Decision::Deny) = q.decision {
		return back(&q.link.redirect_uri, &[("error", "access_denied")], q.state.as_deref());
	}
	let here = uri.path_and_query().expect("a request line has a path").as_str();
	let caller = match signin::check(&s, &headers, Freshness::Cached).await {
		Ok(c) => c,
		Err(r) if r.code() == Code::Unauthenticated => return private(Redirect::to(&format!("/auth/login?return_to={}", signin::encode(here))).into_response()),
		Err(r) => return signin::refused(&s, r),
	};
	let Some(csrf) = s.cookies().get(&headers, cookies::CSRF) else {
		return bad_link();
	};
	let field = |name: &str, value: &str| format!("<input type=\"hidden\" name=\"{name}\" value=\"{}\">", escape(value));
	let fields = [
		field("client_id", &q.link.client_id),
		field("redirect_uri", &q.link.redirect_uri),
		field("code_challenge", &q.link.code_challenge),
		q.state.as_deref().map(|st| field("state", st)).unwrap_or_default(),
		field("csrf", csrf),
	]
	.concat();
	let client_name = escape(client.name.as_deref().unwrap_or("An MCP client"));
	let body = format!(
		"<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
		 <title>Connect {client_name}</title></head><body><main><h1>{client_name} wants to act as you on Service-Arb</h1>\
		 <p>Signed in as <b>{email}</b>.</p>\
		 <p>It will read and change what you can in the panel. It asks you before anything that removes or spends.</p>\
		 <p>You will be returned to <code>{redirect}</code>.</p>\
		 <form method=\"post\" action=\"{PATH}/authorize\">{fields}<button>Allow</button> <a href=\"{cancel}\">Cancel</a></form>\
		 </main></body></html>",
		email = escape(&caller.email),
		redirect = escape(&q.link.redirect_uri),
		cancel = escape(&format!("{here}&decision=deny")),
	);
	let mut res = private((StatusCode::OK, Html(body)).into_response());
	res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CONSENT_CSP));
	res
}

/// What an MCP client's link to authorize names, carried on through the consent page.
#[derive(Deserialize)]
struct Link {
	client_id: String,
	redirect_uri: String,
	code_challenge: String,
}

#[derive(Deserialize)]
struct ConsentForm {
	#[serde(flatten)]
	link: Link,
	state: Option<String>,
	csrf: String,
}

/// The consent page's POST: concierge signs the user in again, for the client.
async fn consent(State(s): State<SignIn>, mut headers: HeaderMap, Form(f): Form<ConsentForm>) -> Response {
	let Ok(csrf) = HeaderValue::from_str(&f.csrf) else {
		return bad_link();
	};
	headers.insert(CSRF_HEADER, csrf);
	if !s.cookies().csrf_ok(&headers) {
		return bad_link();
	}
	if let Err(res) = registered(&s, &f.link).await {
		return *res;
	}
	let caller = match signin::check(&s, &headers, Freshness::Cached).await {
		Ok(c) => c,
		Err(r) => return signin::refused(&s, r),
	};
	let grant = Grant {
		user_id: caller.user_id,
		client_id: f.link.client_id,
		redirect_uri: f.link.redirect_uri,
		challenge: f.link.code_challenge,
		state: f.state,
	};
	// Dev sign-in signs in whoever its configuration names: this user, by name.
	let dev_as = s.concierge.is_dev().then_some(caller.email);
	signin::to_concierge(&s, None, dev_as.as_deref(), Some(&grant), None)
}

/// The callback's end of a consent: concierge signed the user in again, for the client.
pub(crate) async fn granted(s: &SignIn, grant: &Grant, issued: Issued) -> Response {
	if issued.user_id != grant.user_id {
		return Page {
			status: StatusCode::FORBIDDEN,
			title: "Another account",
			message: "evinvest.ltd is signed in as another account than this panel. Sign in as one account, then connect again.",
		}
		.into_response();
	}
	match s.panel.grant(grant, &issued.tokens, Timestamp::now()).await {
		Ok(code) => {
			tracing::info!(user_id = %grant.user_id, client_id = grant.client_id, "an MCP client granted");
			back(&grant.redirect_uri, &[("code", &code)], grant.state.as_deref())
		}
		Err(e) => {
			crate::report(&e, "granting an MCP client");
			Page {
				status: StatusCode::INTERNAL_SERVER_ERROR,
				title: "Connecting failed",
				message: "Something went wrong on our side. Try again.",
			}
			.into_response()
		}
	}
}

#[derive(Deserialize)]
struct TokenForm {
	grant_type: Option<String>,
	client_id: Option<String>,
	code: Option<String>,
	code_verifier: Option<String>,
	redirect_uri: Option<String>,
}

async fn token(State(s): State<SignIn>, Form(f): Form<TokenForm>) -> Response {
	if f.grant_type.as_deref() != Some("authorization_code") {
		return oauth_error(OauthError::UnsupportedGrantType, "authorization_code");
	}
	let (Some(client_id), Some(code), Some(verifier), Some(redirect_uri)) = (f.client_id, f.code, f.code_verifier, f.redirect_uri) else {
		return oauth_error(OauthError::InvalidRequest, "client_id, code, code_verifier and redirect_uri");
	};
	let request = TokenRequest {
		code: &code,
		client_id: &client_id,
		redirect_uri: &redirect_uri,
		verifier: &verifier,
	};
	match s.panel.redeem(request, Timestamp::now()).await {
		Ok(Some(bearer)) => private(Json(json!({ "access_token": bearer.as_str(), "token_type": "Bearer" })).into_response()),
		Ok(None) => oauth_error(OauthError::InvalidGrant, "the code is spent, expired, or not this client's"),
		Err(e) => internal(&e, "redeeming an MCP code"),
	}
}

/// The client, when `redirect_uri` is one of its own (a loopback one on any port, RFC 8252 §7.3)
/// and the challenge an S256 one.
async fn registered(
	s: &SignIn,
	Link {
		client_id,
		redirect_uri,
		code_challenge: challenge,
	}: &Link,
) -> Result<Client, Box<Response>> {
	if challenge.len() != 43 || !challenge.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') || !redirect_allowed(redirect_uri) {
		return Err(Box::new(bad_link()));
	}
	let client = match s.panel.client(client_id).await {
		Ok(Some(c)) => c,
		Ok(None) => return Err(Box::new(bad_link())),
		Err(e) => {
			crate::report(&e, "reading an MCP client");
			return Err(Box::new(private(
				Page {
					status: StatusCode::INTERNAL_SERVER_ERROR,
					title: "Connecting failed",
					message: "Something went wrong on our side. Try again.",
				}
				.into_response(),
			)));
		}
	};
	let loopback = |u: &str| {
		let mut u = url::Url::parse(u).expect("checked by redirect_allowed");
		(u.scheme() == "http").then(|| u.set_port(None).map(|()| u).expect("http takes a port"))
	};
	match client
		.redirect_uris
		.iter()
		.any(|u| u == redirect_uri || loopback(u).is_some_and(|u| Some(u) == loopback(redirect_uri)))
	{
		true => Ok(client),
		false => Err(Box::new(bad_link())),
	}
}

fn back(redirect_uri: &str, pairs: &[(&str, &str)], state: Option<&str>) -> Response {
	let mut to = url::Url::parse(redirect_uri).expect("a registered redirect_uri parses");
	to.query_pairs_mut().extend_pairs(pairs).extend_pairs(state.map(|st| ("state", st)));
	private(Redirect::to(to.as_str()).into_response())
}

fn bad_link() -> Response {
	private(
		Page {
			status: StatusCode::BAD_REQUEST,
			title: "Connecting failed",
			message: "This link is not one your MCP client would open, or it expired. Connect again from the client.",
		}
		.into_response(),
	)
}

/// RFC 6749 §5.2's and RFC 7591 §3.2.2's.
#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum OauthError {
	InvalidRedirectUri,
	InvalidClientMetadata,
	UnsupportedGrantType,
	InvalidRequest,
	InvalidGrant,
}

fn oauth_error(error: OauthError, description: &str) -> Response {
	private((StatusCode::BAD_REQUEST, Json(json!({ "error": error, "error_description": description }))).into_response())
}

fn internal(e: &eyre::Report, what: &str) -> Response {
	crate::report(e, what);
	(StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "server_error" }))).into_response()
}

fn escape(s: &str) -> String {
	s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}
