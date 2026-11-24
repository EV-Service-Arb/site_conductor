//! A gmail's antidetect browser (docs/ARCHITECTURE.md, Browsers), its owner's only: set up with
//! its proxy, started on sa_browsers' runner, and driven through the conductor alone.
//!
//! ```text
//! PUT    /api/v1/gmails/{id}/browser {provider, device?, proxy}  → Connections; 400 refused, nothing
//!                                     stored; 409 provider or device another: DELETE first; a
//!                                     cloud phone's model is drawn, a browser's device named
//! POST   /api/v1/gmails/{id}/browser/test   → Connections, the exit checked again
//! DELETE /api/v1/gmails/{id}/browser        → 204: stopped, the provider's profile and the session gone
//! POST   /api/v1/gmails/{id}/browser/start {gmb?}  → 204 once up: at that GBP's point, else nowhere
//! POST   /api/v1/gmails/{id}/browser/stop   → 204
//! POST   /api/v1/gmails/{id}/browser/thread {thread}  → 204: a Gmail thread in a new tab of it; 409
//!                                           while it does not run, or a cloud phone
//! PUT    /api/v1/gmails/{id}/browser/film   an mp4 (≤ 512 MiB) → 204: its next launch's back camera,
//!                                           the sound its mic; 400 no sound, 409 while it runs. A
//!                                           cloud phone's (≤ 100 MiB) into Multilogin's Library,
//!                                           picked in its Camera panel; Mimic's: 409, it plays none
//! DELETE /api/v1/gmails/{id}/browser/film   → 204
//! GET    /api/v1/gmails/{id}/browser        WebSocket, the viewer, a row as /live is; one per gmail,
//!                                           a new one closes the old (4409);
//!                                           a browser's through the relay here, a phone's the
//!                                           runner's own, passed through
//! ```
//! Unset PANEL_BROWSERS_URL: 503 `browsers_not_connected`; a provider without its token, 503
//! "`<provider>` not connected".

use std::{
	collections::HashMap,
	sync::{
		Arc, Mutex,
		atomic::{AtomicU64, Ordering},
	},
	time::Duration,
};

use axum::{
	Extension, Json,
	extract::{
		DefaultBodyLimit, FromRequest, State,
		ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade, rejection::WebSocketUpgradeRejection},
	},
	http::{HeaderMap, Method, StatusCode, header},
	response::{NoContent, Response},
};
use ev_lib::api::{
	Body, GenContext, Ids, JsonSchema, OperationInput, Refusal,
	openapi::{MediaType, Operation, ReferenceOr, RequestBody},
};
use eyre::WrapErr;
use futures::{SinkExt, StreamExt};
use jiff::Timestamp;
use sa_auth::{Assertion, Principal, Service, Signer};
use sa_browsers::{
	Launch, Profiles,
	view::{Close, FRAME_MAX, In as ViewerIn, Out as ViewerOut, Tab as TabDto, TouchPhase, web_url},
};
use sa_core::prices::{BrowserStart, PhoneTime, Spends};
use sa_panel::{
	browsers::{Browsers, LIBRARY_MAX, NewBrowser, Provider, Run},
	gmbs::Event,
	live::{Ended, Signal},
	session::SessionKey,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{broadcast::error::RecvError, oneshot};
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use tracing::Instrument;
use zeroize::Zeroizing;

use crate::{
	api::{ApiResult, Code, Id},
	connections::{self, CheckedDto, ConnectionsDto},
	cookies,
	http::{ApiRoute, ApiState, SLOW, USER},
	live,
	signin::{self, Caller, Freshness, SignIn},
};

/// A launch waits on the runner, which bounds it: a phone's start, its adb and its placement
/// the longest.
const LAUNCH_WITHIN: Duration = Duration::from_secs(9 * 60);

/// How often the viewer's session is asked again.
const RECHECK: Duration = Duration::from_secs(60);

/// How long one frame may take to go out before the viewer is dropped.
const SEND_WITHIN: Duration = Duration::from_secs(10);
/// A film at most.
const FILM_MAX: usize = 512 << 20;
type Devtools = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

type Held = (u64, oneshot::Sender<()>);

/// Where a gmail's proxy comes out, at its last check.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Exit")]
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
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Browser")]
pub struct BrowserDto {
	pub provider: Provider,
	/// A `Device`; a cloud phone's, its model.
	pub device: String,
	/// The proxy's host:port.
	pub endpoint: String,
	pub exit: ExitDto,
	pub set_at: String,
	/// `error` null: through the proxy, out in the country of every GBP of the gmail.
	pub checked: CheckedDto,
	/// What it runs for now: `gmb` null, the gmail alone; `stream` a cloud phone's own view at
	/// Multilogin; null, it does not run.
	pub running: Option<RunningDto>,
	/// Its next launch plays the film uploaded as its back camera; a phone's is in Multilogin's
	/// Library.
	pub film: bool,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Running")]
pub struct RunningDto {
	pub gmb: Option<i64>,
	pub stream: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SetBrowser {
	pub provider: Provider,
	/// A browser's; a cloud phone's model is drawn.
	pub device: Option<sa_browsers::Device>,
	/// `socks5://` or `http://`, `user:pass@host:port`.
	pub proxy: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartBrowser {
	/// At this GBP's point; none, the position blocked.
	pub gmb: Option<i64>,
}

/// The browsers connected: the providers and the runner, with PANEL_BROWSERS_URL.
#[derive(Clone)]
pub struct Connected {
	browsers: Arc<Browsers>,
	runner: Runner,
	viewers: Viewers,
}

impl Connected {
	pub fn try_new(browsers: Browsers, runner: &str, signer: Signer) -> eyre::Result<Self> {
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
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

	pub(crate) fn browsers(&self) -> &Browsers {
		&self.browsers
	}

	/// What the gmail's browser runs for now, and whether its next launch plays a film.
	/// A page of the runner's usage feed after `after`: its status and body.
	pub(crate) async fn usage(&self, after: i64, limit: usize) -> eyre::Result<(StatusCode, Vec<u8>)> {
		match self.runner.call(Method::GET, &format!("/usage?after={after}&limit={limit}"), None).await {
			Ok((status, text)) => Ok((status, text.into_bytes())),
			Err(e) => Err(eyre::eyre!("the browsers' runner's usage: {e:?}")),
		}
	}

	pub(crate) async fn state(&self, gmail: i64) -> ApiResult<(Option<RunningDto>, bool)> {
		let (status, body) = self.runner.call(Method::GET, "/profiles", None).await?;
		if status != StatusCode::OK {
			return Err(Refusal::new(Code::Unavailable, format!("the browsers' runner answered {status}: {body}")));
		}
		let profiles: Profiles = serde_json::from_str(&body).map_err(|e| eyre::eyre!("the runner's profiles: {e}"))?;
		let running = match profiles.running.iter().find(|r| r.gmail == gmail) {
			Some(r) => Some(RunningDto {
				gmb: r.run.parse::<Run>().map_err(|e| eyre::eyre!("the runner's run: {e}"))?.0,
				stream: r.stream.clone(),
			}),
			None => None,
		};
		Ok((running, profiles.films.contains(&gmail)))
	}
}

/// A Gmail thread for the gmail's browser to open.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenThread {
	/// Gmail's X-GM-THRID, hex, as an `email` of a GBP's log names it.
	pub thread: String,
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
			// an assertion names the path, not its query, as the forward's do
			.header(sa_auth::HEADER, self.assertion(&method, path.split_once('?').map_or(path, |(p, _)| p)));
		let req = match body {
			Some(b) => req.json(b),
			None => req,
		};
		let res = req
			.send()
			.await
			.map_err(|e| Refusal::new(Code::Unavailable, format!("the browsers' runner did not answer: {e}")))?;
		let status = res.status();
		let text = res
			.text()
			.await
			.map_err(|e| Refusal::new(Code::Unavailable, format!("the browsers' runner's answer broke off: {e}")))?;
		Ok((status, text))
	}

	/// 204 expected, anything else said back as the runner put it.
	/// The runner's 409 is the inner `Err`, its text.
	async fn expect_done(&self, method: Method, path: &str, body: Option<&Launch>) -> ApiResult<Result<(), String>> {
		let (status, text) = self.call(method, path, body).await?;
		match status {
			StatusCode::NO_CONTENT => Ok(Ok(())),
			StatusCode::CONFLICT => Ok(Err(text)),
			StatusCode::SERVICE_UNAVAILABLE | StatusCode::BAD_GATEWAY => Err(Refusal::new(Code::Unavailable, text)),
			s => Err(eyre::eyre!("the browsers' runner answered {path} with {s}: {text}").into()),
		}
	}

	/// The runner's socket for the gmail: `cdp`, a browser's devtools; `view`, a phone's viewer.
	async fn socket(&self, gmail: i64, which: &str) -> eyre::Result<Option<Devtools>> {
		let path = format!("/profiles/{gmail}/{which}");
		let ws = format!("{}{path}", self.base.replacen("http", "ws", 1));
		let mut req = ws.into_client_request().wrap_err("the runner's devtools URL")?;
		req.headers_mut()
			.insert(sa_auth::HEADER, self.assertion(&Method::GET, &path).parse().expect("a JWS is base64url and dots"));
		match tokio_tungstenite::connect_async(req).await {
			Ok((ws, _)) => Ok(Some(ws)),
			Err(tungstenite::Error::Http(res)) if res.status() == StatusCode::NOT_FOUND => Ok(None),
			Err(e) => Err(eyre::Report::new(e).wrap_err(format!("the runner's {which} socket"))),
		}
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::PUT, "/gmails/{id}/browser", USER, set_browser),
		ApiRoute::new(M::POST, "/gmails/{id}/browser/test", USER, test_browser),
		ApiRoute::new(M::DELETE, "/gmails/{id}/browser", USER, remove_browser),
		// A launch waits on the provider and the browser's devtools, a film's upload on its bytes.
		ApiRoute::new(M::POST, "/gmails/{id}/browser/start", SLOW, start_browser),
		ApiRoute::new(M::POST, "/gmails/{id}/browser/stop", USER, stop_browser),
		ApiRoute::new(M::POST, "/gmails/{id}/browser/thread", USER, open_thread),
		ApiRoute::new(M::PUT, "/gmails/{id}/browser/film", SLOW, set_film).layer(|h| h.layer(DefaultBodyLimit::max(FILM_MAX))),
		ApiRoute::new(M::DELETE, "/gmails/{id}/browser/film", USER, remove_film),
		ApiRoute::new(M::GET, "/gmails/{id}/browser", USER, viewer),
	]
}

/// An mp4, as it streams in.
pub(crate) struct Mp4(axum::body::Body);

impl<S: Send + Sync> FromRequest<S> for Mp4 {
	type Rejection = std::convert::Infallible;

	async fn from_request(req: axum::extract::Request, _: &S) -> Result<Self, Self::Rejection> {
		Ok(Self(req.into_body()))
	}
}

impl OperationInput for Mp4 {
	fn operation_input(_: &mut GenContext, op: &mut Operation) {
		op.request_body = Some(ReferenceOr::Item(RequestBody {
			content: [("video/mp4".into(), MediaType::default())].into_iter().collect(),
			required: true,
			..RequestBody::default()
		}));
	}
}

fn connected(state: &ApiState) -> ApiResult<&Connected> {
	state.browsers.as_ref().ok_or_else(|| Refusal::new(Code::BrowsersNotConnected, "browsers not connected"))
}

/// Sets the gmail's browser up, kept only once its proxy comes out in the country of every GBP of
/// the gmail; the provider and device are a fingerprint, set once, the proxy replaced under them.
///
/// 400 refused, nothing stored; 409 another provider or device; a phone names no device.
async fn set_browser(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>, Body(b): Body<SetBrowser>) -> ApiResult<Json<ConnectionsDto>> {
	let c = connected(&state)?;
	let new = NewBrowser {
		provider: b.provider,
		device: b.device,
		proxy: Zeroizing::new(b.proxy),
	};
	state.sign_in.panel.set_browser(&c.browsers, caller.user_id, gmail, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's browser set");
	connections::answer(&state, caller.user_id, gmail).await.map(Json)
}

/// Checks the browser's proxy exit again.
async fn test_browser(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<Json<ConnectionsDto>> {
	state.sign_in.panel.check_browser(&connected(&state)?.browsers, caller.user_id, gmail, Timestamp::now()).await??;
	connections::answer(&state, caller.user_id, gmail).await.map(Json)
}

/// Removes the gmail's browser: stopped, the provider's profile (a phone's film) and session gone.
async fn remove_browser(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	let c = connected(&state)?;
	state.sign_in.panel.browser_of(caller.user_id, gmail).await??;
	c.runner
		.expect_done(Method::DELETE, &format!("/profiles/{gmail}"), None)
		.await?
		.map_err(|t| Refusal::new(Code::Conflict, t))?;
	state.sign_in.panel.delete_browser(&c.browsers, caller.user_id, gmail).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's browser removed");
	Ok(NoContent)
}

/// Starts the gmail's browser, answering once it is up: at a GBP's point, its zone and its
/// country's locale; for the gmail alone the position blocked. A cloud phone bills while it runs.
///
/// 409 it runs for another.
async fn start_browser(
	State(state): State<ApiState>,
	Spends(caller, by): Spends<(PhoneTime, BrowserStart), Caller>,
	Ids(Id { id: gmail }): Ids<Id<i64>>,
	Body(b): Body<StartBrowser>,
) -> ApiResult<NoContent> {
	let c = connected(&state)?;
	let launch = state.sign_in.panel.launch(&c.browsers, by, gmail, b.gmb, Timestamp::now()).await??;
	let (_, film) = c.state(gmail).await?;
	c.runner
		.expect_done(Method::POST, &format!("/profiles/{gmail}/launch"), Some(&launch))
		.await?
		.map_err(|_| Refusal::new(Code::Conflict, "it runs for another GBP, or for the gmail alone: stop it first"))?;
	state.sign_in.panel.log_browser(caller.user_id, gmail, b.gmb, &Event::Launched { film }).await?;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, run = launch.run, "gmail's browser up");
	Ok(NoContent)
}

/// Stops the gmail's browser.
async fn stop_browser(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	let c = connected(&state)?;
	state.sign_in.panel.browser_of(caller.user_id, gmail).await??;
	let (running, _) = c.state(gmail).await?;
	c.runner
		.expect_done(Method::POST, &format!("/profiles/{gmail}/stop"), None)
		.await?
		.map_err(|t| Refusal::new(Code::Conflict, t))?;
	if let Some(r) = running {
		state.sign_in.panel.log_browser(caller.user_id, gmail, r.gmb, &Event::Stopped).await?;
	}
	Ok(NoContent)
}

/// Takes the browser's film away.
async fn remove_film(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	let c = connected(&state)?;
	match state.sign_in.panel.browser_of(caller.user_id, gmail).await??.provider {
		Provider::Multilogin => state.sign_in.panel.remove_phone_film(&c.browsers, caller.user_id, gmail).await??,
		Provider::MultiloginBrowser | Provider::Gologin => c
			.runner
			.expect_done(Method::DELETE, &format!("/profiles/{gmail}/film"), None)
			.await?
			.map_err(|t| Refusal::new(Code::Conflict, t))?,
	}
	Ok(NoContent)
}

/// Uploads an mp4 as the camera of the browser's next launch, its sound the mic: ≤ 512 MiB; a
/// cloud phone's ≤ 100 MiB, to Multilogin's Library.
///
/// 409 for Multilogin's browser: its camera is the host's.
async fn set_film(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>, Mp4(film): Mp4) -> ApiResult<NoContent> {
	let c = connected(&state)?;
	match state.sign_in.panel.browser_of(caller.user_id, gmail).await??.provider {
		Provider::Gologin => {}
		Provider::MultiloginBrowser => return Err(Refusal::new(Code::Conflict, "Multilogin's browser plays no film: its camera is the host's")),
		Provider::Multilogin => {
			let mp4 = axum::body::to_bytes(film, LIBRARY_MAX)
				.await
				.map_err(|_| Refusal::new(Code::TooLarge, format!("a phone's film is {} MiB at most", LIBRARY_MAX >> 20)))?;
			state.sign_in.panel.set_phone_film(&c.browsers, caller.user_id, gmail, mp4.to_vec()).await??;
			tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "phone's film in Multilogin's Library");
			return Ok(NoContent);
		}
	}
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
		.map_err(|e| Refusal::new(Code::Unavailable, format!("the browsers' runner did not take the film: {e}")))?;
	let status = sent.status();
	let text = sent
		.text()
		.await
		.map_err(|e| Refusal::new(Code::Unavailable, format!("the browsers' runner's answer broke off: {e}")))?;
	match status {
		StatusCode::NO_CONTENT => {
			tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's film set");
			Ok(NoContent)
		}
		StatusCode::BAD_REQUEST => Err(Refusal::new(Code::BadRequest, text)),
		StatusCode::CONFLICT => Err(Refusal::new(Code::Conflict, text)),
		StatusCode::PAYLOAD_TOO_LARGE => Err(Refusal::new(Code::TooLarge, format!("a film is {} MiB at most", FILM_MAX >> 20))),
		s => Err(eyre::eyre!("the browsers' runner answered a film with {s}: {text}").into()),
	}
}

/// Opens a thread in a new tab of the gmail's own running browser, on the Google session signed in
/// there: Gmail is never asked from here. 409 while it does not run, or on a cloud phone.
async fn open_thread(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>, Body(b): Body<OpenThread>) -> ApiResult<NoContent> {
	let c = connected(&state)?;
	if b.thread.is_empty() || b.thread.len() > 16 || !b.thread.bytes().all(|x| x.is_ascii_hexdigit()) {
		return Err(Refusal::new(Code::BadRequest, "a thread is Gmail's X-GM-THRID, hex"));
	}
	let panel = &state.sign_in.panel;
	if panel.browser_of(caller.user_id, gmail).await??.provider == Provider::Multilogin {
		return Err(Refusal::new(Code::Conflict, "a cloud phone opens it in its own Gmail app"));
	}
	let address = panel
		.gmails(caller.user_id)
		.await?
		.0
		.into_iter()
		.find(|g| g.id == gmail)
		.expect("its browser is the owner's, so is it")
		.gmail;
	let Some(mut devtools) = c.runner.socket(gmail, "cdp").await? else {
		return Err(Refusal::new(Code::Conflict, "its browser is not running: start it"));
	};
	let url = format!("https://mail.google.com/mail/?authuser={address}#all/{}", b.thread);
	let created = async {
		devtools
			.send(tungstenite::Message::text(
				json!({ "id": 1, "method": "Target.createTarget", "params": { "url": url } }).to_string(),
			))
			.await
			.wrap_err("asking for a tab")?;
		//LOOP: its events until the answer; the timeout bounds it
		loop {
			let Some(m) = devtools.next().await else {
				eyre::bail!("its devtools closed before a tab opened")
			};
			if let tungstenite::Message::Text(t) = m.wrap_err("its devtools")? {
				let answer: Value = serde_json::from_str(&t).wrap_err("a devtools message")?;
				if answer["id"] == 1 {
					eyre::ensure!(answer.get("error").is_none(), "a tab refused: {}", answer["error"]);
					return Ok(());
				}
			}
		}
	};
	let opened = tokio::time::timeout(SEND_WITHIN, created).await;
	let _closed = devtools.close(None).await; // the runner counts its clients by their sockets
	opened.map_err(|_| Refusal::new(Code::Unavailable, "its browser did not open a tab in time"))??;
	Ok(NoContent)
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

/// The gmail's browser on screen: its frames down, the viewer's input up. A browser's through the
/// relay here, a phone's the runner's own, passed through.
///
/// One per gmail: a new one closes the one before (4409). Opened from the panel's own pages, on
/// the browser's session: 403 from another origin.
async fn viewer(
	State(s): State<ApiState>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id: gmail }): Ids<Id<i64>>,
	headers: HeaderMap,
	ws: Result<WebSocketUpgrade, WebSocketUpgradeRejection>,
) -> Result<Response, Refusal<Code>> {
	if !live::same_origin(&headers, s.sign_in.panel_origin()) {
		tracing::info!(origin = ?headers.get(header::ORIGIN), "browser: a handshake from another origin refused");
		return Err(Refusal::new(Code::Csrf, "the page is out of date: reload it"));
	}
	let c = connected(&s)?.clone();
	let phone = s.sign_in.panel.browser_of(caller.user_id, gmail).await??.provider == Provider::Multilogin;
	let ws = ws.map_err(|r| Refusal::new(Code::BadRequest, r.body_text()))?;
	let Some(key) = s.sign_in.cookies().get(&headers, cookies::SESSION).and_then(SessionKey::of_cookie) else {
		return Err(Refusal::new(Code::Unauthenticated, "a viewer opens on the browser's session"));
	};
	let mut cookie = HeaderMap::new();
	for v in headers.get_all(header::COOKIE) {
		cookie.append(header::COOKIE, v.clone());
	}
	let bus = s.sign_in.panel.bus().subscribe();
	let span = tracing::info_span!("browser", user_id = %caller.user_id, gmail_id = gmail);
	Ok(ws.max_message_size(FRAME_MAX).max_frame_size(FRAME_MAX).max_write_buffer_size(4 << 20).on_upgrade(move |socket| {
		async move {
			let (viewing, replaced) = c.viewers.take(gmail);
			let watch = Watch {
				sign_in: s.sign_in,
				cookie,
				key,
				caller,
				bus,
				replaced,
				recheck: tokio::time::interval_at(tokio::time::Instant::now() + RECHECK, RECHECK),
			};
			let (code, reason) = match c.runner.socket(gmail, if phone { "view" } else { "cdp" }).await {
				Ok(Some(runner)) if phone => pass(socket, runner, watch).await,
				Ok(Some(devtools)) =>
					View {
						socket,
						devtools,
						watch,
						next: 0,
						pending: HashMap::new(),
						tabs: Vec::new(),
						active: None,
						last: None,
					}
					.run()
					.await,
				Ok(None) => close(socket, Close::NotRunning, "not running: start it").await,
				Err(e) => {
					crate::report(&e, "browser");
					close(socket, Close::Stopped, "its devtools did not answer").await
				}
			};
			drop(viewing);
			tracing::debug!(code = code.code(), reason, "browser: viewer closed");
		}
		.instrument(span)
	}))
}

async fn close(mut socket: WebSocket, code: Close, reason: &'static str) -> (Close, &'static str) {
	// the last word: whether it reached the viewer changes nothing here
	let _last = socket
		.send(Message::Close(Some(CloseFrame {
			code: code.code(),
			reason: reason.into(),
		})))
		.await;
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

/// What closes a viewer but its sockets: its session ended, another viewer, the server stopping.
struct Watch {
	sign_in: SignIn,
	cookie: HeaderMap,
	key: SessionKey,
	caller: Caller,
	bus: tokio::sync::broadcast::Receiver<sa_panel::live::Signal>,
	replaced: oneshot::Receiver<()>,
	recheck: tokio::time::Interval,
}

impl Watch {
	/// Why the viewer is to close, once it is.
	async fn ended(&mut self) -> (Close, &'static str) {
		//LOOP: until one of them says close
		loop {
			tokio::select! {
				_ = &mut self.replaced => return (Close::Replaced, "another viewer took over"),
				signal = self.bus.recv() => match signal {
					Ok(Signal::SessionsEnded(Ended::User(u))) if u == self.caller.user_id => return (Close::SignedOut, "session ended"),
					Ok(Signal::SessionsEnded(Ended::Session(k))) if k == self.key => return (Close::SignedOut, "session ended"),
					Ok(Signal::GoingAway) | Err(RecvError::Closed) => return (Close::GoingAway, "server stopping"),
					Ok(_) | Err(RecvError::Lagged(_)) => {}
				},
				_ = self.recheck.tick() => match signin::check(&self.sign_in, &self.cookie, Freshness::Cached).await {
					Err(r) if r.code() == Code::Unauthenticated => return (Close::SignedOut, "session ended"),
					// a failure of ours was reported as it was refused
					Ok(_) | Err(_) => {}
				},
			}
		}
	}
}

/// A cloud phone's viewer, the runner's: each of the viewer's frames checked, then passed on.
async fn pass(mut socket: WebSocket, mut runner: Devtools, mut watch: Watch) -> (Close, &'static str) {
	//LOOP: until either socket ends or the viewer is to go
	let (code, reason) = loop {
		tokio::select! {
			m = socket.recv() => match m {
				Some(Ok(Message::Text(t))) => {
					if serde_json::from_str::<ViewerIn>(&t).is_err() {
						break (Close::NotAFrame, "not a frame of the viewer's");
					}
					if runner.send(tungstenite::Message::text(t.as_str())).await.is_err() {
						break (Close::Stopped, "the phone stopped");
					}
				}
				Some(Ok(Message::Ping(_) | Message::Pong(_))) => {}
				Some(Ok(Message::Binary(_))) => break (Close::TextOnly, "text frames only"),
				Some(Ok(Message::Close(_)) | Err(_)) | None => break (Close::Normal, "viewer left"),
			},
			m = runner.next() => match m {
				Some(Ok(tungstenite::Message::Text(t))) => match tokio::time::timeout(SEND_WITHIN, socket.send(Message::Text(t.as_str().into()))).await {
					Ok(Ok(())) => {}
					_ => break (Close::Normal, "the viewer did not keep up"),
				},
				Some(Ok(tungstenite::Message::Close(_)) | Err(_)) | None => break (Close::Stopped, "the phone stopped"),
				Some(Ok(_)) => {}
			},
			end = watch.ended() => break end,
		}
	};
	let _closed = runner.close(None).await; // the runner counts its viewers by their sockets
	close(socket, code, reason).await
}

/// One viewer of one browser: the devtools' events into frames, the viewer's into input.
struct View {
	socket: WebSocket,
	devtools: Devtools,
	watch: Watch,
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
	async fn run(mut self) -> (Close, &'static str) {
		if self
			.cdp(None, "Target.setDiscoverTargets", json!({ "discover": true }), Asked::Done("discovering the tabs"))
			.await
			.is_err()
		{
			return close(self.socket, Close::Stopped, "its devtools went").await;
		}
		//LOOP: until either socket ends or the viewer is to go
		loop {
			let step: Result<(), (Close, &'static str)> = tokio::select! {
				m = self.socket.recv() => match m {
					Some(Ok(Message::Text(t))) => self.heard(&t).await,
					Some(Ok(Message::Ping(_) | Message::Pong(_))) => Ok(()),
					Some(Ok(Message::Binary(_))) => Err((Close::TextOnly, "text frames only")),
					Some(Ok(Message::Close(_)) | Err(_)) | None => return (Close::Normal, "viewer left"),
				},
				m = self.devtools.next() => match m {
					Some(Ok(tungstenite::Message::Text(t))) => self.told(&t).await,
					Some(Ok(tungstenite::Message::Close(_)) | Err(_)) | None => Err((Close::Stopped, "the browser stopped")),
					Some(Ok(_)) => Ok(()),
				},
				end = self.watch.ended() => Err(end),
			};
			if let Err((code, reason)) = step {
				let _closed = self.devtools.close(None).await; // the runner counts its clients by their sockets
				return close(self.socket, code, reason).await;
			}
		}
	}

	/// Sends a command, its answer handled when it comes; `Err` the devtools gone.
	async fn cdp(&mut self, session: Option<&str>, method: &str, params: Value, asked: Asked) -> Result<(), (Close, &'static str)> {
		self.next += 1;
		let mut m = json!({ "id": self.next, "method": method, "params": params });
		if let Some(s) = session {
			m["sessionId"] = s.into();
		}
		self.pending.insert(self.next, asked);
		self.devtools
			.send(tungstenite::Message::text(m.to_string()))
			.await
			.map_err(|_| (Close::Stopped, "its devtools went"))
	}

	/// On the shown tab, if any.
	async fn on_active(&mut self, method: &str, params: Value, asked: Asked) -> Result<(), (Close, &'static str)> {
		let Some((_, session)) = self.active.clone() else {
			return Ok(());
		};
		self.cdp(Some(&session), method, params, asked).await
	}

	async fn say(&mut self, out: &ViewerOut) -> Result<(), (Close, &'static str)> {
		let text = serde_json::to_string(out).expect("a derived Serialize of strings and numbers");
		match tokio::time::timeout(SEND_WITHIN, self.socket.send(Message::Text(text.into()))).await {
			Ok(Ok(())) => Ok(()),
			_ => Err((Close::Normal, "the viewer did not keep up")),
		}
	}

	async fn heard(&mut self, text: &str) -> Result<(), (Close, &'static str)> {
		let input: ViewerIn = match serde_json::from_str(text) {
			Ok(i) => i,
			Err(e) => {
				tracing::debug!(error = %e, "browser: a viewer frame refused");
				return Err((Close::NotAFrame, "not a frame of the viewer's"));
			}
		};
		match input {
			ViewerIn::Touch { phase, x, y } => {
				let (kind, points) = match phase {
					TouchPhase::Start => ("touchStart", json!([{ "x": x, "y": y }])),
					TouchPhase::Move => ("touchMove", json!([{ "x": x, "y": y }])),
					TouchPhase::End => ("touchEnd", json!([])),
				};
				self.on_active("Input.dispatchTouchEvent", json!({ "type": kind, "touchPoints": points }), Asked::Done("a touch"))
					.await
			}
			ViewerIn::Wheel { x, y, dx, dy } => {
				// a phone scrolls under a finger: the wheel's turn as a swipe the other way
				self.on_active(
					"Input.dispatchTouchEvent",
					json!({ "type": "touchStart", "touchPoints": [{ "x": x, "y": y }] }),
					Asked::Done("a swipe"),
				)
				.await?;
				self.on_active(
					"Input.dispatchTouchEvent",
					json!({ "type": "touchMove", "touchPoints": [{ "x": x - dx, "y": y - dy }] }),
					Asked::Done("a swipe"),
				)
				.await?;
				self.on_active("Input.dispatchTouchEvent", json!({ "type": "touchEnd", "touchPoints": [] }), Asked::Done("a swipe"))
					.await
			}
			ViewerIn::Key { key, ctrl } => {
				let Some(k) = key_of(&key, ctrl) else {
					return self
						.say(&ViewerOut::Error {
							message: format!("the key {key:?} is not passed"),
						})
						.await;
				};
				self.on_active("Input.dispatchKeyEvent", k.down, Asked::Done("a key")).await?;
				self.on_active("Input.dispatchKeyEvent", k.up, Asked::Done("a key")).await
			}
			ViewerIn::Text { text } => self.on_active("Input.insertText", json!({ "text": text }), Asked::Done("typing")).await,
			ViewerIn::Navigate { url } => match web_url(&url) {
				Some(url) => self.on_active("Page.navigate", json!({ "url": url }), Asked::Done("navigating")).await,
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
				self.on_active("Page.handleJavaScriptDialog", p, Asked::Done("answering the dialog")).await
			}
		}
	}

	/// Shows tab `target`: the one shown before let go, this one attached and cast.
	async fn show(&mut self, target: &str) -> Result<(), (Close, &'static str)> {
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

	async fn tabs_changed(&mut self) -> Result<(), (Close, &'static str)> {
		let out = ViewerOut::Tabs {
			tabs: self.tabs.clone(),
			active: self.active.as_ref().map(|(t, _)| t.clone()),
		};
		self.say(&out).await
	}

	async fn told(&mut self, text: &str) -> Result<(), (Close, &'static str)> {
		let v: Value = serde_json::from_str(text).map_err(|_| (Close::Stopped, "its devtools spoke something else"))?;
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
					let session = r["sessionId"].as_str().ok_or((Close::Stopped, "an attach without a session"))?.to_owned();
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
						Some(entry) =>
							self.on_active("Page.navigateToHistoryEntry", json!({ "entryId": entry }), Asked::Done("going back or forward"))
								.await,
						None => Ok(()),
					}
				}
			};
		}
		let method = v["method"].as_str().unwrap_or_default();
		let p = &v["params"];
		let from_active = v["sessionId"].as_str().is_some_and(|s| self.active.as_ref().is_some_and(|(_, a)| a == s));
		match method {
			// Mimic docks its own devtools in the window: not a tab, and closing it stops the page's frames
			"Target.targetCreated" | "Target.targetInfoChanged" if p["targetInfo"]["url"].as_str().is_some_and(|u| u.starts_with("devtools://")) => Ok(()),
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
				self.on_active("Page.screencastFrameAck", json!({ "sessionId": number }), Asked::Done("acking a frame")).await
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
