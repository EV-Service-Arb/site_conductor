//! `sa_browsers serve`: the gmails' browsers, headless, one session directory each, and their
//! cloud phones, driven only through the conductor (the routes are the lib's). Nothing here holds
//! a secret of the conductor's: a launch brings its tokens and proxy, and an assertion is all it
//! trusts.

mod adb;
mod film;
mod mimic;
mod phone;
mod profile;

use std::{
	collections::HashMap,
	net::SocketAddr,
	path::PathBuf,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{
	Json, Router,
	extract::{
		DefaultBodyLimit, Path, Request, State, WebSocketUpgrade,
		ws::{self, rejection::WebSocketUpgradeRejection},
	},
	http::StatusCode,
	middleware::{self, Next},
	response::{IntoResponse, Response},
	routing::{delete, get, post, put},
};
use clap::{Parser, Subcommand};
use eyre::WrapErr;
use futures::{SinkExt, StreamExt, stream::FuturesUnordered};
use sa_auth::{Keys, Principal, Service};
use sa_browsers::{Launch, Profiles, Running};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite;

/// A film's upload at most.
const FILM_MAX: usize = 512 << 20;

#[derive(Parser)]
struct Cli {
	#[command(subcommand)]
	cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
	Serve {
		#[arg(long, env = "BROWSERS_BIND")]
		bind: SocketAddr,
		/// One session directory per gmail, kept across runs: signed-in Google lives there.
		#[arg(long, env = "BROWSERS_DIR")]
		dir: PathBuf,
		/// Browsers running at once; a launch past it is 503.
		#[arg(long, env = "BROWSERS_MAX")]
		max: usize,
		/// The flake's `orbita`.
		#[arg(long, env = "BROWSERS_ORBITA")]
		orbita: PathBuf,
		/// What makes a film the camera's and mic's files, and a phone's screen a frame.
		#[arg(long, env = "BROWSERS_FFMPEG")]
		ffmpeg: PathBuf,
		/// The flake's `mlx`: the Multilogin app on its own display, whose launcher starts Mimic.
		#[arg(long, env = "BROWSERS_MLX")]
		mlx: PathBuf,
		/// Its launcher, e.g. `https://launcher.mlx.yt:45001` (the name is the loopback's).
		#[arg(long, env = "BROWSERS_MLX_LAUNCHER")]
		launcher: String,
		/// Multilogin's API, e.g. `https://api.multilogin.com`: the phones' starts and stops.
		#[arg(long, env = "BROWSERS_MULTILOGIN_API")]
		multilogin: String,
		#[arg(long, env = "BROWSERS_ADB")]
		adb: PathBuf,
		/// A static aarch64 busybox: a phone's exit read from inside it.
		#[arg(long, env = "BROWSERS_BUSYBOX")]
		busybox: PathBuf,
		/// ADBKeyBoard's apk: a phone's text past ASCII.
		#[arg(long, env = "BROWSERS_ADB_KEYBOARD")]
		adb_keyboard: PathBuf,
		/// The conductor's public keys, as the services take them (`sa_auth::Keys`).
		#[arg(long, env = "PANEL_ASSERTION_KEYS")]
		keys: String,
	},
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
	color_eyre::install()?;
	tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
	let Cmd::Serve {
		bind,
		dir,
		max,
		orbita,
		ffmpeg,
		mlx,
		launcher,
		multilogin,
		adb,
		busybox,
		adb_keyboard,
		keys,
	} = Cli::parse().cmd;
	let keys: Keys = keys.parse().map_err(|e| eyre::eyre!("PANEL_ASSERTION_KEYS: {e}"))?;
	eyre::ensure!(max > 0, "BROWSERS_MAX is at least 1");
	std::fs::create_dir_all(&dir).wrap_err_with(|| format!("creating {}", dir.display()))?;
	let (starts, mut started) = mpsc::channel(16);
	let http = reqwest::Client::builder().timeout(Duration::from_secs(30)).build().wrap_err("the runner's client")?;
	let runner = Runner(Arc::new(Inner {
		dir,
		max,
		orbita,
		ffmpeg,
		launcher: launcher.trim_end_matches('/').to_owned(),
		multilogin: phone::Api {
			base: multilogin.trim_end_matches('/').to_owned(),
			http: http.clone(),
		},
		http,
		adb,
		busybox,
		adb_keyboard,
		minutes: phone::MobileMinutes::of_serve(),
		records: Mutex::default(),
		keys,
		profiles: Mutex::default(),
		starts,
	}));
	phone::reconcile(&runner).await?;
	let mlx_log = std::fs::File::create(runner.0.dir.join("mlx.log")).wrap_err("creating mlx.log")?;
	let mut mlx = tokio::process::Command::new(&mlx)
		.env("MLX_OPENED", runner.0.dir.join("mlx.opened"))
		.stdin(std::process::Stdio::null())
		.stdout(mlx_log.try_clone()?)
		.stderr(mlx_log)
		.process_group(0)
		.spawn()
		.wrap_err_with(|| format!("starting {}", mlx.display()))?;
	// TERM asks "Close Multilogin?" on its display, where nobody answers: its profiles are each
	// stopped through the launcher, then the group killed
	let mlx_group = profile::Group::of(&mlx);
	let listener = tokio::net::TcpListener::bind(bind).await.wrap_err_with(|| format!("binding {bind}"))?;
	tracing::info!(%bind, "serving");
	let (stop, stopping) = watch::channel(false);
	let server = std::future::IntoFuture::into_future(axum::serve(listener, routes(runner.clone())).with_graceful_shutdown(shutdown_signal()));
	tokio::pin!(server);
	let mut running = FuturesUnordered::new();
	//LOOP: until the server stops; each browser is one future here, never spawned
	let served = loop {
		tokio::select! {
			served = &mut server => break served.wrap_err("HTTP server"),
			status = mlx.wait() => break Err(eyre::eyre!("the Multilogin app exited ({}); see {}/mlx.log", status?, runner.0.dir.display())),
			Some(start) = started.recv() => running.push(profile::run(runner.clone(), start, stopping.clone())),
			Some(()) = running.next() => {}
		}
	};
	stop.send(true).expect("the receiver is ours");
	while running.next().await.is_some() {}
	drop(mlx_group);
	served
}

#[derive(Clone)]
struct Runner(Arc<Inner>);
impl Runner {
	/// Returns once `gmail`'s browser has stopped, or at once when none runs.
	async fn stopped(&self, gmail: i64) {
		let ended = self.0.profiles.lock().expect("never held across a panic").get(&gmail).map(profile::Slot::stop);
		if let Some(mut ended) = ended {
			// a dropped sender is the profile's future gone: stopped too
			let _ended = ended.wait_for(|e| *e).await;
		}
	}
}

struct Inner {
	dir: PathBuf,
	max: usize,
	orbita: PathBuf,
	ffmpeg: PathBuf,
	launcher: String,
	multilogin: phone::Api,
	http: reqwest::Client,
	adb: PathBuf,
	busybox: PathBuf,
	adb_keyboard: PathBuf,
	minutes: phone::MobileMinutes,
	/// Held across a read and write of the phones' records.
	records: Mutex<()>,
	keys: Keys,
	profiles: Mutex<HashMap<i64, profile::Slot>>,
	starts: mpsc::Sender<profile::Start>,
}

fn routes(runner: Runner) -> Router {
	Router::new()
		.route("/profiles", get(list))
		.route("/profiles/{gmail}/launch", post(launch))
		.route("/profiles/{gmail}/stop", post(stop))
		.route("/profiles/{gmail}", delete(remove))
		.route("/profiles/{gmail}/cdp", get(cdp))
		.route("/profiles/{gmail}/view", get(view))
		.route("/profiles/{gmail}/film", put(set_film).delete(remove_film).layer(DefaultBodyLimit::max(FILM_MAX)))
		.layer(middleware::from_fn_with_state(runner.clone(), conductor_only))
		.with_state(runner)
}

/// The conductor's assertion for exactly this request, else 401.
async fn conductor_only(State(runner): State<Runner>, req: Request, next: Next) -> Response {
	let token = req.headers().get(sa_auth::HEADER).and_then(|v| v.to_str().ok());
	let now = jiff::Timestamp::now().as_second();
	let verified = token.map(|t| sa_auth::verify(&runner.0.keys, t, Service::Browsers, req.method().as_str(), req.uri().path(), None, now));
	match verified {
		Some(Ok(a)) if a.principal == Principal::Conductor => next.run(req).await,
		refused => {
			tracing::warn!(path = req.uri().path(), ?refused, "refused: not the conductor's");
			(StatusCode::UNAUTHORIZED, "the conductor's assertion only").into_response()
		}
	}
}

async fn list(State(runner): State<Runner>) -> Response {
	let running = {
		let profiles = runner.0.profiles.lock().expect("never held across a panic");
		profiles
			.iter()
			.map(|(gmail, slot)| Running {
				gmail: *gmail,
				run: slot.run.clone(),
				stream: slot.stream(),
			})
			.collect()
	};
	let mut films = Vec::new();
	let mut dirs = match tokio::fs::read_dir(&runner.0.dir).await {
		Ok(d) => d,
		Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("listing {}: {e}", runner.0.dir.display())).into_response(),
	};
	//LOOP: one session directory per gmail
	loop {
		match dirs.next_entry().await {
			Ok(Some(e)) => {
				if let Some(gmail) = e.file_name().to_str().and_then(|n| n.parse::<i64>().ok())
					&& film::present(&e.path())
				{
					films.push(gmail);
				}
			}
			Ok(None) => break,
			Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, format!("listing {}: {e}", runner.0.dir.display())).into_response(),
		}
	}
	Json(Profiles { running, films }).into_response()
}

/// The film the next launch plays; not while it runs.
async fn set_film(State(runner): State<Runner>, Path(gmail): Path<i64>, mp4: axum::body::Bytes) -> Response {
	if runner.0.profiles.lock().expect("never held across a panic").contains_key(&gmail) {
		return (StatusCode::CONFLICT, "it runs: stop it first, the next launch plays the film").into_response();
	}
	match film::set(&runner.0.ffmpeg, &runner.0.dir.join(gmail.to_string()), &mp4).await {
		Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
		Ok(Err(why)) => (StatusCode::BAD_REQUEST, why).into_response(),
		Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
	}
}

async fn remove_film(State(runner): State<Runner>, Path(gmail): Path<i64>) -> Response {
	if runner.0.profiles.lock().expect("never held across a panic").contains_key(&gmail) {
		return (StatusCode::CONFLICT, "it runs: stop it first").into_response();
	}
	match film::remove(&runner.0.dir.join(gmail.to_string())).await {
		Ok(()) => StatusCode::NO_CONTENT.into_response(),
		Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response(),
	}
}

async fn launch(State(runner): State<Runner>, Path(gmail): Path<i64>, Json(launch): Json<Launch>) -> Response {
	let up = {
		let mut profiles = runner.0.profiles.lock().expect("never held across a panic");
		let full = profiles.len() >= runner.0.max;
		match profiles.get_mut(&gmail) {
			Some(slot) if slot.run != launch.run => return (StatusCode::CONFLICT, format!("running for {}: stop it first", slot.run)).into_response(),
			Some(slot) => slot.wait_up(),
			None if full => return (StatusCode::SERVICE_UNAVAILABLE, format!("{} browsers run already", runner.0.max)).into_response(),
			None => {
				let (mut slot, start) = profile::Slot::starting(gmail, launch);
				let up = slot.wait_up();
				profiles.insert(gmail, slot);
				if runner.0.starts.try_send(start).is_err() {
					profiles.remove(&gmail);
					return (StatusCode::SERVICE_UNAVAILABLE, "launches queued: try again").into_response();
				}
				up
			}
		}
	};
	match up.await {
		Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
		Ok(Err(why)) => (StatusCode::BAD_GATEWAY, why).into_response(),
		Err(_) => (StatusCode::BAD_GATEWAY, "it stopped while starting").into_response(),
	}
}

async fn stop(State(runner): State<Runner>, Path(gmail): Path<i64>) -> StatusCode {
	runner.stopped(gmail).await;
	StatusCode::NO_CONTENT
}

async fn remove(State(runner): State<Runner>, Path(gmail): Path<i64>) -> Response {
	runner.stopped(gmail).await;
	let dir = runner.0.dir.join(gmail.to_string());
	match tokio::fs::remove_dir_all(&dir).await {
		Ok(()) => StatusCode::NO_CONTENT.into_response(),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => StatusCode::NO_CONTENT.into_response(),
		Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("removing {}: {e}", dir.display())).into_response(),
	}
}

/// The browser's devtools socket, as is, while it runs; its client counted for the idle stop.
async fn cdp(State(runner): State<Runner>, Path(gmail): Path<i64>, upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>) -> Response {
	let found = runner.0.profiles.lock().expect("never held across a panic").get(&gmail).and_then(profile::Slot::client);
	let (devtools, client) = match found {
		Some((profile::Up::Devtools(d), client)) => (d, client),
		Some((profile::Up::Phone { .. }, _)) => return (StatusCode::CONFLICT, "a cloud phone has no devtools: its viewer is /view").into_response(),
		None => return (StatusCode::NOT_FOUND, "not running").into_response(),
	};
	let upgrade = match upgrade {
		Ok(u) => u,
		Err(rejection) => return rejection.into_response(),
	};
	let (browser, _) = match tokio_tungstenite::connect_async(devtools.as_str()).await {
		Ok(c) => c,
		Err(e) => return (StatusCode::BAD_GATEWAY, format!("its devtools: {e}")).into_response(),
	};
	upgrade.max_message_size(64 << 20).on_upgrade(move |conductor| async move {
		let _client = client;
		pass(conductor, browser).await;
	})
}

/// A cloud phone's viewer, spoken here; its client counted for the idle stop.
async fn view(State(runner): State<Runner>, Path(gmail): Path<i64>, upgrade: Result<WebSocketUpgrade, WebSocketUpgradeRejection>) -> Response {
	let found = runner.0.profiles.lock().expect("never held across a panic").get(&gmail).and_then(profile::Slot::client);
	let (phone, client) = match found {
		Some((profile::Up::Phone { adb, .. }, client)) => (adb, client),
		Some((profile::Up::Devtools(_), _)) => return (StatusCode::CONFLICT, "a browser's viewer is the conductor's relay over /cdp").into_response(),
		None => return (StatusCode::NOT_FOUND, "not running").into_response(),
	};
	let upgrade = match upgrade {
		Ok(u) => u,
		Err(rejection) => return rejection.into_response(),
	};
	upgrade.max_message_size(4096).on_upgrade(move |socket| async move {
		let _client = client;
		adb::view(socket, &phone).await;
	})
}

/// Text both ways until either side closes.
async fn pass(conductor: ws::WebSocket, browser: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>) {
	let (mut to_conductor, mut from_conductor) = conductor.split();
	let (mut to_browser, mut from_browser) = browser.split();
	//LOOP: one frame either way per turn
	loop {
		tokio::select! {
			m = from_conductor.next() => match m {
				Some(Ok(ws::Message::Text(t))) => if to_browser.send(tungstenite::Message::text(t.as_str())).await.is_err() { break },
				Some(Ok(ws::Message::Ping(_) | ws::Message::Pong(_))) => {}
				Some(Ok(ws::Message::Binary(_))) => tracing::warn!("devtools speaks text: a binary frame dropped"),
				Some(Ok(ws::Message::Close(_)) | Err(_)) | None => break,
			},
			m = from_browser.next() => match m {
				Some(Ok(tungstenite::Message::Text(t))) => if to_conductor.send(ws::Message::Text(t.as_str().into())).await.is_err() { break },
				Some(Ok(tungstenite::Message::Close(_)) | Err(_)) | None => break,
				Some(Ok(_)) => {}
			},
		}
	}
	// either side gone: the other is told by its socket closing
	let _closed = to_conductor.send(ws::Message::Close(None)).await;
	let _closed = to_browser.close().await;
}

async fn shutdown_signal() {
	let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a SIGTERM handler");
	tokio::select! {
		r = tokio::signal::ctrl_c() => r.expect("a Ctrl-C handler"),
		_ = term.recv() => {}
	}
	tracing::info!("stopping: every browser with it");
}
