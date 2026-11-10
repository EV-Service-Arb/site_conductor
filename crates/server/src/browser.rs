//! A gmail's antidetect browser (docs/ARCHITECTURE.md, Browsers), its owner's only: set up with
//! its proxy, started on sa_browsers' runner, and driven through the conductor alone.
//!
//! ```text
//! PUT    /api/v1/gmails/{id}/browser {provider, device, proxy}  → Connections; 400 refused, nothing
//!                                     stored; 409 provider or device another: DELETE first
//! POST   /api/v1/gmails/{id}/browser/test   → Connections, the exit checked again
//! DELETE /api/v1/gmails/{id}/browser        → 204: stopped, the provider's profile and the session gone
//! POST   /api/v1/gmails/{id}/browser/start {gmb?}  → 204 once up: at that GBP's point, else nowhere
//! POST   /api/v1/gmails/{id}/browser/stop   → 204
//! PUT    /api/v1/gmails/{id}/browser/film   an mp4 (≤ 512 MiB) → 204: its next launch's back camera,
//!                                           the sound its mic; 400 no sound, 409 while it runs
//! DELETE /api/v1/gmails/{id}/browser/film   → 204
//! GET    /api/v1/gmails/{id}/browser        WebSocket, the viewer: outside the /api/v1 gate, as
//!                                           /live; one per gmail, a new one closes the old (4409)
//! ```
//! Unset GOLOGIN_TOKEN and PANEL_BROWSERS_URL: 503, "browsers not connected".

use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use axum::{
	Extension, Json, Router,
	extract::{
		Path, State,
		rejection::JsonRejection,
		ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
	},
	http::{HeaderMap, Method, StatusCode, header},
	response::{IntoResponse, Response},
	routing::get,
};
use eyre::WrapErr;
use futures::{SinkExt, StreamExt};
use jiff::Timestamp;
use sa_auth::{Assertion, Principal, Service, Signer};
use sa_browsers::{
	Launch, Profiles,
	view::{In as ViewerIn, Out as ViewerOut, Tab as TabDto, TouchPhase, web_url},
};
use sa_panel::{
	browsers::{Browsers, NewBrowser, Provider, Run},
	live::{Ended, Signal},
	session::SessionKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{broadcast::error::RecvError, oneshot};
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use tracing::Instrument;
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::{
	api::{ApiError, ApiResult, body},
	connections::{self, CheckedDto, ConnectionsDto},
	cookies,
	http::{ApiRoute, ApiState},
	live,
	signin::{self, Caller, Denied, Freshness, SignIn},
};

/// The viewer was replaced by another of the gmail's.
pub const CLOSE_REPLACED: u16 = 4409;
/// Nothing runs for the gmail: start it first.
pub const CLOSE_NOT_RUNNING: u16 = 4404;
/// The browser stopped, or its devtools went.
pub const CLOSE_STOPPED: u16 = 4410;

/// A launch waits on the browser's devtools (60 s at the runner) and the provider's calls.
const LAUNCH_WITHIN: Duration = Duration::from_secs(90);

const NOT_CONNECTED: &str = "browsers not connected";

/// How often the viewer's session is asked again.
const RECHECK: Duration = Duration::from_secs(60);

/// How long one frame may take to go out before the viewer is dropped.
const SEND_WITHIN: Duration = Duration::from_secs(10);
/// Under the gate, outside the API's bounds (`http::app`).
pub(crate) const FILM: &str = "/api/v1/gmails/{id}/browser/film";
pub(crate) const START: &str = "/api/v1/gmails/{id}/browser/start";

/// A film at most.
pub(crate) const FILM_MAX: usize = 512 << 20;
type Devtools = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

type Held = (u64, oneshot::Sender<()>);

/// Where a gmail's proxy comes out, at its last check.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Exit")]
pub struct ExitDto {
	pub ip: String,
	/// ISO 3166-1 alpha-2.
	pub country: String,
	pub city: String,
	pub zone: String,
	/// A mobile carrier's.
	pub mobile: bool,
}

/// A gmail's browser, as its owner sees it: never the proxy's credentials.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Browser")]
pub struct BrowserDto {
	#[ts(type = "Provider")]
	pub provider: String,
	#[ts(type = "Device")]
	pub device: sa_browsers::Device,
	/// The proxy's host:port.
	pub endpoint: String,
	pub exit: ExitDto,
	pub set_at: String,
	/// `error` null: through the proxy, out in the country of every GBP of the gmail.
	pub checked: CheckedDto,
	/// What it runs for now: `gmb` null, the gmail alone; null, it does not run.
	#[ts(type = "{ gmb: number | null } | null")]
	pub running: Option<RunningDto>,
	/// Its next launch plays the film uploaded as its back camera.
	pub film: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct RunningDto {
	pub gmb: Option<i64>,
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "SetBrowser")]
pub struct SetBrowser {
	#[ts(type = "Provider")]
	pub provider: String,
	#[ts(type = "Device")]
	pub device: sa_browsers::Device,
	/// `socks5://` or `http://`, `user:pass@host:port`.
	pub proxy: String,
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(rename = "StartBrowser")]
pub struct StartBrowser {
	/// At this GBP's point; none, the position blocked.
	#[ts(type = "number | null")]
	pub gmb: Option<i64>,
}

/// The browsers connected: the provider and the runner, with GOLOGIN_TOKEN and PANEL_BROWSERS_URL.
#[derive(Clone)]
pub struct Connected {
	browsers: Arc<Browsers>,
	runner: Runner,
	viewers: Viewers,
}

impl Connected {
	pub fn try_new(browsers: Browsers, runner: &str, signer: Signer) -> eyre::Result<Self> {
		Ok(Self {
			browsers: Arc::new(browsers),
			runner: Runner {
				base: runner.trim_end_matches('/').to_owned(),
				signer: Arc::new(signer),
				http: reqwest::Client::builder().timeout(LAUNCH_WITHIN).build().wrap_err("the runner's client")?,
			},
			viewers: Viewers::default(),
		})
	}

	/// What the gmail's browser runs for now, and whether its next launch plays a film.
	pub(crate) async fn state(&self, gmail: i64) -> ApiResult<(Option<RunningDto>, bool)> {
		let (status, body) = self.runner.call(Method::GET, "/profiles", None).await?;
		if status != StatusCode::OK {
			return Err(ApiError::Unavailable(format!("the browsers' runner answered {status}: {body}")));
		}
		let profiles: Profiles = serde_json::from_str(&body).map_err(|e| eyre::eyre!("the runner's profiles: {e}"))?;
		let running = match profiles.running.iter().find(|r| r.gmail == gmail) {
			Some(r) => Some(RunningDto {
				gmb: r.run.parse::<Run>().map_err(|e| eyre::eyre!("the runner's run: {e}"))?.0,
			}),
			None => None,
		};
		Ok((running, profiles.films.contains(&gmail)))
	}
}

/// sa_browsers' runner, told by the conductor alone.
#[derive(Clone)]
struct Runner {
	base: String,
	signer: Arc<Signer>,
	http: reqwest::Client,
}

impl Runner {
	fn assertion(&self, method: &Method, path: &str) -> String {
		self.signer.sign(&Assertion {
			aud: Service::Browsers,
			principal: Principal::Conductor,
			method: method.to_string(),
			path: path.to_owned(),
			exp: Timestamp::now().as_second() + sa_auth::TTL,
		})
	}

	/// Its status and body; unreachable is a 503.
	async fn call(&self, method: Method, path: &str, body: Option<&Launch>) -> ApiResult<(StatusCode, String)> {
		let req = self
			.http
			.request(method.clone(), format!("{}{path}", self.base))
			.header(sa_auth::HEADER, self.assertion(&method, path));
		let req = match body {
			Some(b) => req.json(b),
			None => req,
		};
		let res = req.send().await.map_err(|e| ApiError::Unavailable(format!("the browsers' runner did not answer: {e}")))?;
		let status = res.status();
		let text = res.text().await.map_err(|e| ApiError::Unavailable(format!("the browsers' runner's answer broke off: {e}")))?;
		Ok((status, text))
	}

	/// 204 expected, anything else said back as the runner put it.
	async fn expect_done(&self, method: Method, path: &str, body: Option<&Launch>) -> ApiResult<()> {
		let (status, text) = self.call(method, path, body).await?;
		match status {
			StatusCode::NO_CONTENT => Ok(()),
			StatusCode::CONFLICT => Err(ApiError::Conflict(text)),
			StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => Err(ApiError::Unavailable(text)),
			s => Err(eyre::eyre!("the browsers' runner answered {path} with {s}: {text}").into()),
		}
	}

	async fn devtools(&self, gmail: i64) -> eyre::Result<Option<Devtools>> {
		let path = format!("/profiles/{gmail}/cdp");
		let ws = format!("{}{path}", self.base.replacen("http", "ws", 1));
		let mut req = ws.into_client_request().wrap_err("the runner's devtools URL")?;
		req.headers_mut()
			.insert(sa_auth::HEADER, self.assertion(&Method::GET, &path).parse().expect("a JWS is base64url and dots"));
		match tokio_tungstenite::connect_async(req).await {
			Ok((ws, _)) => Ok(Some(ws)),
			Err(tungstenite::Error::Http(res)) if res.status() == StatusCode::NOT_FOUND => Ok(None),
			Err(e) => Err(eyre::Report::new(e).wrap_err("the runner's devtools socket")),
		}
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	vec![
		ApiRoute::new(M::PUT, "/gmails/{id}/browser", None, Cached, set),
		ApiRoute::new(M::POST, "/gmails/{id}/browser/test", None, Cached, test),
		ApiRoute::new(M::DELETE, "/gmails/{id}/browser", None, Cached, remove),
		ApiRoute::new(M::POST, "/gmails/{id}/browser/stop", None, Cached, stop),
		ApiRoute::new(M::DELETE, "/gmails/{id}/browser/film", None, Cached, remove_film),
	]
}

/// The owner's screens read the gmail's browser again: set, checked, started, stopped, its film.
fn changed(state: &ApiState, owner: uuid::Uuid, gmail: i64) {
	state.panel.bus().changed(sa_panel::live::Change {
		topic: "browsers".into(),
		brand: None,
		id: Some(gmail.to_string()),
		at: Timestamp::now(),
		audience: sa_panel::live::Audience::User(owner),
	});
}

fn connected(state: &ApiState) -> ApiResult<&Connected> {
	state.browsers.as_ref().ok_or_else(|| ApiError::Unavailable(NOT_CONNECTED.to_owned()))
}

fn id(raw: &str) -> ApiResult<i64> {
	raw.parse().map_err(|_| ApiError::NotFound)
}

async fn set(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetBrowser>, JsonRejection>) -> ApiResult<Json<ConnectionsDto>> {
	let gmail = id(&raw)?;
	let c = connected(&state)?;
	let b = body(b)?;
	let provider: Provider = b.provider.parse().map_err(|_| ApiError::BadRequest(format!("no provider {:?}", b.provider)))?;
	let new = NewBrowser {
		provider,
		device: b.device,
		proxy: Zeroizing::new(b.proxy),
	};
	state.panel.set_browser(&c.browsers, caller.user_id, gmail, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's browser set");
	changed(&state, caller.user_id, gmail);
	connections::answer(&state, caller.user_id, gmail).await.map(Json)
}

async fn test(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<Json<ConnectionsDto>> {
	let gmail = id(&raw)?;
	state.panel.check_browser(&connected(&state)?.browsers, caller.user_id, gmail, Timestamp::now()).await??;
	changed(&state, caller.user_id, gmail);
	connections::answer(&state, caller.user_id, gmail).await.map(Json)
}

async fn remove(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	let c = connected(&state)?;
	state.panel.browser_of(caller.user_id, gmail).await??;
	c.runner.expect_done(Method::DELETE, &format!("/profiles/{gmail}"), None).await?;
	state.panel.delete_browser(&c.browsers, caller.user_id, gmail).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's browser removed");
	changed(&state, caller.user_id, gmail);
	Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn start(
	State(state): State<ApiState>,
	Extension(caller): Extension<Caller>,
	Path(raw): Path<String>,
	b: Result<Json<StartBrowser>, JsonRejection>,
) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	let c = connected(&state)?;
	let launch = state.panel.launch(&c.browsers, caller.user_id, gmail, body(b)?.gmb, Timestamp::now()).await??;
	c.runner
		.expect_done(Method::POST, &format!("/profiles/{gmail}/launch"), Some(&launch))
		.await
		.map_err(|e| match e {
			ApiError::Conflict(_) => ApiError::Conflict("it runs for another GBP, or for the gmail alone: stop it first".to_owned()),
			e => e,
		})?;
	changed(&state, caller.user_id, gmail);
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, run = launch.run, "gmail's browser up");
	Ok(StatusCode::NO_CONTENT)
}

async fn stop(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	let c = connected(&state)?;
	state.panel.browser_of(caller.user_id, gmail).await??;
	c.runner.expect_done(Method::POST, &format!("/profiles/{gmail}/stop"), None).await?;
	changed(&state, caller.user_id, gmail);
	Ok(StatusCode::NO_CONTENT)
}

async fn remove_film(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	let c = connected(&state)?;
	state.panel.browser_of(caller.user_id, gmail).await??;
	c.runner.expect_done(Method::DELETE, &format!("/profiles/{gmail}/film"), None).await?;
	changed(&state, caller.user_id, gmail);
	Ok(StatusCode::NO_CONTENT)
}

/// `PUT` an mp4, streamed to the runner: the camera of the gmail's next launch, its sound the mic.
pub(crate) async fn set_film(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, film: axum::body::Body) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	let c = connected(&state)?;
	state.panel.browser_of(caller.user_id, gmail).await??;
	let path = format!("/profiles/{gmail}/film");
	let sent = c
		.runner
		.http
		.put(format!("{}{path}", c.runner.base))
		.header(sa_auth::HEADER, c.runner.assertion(&Method::PUT, &path))
		.timeout(Duration::from_secs(600))
		.body(reqwest::Body::wrap_stream(film.into_data_stream()))
		.send()
		.await
		.map_err(|e| ApiError::Unavailable(format!("the browsers' runner did not take the film: {e}")))?;
	let status = sent.status();
	let text = sent.text().await.map_err(|e| ApiError::Unavailable(format!("the browsers' runner's answer broke off: {e}")))?;
	match status {
		StatusCode::NO_CONTENT => {
			tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's film set");
			changed(&state, caller.user_id, gmail);
			Ok(StatusCode::NO_CONTENT)
		}
		StatusCode::BAD_REQUEST => Err(ApiError::BadRequest(text)),
		StatusCode::CONFLICT => Err(ApiError::Conflict(text)),
		StatusCode::PAYLOAD_TOO_LARGE => Err(ApiError::TooLarge(format!("a film is {} MiB at most", FILM_MAX >> 20))),
		s => Err(eyre::eyre!("the browsers' runner answered a film with {s}: {text}").into()),
	}
}

/// The viewer's route: outside the /api/v1 gate, as /live is.
pub(crate) fn viewer_routes(sign_in: SignIn, browsers: Option<Connected>) -> Router {
	Router::new().route("/api/v1/gmails/{id}/browser", get(upgrade)).with_state(ViewerState { sign_in, browsers })
}

#[derive(Clone)]
struct ViewerState {
	sign_in: SignIn,
	browsers: Option<Connected>,
}

/// One viewer per gmail: a new one closes the one before.
#[derive(Clone, Default)]
struct Viewers {
	/// Per gmail, its viewer's turn and what tells it to go.
	held: Arc<Mutex<HashMap<i64, Held>>>,
	next: Arc<AtomicU64>,
}
impl Viewers {
	/// The gmail's viewer from now: told to close when another takes over.
	fn take(&self, gmail: i64) -> (Viewing, oneshot::Receiver<()>) {
		let turn = self.next.fetch_add(1, Ordering::Relaxed);
		let (tx, rx) = oneshot::channel();
		if let Some((_, before)) = self.held.lock().expect("never held across a panic").insert(gmail, (turn, tx)) {
			// the one before may be gone already
			let _told = before.send(());
		}
		(Viewing { viewers: self.clone(), gmail, turn }, rx)
	}
}

struct Viewing {
	viewers: Viewers,
	gmail: i64,
	turn: u64,
}

impl Drop for Viewing {
	fn drop(&mut self) {
		let mut held = self.viewers.held.lock().expect("never held across a panic");
		if held.get(&self.gmail).is_some_and(|(turn, _)| *turn == self.turn) {
			held.remove(&self.gmail);
		}
	}
}

async fn upgrade(State(s): State<ViewerState>, Path(raw): Path<String>, headers: HeaderMap, ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>) -> Response {
	if !live::same_origin(&headers, s.sign_in.panel_origin()) {
		tracing::info!(origin = ?headers.get(header::ORIGIN), "browser: a handshake from another origin refused");
		return ApiError::Forbidden.into_response();
	}
	let caller = match signin::check(&s.sign_in, &headers, Freshness::Cached).await {
		Ok(c) => c,
		Err(denied) => return denied.into_response(&s.sign_in),
	};
	let Some(c) = s.browsers.clone() else {
		return ApiError::Unavailable(NOT_CONNECTED.to_owned()).into_response();
	};
	let Ok(gmail) = raw.parse::<i64>() else {
		return ApiError::NotFound.into_response();
	};
	match s.sign_in.panel.browser_of(caller.user_id, gmail).await {
		Ok(Ok(_)) => {}
		Ok(Err(r)) => return ApiError::from(r).into_response(),
		Err(e) => return ApiError::Internal(e).into_response(),
	}
	let ws = match ws {
		Ok(ws) => ws,
		Err(rejection) => return rejection.into_response(),
	};
	let Some(key) = s.sign_in.cookies().get(&headers, cookies::SESSION).and_then(SessionKey::of_cookie) else {
		return Denied::Unauthenticated.into_response(&s.sign_in);
	};
	let mut cookie = HeaderMap::new();
	for v in headers.get_all(header::COOKIE) {
		cookie.append(header::COOKIE, v.clone());
	}
	let bus = s.sign_in.panel.bus().subscribe();
	let span = tracing::info_span!("browser", user_id = %caller.user_id, gmail_id = gmail);
	ws.max_message_size(4096).max_frame_size(4096).max_write_buffer_size(4 << 20).on_upgrade(move |socket| {
		async move {
			let (viewing, replaced) = c.viewers.take(gmail);
			let (code, reason) = match c.runner.devtools(gmail).await {
				Ok(Some(devtools)) =>
					View {
						socket,
						devtools,
						sign_in: s.sign_in,
						cookie,
						key,
						caller,
						bus,
						next: 0,
						pending: HashMap::new(),
						tabs: Vec::new(),
						active: None,
						last: None,
					}
					.run(replaced)
					.await,
				Ok(None) => close(socket, CLOSE_NOT_RUNNING, "not running: start it").await,
				Err(e) => {
					crate::report(&e, "browser");
					close(socket, CLOSE_STOPPED, "its devtools did not answer").await
				}
			};
			drop(viewing);
			tracing::debug!(code, reason, "browser: viewer closed");
		}
		.instrument(span)
	})
}

async fn close(mut socket: WebSocket, code: u16, reason: &'static str) -> (u16, &'static str) {
	// the last word: whether it reached the viewer changes nothing here
	let _last = socket.send(Message::Close(Some(CloseFrame { code, reason: reason.into() }))).await;
	(code, reason)
}

/// What a sent devtools command's answer is for.
enum Asked {
	/// Nothing to do with it but report a refusal.
	Done(&'static str),
	Attach(String),
	Created,
	History(i32),
}

/// One viewer of one browser: the devtools' events into frames, the viewer's into input.
struct View {
	socket: WebSocket,
	devtools: Devtools,
	sign_in: SignIn,
	cookie: HeaderMap,
	key: SessionKey,
	caller: Caller,
	bus: tokio::sync::broadcast::Receiver<sa_panel::live::Signal>,
	next: u64,
	pending: HashMap<u64, Asked>,
	/// Pages, in the order they opened.
	tabs: Vec<TabDto>,
	/// The tab shown: its target and devtools session.
	active: Option<(String, String)>,
	/// The last frame's page size, CSS px: where the viewer's points are.
	last: Option<(f64, f64)>,
}

impl View {
	async fn run(mut self, mut replaced: oneshot::Receiver<()>) -> (u16, &'static str) {
		if self
			.cdp(None, "Target.setDiscoverTargets", json!({ "discover": true }), Asked::Done("discovering the tabs"))
			.await
			.is_err()
		{
			return close(self.socket, CLOSE_STOPPED, "its devtools went").await;
		}
		let mut recheck = tokio::time::interval_at(tokio::time::Instant::now() + RECHECK, RECHECK);
		//LOOP: until either socket ends or the viewer is to go
		loop {
			let step: Result<(), (u16, &'static str)> = tokio::select! {
				m = self.socket.recv() => match m {
					Some(Ok(Message::Text(t))) => self.heard(&t).await,
					Some(Ok(Message::Ping(_) | Message::Pong(_))) => Ok(()),
					Some(Ok(Message::Binary(_))) => Err((1003, "text frames only")),
					Some(Ok(Message::Close(_)) | Err(_)) | None => return (1000, "viewer left"),
				},
				m = self.devtools.next() => match m {
					Some(Ok(tungstenite::Message::Text(t))) => self.told(&t).await,
					Some(Ok(tungstenite::Message::Close(_)) | Err(_)) | None => Err((CLOSE_STOPPED, "the browser stopped")),
					Some(Ok(_)) => Ok(()),
				},
				_ = &mut replaced => Err((CLOSE_REPLACED, "another viewer took over")),
				signal = self.bus.recv() => match signal {
					Ok(Signal::SessionsEnded(Ended::User(u))) if u == self.caller.user_id => Err((live::CLOSE_SESSION_ENDED, "session ended")),
					Ok(Signal::SessionsEnded(Ended::Session(k))) if k == self.key => Err((live::CLOSE_SESSION_ENDED, "session ended")),
					Ok(Signal::GoingAway) | Err(RecvError::Closed) => Err((live::CLOSE_GOING_AWAY, "server stopping")),
					Ok(_) | Err(RecvError::Lagged(_)) => Ok(()),
				},
				_ = recheck.tick() => match signin::check(&self.sign_in, &self.cookie, Freshness::Cached).await {
					Err(Denied::Unauthenticated) => Err((live::CLOSE_SESSION_ENDED, "session ended")),
					_ => Ok(()),
				},
			};
			if let Err((code, reason)) = step {
				let _closed = self.devtools.close(None).await; // the runner counts its clients by their sockets
				return close(self.socket, code, reason).await;
			}
		}
	}

	/// Sends a command, its answer handled when it comes; `Err` the devtools gone.
	async fn cdp(&mut self, session: Option<&str>, method: &str, params: Value, asked: Asked) -> Result<(), (u16, &'static str)> {
		self.next += 1;
		let mut m = json!({ "id": self.next, "method": method, "params": params });
		if let Some(s) = session {
			m["sessionId"] = s.into();
		}
		self.pending.insert(self.next, asked);
		self.devtools
			.send(tungstenite::Message::text(m.to_string()))
			.await
			.map_err(|_| (CLOSE_STOPPED, "its devtools went"))
	}

	/// On the shown tab, if any.
	async fn on_active(&mut self, method: &str, params: Value, what: &'static str) -> Result<(), (u16, &'static str)> {
		let Some((_, session)) = self.active.clone() else {
			return Ok(());
		};
		self.cdp(Some(&session), method, params, Asked::Done(what)).await
	}

	async fn say(&mut self, out: &ViewerOut) -> Result<(), (u16, &'static str)> {
		let text = serde_json::to_string(out).expect("a derived Serialize of strings and numbers");
		match tokio::time::timeout(SEND_WITHIN, self.socket.send(Message::Text(text.into()))).await {
			Ok(Ok(())) => Ok(()),
			_ => Err((1000, "the viewer did not keep up")),
		}
	}

	async fn heard(&mut self, text: &str) -> Result<(), (u16, &'static str)> {
		let input: ViewerIn = match serde_json::from_str(text) {
			Ok(i) => i,
			Err(e) => {
				tracing::debug!(error = %e, "browser: a viewer frame refused");
				return Err((1007, "not a frame of the viewer's"));
			}
		};
		match input {
			ViewerIn::Touch { phase, x, y } => {
				let (kind, points) = match phase {
					TouchPhase::Start => ("touchStart", json!([{ "x": x, "y": y }])),
					TouchPhase::Move => ("touchMove", json!([{ "x": x, "y": y }])),
					TouchPhase::End => ("touchEnd", json!([])),
				};
				self.on_active("Input.dispatchTouchEvent", json!({ "type": kind, "touchPoints": points }), "a touch").await
			}
			ViewerIn::Wheel { x, y, dx, dy } => {
				// a phone scrolls under a finger: the wheel's turn as a swipe the other way
				self.on_active("Input.dispatchTouchEvent", json!({ "type": "touchStart", "touchPoints": [{ "x": x, "y": y }] }), "a swipe")
					.await?;
				self.on_active(
					"Input.dispatchTouchEvent",
					json!({ "type": "touchMove", "touchPoints": [{ "x": x - dx, "y": y - dy }] }),
					"a swipe",
				)
				.await?;
				self.on_active("Input.dispatchTouchEvent", json!({ "type": "touchEnd", "touchPoints": [] }), "a swipe").await
			}
			ViewerIn::Key { key, ctrl } => {
				let Some(k) = key_of(&key, ctrl) else {
					return self
						.say(&ViewerOut::Error {
							message: format!("the key {key:?} is not passed"),
						})
						.await;
				};
				self.on_active("Input.dispatchKeyEvent", k.down, "a key").await?;
				self.on_active("Input.dispatchKeyEvent", k.up, "a key").await
			}
			ViewerIn::Text { text } => self.on_active("Input.insertText", json!({ "text": text }), "typing").await,
			ViewerIn::Navigate { url } => match web_url(&url) {
				Some(url) => self.on_active("Page.navigate", json!({ "url": url }), "navigating").await,
				None =>
					self.say(&ViewerOut::Error {
						message: format!("{url:?} is not a web address"),
					})
					.await,
			},
			ViewerIn::History { delta } => {
				let Some((_, session)) = self.active.clone() else { return Ok(()) };
				self.cdp(Some(&session), "Page.getNavigationHistory", json!({}), Asked::History(delta)).await
			}
			ViewerIn::Activate { id } => self.show(&id).await,
			ViewerIn::NewTab => self.cdp(None, "Target.createTarget", json!({ "url": "about:blank" }), Asked::Created).await,
			ViewerIn::CloseTab { id } => self.cdp(None, "Target.closeTarget", json!({ "targetId": id }), Asked::Done("closing a tab")).await,
			ViewerIn::Dialog { accept, text } => {
				let mut p = json!({ "accept": accept });
				if let Some(t) = text {
					p["promptText"] = t.into();
				}
				self.on_active("Page.handleJavaScriptDialog", p, "answering the dialog").await
			}
		}
	}

	/// Shows tab `target`: the one shown before let go, this one attached and cast.
	async fn show(&mut self, target: &str) -> Result<(), (u16, &'static str)> {
		if !self.tabs.iter().any(|t| t.id == target) || self.active.as_ref().is_some_and(|(t, _)| t == target) {
			return Ok(());
		}
		if let Some((_, session)) = self.active.take() {
			self.cdp(None, "Target.detachFromTarget", json!({ "sessionId": session }), Asked::Done("letting a tab go"))
				.await?;
		}
		self.cdp(None, "Target.activateTarget", json!({ "targetId": target }), Asked::Done("bringing a tab forward"))
			.await?;
		self.cdp(None, "Target.attachToTarget", json!({ "targetId": target, "flatten": true }), Asked::Attach(target.to_owned()))
			.await?;
		self.tabs_changed().await
	}

	async fn tabs_changed(&mut self) -> Result<(), (u16, &'static str)> {
		let out = ViewerOut::Tabs {
			tabs: self.tabs.clone(),
			active: self.active.as_ref().map(|(t, _)| t.clone()),
		};
		self.say(&out).await
	}

	async fn told(&mut self, text: &str) -> Result<(), (u16, &'static str)> {
		let v: Value = serde_json::from_str(text).map_err(|_| (CLOSE_STOPPED, "its devtools spoke something else"))?;
		if let Some(id) = v["id"].as_u64() {
			let Some(asked) = self.pending.remove(&id) else {
				tracing::warn!(id, "browser: devtools answered what was never asked");
				return Ok(());
			};
			if let Some(e) = v.get("error") {
				let what = match asked {
					Asked::Done(what) => what,
					Asked::Attach(_) => "showing a tab",
					Asked::Created => "opening a tab",
					Asked::History(_) => "going back or forward",
				};
				tracing::debug!(error = %e, what, "browser: devtools refused");
				return self
					.say(&ViewerOut::Error {
						message: format!("{what}: {}", e["message"].as_str().unwrap_or("refused")),
					})
					.await;
			}
			let r = &v["result"];
			return match asked {
				Asked::Done(_) => Ok(()),
				Asked::Attach(target) => {
					let session = r["sessionId"].as_str().ok_or((CLOSE_STOPPED, "an attach without a session"))?.to_owned();
					self.active = Some((target, session.clone()));
					self.cdp(Some(&session), "Page.enable", json!({}), Asked::Done("watching the page")).await?;
					let cast = json!({ "format": "jpeg", "quality": 60, "maxWidth": 1080, "maxHeight": 2400, "everyNthFrame": 1 });
					self.cdp(Some(&session), "Page.startScreencast", cast, Asked::Done("casting the page")).await?;
					self.tabs_changed().await
				}
				Asked::Created => match r["targetId"].as_str() {
					Some(t) => {
						let t = t.to_owned();
						self.show(&t).await
					}
					None => Ok(()),
				},
				Asked::History(delta) => {
					let at = r["currentIndex"].as_i64().unwrap_or(-1) + i64::from(delta);
					match usize::try_from(at).ok().and_then(|i| r["entries"].get(i)).and_then(|e| e["id"].as_i64()) {
						Some(entry) => self.on_active("Page.navigateToHistoryEntry", json!({ "entryId": entry }), "going back or forward").await,
						None => Ok(()),
					}
				}
			};
		}
		let method = v["method"].as_str().unwrap_or_default();
		let p = &v["params"];
		let from_active = v["sessionId"].as_str().is_some_and(|s| self.active.as_ref().is_some_and(|(_, a)| a == s));
		match method {
			"Target.targetCreated" | "Target.targetInfoChanged" if p["targetInfo"]["type"] == "page" => {
				let info = &p["targetInfo"];
				let tab = TabDto {
					id: info["targetId"].as_str().unwrap_or_default().to_owned(),
					title: info["title"].as_str().unwrap_or_default().to_owned(),
					url: info["url"].as_str().unwrap_or_default().to_owned(),
				};
				let opened = method == "Target.targetCreated" && !self.tabs.iter().any(|t| t.id == tab.id);
				let opener = info["openerId"].is_string();
				match self.tabs.iter_mut().find(|t| t.id == tab.id) {
					Some(t) => *t = tab.clone(),
					None => self.tabs.push(tab.clone()),
				}
				// a page's own new tab comes forward, as on a phone
				if self.active.is_none() || (opened && opener) {
					return self.show(&tab.id).await;
				}
				self.tabs_changed().await
			}
			"Target.targetDestroyed" => {
				let gone = p["targetId"].as_str().unwrap_or_default();
				self.tabs.retain(|t| t.id != gone);
				if self.active.as_ref().is_some_and(|(t, _)| t == gone) {
					self.active = None;
					self.last = None;
					if let Some(next) = self.tabs.last().map(|t| t.id.clone()) {
						return self.show(&next).await;
					}
				}
				self.tabs_changed().await
			}
			"Page.screencastFrame" if from_active => {
				let m = &p["metadata"];
				let (width, height) = (m["deviceWidth"].as_f64().unwrap_or_default(), m["deviceHeight"].as_f64().unwrap_or_default());
				self.last = Some((width, height));
				let frame = ViewerOut::Frame {
					data: p["data"].as_str().unwrap_or_default().to_owned(),
					width,
					height,
				};
				self.say(&frame).await?;
				// acked once it went out: a viewer behind gets fewer frames, never a backlog
				let number = p["sessionId"].clone();
				self.on_active("Page.screencastFrameAck", json!({ "sessionId": number }), "acking a frame").await
			}
			"Page.javascriptDialogOpening" if from_active => {
				let d = ViewerOut::Dialog {
					kind: p["type"].as_str().unwrap_or_default().to_owned(),
					message: p["message"].as_str().unwrap_or_default().to_owned(),
					default: p["defaultPrompt"].as_str().map(str::to_owned),
				};
				self.say(&d).await
			}
			"Page.javascriptDialogClosed" if from_active => self.say(&ViewerOut::DialogClosed).await,
			_ => Ok(()),
		}
	}
}

struct KeyEvents {
	down: Value,
	up: Value,
}

/// The keys a viewer may press that type nothing.
fn key_of(key: &str, ctrl: bool) -> Option<KeyEvents> {
	if ctrl {
		return (key == "a").then(|| KeyEvents {
			down: json!({ "type": "rawKeyDown", "key": "a", "code": "KeyA", "windowsVirtualKeyCode": 65, "modifiers": 2, "commands": ["selectAll"] }),
			up: json!({ "type": "keyUp", "key": "a", "code": "KeyA", "windowsVirtualKeyCode": 65, "modifiers": 2 }),
		});
	}
	let (code, vk, text) = match key {
		"Enter" => ("Enter", 13, Some("\r")),
		"Backspace" => ("Backspace", 8, None),
		"Tab" => ("Tab", 9, None),
		"Escape" => ("Escape", 27, None),
		"Delete" => ("Delete", 46, None),
		"Home" => ("Home", 36, None),
		"End" => ("End", 35, None),
		"ArrowLeft" => ("ArrowLeft", 37, None),
		"ArrowUp" => ("ArrowUp", 38, None),
		"ArrowRight" => ("ArrowRight", 39, None),
		"ArrowDown" => ("ArrowDown", 40, None),
		_ => return None,
	};
	let mut down = json!({ "type": if text.is_some() { "keyDown" } else { "rawKeyDown" }, "key": key, "code": code, "windowsVirtualKeyCode": vk });
	if let Some(t) = text {
		down["text"] = t.into();
	}
	Some(KeyEvents {
		down,
		up: json!({ "type": "keyUp", "key": key, "code": code, "windowsVirtualKeyCode": vk }),
	})
}
