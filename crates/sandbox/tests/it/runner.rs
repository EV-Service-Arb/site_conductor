//! The runner as the conductor drives it, a stand-in in the conductor's place and a stand-in
//! prelude: a run reaches the conductor and nothing else, a type error runs nothing, a run past its
//! limit is killed, and nobody but the conductor is heard.

use std::{
	path::PathBuf,
	process::Stdio,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};
use tokio::sync::Notify;

use axum::{Router, extract::State, routing::get};
use sa_auth::{Assertion, Principal, Service, Signer};
use sa_sandbox::{Ended, OUTPUT, Ran};
use serde_json::json;

const LIMIT: Duration = Duration::from_secs(3); // the runner's, set below: the shipped 60 s would hold the suite
const SEED: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// What the stand-in conductor was sent.
struct Heard {
	requests: AtomicUsize,
	/// A permit per request: awaited after one, it returns at once.
	request: Notify,
}

struct Runner {
	base: String,
	signer: Signer,
	http: reqwest::Client,
	heard: Arc<Heard>,
	/// An origin a run must not reach.
	elsewhere: String,
	_js: tempfile::TempDir,
	_child: tokio::process::Child,
}
impl Runner {
	async fn start(max: usize) -> Self {
		let heard = Arc::new(Heard { requests: AtomicUsize::new(0), request: Notify::new() });
		let conductor = listen(heard.clone()).await;
		let elsewhere = listen(Arc::new(Heard { requests: AtomicUsize::new(0), request: Notify::new() })).await;
		let js = tempfile::tempdir().unwrap();
		std::fs::write(js.path().join("prelude.ts"), "export const api = { url: Deno.env.get(\"SA_RUN_URL\")! };\n").unwrap();
		std::fs::write(js.path().join("deno.json"), "{}\n").unwrap();
		let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
		let signer: Signer = SEED.parse().unwrap();
		let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sa_sandbox"))
			.arg("serve")
			.env("SANDBOX_BIND", format!("127.0.0.1:{port}"))
			.env("SANDBOX_MAX", max.to_string())
			.env("SANDBOX_LIMIT_SECS", LIMIT.as_secs().to_string())
			.env("SANDBOX_DENO", deno())
			.env("SANDBOX_JS", js.path())
			.env("SANDBOX_CONDUCTOR_URL", format!("{conductor}/api/run"))
			.env("PANEL_ASSERTION_KEYS", signer.public())
			.stdout(Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let base = format!("http://127.0.0.1:{port}");
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		let r = Self {
			base,
			signer,
			http: reqwest::Client::new(),
			heard,
			elsewhere,
			_js: js,
			_child: child,
		};
		for _ in 0..100 {
			if r.http.get(format!("{}/health", r.base)).send().await.is_ok() {
				return r;
			}
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
		panic!("the runner never listened");
	}

	async fn post(&self, code: &str, signed: bool) -> (u16, String) {
		let mut req = self.http.post(format!("{}/runs", self.base)).json(&json!({ "code": code, "token": "the-run" }));
		if signed {
			req = req.header(
				sa_auth::HEADER,
				self.signer.sign(&Assertion {
					aud: Service::Sandbox,
					principal: Principal::Conductor,
					method: "POST".into(),
					path: "/runs".into(),
					exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
				}),
			);
		}
		let res = req.send().await.unwrap();
		(res.status().as_u16(), res.text().await.unwrap())
	}

	async fn run(&self, code: &str) -> Ran {
		let (status, body) = self.post(code, true).await;
		assert_eq!(status, 200, "{body}");
		serde_json::from_str(&body).unwrap()
	}
}

async fn listen(heard: Arc<Heard>) -> String {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let at = listener.local_addr().unwrap();
	let app = Router::new()
		.route(
			"/api/run/hello",
			get(|State(heard): State<Arc<Heard>>| async move {
				heard.requests.fetch_add(1, Ordering::SeqCst);
				heard.request.notify_one();
				"hello from the conductor"
			}),
		)
		.with_state(heard);
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	format!("http://{at}")
}

fn deno() -> PathBuf {
	std::env::split_paths(&std::env::var_os("PATH").unwrap())
		.map(|d| d.join("deno"))
		.find(|p| p.is_file())
		.expect("deno on PATH: the dev shell has it")
}

#[tokio::test]
async fn only_the_conductor_is_heard() {
	let r = Runner::start(1).await;
	assert_eq!(r.post("console.log(1)", false).await.0, 401);
	let other = r.signer.sign(&Assertion {
		aud: Service::Browsers,
		principal: Principal::Conductor,
		method: "POST".into(),
		path: "/runs".into(),
		exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
	});
	let res = r
		.http
		.post(format!("{}/runs", r.base))
		.header(sa_auth::HEADER, other)
		.json(&json!({ "code": "", "token": "" }))
		.send()
		.await
		.unwrap();
	assert_eq!(res.status().as_u16(), 401, "an assertion for the browsers' runner");
}

#[tokio::test]
async fn a_run_reaches_the_conductor_and_nothing_else() {
	let r = Runner::start(1).await;
	let code = format!(
		r#"
console.log(await (await fetch(`${{api.url}}/hello`)).text());
const tries: Record<string, () => Promise<unknown>> = {{
  elsewhere: () => fetch("{elsewhere}/"),
  read: () => Deno.readTextFile("/etc/hostname"),
  env: async () => Deno.env.get("HOME"),
  run: () => new Deno.Command("/bin/sh").output(),
}};
for (const [what, attempt] of Object.entries(tries)) {{
  try {{ await attempt(); console.log(what, "allowed"); }} catch (e) {{ console.log(what, (e as Error).name); }}
}}
"#,
		elsewhere = r.elsewhere
	);
	let ran = r.run(&code).await;
	assert_eq!(ran.ended, Ended::Exited(0), "{}", ran.stderr);
	assert_eq!(ran.stdout, "hello from the conductor\nelsewhere NotCapable\nread NotCapable\nenv NotCapable\nrun NotCapable\n");
	assert_eq!(r.heard.requests.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_type_error_runs_nothing() {
	let r = Runner::start(1).await;
	let ran = r.run("await fetch(`${api.url}/hello`);\nconst n: number = \"seven\";\n").await;
	assert_eq!(ran.ended, Ended::Exited(1));
	assert!(ran.stderr.contains("TS2322"), "{}", ran.stderr);
	assert!(ran.stderr.contains("main.ts:2"), "{}", ran.stderr);
	assert_eq!(r.heard.requests.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn output_is_kept_up_to_its_cap() {
	let r = Runner::start(1).await;
	let ran = r.run(&format!("console.log(\"x\".repeat({}));", OUTPUT * 2)).await;
	assert_eq!((ran.ended, ran.stdout.len(), ran.truncated), (Ended::Exited(0), OUTPUT, true));
}

#[tokio::test]
async fn a_run_past_its_limit_is_killed_and_holds_its_slot_until_then() {
	let r = Runner::start(1).await;
	let endless = r.run("await fetch(`${api.url}/hello`);\nsetInterval(() => {}, 1000);\n");
	let busy = async {
		r.heard.request.notified().await; // the run is up, so the one slot is its
		r.post("console.log(1)", true).await.0
	};
	let (ran, busy) = tokio::join!(endless, busy);
	assert_eq!(busy, 503, "the one slot is the endless run's");
	assert_eq!(ran.ended, Ended::Killed);
	assert_eq!(r.run("console.log(1)").await.stdout, "1\n");
}
