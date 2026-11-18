//! `execute` and `poll`: an agent's code, in a session of sa_sandbox's, and `/api/run`, the one way back
//! from it (docs/ARCHITECTURE.md, Code mode). A session is its chat's and asker's, or its MCP
//! session's, and never another's: its globals hold what that caller's permissions fetched. A cell's
//! token is a handle to its caller's credentials while the cell lasts; a call the approval level holds
//! waits in the relay for the person, and the cell with it.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{
	Router,
	body::{Body, to_bytes},
	extract::{Request, State},
	http::{HeaderMap, StatusCode, header},
	response::{IntoResponse, Response},
	routing::any,
};
use eyre::WrapErr;
use jiff::Timestamp;
use sa_auth::{Assertion, Principal, Service, Signer};
use sa_core::agent::Approval;
use sa_sandbox::{CALLS, CHECK, Cell, Ended, Opened, Poll, REFUSED, Ran, Seen, Until, WAIT};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::sync::{Notify, oneshot};
use uuid::Uuid;

use crate::tools::{self, Call, Tools};

/// Where a cell's requests come back, in-cluster only.
pub const PREFIX: &str = "/api/run";
/// How long a held call waits for the person before it fails in the code.
pub const PAUSE: Duration = Duration::from_secs(10 * 60);

/// Whose a session is.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Key {
	/// The in-page assistant's: a chat, as one person asks in it (acting as another, as that pair).
	Chat { chat: Uuid, asker: Uuid, actor: Option<String> },
	/// An MCP session's.
	Mcp(String),
}

/// The runner, and the sessions it holds for whom.
#[derive(Clone)]
pub struct Sandbox {
	base: String,
	signer: Arc<Signer>,
	http: reqwest::Client,
	sessions: Arc<Mutex<HashMap<Key, Arc<Live>>>>,
	/// Each live cell's token, and whose session it runs in.
	tokens: Arc<Mutex<HashMap<String, Key>>>,
}

/// A session the runner holds.
struct Live {
	/// The runner's id of it.
	runner: String,
	state: Mutex<LiveState>,
	/// A call was held.
	paused: Notify,
}

struct LiveState {
	/// The caller's, as of their latest request about this session.
	credentials: HeaderMap,
	cell: Option<LiveCell>,
}

struct LiveCell {
	n: usize,
	token: String,
	approval: Approval,
	/// Whether a held call may wait for the person; else it fails at once, its proposal in the error.
	pause: bool,
	calls: usize,
	seen: Seen,
	ended: Option<Ended>,
	held: Option<Held>,
	/// What destroys or spends and ran on its own, since it was last told.
	done: Vec<Call>,
}

/// A call waiting for the person.
struct Held {
	call: Call,
	/// The in-page answer's action proposing it.
	action: Option<String>,
	decide: oneshot::Sender<bool>,
}

/// A cell as an agent is told it: what it printed since it was last told.
#[derive(Serialize)]
pub struct Told {
	pub cell: usize,
	pub state: CellState,
	pub stdout: String,
	pub stderr: String,
	pub truncated: bool,
	/// What destroys or spends and ran on its own since it was last told.
	pub ran_on_its_own: Vec<Call>,
}

impl Told {
	/// What was told, then `next`: one telling of both.
	pub(crate) fn then(self, next: Told) -> Told {
		Told {
			stdout: self.stdout + &next.stdout,
			stderr: self.stderr + &next.stderr,
			truncated: self.truncated || next.truncated,
			ran_on_its_own: [self.ran_on_its_own, next.ran_on_its_own].concat(),
			..next
		}
	}
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CellState {
	Running,
	/// Waiting for the person on this call.
	Paused(Call),
	Ended(Ended),
}

impl Sandbox {
	pub fn try_new(runner: &str, signer: Signer) -> eyre::Result<Self> {
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		Ok(Self {
			base: runner.trim_end_matches('/').to_owned(),
			signer: Arc::new(signer),
			// past the runner's own bounds: its answer comes first
			http: reqwest::Client::builder().timeout(CHECK + WAIT + Duration::from_secs(15)).build().wrap_err("the sandbox's client")?,
			sessions: Arc::default(),
			tokens: Arc::default(),
		})
	}

	async fn send(&self, method: reqwest::Method, path: &str, query: Option<&Poll>, body: Option<&Cell>) -> Result<(StatusCode, String), String> {
		let assertion = self.signer.sign(&Assertion {
			aud: Service::Sandbox,
			principal: Principal::Conductor,
			method: method.as_str().into(),
			path: path.into(),
			exp: Timestamp::now().as_second() + sa_auth::TTL,
		});
		let mut req = self.http.request(method, format!("{}{path}", self.base)).header(sa_auth::HEADER, assertion);
		if let Some(q) = query {
			req = req.query(q);
		}
		if let Some(b) = body {
			req = req.json(b);
		}
		let res = req.send().await.map_err(|e| format!("the sandbox did not answer: {e}"))?;
		let status = res.status();
		let text = res.text().await.map_err(|e| format!("the sandbox's answer broke off: {e}"))?;
		Ok((status, text))
	}

	/// What the runner answered of a session, or why it is no more.
	fn read(&self, key: &Key, live: &Live, (status, text): (StatusCode, String)) -> Result<Ran, String> {
		match status {
			StatusCode::OK => Ok(serde_json::from_str(&text).expect("the runner answers a Ran")),
			StatusCode::GONE => {
				self.forget(key, live);
				Err("the sandbox no longer holds this session (idle too long, or it ended): what earlier cells declared is gone. The next execute starts a new one.".into())
			}
			StatusCode::SERVICE_UNAVAILABLE => Err(format!("the sandbox is busy: {text}")),
			s => Err(format!("the sandbox answered {s}: {text}")),
		}
	}

	fn forget(&self, key: &Key, live: &Live) {
		let mut sessions = self.sessions.lock().expect("never held across a panic");
		if sessions.get(key).is_some_and(|l| l.runner == live.runner) {
			sessions.remove(key);
		}
		if let Some(c) = live.state.lock().expect("never held across a panic").cell.take() {
			self.tokens.lock().expect("never held across a panic").remove(&c.token);
		}
	}

	/// Runs `code` as a new cell of `key`'s session, opened if there is none, as the caller
	/// `credentials` names; its calls held as `approval` holds them, waiting for the person when
	/// `pause`. Answers once it ended, paused, or [`WAIT`] passed.
	pub(crate) async fn execute(&self, key: Key, credentials: HeaderMap, code: String, approval: Approval, pause: bool) -> Result<Told, String> {
		let live = self.sessions.lock().expect("never held across a panic").get(&key).cloned();
		let live = match live {
			Some(live) => live,
			None => {
				let (status, text) = self.send(reqwest::Method::POST, "/sessions", None, None).await?;
				let opened: Opened = match status {
					StatusCode::CREATED => serde_json::from_str(&text).expect("the runner answers an Opened"),
					StatusCode::SERVICE_UNAVAILABLE => return Err(format!("the sandbox is busy: {text}")),
					s => return Err(format!("the sandbox answered {s}: {text}")),
				};
				let live = Arc::new(Live {
					runner: opened.session,
					state: Mutex::new(LiveState { credentials: HeaderMap::new(), cell: None }),
					paused: Notify::new(),
				});
				self.sessions.lock().expect("never held across a panic").insert(key.clone(), live.clone());
				live
			}
		};
		let running = live.state.lock().expect("never held across a panic").cell.as_ref().is_some_and(|c| c.ended.is_none() && c.held.is_none());
		if running {
			// the record may be behind the runner: a cell that ended since is told now
			let empty = Told {
				cell: 0,
				state: CellState::Running,
				stdout: String::new(),
				stderr: String::new(),
				truncated: false,
				ran_on_its_own: Vec::new(),
			};
			self.wait(&key, &live, Until::Now, empty).await?;
		}
		let token = {
			let mut st = live.state.lock().expect("never held across a panic");
			st.credentials = credentials;
			match &st.cell {
				Some(c) if c.held.is_some() => return Err(format!("cell {} waits for the person on a call: it goes on once they decide", c.n)),
				Some(c) if c.ended.is_none() => return Err(format!("cell {} still runs: poll it to its end first", c.n)),
				_ => {}
			}
			if let Some(c) = st.cell.take() {
				self.tokens.lock().expect("never held across a panic").remove(&c.token);
			}
			let token = random();
			st.cell = Some(LiveCell {
				n: usize::MAX,
				token: token.clone(),
				approval,
				pause,
				calls: 0,
				seen: Seen::default(),
				ended: None,
				held: None,
				done: Vec::new(),
			});
			self.tokens.lock().expect("never held across a panic").insert(token.clone(), key.clone());
			token
		};
		let started = self.send(reqwest::Method::POST, &format!("/sessions/{}/cells", live.runner), None, Some(&Cell { code, token })).await;
		let ran = match started.and_then(|answer| self.read(&key, &live, answer)) {
			Ok(ran) => ran,
			Err(e) => {
				self.forget_cell(&live);
				return Err(e);
			}
		};
		let first = self.told(&live, ran);
		match first.state {
			CellState::Running => self.wait(&key, &live, Until::Ended, first).await,
			_ => Ok(first),
		}
	}

	/// A cell that never started: its token dies.
	fn forget_cell(&self, live: &Live) {
		if let Some(c) = live.state.lock().expect("never held across a panic").cell.take() {
			self.tokens.lock().expect("never held across a panic").remove(&c.token);
		}
	}

	/// Cell `n` of `key`'s session past what was told, once `until` holds, it paused, or [`WAIT`] passed.
	pub(crate) async fn poll(&self, key: Key, credentials: HeaderMap, n: usize, until: Until) -> Result<Told, String> {
		let live = self.sessions.lock().expect("never held across a panic").get(&key).cloned().ok_or("no session here holds a cell: execute starts one")?;
		{
			let mut st = live.state.lock().expect("never held across a panic");
			st.credentials = credentials;
			match &st.cell {
				Some(c) if c.n == n => {}
				Some(c) => return Err(format!("cell {n} is not the session's latest: cell {} is", c.n)),
				None => return Err("the session holds no cell: execute starts one".into()),
			}
		}
		let empty = Told {
			cell: n,
			state: CellState::Running,
			stdout: String::new(),
			stderr: String::new(),
			truncated: false,
			ran_on_its_own: Vec::new(),
		};
		self.wait(&key, &live, until, empty).await
	}

	/// The cell past what was told, joined onto `so_far`, once `until` holds, it paused, or the runner's
	/// wait passed.
	async fn wait(&self, key: &Key, live: &Live, until: Until, so_far: Told) -> Result<Told, String> {
		let paused = live.paused.notified();
		tokio::pin!(paused);
		paused.as_mut().enable();
		let (n, seen, held) = {
			let st = live.state.lock().expect("never held across a panic");
			let c = st.cell.as_ref().ok_or("the session's cell is gone")?;
			(c.n, c.seen, c.held.is_some())
		};
		let path = format!("/sessions/{}/cells/{n}", live.runner);
		let poll = |until| Poll {
			stdout: seen.stdout,
			stderr: seen.stderr,
			until,
		};
		let now = poll(Until::Now);
		let answer = if held {
			self.send(reqwest::Method::GET, &path, Some(&now), None).await
		} else {
			let waiting = poll(until);
			tokio::select! {
				answer = self.send(reqwest::Method::GET, &path, Some(&waiting), None) => answer,
				// a poll is only read: dropped, nothing is lost
				() = paused => self.send(reqwest::Method::GET, &path, Some(&now), None).await,
			}
		};
		Ok(so_far.then(self.told(live, self.read(key, live, answer?)?)))
	}

	/// What `ran` tells, the cell's record moved past it.
	fn told(&self, live: &Live, ran: Ran) -> Told {
		let mut st = live.state.lock().expect("never held across a panic");
		let c = st.cell.as_mut().expect("a cell is told only while it is the session's");
		c.n = ran.cell;
		c.seen = ran.seen;
		c.ended = ran.ended;
		let state = match (ran.ended, &c.held) {
			(Some(e), _) => CellState::Ended(e),
			(None, Some(h)) => CellState::Paused(h.call.clone()),
			(None, None) => CellState::Running,
		};
		if ran.ended.is_some() {
			self.tokens.lock().expect("never held across a panic").remove(&c.token);
		}
		Told {
			cell: ran.cell,
			state,
			stdout: ran.stdout,
			stderr: ran.stderr,
			truncated: ran.truncated,
			ran_on_its_own: std::mem::take(&mut c.done),
		}
	}

	/// Names the in-page action that proposes the call `key`'s cell waits on.
	pub(crate) fn proposed_as(&self, key: &Key, action: String) {
		let live = self.sessions.lock().expect("never held across a panic").get(key).cloned();
		let Some(live) = live else { return };
		if let Some(h) = live.state.lock().expect("never held across a panic").cell.as_mut().and_then(|c| c.held.as_mut()) {
			h.action = Some(action);
		}
	}

	/// The person's word on the call `key`'s cell waits on (the one `action` proposes, when named):
	/// the cell that waited on it, and its approval level.
	pub(crate) fn decide(&self, key: &Key, action: Option<&str>, approve: bool) -> Option<(usize, Approval)> {
		let live = self.sessions.lock().expect("never held across a panic").get(key).cloned()?;
		let mut st = live.state.lock().expect("never held across a panic");
		let c = st.cell.as_mut()?;
		if action.is_some() && c.held.as_ref()?.action.as_deref() != action {
			return None;
		}
		let held = c.held.take()?;
		if held.decide.send(approve).is_err() {
			return None; // its wait passed as the person decided
		}
		Some((c.n, c.approval))
	}

	/// Lets go of `key`'s session.
	pub(crate) async fn close(&self, key: &Key) {
		let live = self.sessions.lock().expect("never held across a panic").get(key).cloned();
		let Some(live) = live else { return };
		self.forget(key, &live);
		match self.send(reqwest::Method::DELETE, &format!("/sessions/{}", live.runner), None, None).await {
			Ok((StatusCode::NO_CONTENT | StatusCode::GONE, _)) => {}
			Ok((s, text)) => tracing::warn!(%s, text, "closing a sandbox session"),
			Err(e) => tracing::warn!(e, "closing a sandbox session"),
		}
	}

	/// `/api/run/*`: a cell's requests, as its caller's own once the gate lets them by.
	pub(crate) fn routes(tools: Tools) -> Router {
		Router::new().route(&format!("{PREFIX}/{{*path}}"), any(relay)).with_state(tools)
	}
}

/// 256 random bits, as hex.
fn random() -> String {
	let mut bytes = [0; 32];
	getrandom::fill(&mut bytes).expect("the OS has randomness");
	hex::encode(bytes)
}

/// The gate's own refusal ([`REFUSED`]): the code reads it as the hop's `refused`, and decides what follows.
fn refused(why: String) -> Response {
	(StatusCode::FORBIDDEN, [(REFUSED, "1")], axum::Json(serde_json::json!({ "code": "refused", "error": why }))).into_response()
}

async fn relay(State(tools): State<Tools>, req: Request) -> Response {
	let sandbox = tools.sandbox().expect("mounted with the sandbox");
	let token = req
		.headers()
		.get(header::AUTHORIZATION)
		.and_then(|v| v.to_str().ok())
		.and_then(|v| v.strip_prefix("Bearer "))
		.map(str::to_owned);
	let path = req.uri().path().strip_prefix(PREFIX).expect("mounted under PREFIX").to_owned();
	let method = req.method().clone();
	let gone = || refused("no cell holds this token: it ended".into());
	let Some(key) = token.as_deref().and_then(|t| sandbox.tokens.lock().expect("never held across a panic").get(t).cloned()) else {
		return gone();
	};
	let Some(live) = sandbox.sessions.lock().expect("never held across a panic").get(&key).cloned() else {
		return gone();
	};
	let (credentials, approval, pause) = {
		let mut st = live.state.lock().expect("never held across a panic");
		let credentials = st.credentials.clone();
		let Some(c) = st.cell.as_mut().filter(|c| Some(&c.token) == token.as_ref()) else {
			return gone();
		};
		c.calls += 1;
		if c.calls > CALLS {
			return refused(format!("a cell calls at most {CALLS} operations"));
		}
		(credentials, c.approval, c.pause)
	};
	let Some((op, params)) = tools.catalog().operation(&method, &path) else {
		return refused(format!("{method} {path} is no operation of the catalog's"));
	};
	let id = op.operation_id.clone().expect("every operation has an id");
	let query: Map<String, Value> = url::form_urlencoded::parse(req.uri().query().unwrap_or_default().as_bytes())
		.map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
		.collect();
	let hints = tools::hints(method.as_str().to_lowercase().as_str(), &op);
	let (parts, body) = req.into_parts();
	let mut body = body;
	if hints.held_by(Approval::Ask) {
		let json = parts.headers.get(header::CONTENT_TYPE).is_some_and(|t| t.as_bytes().starts_with(b"application/json"));
		let bytes = match to_bytes(body, tools::ANSWER).await {
			Ok(b) => b,
			Err(e) => return refused(format!("{id}'s body: {e}")),
		};
		let value = match &bytes {
			b if b.is_empty() => None,
			b if json => match serde_json::from_slice(b) {
				Ok(v) => Some(v),
				Err(e) => return refused(format!("{id}'s body is no JSON: {e}")),
			},
			_ => return refused(format!("{id} destroys or spends, and its body is no JSON to record: call it with `call`")),
		};
		let call = Call {
			operation: id.clone(),
			path: Some(params),
			query: Some(query),
			body: value,
		};
		if hints.held_by(approval) {
			if !pause {
				let proposal = tools.proposal(&call).map_or_else(|| id.clone(), |p| p.to_string());
				return refused(format!("not run: the person agrees to it first, and this client cannot ask them from within the code. `call` it with \"confirmed\": true once they agree: {proposal}"));
			}
			let (decide, decided) = oneshot::channel();
			{
				let mut st = live.state.lock().expect("never held across a panic");
				let Some(c) = st.cell.as_mut().filter(|c| Some(&c.token) == token.as_ref()) else {
					return gone();
				};
				if c.held.is_some() {
					return refused("another call of this cell waits for the person: one at a time".into());
				}
				c.held = Some(Held { call: call.clone(), action: None, decide });
			}
			live.paused.notify_waiters();
			let decided = tokio::time::timeout(PAUSE, decided).await;
			let mut st = live.state.lock().expect("never held across a panic");
			if let Some(c) = st.cell.as_mut().filter(|c| Some(&c.token) == token.as_ref()) {
				c.held = None;
			}
			match decided {
				Ok(Ok(true)) => {}
				Ok(Ok(false)) => return refused(format!("the person declined {id}: it did not run")),
				Ok(Err(_)) => return gone(),
				Err(_) => return refused(format!("no one agreed to {id} within {} s: it did not run", PAUSE.as_secs())),
			}
		}
		else if let Some(c) = live.state.lock().expect("never held across a panic").cell.as_mut().filter(|c| Some(&c.token) == token.as_ref()) {
			c.done.push(call);
		}
		body = Body::from(bytes);
	}
	let uri = parts.uri.query().map_or_else(|| path.clone(), |q| format!("{path}?{q}"));
	let mut inner = Request::builder().method(method).uri(uri);
	for name in [header::CONTENT_TYPE, header::ACCEPT] {
		if let Some(v) = parts.headers.get(&name) {
			inner = inner.header(name, v);
		}
	}
	let mut res = tools.send(&credentials, inner, body).await;
	// a rotated session's cookie is the caller's alone
	res.headers_mut().remove(header::SET_COOKIE);
	res
}
