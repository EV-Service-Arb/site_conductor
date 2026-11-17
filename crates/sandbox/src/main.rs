//! `sa_sandbox serve`: the agents' code, each run one Deno process with no permission but the net to
//! the conductor's `/api/run`. Nothing here holds a secret of the conductor's: a run brings its own
//! token, and an assertion is all the runner trusts.

use std::{net::SocketAddr, os::unix::process::ExitStatusExt, path::PathBuf, process::Stdio, sync::Arc};

use axum::{
	Json, Router,
	extract::{Request, State},
	http::StatusCode,
	middleware::{self, Next},
	response::{IntoResponse, Response},
	routing::{get, post},
};
use clap::{Parser, Subcommand};
use eyre::WrapErr;
use sa_auth::{Keys, Principal, Service};
use sa_sandbox::{Ended, LIMIT, OUTPUT, Ran, Run};
use tokio::{
	io::{AsyncRead, AsyncReadExt},
	sync::Semaphore,
};
use url::Url;

/// The V8 heap a run's code may grow to.
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
		/// Runs at once; a run past it is 503.
		#[arg(long, env = "SANDBOX_MAX")]
		max: usize,
		#[arg(long, env = "SANDBOX_DENO")]
		deno: PathBuf,
		/// `crates/sandbox/js`, its node_modules installed: the prelude, the catalog and its types.
		#[arg(long, env = "SANDBOX_JS")]
		js: PathBuf,
		/// The conductor's `/api/run`, e.g. `http://panel:59120/api/run`: the one origin a run may reach.
		#[arg(long, env = "SANDBOX_CONDUCTOR_URL")]
		conductor: Url,
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
		max,
		deno,
		js,
		conductor,
		keys,
	} = Cli::parse().cmd;
	let keys: Keys = keys.parse().map_err(|e| eyre::eyre!("PANEL_ASSERTION_KEYS: {e}"))?;
	eyre::ensure!(max > 0, "SANDBOX_MAX is at least 1");
	eyre::ensure!(conductor.scheme() == "http", "SANDBOX_CONDUCTOR_URL is the conductor's in-cluster http origin, not {conductor}");
	let host = conductor.host_str().ok_or_else(|| eyre::eyre!("SANDBOX_CONDUCTOR_URL names no host"))?;
	let port = conductor.port_or_known_default().expect("http has a known port");
	for file in ["prelude.ts", "deno.json"] {
		eyre::ensure!(js.join(file).is_file(), "SANDBOX_JS has no {file}: {}", js.display());
	}
	let prelude = Url::from_file_path(js.join("prelude.ts").canonicalize().wrap_err("SANDBOX_JS")?).expect("canonical paths are absolute");
	let cache = std::env::temp_dir().join("sa_sandbox-deno");
	std::fs::create_dir_all(&cache).wrap_err_with(|| format!("creating {}", cache.display()))?;
	let runner = Runner(Arc::new(Inner {
		slots: Semaphore::new(max),
		deno,
		config: js.join("deno.json"),
		prelude,
		net: format!("{host}:{port}"),
		origin: conductor.as_str().trim_end_matches('/').to_owned(),
		cache,
		keys,
	}));
	let listener = tokio::net::TcpListener::bind(bind).await.wrap_err_with(|| format!("binding {bind}"))?;
	tracing::info!(%bind, net = runner.0.net, "serving");
	axum::serve(listener, routes(runner)).with_graceful_shutdown(shutdown_signal()).await.wrap_err("HTTP server")
}

#[derive(Clone)]
struct Runner(Arc<Inner>);
impl Runner {
	async fn run(&self, run: Run) -> eyre::Result<Ran> {
		let r = &self.0;
		let dir = tempfile::tempdir().wrap_err("a run's directory")?;
		let main = dir.path().join("main.ts");
		// imports hoist: put last, the code's line numbers stay its own
		std::fs::write(&main, format!("{}\nimport {{ api }} from \"{}\";\n", run.code, r.prelude)).wrap_err("writing main.ts")?;
		let mut child = tokio::process::Command::new(&r.deno)
			.args(["run", "--check", "--no-prompt", "--no-remote", "--cached-only", "--allow-env=SA_RUN_URL,SA_RUN_TOKEN"])
			.arg("--config")
			.arg(&r.config)
			.arg(format!("--allow-net={}", r.net))
			.arg(format!("--v8-flags=--max-old-space-size={HEAP_MB}"))
			.arg(&main)
			.env_clear()
			.env("DENO_DIR", &r.cache)
			.env("DENO_NO_UPDATE_CHECK", "1")
			.env("NO_COLOR", "1")
			.env("SA_RUN_URL", &r.origin)
			.env("SA_RUN_TOKEN", &run.token)
			.current_dir(dir.path())
			.stdin(Stdio::null())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.kill_on_drop(true)
			.spawn()
			.wrap_err_with(|| format!("starting {}", r.deno.display()))?;
		let (mut stdout, mut stderr) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
		let (mut out, mut err) = (Kept::default(), Kept::default());
		let finished = tokio::time::timeout(LIMIT, async {
			tokio::try_join!(out.drain(&mut stdout), err.drain(&mut stderr))?;
			child.wait().await
		})
		.await;
		let ended = match finished {
			Ok(status) => {
				let status = status.wrap_err("a run's process")?;
				match status.code() {
					Some(code) => Ended::Exited(code),
					None => Ended::Signalled(status.signal().expect("no exit code: ended by a signal")),
				}
			}
			Err(_) => {
				child.kill().await.wrap_err("killing a run past its limit")?;
				Ended::Killed
			}
		};
		Ok(Ran {
			ended,
			truncated: out.cut || err.cut,
			stdout: out.text(),
			stderr: err.text(),
		})
	}
}

struct Inner {
	slots: Semaphore,
	deno: PathBuf,
	config: PathBuf,
	prelude: Url,
	/// `--allow-net`'s one `host:port`.
	net: String,
	/// `SA_RUN_URL`: where the prelude's client sends.
	origin: String,
	/// `DENO_DIR`, shared by the runs: the code cannot write, only Deno's own caches do.
	cache: PathBuf,
	keys: Keys,
}

fn routes(runner: Runner) -> Router {
	Router::new()
		.route("/runs", post(run))
		.layer(middleware::from_fn_with_state(runner.clone(), conductor_only))
		.route("/health", get(|| async { StatusCode::OK }))
		.with_state(runner)
}

/// The conductor's assertion for exactly this request, else 401.
async fn conductor_only(State(runner): State<Runner>, req: Request, next: Next) -> Response {
	let token = req.headers().get(sa_auth::HEADER).and_then(|v| v.to_str().ok());
	let now = jiff::Timestamp::now().as_second();
	let verified = token.map(|t| sa_auth::verify(&runner.0.keys, t, Service::Sandbox, req.method().as_str(), req.uri().path(), None, now));
	match verified {
		Some(Ok(a)) if a.principal == Principal::Conductor => next.run(req).await,
		refused => {
			tracing::warn!(path = req.uri().path(), ?refused, "refused: not the conductor's");
			(StatusCode::UNAUTHORIZED, "the conductor's assertion only").into_response()
		}
	}
}

async fn run(State(runner): State<Runner>, Json(run): Json<Run>) -> Response {
	let Ok(_slot) = runner.0.slots.try_acquire() else {
		return (StatusCode::SERVICE_UNAVAILABLE, "every run's slot is taken: ask again shortly").into_response();
	};
	match runner.run(run).await {
		Ok(ran) => Json(ran).into_response(),
		Err(e) => {
			tracing::error!("a run: {e:?}");
			(StatusCode::INTERNAL_SERVER_ERROR, format!("{e:#}")).into_response()
		}
	}
}

/// A stream's first [`OUTPUT`] bytes; the rest read and let go, so the code never blocks on a full pipe.
#[derive(Default)]
struct Kept {
	bytes: Vec<u8>,
	cut: bool,
}

impl Kept {
	async fn drain(&mut self, mut from: impl AsyncRead + Unpin) -> std::io::Result<()> {
		let mut buf = [0; 8192];
		loop {
			let n = from.read(&mut buf).await?;
			if n == 0 {
				return Ok(());
			}
			let room = OUTPUT - self.bytes.len();
			self.bytes.extend_from_slice(&buf[..n.min(room)]);
			self.cut |= n > room;
		}
	}

	fn text(self) -> String {
		String::from_utf8_lossy(&self.bytes).into_owned() // Deno writes UTF-8: only a cut may split a character
	}
}

async fn shutdown_signal() {
	let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("a SIGTERM handler");
	tokio::select! {
		r = tokio::signal::ctrl_c() => r.expect("a Ctrl-C handler"),
		_ = term.recv() => {}
	}
	tracing::info!("stopping once the runs in flight end");
}
