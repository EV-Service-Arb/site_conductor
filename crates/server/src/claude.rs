//! Claude: the Messages API the assistant asks, on the panel's subscription or a person's own, and
//! a person's own connected as `claude setup-token` does it (OAuth PKCE on Claude Code's public
//! client). Its redirect is Anthropic's page, never ours: the code comes back pasted.
//!
//! ```text
//! POST /me/claude/connect ─ { url } ─▶ claude.com authorize ─▶ the person copies code#state
//! PUT /me/claude { code } ─ state checked ─▶ token exchanged ─▶ proved by a 1-token answer ─▶ sealed
//! ```

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, JsonSchema, Refusal};
use jiff::{SignedDuration, Timestamp};
use sa_panel::{
	claude::Claude,
	session::{pkce_challenge, random_token},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
	api::{ApiResult, Code},
	connections::CheckedDto,
	http::{ApiRoute, ApiState, USER},
	signin::Caller,
};

pub(crate) const MODEL: &str = "claude-opus-5-5";
/// A subscription's token is served only to a system prompt that opens so.
pub(crate) const SUBSCRIPTION: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const AUTHORIZE: &str = "https://claude.com/cai/oauth/authorize";
const REDIRECT: &str = "https://platform.claude.com/oauth/code/callback";
/// What `claude setup-token` asks for: a year.
const LIFETIME: i64 = 31_536_000;
/// How long a Connect waits for its code.
const PENDING: SignedDuration = SignedDuration::from_mins(10);

/// The Claude API, and the panel's own subscription (`CLAUDE_CODE_OAUTH_TOKEN`) if it has one.
#[derive(Clone)]
pub struct Model {
	http: reqwest::Client,
	/// `https://api.anthropic.com`.
	base: String,
	/// `https://platform.claude.com`: where a code is exchanged.
	oauth: String,
	owner: Option<Arc<Zeroizing<String>>>,
	/// Each person's Connect, until its code is pasted. A restart loses them: Connect again.
	pending: Arc<Mutex<HashMap<Uuid, Pending>>>,
}
impl Model {
	pub fn new(base: String, oauth: String, owner: Option<Zeroizing<String>>) -> Self {
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		let http = reqwest::Client::builder().timeout(Duration::from_secs(300)).build().expect("the TLS backend initialises at boot");
		Self {
			http,
			base,
			oauth,
			owner: owner.map(Arc::new),
			pending: Arc::default(),
		}
	}

	/// The panel's own subscription's token.
	pub fn owner(&self) -> Option<&Zeroizing<String>> {
		self.owner.as_deref()
	}

	/// What the model answered on `token`; `Err`, Claude refused the token, with what it said.
	pub(crate) async fn answer(&self, token: &str, body: &Value) -> ApiResult<Result<Value, String>> {
		let res = self
			.http
			.post(format!("{}/v1/messages", self.base))
			.bearer_auth(token)
			.header("anthropic-version", "2026-10-21")
			.header("anthropic-beta", "server-side-fallback-2026-07-01,oauth-2025-04-20")
			.json(body)
			.send()
			.await
			.map_err(|e| Refusal::new(Code::Unavailable, format!("the assistant's model did not answer: {e}")))?;
		let status = res.status();
		let body: Value = res
			.json()
			.await
			.map_err(|e| Refusal::new(Code::Unavailable, format!("the assistant's model answered no JSON: {e}")))?;
		match status {
			s if s.is_success() => Ok(Ok(body)),
			reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => Ok(Err(body["error"]["message"].as_str().map_or_else(|| body.to_string(), str::to_owned))),
			s => Err(Refusal::new(Code::Unavailable, format!("the assistant's model refused with {s}: {body}"))),
		}
	}

	/// The authorize URL of a new Connect for `user`, which replaces theirs.
	fn connect(&self, user: Uuid, now: Timestamp) -> eyre::Result<String> {
		let verifier = Zeroizing::new(random_token()?);
		let state = random_token()?;
		let mut url = Url::parse(AUTHORIZE).expect("a constant URL");
		url.query_pairs_mut()
			.append_pair("code", "true")
			.append_pair("client_id", CLIENT_ID)
			.append_pair("response_type", "code")
			.append_pair("redirect_uri", REDIRECT)
			.append_pair("scope", "user:inference")
			.append_pair("code_challenge", &pkce_challenge(&verifier))
			.append_pair("code_challenge_method", "S256")
			.append_pair("state", &state);
		let mut pending = self.pending.lock().expect("never poisoned: no panic while held");
		pending.retain(|_, p| now.duration_since(p.at) < PENDING);
		pending.insert(user, Pending { verifier, state, at: now });
		Ok(url.into())
	}

	/// The token `pasted` (`code#state`) is exchanged for, and when it expires; the user's Connect
	/// is spent once the state is theirs.
	async fn exchange(&self, user: Uuid, pasted: &str, now: Timestamp) -> ApiResult<(Zeroizing<String>, Timestamp)> {
		let (code, state) = pasted
			.trim()
			.split_once('#')
			.ok_or_else(|| Refusal::new(Code::BadRequest, "paste the whole code Claude shows: it has a # in it"))?;
		let verifier = {
			let mut pending = self.pending.lock().expect("never poisoned: no panic while held");
			match pending.get(&user) {
				Some(p) if p.state == state && now.duration_since(p.at) < PENDING => pending.remove(&user).expect("just read").verifier,
				Some(p) if p.state == state => return Err(Refusal::new(Code::BadRequest, "this Connect expired: click Connect again")),
				_ => return Err(Refusal::new(Code::BadRequest, "this code is not from your last Connect: click Connect again")),
			}
		};
		let res = self
			.http
			.post(format!("{}/v1/oauth/token", self.oauth))
			.json(&json!({
				"grant_type": "authorization_code",
				"code": code,
				"redirect_uri": REDIRECT,
				"client_id": CLIENT_ID,
				"code_verifier": *verifier,
				"state": state,
				"expires_in": LIFETIME,
			}))
			.send()
			.await
			.map_err(|e| Refusal::new(Code::Unavailable, format!("Claude's sign-in did not answer: {e}")))?;
		let status = res.status();
		let body: Value = res.json().await.map_err(|e| Refusal::new(Code::Unavailable, format!("Claude's sign-in answered no JSON: {e}")))?;
		if status.is_client_error() {
			let why = body["error_description"].as_str().or(body["error"].as_str()).map_or_else(|| body.to_string(), str::to_owned);
			return Err(Refusal::new(Code::BadRequest, format!("Claude refused the code: {why}")));
		}
		if !status.is_success() {
			return Err(Refusal::new(Code::Unavailable, format!("Claude's sign-in answered {status}: {body}")));
		}
		let token = body["access_token"].as_str().ok_or_else(|| eyre::eyre!("Claude's sign-in answered no access_token"))?;
		let lasts = body["expires_in"].as_i64().ok_or_else(|| eyre::eyre!("Claude's sign-in answered no expires_in"))?;
		Ok((Zeroizing::new(token.to_owned()), now + SignedDuration::from_secs(lasts)))
	}
}

/// A person's Claude: never its token.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Claude")]
pub struct ClaudeDto {
	/// RFC 3339.
	pub set_at: String,
	/// RFC 3339.
	pub expires_at: String,
	/// The last time Claude took it or refused it.
	pub checked: CheckedDto,
}

/// Where the person authorizes the panel on claude.ai.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "ClaudeConnect")]
pub struct ConnectDto {
	pub url: String,
}

struct Pending {
	verifier: Zeroizing<String>,
	state: String,
	at: Timestamp,
}

impl From<Claude> for ClaudeDto {
	fn from(c: Claude) -> Self {
		Self {
			set_at: c.set_at.to_string(),
			expires_at: c.expires_at.to_string(),
			checked: c.checked.into(),
		}
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pasted {
	/// What Claude's page shows once authorized: `code#state`.
	code: String,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/me/claude", USER, claude),
		ApiRoute::new(M::POST, "/me/claude/connect", USER, connect),
		ApiRoute::new(M::PUT, "/me/claude", USER, set_claude),
		ApiRoute::new(M::DELETE, "/me/claude", USER, remove_claude),
	]
}

fn model(state: &ApiState) -> ApiResult<&Model> {
	state.model.as_ref().ok_or_else(|| Refusal::new(Code::Unavailable, "the assistant is not connected"))
}

/// The caller's own Claude, which their assistant's answers run on; `null`, they run on the panel's.
async fn claude(State(state): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<Json<Option<ClaudeDto>>> {
	Ok(Json(state.sign_in.panel.claude(caller.user_id).await?.map(ClaudeDto::from)))
}

/// Starts connecting the caller's own Claude: open `url`, authorize, and paste the code it shows
/// into `PUT /me/claude` within 10 minutes.
async fn connect(State(state): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<Json<ConnectDto>> {
	let url = model(&state)?.connect(caller.user_id, Timestamp::now())?;
	Ok(Json(ConnectDto { url }))
}

/// Connects the caller's own Claude from the code its page showed, kept only once Claude answers
/// on it.
///
/// 400 the code is not their last Connect's, it expired, or Claude refused it or its token.
async fn set_claude(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Body(b): Body<Pasted>) -> ApiResult<Json<ClaudeDto>> {
	let model = model(&state)?;
	let now = Timestamp::now();
	let (token, expires_at) = model.exchange(caller.user_id, &b.code, now).await?;
	let probe = json!({
		"model": MODEL,
		"max_tokens": 1,
		"system": [{ "type": "text", "text": SUBSCRIPTION }],
		"messages": [{ "role": "user", "content": "hi" }],
	});
	if let Err(said) = model.answer(&token, &probe).await? {
		return Err(Refusal::new(Code::BadRequest, format!("Claude refuses the token it gave: {said}")));
	}
	let set = state.sign_in.panel.set_claude(caller.user_id, &token, expires_at, now).await?;
	tracing::info!(user_id = %caller.user_id, "a person's Claude connected");
	Ok(Json(set.into()))
}

/// Disconnects the caller's own Claude: their answers run on the panel's again. 404 none.
async fn remove_claude(State(state): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<NoContent> {
	state.sign_in.panel.remove_claude(caller.user_id).await??;
	tracing::info!(user_id = %caller.user_id, "a person's Claude disconnected");
	Ok(NoContent)
}
