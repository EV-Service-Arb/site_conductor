//! Signing in through concierge (spec §4), and the gate every `/api/v1` request passes.
//!
//! ```text
//! GET /auth/login      ?return_to=<path on this origin>, else 400; it, the state and the
//!                      PKCE verifier sealed into the pre-login cookie
//!                      → 302 concierge /api/auth/authorize?client_id=sa&…&code_challenge
//!                      [&prompt=select_account, as asked]
//!                      [as=<email>: dev sign-in only, else 400; sealed in the pre-login]
//! GET /auth/callback   state from the cookie = state in the URL (constant time), and the
//!                      state not redeemed before, else 400 and the code is never presented;
//!                      ExchangeCode(code, verifier); the browser's previous session closed
//!                      → server-side session, cookies → 303 return_to, else /
//! /api/v1/*            each row's gate (`Vouch for ApiState`): an MCP client's bearer
//!                      ([`crate::oauth`]), else CSRF on anything but GET and the cookie; the
//!                      session (access token rotated when it is about to expire); GetMe
//!                      (≤ 60 s cache; fresh where the row says) → the caller's permissions,
//!                      else 401/503; then the row's permission, else 403
//! POST /auth/logout    CSRF; every session of the user is closed
//! ```
//!
//! Signing out of evinvest.ltd revokes the concierge token family: the panel's session ends
//! at its next rotation, within the access token's lifetime. A permission revoked at concierge
//! is seen within [`ME_TTL`].
//!
//! The browser never holds a concierge token: only a random session id, whose hash names a
//! row holding the tokens sealed.
//!
//! Under `PANEL_DEV_SIGN_IN` (development, loopback only) there is no concierge:
//! `/auth/login` sends the browser straight to `/auth/callback` with [`DEV_CODE`], and
//! [`Concierge::dev`] answers for the configured user, or `?as=`'s; everything else above runs
//! unchanged.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use axum::{
	Extension,
	extract::{FromRequestParts, Query, State},
	http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
	response::{Html, IntoResponse, NoContent, Response},
};
use ev_lib::api::{Body, JsonSchema, OperationInput, Refusal};
use jiff::Timestamp;
use sa_auth::{Members, PermissionSet, Principal, Vouch};
use sa_panel::{
	Panel,
	oauth::Grant,
	session::{PRELOGIN_TTL, Returned, SessionError, SessionKey},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	access::PermissionName,
	api::{ApiResult, Code},
	concierge::{CLIENT_ID, CodeExchange, Concierge, ConciergeError, DEV_CODE, DevIdentity, Me},
	cookies::{self, Cookies},
	grafana::Loopback,
	http::{ApiState, Flags, Gate},
};

/// How long `GetMe` is trusted for a session: a grant revoked at concierge shuts the panel
/// at most this much later (at the next token rotation, concierge refuses outright).
pub const ME_TTL: Duration = Duration::from_secs(60);

/// What a sign-in page may load and do: nothing but this origin, and never inside a frame.
pub const PAGE_CSP: &str = "default-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

/// Longest `return_to`: a path the panel links to, with room for its query.
const MAX_RETURN_TO: usize = 512;

/// Longest dev sign-in `as`: RFC 5321's longest address.
const MAX_DEV_AS: usize = 254;

/// What signing in needs.
#[derive(Clone, Debug)]
pub struct SignInConfig {
	/// e.g. `https://sa.evinvest.ltd`, no trailing slash.
	pub panel_origin: String,
	/// e.g. `https://evinvest.ltd`.
	pub concierge_origin: String,
}

/// The sign-in's state, shared by the routes and the gate.
#[derive(Clone, Debug)]
pub struct SignIn {
	pub panel: Panel,
	pub concierge: Concierge,
	config: Arc<SignInConfig>,
	cookies: Cookies,
	me: Arc<Mutex<HashMap<SessionKey, (Instant, Me)>>>,
}

impl SignIn {
	pub fn new(panel: Panel, concierge: Concierge, mut config: SignInConfig) -> Self {
		config.panel_origin = config.panel_origin.trim_end_matches('/').to_owned();
		config.concierge_origin = config.concierge_origin.trim_end_matches('/').to_owned();
		let cookies = Cookies::new(config.panel_origin.starts_with("https://"));
		Self {
			panel,
			concierge,
			config: Arc::new(config),
			cookies,
			me: Arc::default(),
		}
	}

	pub fn cookies(&self) -> &Cookies {
		&self.cookies
	}

	/// The panel's own origin, e.g. `https://sa.evinvest.ltd`: the one a live socket's
	/// handshake must come from.
	pub fn panel_origin(&self) -> &str {
		&self.config.panel_origin
	}

	fn redirect_uri(&self) -> String {
		format!("{}/auth/callback", self.config.panel_origin)
	}

	fn cached_me(&self, key: &SessionKey) -> Option<Me> {
		let cache = self.me.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
		cache.get(key).filter(|(at, _)| at.elapsed() < ME_TTL).map(|(_, me)| me.clone())
	}

	fn remember_me(&self, key: SessionKey, me: Me) {
		let mut cache = self.me.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
		// Bounded without a sweeper: stale entries go whenever the map has grown.
		if cache.len() >= 1024 {
			cache.retain(|_, (at, _)| at.elapsed() < ME_TTL);
		}
		cache.insert(key, (Instant::now(), me));
	}

	/// Forgets every cached `GetMe` of the user: their sessions are all closed.
	fn forget_user(&self, user_id: uuid::Uuid) {
		self.me.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|_, (_, me)| me.user_id != user_id);
	}

	fn forget_me(&self, key: &SessionKey) {
		self.me.lock().unwrap_or_else(std::sync::PoisonError::into_inner).remove(key);
	}
}

/// The signed-in user of an `/api/v1` request, put in its extensions by the gate; `GET /me`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
pub struct Caller {
	pub user_id: Uuid,
	pub email: String,
	#[serde(skip)]
	pub email_verified: bool,
	pub preferred_name: String,
	/// Concrete `sa` permissions, as concierge resolved them.
	#[schemars(with = "Vec<PermissionName>")]
	pub permissions: PermissionSet,
	/// Signed in by `PANEL_DEV_SIGN_IN`, not concierge: `/me` says so, for the UI to show.
	pub dev_sign_in: bool,
	/// Who acts in this account, lending their permissions; `None`: its own user.
	#[serde(rename = "acting")]
	pub actor: Option<sa_auth::Actor>,
	/// What an MCP client's grant lets it do without asking; `None`: a browser's, whose assistant is told with each question.
	#[serde(skip)]
	pub granted: Option<sa_core::agent::Approval>,
}

impl Caller {
	/// Who is at the session: the actor while acting, whosever account it is.
	pub(crate) fn person(&self) -> Uuid {
		match &self.actor {
			Some(a) => a.sub.parse().expect("an actor's sub is its own user's id"),
			None => self.user_id,
		}
	}
}

impl<S: Send + Sync> FromRequestParts<S> for Caller {
	type Rejection = std::convert::Infallible;

	async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
		Ok(parts.extensions.get::<Self>().expect("routed under the /api/v1 gate").clone())
	}
}

impl OperationInput for Caller {}

impl sa_core::prices::Payer for Caller {
	type Who = Uuid;

	fn payer(&self) -> Uuid {
		self.user_id
	}
}

#[derive(Deserialize)]
pub struct Login {
	return_to: Option<String>,
	prompt: Option<Prompt>,
	#[serde(rename = "as")]
	dev_as: Option<String>,
}

/// `GET /auth/login`.
pub async fn login(State(s): State<SignIn>, Query(q): Query<Login>) -> Response {
	if q.return_to.as_deref().is_some_and(|p| !same_origin_path(p)) {
		return private(
			Page {
				status: StatusCode::BAD_REQUEST,
				title: "Sign-in failed",
				message: "This sign-in link is not valid here. Start again.",
			}
			.into_response(),
		);
	}
	if q.dev_as.as_deref().is_some_and(|e| !s.concierge.is_dev() || !dev_email(e)) {
		return private(
			Page {
				status: StatusCode::BAD_REQUEST,
				title: "Sign-in failed",
				message: "This sign-in link is not valid here. Start again.",
			}
			.into_response(),
		);
	}
	to_concierge(&s, q.return_to.as_deref(), q.dev_as.as_deref(), None, q.prompt)
}

#[derive(Deserialize)]
pub struct Callback {
	code: Option<String>,
	state: Option<String>,
	error: Option<String>,
}

/// `GET /auth/callback`.
pub async fn callback(State(s): State<SignIn>, headers: HeaderMap, Query(q): Query<Callback>) -> Response {
	// The pre-login is single-use whatever happens next.
	let clear = s.cookies.clear(cookies::PRELOGIN);
	let with_clear = |res: Response| {
		let mut res = private(res);
		res.headers_mut().append(header::SET_COOKIE, clear.clone());
		res
	};
	if let Some(error) = q.error.as_deref() {
		// Fixed pages: concierge's error word picks one, and is not shown.
		return with_clear(match error {
			"access_denied" => Page {
				status: StatusCode::FORBIDDEN,
				title: "No access",
				message: "Your account has no access to the Service-Arb panel. Ask its admin for one.",
			}
			.into_response(),
			"temporarily_unavailable" => Page {
				status: StatusCode::SERVICE_UNAVAILABLE,
				title: "Sign-in unavailable",
				message: "Signing in is unavailable right now. Try again in a minute.",
			}
			.into_response(),
			_ => Page {
				status: StatusCode::BAD_REQUEST,
				title: "Sign-in failed",
				message: "The sign-in could not be completed. Try again.",
			}
			.into_response(),
		});
	}
	let now = Timestamp::now();
	let pre = match (s.cookies.get(&headers, cookies::PRELOGIN), q.state.as_deref()) {
		(Some(cookie), Some(state)) => s.panel.finish_sign_in(Returned { cookie, state }, now),
		_ => None,
	};
	let (Some(pre), Some(code)) = (pre, q.code.as_deref().filter(|c| !c.is_empty())) else {
		tracing::info!("sign-in callback without a matching pre-login; the code is not presented");
		return with_clear(
			Page {
				status: StatusCode::BAD_REQUEST,
				title: "Sign-in failed",
				message: "This sign-in link is not valid here, or has expired. Start again.",
			}
			.into_response(),
		);
	};
	// A replayed callback (a leaked URL with the pre-login cookie still in the jar) stops
	// here: a state is redeemed once, across replicas.
	match s.panel.consume_state(&pre.state, now).await {
		Ok(true) => {}
		Ok(false) => {
			tracing::info!("sign-in callback with a state redeemed before; the code is not presented");
			return with_clear(
				Page {
					status: StatusCode::BAD_REQUEST,
					title: "Sign-in failed",
					message: "This sign-in was already used. Start again.",
				}
				.into_response(),
			);
		}
		Err(e) => {
			crate::report(&e, "redeeming a sign-in state");
			return with_clear(
				Page {
					status: StatusCode::INTERNAL_SERVER_ERROR,
					title: "Sign-in failed",
					message: "Something went wrong on our side. Try again.",
				}
				.into_response(),
			);
		}
	}
	let concierge = match pre.dev_as {
		None => s.concierge.clone(),
		Some(email) => match DevIdentity::user_of(&s.panel, &email).await {
			Ok(user_id) => s.concierge.dev_as(user_id, email),
			Err(e) => {
				crate::report(&e, "finding the dev sign-in's user");
				return with_clear(
					Page {
						status: StatusCode::INTERNAL_SERVER_ERROR,
						title: "Sign-in failed",
						message: "Something went wrong on our side. Try again.",
					}
					.into_response(),
				);
			}
		},
	};
	let issued = match concierge
		.exchange_code(CodeExchange {
			code,
			redirect_uri: &s.redirect_uri(),
			verifier: &pre.verifier,
		})
		.await
	{
		Ok(i) => i,
		Err(ConciergeError::Refused(tonic::Code::PermissionDenied)) =>
			return with_clear(
				Page {
					status: StatusCode::FORBIDDEN,
					title: "No access",
					message: "Your account has no access to the Service-Arb panel. Ask its admin for one.",
				}
				.into_response(),
			),
		Err(ConciergeError::Refused(_)) =>
			return with_clear(
				Page {
					status: StatusCode::BAD_REQUEST,
					title: "Sign-in failed",
					message: "This sign-in has expired or was already used. Start again.",
				}
				.into_response(),
			),
		Err(ConciergeError::Unavailable(why)) => {
			tracing::warn!(why, "concierge unavailable at the sign-in callback");
			return with_clear(
				Page {
					status: StatusCode::SERVICE_UNAVAILABLE,
					title: "Sign-in unavailable",
					message: "Signing in is unavailable right now. Try again in a minute.",
				}
				.into_response(),
			);
		}
		Err(e @ ConciergeError::Failed(_)) => {
			crate::report(&eyre::eyre!(e), "redeeming a sign-in code");
			return with_clear(
				Page {
					status: StatusCode::BAD_GATEWAY,
					title: "Sign-in failed",
					message: "Something went wrong. Try again.",
				}
				.into_response(),
			);
		}
	};
	if let Some(grant) = pre.grant {
		return with_clear(crate::oauth::granted(&s, &grant, issued).await);
	}
	// Signing in again from a browser with a session: the old one is closed, not orphaned.
	if let Some(old) = s.cookies.get(&headers, cookies::SESSION).and_then(SessionKey::of_cookie) {
		s.forget_me(&old);
		if let Err(e) = s.panel.close_session(&old).await {
			crate::report(&e, "closing the session a new sign-in replaces");
			return with_clear(
				Page {
					status: StatusCode::INTERNAL_SERVER_ERROR,
					title: "Sign-in failed",
					message: "Something went wrong on our side. Try again.",
				}
				.into_response(),
			);
		}
	}
	let opened = match s.panel.open_session(issued.user_id, &issued.tokens, now).await {
		Ok(o) => o,
		Err(e) => {
			crate::report(&e, "opening a session");
			return with_clear(
				Page {
					status: StatusCode::INTERNAL_SERVER_ERROR,
					title: "Sign-in failed",
					message: "Something went wrong on our side. Try again.",
				}
				.into_response(),
			);
		}
	};
	let csrf = match sa_panel::session::random_token() {
		Ok(t) => t,
		Err(e) => {
			crate::report(&e, "making a CSRF token");
			return with_clear(
				Page {
					status: StatusCode::INTERNAL_SERVER_ERROR,
					title: "Sign-in failed",
					message: "Something went wrong on our side. Try again.",
				}
				.into_response(),
			);
		}
	};
	let max_age = opened.expires_at.duration_since(now).as_secs();
	tracing::info!(user_id = %issued.user_id, "signed in");
	let landing = HeaderValue::from_str(pre.return_to.as_deref().unwrap_or("/")).expect("`/auth/login` let in visible ASCII only");
	let mut res = with_clear((StatusCode::SEE_OTHER, [(header::LOCATION, landing)]).into_response());
	let h = res.headers_mut();
	h.append(header::SET_COOKIE, s.cookies.set(cookies::SESSION, &opened.cookie, max_age, true));
	h.append(header::SET_COOKIE, s.cookies.set(cookies::CSRF, &csrf, max_age, false));
	if s.concierge.is_dev() {
		h.append(header::SET_COOKIE, s.cookies.set(cookies::DEV_SIGN_IN, "1", max_age, false));
	}
	res
}

/// `POST /auth/logout`: CSRF-checked; every session of the user is closed (a sign-out is
/// meant to end access, not one browser's copy of it) and the cookies cleared.
pub async fn logout(State(s): State<SignIn>, headers: HeaderMap) -> Response {
	if !s.cookies.csrf_ok(&headers) {
		return Refusal::new(Code::Csrf, "the page is out of date: reload it").into_response();
	}
	if let Some(key) = s.cookies.get(&headers, cookies::SESSION).and_then(SessionKey::of_cookie) {
		s.forget_me(&key);
		match s.panel.close_all_sessions(&key).await {
			Ok(Some(user_id)) => {
				s.forget_user(user_id);
				tracing::info!(%user_id, "signed out everywhere");
			}
			Ok(None) => {}
			Err(e) => {
				return Refusal::<Code>::from(e.wrap_err("closing a user's sessions")).into_response();
			}
		}
	}
	signed_out(&s, StatusCode::NO_CONTENT.into_response())
}

/// Sends the browser to concierge with a fresh pre-login: the callback then signs it in, or opens
/// the session `grant` gives an MCP client.
pub(crate) fn to_concierge(s: &SignIn, return_to: Option<&str>, dev_as: Option<&str>, grant: Option<&Grant>, prompt: Option<Prompt>) -> Response {
	let begun = match s.panel.begin_sign_in(return_to, dev_as, grant, Timestamp::now()) {
		Ok(b) => b,
		Err(e) => {
			crate::report(&e, "starting a sign-in");
			return private(
				Page {
					status: StatusCode::INTERNAL_SERVER_ERROR,
					title: "Sign-in failed",
					message: "Something went wrong on our side. Try again.",
				}
				.into_response(),
			);
		}
	};
	let location = if s.concierge.is_dev() {
		// No concierge to send the browser to: straight back to the callback, which then runs
		// as after concierge — pre-login, state redeemed once, the code exchanged, a session.
		format!("{}?code={DEV_CODE}&state={}", s.redirect_uri(), begun.state)
	} else {
		format!(
			"{}/api/auth/authorize?client_id={CLIENT_ID}&redirect_uri={}&response_type=code&state={}&code_challenge={}&code_challenge_method=S256{}",
			s.config.concierge_origin,
			encode(&s.redirect_uri()),
			begun.state,
			begun.challenge,
			match prompt {
				Some(Prompt::SelectAccount) => "&prompt=select_account",
				None => "",
			},
		)
	};
	let Ok(location) = HeaderValue::from_str(&location) else {
		return private(
			Page {
				status: StatusCode::INTERNAL_SERVER_ERROR,
				title: "Sign-in failed",
				message: "The sign-in is misconfigured.",
			}
			.into_response(),
		);
	};
	let prelogin = s.cookies.set(cookies::PRELOGIN, &begun.cookie, PRELOGIN_TTL.as_secs(), true);
	private((StatusCode::FOUND, [(header::LOCATION, location), (header::SET_COOKIE, prelogin)]).into_response())
}

/// A page on this origin with fixed text: nothing from the request is echoed.
pub(crate) struct Page {
	pub(crate) status: StatusCode,
	pub(crate) title: &'static str,
	pub(crate) message: &'static str,
}

impl IntoResponse for Page {
	fn into_response(self) -> Response {
		let Self { status, title, message } = self;
		let body = format!(
			"<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
			 <title>{title}</title></head><body><main><h1>{title}</h1><p>{message}</p><p><a href=\"/auth/login\">Sign in again</a></p></main></body></html>"
		);
		let mut res = (status, Html(body)).into_response();
		res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP));
		res
	}
}

/// [`Page`] with a message of ours that may quote Google: escaped, never the request's.
pub(crate) struct Said {
	pub(crate) title: &'static str,
	pub(crate) message: String,
}

impl IntoResponse for Said {
	fn into_response(self) -> Response {
		let message = self.message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;");
		let body = format!(
			"<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
			 <title>{t}</title></head><body><main><h1>{t}</h1><p>{message}</p><p><a href=\"/\">Back</a></p></main></body></html>",
			t = self.title
		);
		let mut res = (StatusCode::BAD_REQUEST, Html(body)).into_response();
		res.headers_mut().insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(PAGE_CSP));
		res
	}
}

/// The sign-in's answers carry a code or a token in their URLs: kept out of caches and out
/// of the `Referer` of whatever the page loads next.
pub(crate) fn private(mut res: Response) -> Response {
	let h = res.headers_mut();
	h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
	h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
	res
}

/// Percent-encodes a query value: everything but RFC 3986's unreserved characters.
pub(crate) fn encode(value: &str) -> String {
	let mut out = String::with_capacity(value.len());
	for b in value.bytes() {
		if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
			out.push(char::from(b));
		} else {
			out.push_str(&format!("%{b:02X}"));
		}
	}
	out
}

/// OIDC `prompt`, passed on to concierge.
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Prompt {
	/// Google's account chooser, past the evinvest.ltd session the browser may already have.
	SelectAccount,
}

/// Whether `/auth/callback` may send the browser to `path`: a path on this origin, and
/// nothing a browser could read as another (`//host`, `/\host`), nor anything but visible
/// ASCII (no control character, no space).
fn same_origin_path(path: &str) -> bool {
	path.len() <= MAX_RETURN_TO && path.starts_with('/') && !path[1..].starts_with('/') && !path.contains('\\') && path.bytes().all(|b| b.is_ascii_graphic())
}

fn dev_email(email: &str) -> bool {
	email.len() <= MAX_DEV_AS && email.contains('@') && email.bytes().all(|b| b.is_ascii_graphic())
}

pub(crate) fn signed_out(s: &SignIn, mut res: Response) -> Response {
	let h = res.headers_mut();
	h.append(header::SET_COOKIE, s.cookies.clear(cookies::SESSION));
	h.append(header::SET_COOKIE, s.cookies.clear(cookies::CSRF));
	res
}

/// Whether a route trusts the cached `GetMe`, or asks concierge afresh: for what must not
/// outlive a revoked permission by even [`ME_TTL`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Freshness {
	Cached,
	Fresh,
}

/// The gate of an `/api/v1` row: on Grafana's loopback, the viewer a token stands for; else a
/// grant's bearer, or the session's cookie with CSRF for anything that is not a read. The caller
/// goes into the request's extensions as a [`Caller`]; the row's permission is the gate's.
impl Vouch<Flags> for ApiState {
	type Refusal = Code;

	const REFUSALS: &'static [Code] = &[Code::Csrf, Code::Unavailable, Code::BadGateway, Code::Internal];

	async fn vouch(&self, gate: &Gate, parts: &mut Parts) -> Result<Principal, Refusal<Code>> {
		let caller = match parts.extensions.get::<Loopback>() {
			Some(loopback) => loopback.viewer(parts).ok_or_else(|| Refusal::new(Code::Unauthenticated, "a viewer's token, on a GET"))?,
			None => caller_of(&self.sign_in, &parts.method, &parts.headers, gate.flags.freshness).await?,
		};
		let user = sa_auth::User {
			sub: caller.user_id.to_string(),
			email: caller.email.clone(),
			email_verified: caller.email_verified,
			name: caller.preferred_name.clone(),
			permissions: caller.permissions.clone(),
			actor: caller.actor.clone(),
		};
		parts.extensions.insert(caller);
		Ok(Principal::User(user))
	}
}

/// Who calls: an MCP client's bearer, else the session's cookie, with CSRF on anything that is
/// not a read.
pub(crate) async fn caller_of(s: &SignIn, method: &Method, headers: &HeaderMap, freshness: Freshness) -> Result<Caller, Refusal<Code>> {
	if headers.contains_key(header::AUTHORIZATION) {
		// No cookie is read: nothing a cross-site request rides on.
		return bearer(s, headers, freshness).await;
	}
	if !matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS) && !s.cookies.csrf_ok(headers) {
		return Err(Refusal::new(Code::Csrf, "the page is out of date: reload it"));
	}
	check(s, headers, freshness).await
}

/// `GetMe`, asked once more when concierge did not answer (a timeout included): one slow
/// answer should not be a 503 for the user.
async fn get_me(s: &SignIn, access: &str) -> Result<Me, ConciergeError> {
	match s.concierge.me(access).await {
		Err(ConciergeError::Unavailable(why)) => {
			tracing::debug!(why, "GetMe unavailable; asking once more");
			s.concierge.me(access).await
		}
		answer => answer,
	}
}

/// A non-table route's answer to `refused`: with a 401, the cookies cleared.
pub(crate) fn refused(s: &SignIn, refused: Refusal<Code>) -> Response {
	let signed_out_too = refused.code() == Code::Unauthenticated;
	let res = refused.into_response();
	if signed_out_too { signed_out(s, res) } else { res }
}

/// The session behind the request's cookie and what concierge says its user may do: what
/// [`gate`] lets through, and what a live socket asks again while it is open.
pub(crate) async fn check(s: &SignIn, headers: &HeaderMap, freshness: Freshness) -> Result<Caller, Refusal<Code>> {
	let cookie = s.cookies.get(headers, cookies::SESSION).ok_or_else(|| Refusal::new(Code::Unauthenticated, "sign in"))?;
	caller(s, cookie, false, freshness).await
}

/// [`check`] for the session granted an MCP client, named by `Authorization: Bearer`.
pub(crate) async fn bearer(s: &SignIn, headers: &HeaderMap, freshness: Freshness) -> Result<Caller, Refusal<Code>> {
	let token = headers
		.get(header::AUTHORIZATION)
		.and_then(|v| v.to_str().ok())
		.and_then(|v| v.strip_prefix("Bearer "))
		.ok_or_else(|| Refusal::new(Code::Unauthenticated, "sign in"))?;
	caller(s, token, true, freshness).await
}

async fn caller(s: &SignIn, id: &str, bearer: bool, freshness: Freshness) -> Result<Caller, Refusal<Code>> {
	let session = match s.panel.session(id, Timestamp::now(), &s.concierge).await {
		// A browser's id is no bearer, nor a client's a cookie.
		Ok(session) if session.client.is_some() != bearer => return Err(Refusal::new(Code::Unauthenticated, "sign in")),
		Ok(session) => session,
		Err(SessionError::Missing | SessionError::Rejected) => return Err(Refusal::new(Code::Unauthenticated, "sign in")),
		Err(SessionError::Unavailable) => return Err(Refusal::new(Code::Unavailable, "sign-in is unavailable, try again")),
		Err(SessionError::Internal(e)) => return Err(e.wrap_err("reading a session").into()),
	};
	// The user is here: the bot may ask concierge with this session for another week.
	if let Err(e) = s.panel.touch_session(&session.key, Timestamp::now()).await {
		crate::report(&e, "marking a session used");
	}
	let cached = if freshness == Freshness::Cached { s.cached_me(&session.key) } else { None };
	let fresh = cached.is_none();
	let me = match cached {
		Some(me) => me,
		None => match get_me(s, &session.access).await {
			Ok(me) => {
				s.remember_me(session.key, me.clone());
				me
			}
			// The token revoked at concierge (signed out of evinvest.ltd, the user held or
			// disabled): the session is over here too.
			Err(ConciergeError::Refused(code)) => {
				tracing::info!(user_id = %session.user_id, ?code, "GetMe refused; session closed");
				s.forget_me(&session.key);
				if let Err(e) = s.panel.close_session(&session.key).await {
					crate::report(&e, "closing a refused session");
				}
				return Err(Refusal::new(Code::Unauthenticated, "sign in"));
			}
			Err(ConciergeError::Unavailable(why)) => {
				tracing::warn!(why, "concierge unavailable for GetMe");
				return Err(Refusal::new(Code::Unavailable, "sign-in is unavailable, try again"));
			}
			Err(e @ ConciergeError::Failed(_)) => {
				crate::report(&eyre::eyre!(e), "GetMe");
				return Err(Refusal::new(Code::BadGateway, "sign-in failed"));
			}
		},
	};
	if me.user_id != session.user_id {
		crate::report(&eyre::eyre!("GetMe answered another user than the session's"), "GetMe");
		return Err(Refusal::new(Code::BadGateway, "sign-in failed"));
	}
	if fresh {
		// Alerts go only on permissions concierge confirmed lately; every answer the gate gets is
		// such a confirmation.
		let display = if me.preferred_name.trim().is_empty() { &me.email } else { &me.preferred_name };
		if let Err(e) = s.panel.alerts_access_seen(me.user_id, &me.permissions, display, Timestamp::now()).await {
			crate::report(&e, "recording a user's access for alerts");
		}
		// The user's gmails and tokens hang on this row.
		let user = sa_panel::tokens::User {
			user_id: me.user_id,
			email: me.email.clone(),
			name: me.preferred_name.clone(),
		};
		if let Err(e) = s.panel.user_seen(&user, Timestamp::now()).await {
			return Err(e.wrap_err("recording a user seen").into());
		}
	}
	let own = Caller {
		user_id: me.user_id,
		email: me.email,
		email_verified: me.email_verified,
		preferred_name: me.preferred_name,
		permissions: me.permissions,
		dev_sign_in: s.concierge.is_dev(),
		actor: None,
		granted: session.client.as_ref().map(|c| c.approval),
	};
	let Some(subject) = session.acting_as else { return Ok(own) };
	assert!(!bearer, "PUT /me/acting refuses a bearer");
	if !own.permissions.may(Members::ActAs) {
		tracing::info!(actor = %own.user_id, %subject, "act_as revoked; acting stopped");
		s.panel.act(&session.key, None).await.map_err(|e| e.wrap_err("stopping acting"))?;
		return Ok(own);
	}
	let x = s.panel.user(subject).await?.ok_or_else(|| eyre::eyre!("acting_as references users"))?;
	Ok(Caller {
		user_id: x.user_id,
		email: x.email,
		email_verified: false, // this session never saw concierge vouch for the subject's email
		preferred_name: x.name,
		permissions: own.permissions.lent(),
		dev_sign_in: own.dev_sign_in,
		actor: Some(sa_auth::Actor {
			sub: own.user_id.to_string(),
			email: own.email,
		}),
		granted: None,
	})
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Acting {
	user_id: Uuid,
}

/// Signs this browser's session into `user_id`'s account, lending it the caller's permissions
/// but the bound ones, until `DELETE /me/acting`.
///
/// 400 for oneself or an MCP client's session, 404 for nobody the conductor knows, 409 while acting.
pub(crate) async fn start_acting(State(s): State<ApiState>, Extension(caller): Extension<Caller>, headers: HeaderMap, Body(b): Body<Acting>) -> ApiResult<NoContent> {
	if headers.contains_key(header::AUTHORIZATION) {
		return Err(Refusal::new(Code::BadRequest, "an MCP client's session acts as its own user only"));
	}
	if caller.actor.is_some() {
		return Err(Refusal::new(Code::Conflict, "already acting: stop first"));
	}
	if b.user_id == caller.user_id {
		return Err(Refusal::new(Code::BadRequest, "this is your own account"));
	}
	if s.sign_in.panel.user(b.user_id).await?.is_none() {
		return Err(Refusal::new(Code::NotFound, "no such user"));
	}
	let key = session_key(&s.sign_in, &headers);
	s.sign_in.panel.act(&key, Some(b.user_id)).await?;
	tracing::info!(actor = %caller.user_id, subject = %b.user_id, "acting as");
	Ok(NoContent)
}

/// Back to one's own account.
pub(crate) async fn stop_acting(State(s): State<ApiState>, Extension(caller): Extension<Caller>, headers: HeaderMap) -> ApiResult<NoContent> {
	if headers.contains_key(header::AUTHORIZATION) {
		return Ok(NoContent); // a bearer's session never acts
	}
	if let Some(actor) = &caller.actor {
		tracing::info!(actor = actor.sub, subject = %caller.user_id, "acting stopped");
	}
	s.sign_in.panel.act(&session_key(&s.sign_in, &headers), None).await?;
	Ok(NoContent)
}

fn session_key(s: &SignIn, headers: &HeaderMap) -> SessionKey {
	s.cookies
		.get(headers, cookies::SESSION)
		.and_then(SessionKey::of_cookie)
		.expect("the gate read this cookie's session")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn query_values_are_encoded() {
		assert_eq!(encode("https://sa.evinvest.ltd/auth/callback"), "https%3A%2F%2Fsa.evinvest.ltd%2Fauth%2Fcallback");
		assert_eq!(encode("aZ09-._~ &="), "aZ09-._~%20%26%3D");
	}
}
