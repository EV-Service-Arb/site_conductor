//! `sa_sandbox serve`: the agents' code, in sessions. A session is one Deno process (`js/host.ts`) with
//! no permission but the net to the conductor's `/api/run` and reading its own cells; each cell is a
//! module type-checked first, against the catalog and the declarations of the cells that ran before it,
//! which it imports. Nothing here holds a secret of the conductor's: a cell brings its own token, and
//! an assertion is all the runner trusts.

use std::{
	collections::{BTreeMap, HashMap},
	net::SocketAddr,
	os::unix::process::ExitStatusExt,
	path::{Path, PathBuf},
	process::Stdio,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use axum::{
	Json, Router,
	extract::{Path as Segments, Query, Request, State},
	http::StatusCode,
	middleware::{self, Next},
	response::{IntoResponse, Response},
	routing::{delete, get, post},
};
use clap::{Parser, Subcommand};
use eyre::WrapErr;
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use sa_auth::{Keys, Principal, Service};
use sa_sandbox::{CHECK, Cell, Ended, IDLE, LIMIT, OUTPUT, Opened, Poll, Ran, Seen, Until, WAIT};
use serde::Deserialize;
use tokio::{
	io::{AsyncReadExt, AsyncWriteExt},
	process::{Child, ChildStdin},
	sync::{Notify, mpsc},
};
use url::Url;

/// The V8 heap a session's code may grow to.
const HEAP_MB: usize = 256;

#[derive(Parser)]
struct Cli {
	#[command(subcommand)]
	cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
	Serve {
		#[arg(long, env = "SANDBOX_BIND")]
		bind: SocketAddr,
		/// Sessions held at once; past it the idlest is let go, or 503 when every one runs a cell.
		#[arg(long, env = "SANDBOX_MAX")]
		max: usize,
		#[arg(long, env = "SANDBOX_DENO")]
		deno: PathBuf,
		/// `crates/sandbox/js`, its node_modules installed: the host, the prelude, the catalog and its types.
		#[arg(long, env = "SANDBOX_JS")]
		js: PathBuf,
		/// The conductor's `/api/run`, e.g. `http://panel:59120/api/run`: the one origin a cell may reach.
		#[arg(long, env = "SANDBOX_CONDUCTOR_URL")]
		conductor: Url,
		/// The conductor's public keys, as the services take them (`sa_auth::Keys`).
		#[arg(long, env = "PANEL_ASSERTION_KEYS")]
		keys: String,
		/// How long a cell lives before its session is killed; the conductor tells the agent `LIMIT`.
		#[arg(long, env = "SANDBOX_LIMIT_SECS", default_value_t = LIMIT.as_secs())]
		limit_secs: u64,
		/// How long an answer waits on a cell; the conductor's client waits past `WAIT`.
		#[arg(long, env = "SANDBOX_WAIT_SECS", default_value_t = WAIT.as_secs())]
		wait_secs: u64,
	},
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
	color_eyre::install()?;
	tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
	let Cmd::Serve {
		bind,
		max,
		deno,
		js,
		conductor,
		keys,
		limit_secs,
		wait_secs,
	} = Cli::parse().cmd;
	let keys: Keys = keys.parse().map_err(|e| eyre::eyre!("PANEL_ASSERTION_KEYS: {e}"))?;
	eyre::ensure!(max > 0, "SANDBOX_MAX is at least 1");
	eyre::ensure!(wait_secs <= WAIT.as_secs(), "SANDBOX_WAIT_SECS past WAIT outlasts the conductor's client");
	eyre::ensure!(conductor.scheme() == "http", "SANDBOX_CONDUCTOR_URL is the conductor's in-cluster http origin, not {conductor}");
	let host = conductor.host_str().ok_or_else(|| eyre::eyre!("SANDBOX_CONDUCTOR_URL names no host"))?;
	let port = conductor.port_or_known_default().expect("http has a known port");
	for file in ["host.ts", "names.ts", "prelude.ts", "deno.json"] {
		eyre::ensure!(js.join(file).is_file(), "SANDBOX_JS has no {file}: {}", js.display());
	}
	let js = js.canonicalize().wrap_err("SANDBOX_JS")?;
	let prelude = Url::from_file_path(js.join("prelude.ts")).expect("canonical paths are absolute");
	let cache = std::env::temp_dir().join("sa_sandbox-deno");
	std::fs::create_dir_all(&cache).wrap_err_with(|| format!("creating {}", cache.display()))?;
	let (supervise, watched) = mpsc::channel(max);
	let runner = Runner(Arc::new(Inner {
		max,
		deno,
		js,
		prelude,
		net: format!("{host}:{port}"),
		origin: conductor.as_str().trim_end_matches('/').to_owned(),
		cache,
		keys,
		limit: Duration::from_secs(limit_secs),
		wait: Duration::from_secs(wait_secs),
		sessions: Mutex::default(),
		supervise,
	}));
	let listener = tokio::net::TcpListener::bind(bind).await.wrap_err_with(|| format!("binding {bind}"))?;
	tracing::info!(%bind, net = runner.0.net, "serving");
	let served = axum::serve(listener, routes(runner)).with_graceful_shutdown(shutdown_signal());
	// the sessions' processes are this future's: they end with the server, killed on drop
	tokio::select! {
		served = served => served.wrap_err("HTTP server"),
		() = watch(watched) => unreachable!("the runner holds the sender for as long as the server runs"),
	}
}

/// Drives every session's process until it ends.
async fn watch(mut watched: mpsc::Receiver<BoxFuture<'static, ()>>) {
	let mut sessions = FuturesUnordered::new();
	//LOOP: the runner's whole life; it ends with the server
	loop {
		tokio::select! {
			Some(session) = watched.recv() => sessions.push(session),
			Some(()) = sessions.next(), if !sessions.is_empty() => {}
		}
	}
}

#[derive(Clone)]
struct Runner(Arc<Inner>);
impl Runner {
	fn deno(&self, dir: &Path) -> tokio::process::Command {
		let r = &self.0;
		let mut cmd = tokio::process::Command::new(&r.deno);
		cmd.env_clear()
			.env("DENO_DIR", &r.cache)
			.env("DENO_NO_UPDATE_CHECK", "1")
			.env("NO_COLOR", "1")
			.current_dir(dir)
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.kill_on_drop(true);
		cmd
	}

	/// Lets go of what idled past [`IDLE`], and of the idlest when the slots are full.
	fn make_room(&self) -> Result<(), Refused> {
		let sessions = self.0.sessions.lock().expect("never held across a panic");
		let idle: Vec<(Instant, Arc<Session>)> = sessions
			.values()
			.filter_map(|s| {
				let st = s.state.lock().expect("never held across a panic");
				st.busy.is_none().then(|| (st.used, s.clone()))
			})
			.collect();
		let full = sessions.len() >= self.0.max;
		drop(sessions);
		for (used, s) in &idle {
			if used.elapsed() > IDLE {
				self.end(s);
			}
		}
		if full && self.0.sessions.lock().expect("never held across a panic").len() >= self.0.max {
			let idlest = idle.iter().filter(|(used, _)| used.elapsed() <= IDLE).min_by_key(|(used, _)| *used).ok_or(Refused::Full)?;
			self.end(&idlest.1);
		}
		Ok(())
	}

	/// No longer held: its watcher kills it.
	fn end(&self, s: &Session) {
		self.0.sessions.lock().expect("never held across a panic").remove(&s.id);
		s.kill.notify_one();
	}

	async fn open(&self) -> Result<Opened, Refused> {
		self.make_room()?;
		let r = &self.0;
		let dir = tempfile::tempdir().wrap_err("a session's directory")?;
		let mut child = self
			.deno(dir.path())
			.args(["run", "--no-prompt", "--no-remote", "--cached-only", "--allow-env=SA_RUN_URL"])
			.arg("--config")
			.arg(r.js.join("deno.json"))
			.arg(format!("--allow-net={}", r.net))
			.arg(format!("--allow-read={}", dir.path().display()))
			.arg(format!("--v8-flags=--max-old-space-size={HEAP_MB}"))
			.arg(r.js.join("host.ts"))
			.env("SA_RUN_URL", &r.origin)
			.spawn()
			.wrap_err_with(|| format!("starting {}", r.deno.display()))?;
		let nonce = token();
		let mut stdin = child.stdin.take().expect("piped");
		stdin.write_all(format!("{nonce}\n").as_bytes()).await.wrap_err("a session's nonce")?;
		let id = token();
		let s = Arc::new(Session {
			id: id.clone(),
			dir,
			stdin: tokio::sync::Mutex::new(stdin),
			state: Mutex::new(SessionState {
				cells: Vec::new(),
				names: BTreeMap::new(),
				busy: None,
				deadline: None,
				word: None,
				gone: None,
				used: Instant::now(),
			}),
			changed: Notify::new(),
			kill: Notify::new(),
		});
		r.sessions.lock().expect("never held across a panic").insert(id.clone(), s.clone());
		r.supervise
			.send(supervise(self.clone(), s, child, format!("\0SA:{nonce}:")).boxed())
			.await
			.map_err(|_| eyre::eyre!("the watcher lives as long as the server"))?;
		Ok(Opened { session: id })
	}

	fn session(&self, id: &str) -> Result<Arc<Session>, Refused> {
		let s = self.0.sessions.lock().expect("never held across a panic").get(id).cloned().ok_or(Refused::Gone)?;
		s.state.lock().expect("never held across a panic").used = Instant::now();
		Ok(s)
	}

	async fn cell(&self, id: &str, cell: Cell) -> Result<Ran, Refused> {
		let s = self.session(id)?;
		let (n, names) = {
			let mut st = s.state.lock().expect("never held across a panic");
			if let Some(busy) = st.busy {
				return Err(Refused::Busy(busy));
			}
			let n = st.cells.len();
			st.busy = Some(n);
			st.cells.push(CellState::default());
			(n, st.names.clone())
		};
		// dropped before the cell starts (its request gone), the session takes cells again
		let undo = Unstarted(&s);
		let checked = self.check(&s, n, &cell.code, &names).await;
		std::mem::forget(undo);
		match checked {
			Ok(Ok(declares)) => {
				{
					let mut st = s.state.lock().expect("never held across a panic");
					st.cells[n].declares = declares;
					st.deadline = Some(tokio::time::Instant::now() + self.0.limit);
				}
				let url = Url::from_file_path(s.dir.path().join(format!("cell_{n}.ts"))).expect("a temp dir is absolute");
				let line = serde_json::json!({ "run": url.as_str(), "token": cell.token }).to_string();
				let wrote = s.stdin.lock().await.write_all(format!("{line}\n").as_bytes()).await;
				if let Err(e) = wrote {
					// the process ended: its watcher tells the cell so
					tracing::warn!("a session's stdin: {e}");
				}
				s.changed.notify_waiters(); // the watcher learns of the deadline
			}
			Ok(Err(why)) => {
				let mut st = s.state.lock().expect("never held across a panic");
				st.cells[n].stderr.add(why.as_bytes());
				st.cells[n].ended = Some(Ended::Rejected);
				st.busy = None;
			}
			Err(e) => {
				let mut st = s.state.lock().expect("never held across a panic");
				st.cells.pop();
				st.busy = None;
				return Err(e.into());
			}
		}
		Ok(self.answer(&s, n, Seen::default(), Until::Now).await)
	}

	/// What `code` declares, once it type-checks as cell `n` importing `names`; else what the check said.
	async fn check(&self, s: &Session, n: usize, code: &str, names: &BTreeMap<String, (usize, bool)>) -> eyre::Result<Result<Vec<Name>, String>> {
		let r = &self.0;
		let mut reading = self
			.deno(&r.js)
			.args(["run", "--no-prompt", "--no-remote", "--cached-only", "--allow-env"])
			.arg("--config")
			.arg(r.js.join("deno.json"))
			.arg(r.js.join("names.ts"))
			.spawn()
			.wrap_err("starting names.ts")?;
		let mut stdin = reading.stdin.take().expect("piped");
		stdin.write_all(code.as_bytes()).await.wrap_err("handing names.ts the code")?;
		drop(stdin);
		let read = tokio::time::timeout(CHECK, reading.wait_with_output())
			.await
			.wrap_err("names.ts past CHECK")?
			.wrap_err("names.ts")?;
		eyre::ensure!(read.status.success(), "names.ts: {}", String::from_utf8_lossy(&read.stderr));
		let mut declares: Vec<Name> = serde_json::from_slice(&read.stdout).wrap_err("names.ts's answer")?;
		declares.dedup_by(|a, b| a.name == b.name);
		let mut from: BTreeMap<usize, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
		for (name, (cell, is_type)) in names {
			if !declares.iter().any(|d| &d.name == name) {
				let (values, types) = from.entry(*cell).or_default();
				if *is_type { types } else { values }.push(name);
			}
		}
		// after the code, so its line numbers stay its own: imports hoist
		let mut module = format!("{code}\n");
		for (cell, (values, types)) in &from {
			for (names, kind) in [(values, ""), (types, "type ")] {
				if !names.is_empty() {
					module.push_str(&format!("import {kind}{{ {} }} from \"./cell_{cell}.ts\";\n", names.join(", ")));
				}
			}
		}
		for (is_type, kind) in [(false, ""), (true, "type ")] {
			let own: Vec<&str> = declares.iter().filter(|d| d.is_type == is_type && !d.exported).map(|d| d.name.as_str()).collect();
			if !own.is_empty() {
				module.push_str(&format!("export {kind}{{ {} }};\n", own.join(", ")));
			}
		}
		module.push_str(&format!("import {{ api }} from \"{}\";\n", r.prelude));
		let file = format!("cell_{n}.ts");
		std::fs::write(s.dir.path().join(&file), module).wrap_err("writing a cell")?;
		let checked = self
			.deno(s.dir.path())
			.args(["check", "--no-remote", "--cached-only"])
			.arg("--config")
			.arg(r.js.join("deno.json"))
			.arg(&file)
			.stdin(Stdio::null())
			.output();
		let checked = tokio::time::timeout(CHECK, checked).await.wrap_err("deno check past CHECK")?.wrap_err("deno check")?;
		if checked.status.success() {
			return Ok(Ok(declares));
		}
		let at = format!("{}/", Url::from_directory_path(s.dir.path()).expect("a temp dir is absolute").as_str().trim_end_matches('/'));
		let said = [checked.stdout, checked.stderr].map(|b| String::from_utf8_lossy(&b).replace(&at, "")).concat();
		Ok(Err(said))
	}

	/// Cell `n` past `seen`, once `until` holds or the wait passed.
	async fn answer(&self, s: &Session, n: usize, seen: Seen, until_: Until) -> Ran {
		let until = tokio::time::Instant::now() + self.0.wait;
		loop {
			let changed = s.changed.notified();
			tokio::pin!(changed);
			changed.as_mut().enable();
			{
				let st = s.state.lock().expect("never held across a panic");
				let c = &st.cells[n];
				let said = c.stdout.bytes.len() > seen.stdout || c.stderr.bytes.len() > seen.stderr;
				if c.ended.is_some() || until_ == Until::Now || (until_ == Until::More && said) || tokio::time::Instant::now() >= until {
					let ended = c.ended.is_some();
					let (stdout, out) = c.stdout.since(seen.stdout, ended);
					let (stderr, err) = c.stderr.since(seen.stderr, ended);
					return Ran {
						cell: n,
						ended: c.ended,
						stdout,
						stderr,
						truncated: c.stdout.cut || c.stderr.cut,
						seen: Seen { stdout: out, stderr: err },
					};
				}
			}
			tokio::select! {
				() = changed => {}
				() = tokio::time::sleep_until(until) => {}
			}
		}
	}

	async fn poll(&self, id: &str, n: usize, poll: Poll) -> Result<Ran, Refused> {
		let s = self.session(id)?;
		if n >= s.state.lock().expect("never held across a panic").cells.len() {
			return Err(Refused::NoCell(n));
		}
		Ok(self
			.answer(
				&s,
				n,
				Seen {
					stdout: poll.stdout,
					stderr: poll.stderr,
				},
				poll.until,
			)
			.await)
	}
}

struct Inner {
	max: usize,
	deno: PathBuf,
	/// `SANDBOX_JS`, canonical.
	js: PathBuf,
	prelude: Url,
	/// `--allow-net`'s one `host:port`.
	net: String,
	/// `SA_RUN_URL`: where the prelude's client sends.
	origin: String,
	/// `DENO_DIR`, shared: the code cannot write, only Deno's own caches do.
	cache: PathBuf,
	keys: Keys,
	limit: Duration,
	wait: Duration,
	sessions: Mutex<HashMap<String, Arc<Session>>>,
	supervise: mpsc::Sender<BoxFuture<'static, ()>>,
}

struct Session {
	id: String,
	/// The cells' modules, and nothing else the session may read.
	dir: tempfile::TempDir,
	stdin: tokio::sync::Mutex<ChildStdin>,
	state: Mutex<SessionState>,
	/// A cell said more, or ended.
	changed: Notify,
	/// Asks its watcher to kill it.
	kill: Notify,
}

struct SessionState {
	cells: Vec<CellState>,
	/// Each name a cell that ran to its end declared: the latest such cell, and whether it is a type.
	names: BTreeMap<String, (usize, bool)>,
	/// A cell being checked or running; none other starts until it ends.
	busy: Option<usize>,
	/// When the running cell is killed.
	deadline: Option<tokio::time::Instant>,
	/// The running cell's end, as the first stream to reach the host's word told it.
	word: Option<Ended>,
	/// Why it ended; then it is no longer held.
	gone: Option<Ended>,
	used: Instant,
}

#[derive(Default)]
struct CellState {
	stdout: Kept,
	stderr: Kept,
	ended: Option<Ended>,
	declares: Vec<Name>,
}

/// A top-level declaration of a cell, as `js/names.ts` reads it.
#[derive(Clone, Deserialize)]
struct Name {
	name: String,
	#[serde(rename = "type")]
	is_type: bool,
	exported: bool,
}

/// Why a request about a session was refused.
enum Refused {
	Gone,
	Busy(usize),
	NoCell(usize),
	Full,
	Failed(eyre::Report),
}

impl IntoResponse for Refused {
	fn into_response(self) -> Response {
		match self {
			Self::Gone => (StatusCode::GONE, "the runner holds no such session: its state is gone").into_response(),
			Self::Busy(cell) => (StatusCode::CONFLICT, format!("cell {cell} still runs: poll it to its end first")).into_response(),
			Self::NoCell(cell) => (StatusCode::NOT_FOUND, format!("the session has no cell {cell}")).into_response(),
			Self::Full => (StatusCode::SERVICE_UNAVAILABLE, "every session's slot runs a cell: ask again shortly").into_response(),
			Self::Failed(e) => {
				tracing::error!("a session: {e:?}");
				(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response()
			}
		}
	}
}

impl From<eyre::Report> for Refused {
	fn from(e: eyre::Report) -> Self {
		Self::Failed(e)
	}
}

/// A session's process, until it ends: its output sorted to its cells, its own word read, and killed at
/// a cell's deadline or when let go.
async fn supervise(runner: Runner, s: Arc<Session>, mut child: Child, marker: String) {
	let (mut stdout, mut stderr) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
	let (mut out_buf, mut err_buf) = ([0u8; 8192], [0u8; 8192]);
	let (mut out_open, mut err_open) = (true, true);
	let (mut pending, mut pending_err): (Vec<u8>, Vec<u8>) = (Vec::new(), Vec::new());
	//LOOP: the session's whole life; it breaks when its process ends or is killed
	let ended = loop {
		let changed = s.changed.notified();
		tokio::pin!(changed);
		changed.as_mut().enable(); // before the deadline is read: a cell started after it wakes this
		let deadline = s.state.lock().expect("never held across a panic").deadline;
		tokio::select! {
			n = stdout.read(&mut out_buf), if out_open => match n {
				Ok(0) | Err(_) => out_open = false,
				Ok(n) => {
					pending.extend_from_slice(&out_buf[..n]);
					sort(&s, &mut pending, &marker, |c| &mut c.stdout);
				}
			},
			n = stderr.read(&mut err_buf), if err_open => match n {
				Ok(0) | Err(_) => err_open = false,
				Ok(n) => {
					pending_err.extend_from_slice(&err_buf[..n]);
					sort(&s, &mut pending_err, &marker, |c| &mut c.stderr);
				}
			},
			() = async { tokio::time::sleep_until(deadline.expect("guarded")).await }, if deadline.is_some() => break Ended::Killed,
			() = s.kill.notified() => break Ended::Killed,
			() = changed, if out_open || err_open => {}
			status = child.wait(), if !out_open && !err_open => break match status {
				Ok(status) => match status.code() {
					Some(code) => Ended::Exited(code),
					None => Ended::Signalled(status.signal().expect("no exit code: ended by a signal")),
				},
				Err(e) => {
					tracing::error!("a session's process: {e}");
					Ended::Signalled(0)
				}
			},
		}
	};
	if ended == Ended::Killed
		&& let Err(e) = child.kill().await
	{
		tracing::error!("killing a session: {e}");
	}
	if let Some(c) = s.state.lock().expect("never held across a panic").cells.last_mut() {
		c.stdout.add(&pending);
		c.stderr.add(&pending_err);
	}
	runner.0.sessions.lock().expect("never held across a panic").remove(&s.id);
	let mut st = s.state.lock().expect("never held across a panic");
	st.gone = Some(ended);
	if let Some(busy) = st.busy.take() {
		st.cells[busy].ended.get_or_insert(ended);
	}
	drop(st);
	s.changed.notify_waiters();
}

/// Hands what the session printed on one stream to its cell, up to its own word; the cell ends on the word's second
/// half, so neither stream holds any of its output then.
fn sort(s: &Session, pending: &mut Vec<u8>, marker: &str, stream: fn(&mut CellState) -> &mut Kept) {
	let marker = marker.as_bytes();
	let mut st = s.state.lock().expect("never held across a panic");
	//LOOP: once per marker read, then once more for what follows the last
	loop {
		let at = pending.windows(marker.len()).position(|w| w == marker);
		// what may be a marker's start waits for the rest
		let printed = at.unwrap_or_else(|| pending.iter().rposition(|b| *b == 0).filter(|&p| marker.starts_with(&pending[p..])).unwrap_or(pending.len()));
		if let Some(c) = st.cells.last_mut() {
			stream(c).add(&pending[..printed]);
		}
		pending.drain(..printed);
		let Some(_) = at else { break };
		let Some(end) = pending.iter().position(|b| *b == b'\n') else { break };
		let word: serde_json::Value = serde_json::from_slice(&pending[marker.len()..end]).expect("the host says JSON");
		pending.drain(..=end);
		let ended = match word["ended"].as_str() {
			Some("done") => Ended::Done,
			Some("threw") => Ended::Threw,
			other => panic!("the host ended a cell as {other:?}"),
		};
		let Some(heard) = st.word.replace(ended) else { continue };
		assert_eq!(heard, ended, "the host says one word on both streams");
		st.word = None;
		let n = st.busy.take().expect("the host ends only the cell it was given");
		st.deadline = None;
		st.cells[n].ended = Some(ended);
		if ended == Ended::Done {
			for d in st.cells[n].declares.clone() {
				st.names.insert(d.name, (n, d.is_type));
			}
		}
	}
	drop(st);
	s.changed.notify_waiters();
}

/// A cell checked but not started: undone when its request is dropped.
struct Unstarted<'a>(&'a Session);

impl Drop for Unstarted<'_> {
	fn drop(&mut self) {
		let mut st = self.0.state.lock().expect("never held across a panic");
		st.cells.pop();
		st.busy = None;
	}
}

/// 256 random bits, as hex.
fn token() -> String {
	let mut bytes = [0; 32];
	getrandom::fill(&mut bytes).expect("the OS has randomness");
	hex::encode(bytes)
}

fn routes(runner: Runner) -> Router {
	Router::new()
		.route("/sessions", post(open))
		.route("/sessions/{session}", delete(close))
		.route("/sessions/{session}/cells", post(cell))
		.route("/sessions/{session}/cells/{cell}", get(poll))
		.layer(middleware::from_fn_with_state(runner.clone(), conductor_only))
		.route("/health", get(|| async { StatusCode::OK }))
		.with_state(runner)
}

/// The conductor's assertion for exactly this request, else 401.
async fn conductor_only(State(runner): State<Runner>, req: Request, next: Next) -> Response {
	let token = req.headers().get(sa_auth::HEADER).and_then(|v| v.to_str().ok());
	let now = jiff::Timestamp::now().as_second();
	let verified = token.map(|t| {
		sa_auth::verify(
			&runner.0.keys,
			t,
			Service::Sandbox,
			sa_auth::Request {
				method: req.method().as_str(),
				path: req.uri().path(),
				body: None,
			},
			now,
		)
	});
	match verified {
		Some(Ok(a)) if a.principal == Principal::Conductor => next.run(req).await,
		refused => {
			tracing::warn!(path = req.uri().path(), ?refused, "refused: not the conductor's");
			(StatusCode::UNAUTHORIZED, "the conductor's assertion only").into_response()
		}
	}
}

async fn open(State(runner): State<Runner>) -> Result<(StatusCode, Json<Opened>), Refused> {
	Ok((StatusCode::CREATED, Json(runner.open().await?)))
}

async fn close(State(runner): State<Runner>, Segments(session): Segments<String>) -> Result<StatusCode, Refused> {
	let s = runner.session(&session)?;
	runner.end(&s);
	Ok(StatusCode::NO_CONTENT)
}

async fn cell(State(runner): State<Runner>, Segments(session): Segments<String>, Json(cell): Json<Cell>) -> Result<Json<Ran>, Refused> {
	Ok(Json(runner.cell(&session, cell).await?))
}

async fn poll(State(runner): State<Runner>, Segments((session, cell)): Segments<(String, usize)>, Query(p): Query<Poll>) -> Result<Json<Ran>, Refused> {
	Ok(Json(runner.poll(&session, cell, p).await?))
}

/// A stream's first [`OUTPUT`] bytes; the rest let go, so the code never blocks on a full pipe.
#[derive(Default)]
struct Kept {
	bytes: Vec<u8>,
	cut: bool,
}

impl Kept {
	fn add(&mut self, more: &[u8]) {
		let room = OUTPUT - self.bytes.len();
		self.bytes.extend_from_slice(&more[..more.len().min(room)]);
		self.cut |= more.len() > room;
	}

	/// What follows `from`, to the last whole character unless `ended`, and where that leaves it.
	fn since(&self, from: usize, ended: bool) -> (String, usize) {
		let rest = &self.bytes[from.min(self.bytes.len())..];
		let whole = match std::str::from_utf8(rest) {
			Ok(_) => rest.len(),
			Err(e) if ended || e.error_len().is_some() => rest.len(), // Deno writes UTF-8: only a cut may split a character
			Err(e) => e.valid_up_to(),
		};
		(String::from_utf8_lossy(&rest[..whole]).into_owned(), from.min(self.bytes.len()) + whole)
	}
}

async fn shutdown_signal() {
	let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a SIGTERM handler");
	tokio::select! {
		r = tokio::signal::ctrl_c() => r.expect("a Ctrl-C handler"),
		_ = term.recv() => {}
	}
	tracing::info!("stopping: the sessions end with the runner");
}
