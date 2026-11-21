//! The runner as the conductor drives it, a stand-in in the conductor's place: a session's cells reach
//! the conductor and nothing else, each sees what the cells before it declared, a type error runs
//! nothing, a long cell is answered while it runs, one past its limit takes its session with it, and
//! nobody but the conductor is heard.

use std::{
	path::PathBuf,
	process::Stdio,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
	time::Duration,
};

use axum::{Router, extract::State, routing::get};
use sa_auth::{Assertion, Principal, Service, Signer};
use sa_sandbox::{Ended, OUTPUT, Opened, Ran};
use serde_json::json;

use crate::{SLACK, WAIT};

const SEED: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";
const HELLO: &str = "await fetch(`${Deno.env.get(\"SA_RUN_URL\")}/hello`)";
/// Holds its cell until the test calls [`Runner::release`].
const GATE: &str = "await fetch(`${Deno.env.get(\"SA_RUN_URL\")}/gate`)";

struct Runner {
	base: String,
	signer: Signer,
	http: reqwest::Client,
	heard: Arc<AtomicUsize>,
	gate: Arc<tokio::sync::Semaphore>,
	/// An origin a cell must not reach.
	elsewhere: String,
	_child: tokio::process::Child,
}
impl Runner {
	async fn start(max: usize, limit: Duration) -> Self {
		let heard = Arc::new(AtomicUsize::new(0));
		let gate = Arc::new(tokio::sync::Semaphore::new(0));
		let conductor = listen(heard.clone(), gate.clone()).await;
		let elsewhere = listen(Arc::new(AtomicUsize::new(0)), Arc::new(tokio::sync::Semaphore::new(0))).await;
		let signer: Signer = SEED.parse().unwrap();
		let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sa_sandbox"))
			.arg("serve")
			.env("SANDBOX_BIND", "127.0.0.1:0")
			.env("SANDBOX_MAX", max.to_string())
			.env("SANDBOX_LIMIT_MS", limit.as_millis().to_string())
			.env("SANDBOX_WAIT_MS", WAIT.as_millis().to_string())
			.env("SANDBOX_DENO", deno())
			.env("SANDBOX_JS", crate::js())
			.env("SANDBOX_CONDUCTOR_URL", format!("{conductor}/api/run"))
			.env("PANEL_ASSERTION_KEYS", signer.public())
			.stdout(Stdio::piped())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let mut addr = String::new();
		tokio::io::AsyncBufReadExt::read_line(&mut tokio::io::BufReader::new(child.stdout.take().unwrap()), &mut addr)
			.await
			.unwrap();
		assert!(!addr.is_empty(), "the runner exited before it listened");
		let base = format!("http://{}", addr.trim());
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		Self {
			base,
			signer,
			http: reqwest::Client::new(),
			heard,
			gate,
			elsewhere,
			_child: child,
		}
	}

	fn assertion(&self, method: &str, path: &str) -> String {
		self.signer.sign(&Assertion {
			aud: Service::Sandbox,
			principal: Principal::Conductor,
			method: method.into(),
			path: path.into(),
			exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
		})
	}

	async fn send(&self, method: reqwest::Method, path: &str, body: Option<serde_json::Value>) -> (u16, String) {
		let mut req = self.http.request(method.clone(), format!("{}{path}", self.base));
		req = req.header(sa_auth::HEADER, self.assertion(method.as_str(), path.split('?').next().unwrap()));
		if let Some(b) = body {
			req = req.json(&b);
		}
		let res = req.send().await.unwrap();
		(res.status().as_u16(), res.text().await.unwrap())
	}

	async fn open(&self) -> String {
		let (status, body) = self.send(reqwest::Method::POST, "/sessions", None).await;
		assert_eq!(status, 201, "{body}");
		serde_json::from_str::<Opened>(&body).unwrap().session
	}

	async fn try_cell(&self, session: &str, code: &str) -> (u16, String) {
		self.send(reqwest::Method::POST, &format!("/sessions/{session}/cells"), Some(json!({ "code": code, "token": "the-cell" })))
			.await
	}

	async fn cell(&self, session: &str, code: &str) -> Ran {
		let (status, body) = self.try_cell(session, code).await;
		assert_eq!(status, 200, "{body}");
		serde_json::from_str(&body).unwrap()
	}

	/// What follows `ran`, once `until` holds or the wait passed.
	async fn poll(&self, session: &str, ran: &Ran, until: &str) -> Ran {
		let path = format!("/sessions/{session}/cells/{}?stdout={}&stderr={}&until={until}", ran.cell, ran.seen.stdout, ran.seen.stderr);
		let (status, body) = self.send(reqwest::Method::GET, &path, None).await;
		assert_eq!(status, 200, "{body}");
		serde_json::from_str(&body).unwrap()
	}

	/// Lets one [`GATE`] by.
	fn release(&self) {
		self.gate.add_permits(1);
	}

	/// The cell's answers until it ends, its streams joined.
	async fn end(&self, session: &str, mut ran: Ran) -> Ran {
		while ran.ended.is_none() {
			let more = self.poll(session, &ran, "ended").await;
			ran = Ran {
				stdout: ran.stdout + &more.stdout,
				stderr: ran.stderr + &more.stderr,
				..more
			};
		}
		ran
	}
}

async fn listen(heard: Arc<AtomicUsize>, gate: Arc<tokio::sync::Semaphore>) -> String {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let at = listener.local_addr().unwrap();
	let app = Router::new()
		.route(
			"/api/run/hello",
			get(|State((heard, _)): State<(Arc<AtomicUsize>, Arc<tokio::sync::Semaphore>)>| async move {
				heard.fetch_add(1, Ordering::SeqCst);
				"hello from the conductor"
			}),
		)
		.route(
			"/api/run/gate",
			get(|State((_, gate)): State<(Arc<AtomicUsize>, Arc<tokio::sync::Semaphore>)>| async move {
				gate.acquire().await.expect("never closed").forget();
			}),
		)
		.with_state((heard, gate));
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
	let r = Runner::start(1, SLACK).await;
	let (status, _) = (r.http.post(format!("{}/sessions", r.base)).send().await.unwrap().status().as_u16(), ());
	assert_eq!(status, 401);
	let other = r.signer.sign(&Assertion {
		aud: Service::Browsers,
		principal: Principal::Conductor,
		method: "POST".into(),
		path: "/sessions".into(),
		exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
	});
	let res = r.http.post(format!("{}/sessions", r.base)).header(sa_auth::HEADER, other).send().await.unwrap();
	assert_eq!(res.status().as_u16(), 401, "an assertion for the browsers' runner");
}

#[tokio::test]
async fn a_cell_reaches_the_conductor_and_nothing_else() {
	let r = Runner::start(1, SLACK).await;
	let s = r.open().await;
	let code = format!(
		r#"
console.log(await ({HELLO}).text());
const tries: Record<string, () => Promise<unknown>> = {{
  elsewhere: () => fetch("{elsewhere}/"),
  read: () => Deno.readTextFile("/etc/hostname"),
  env: async () => Deno.env.get("HOME"),
  run: () => new Deno.Command("/bin/sh").output(),
  write: () => Deno.writeTextFile("cell_9.ts", ""),
}};
for (const [what, attempt] of Object.entries(tries)) {{
  try {{ await attempt(); console.log(what, "allowed"); }} catch (e) {{ console.log(what, (e as Error).name); }}
}}
"#,
		elsewhere = r.elsewhere
	);
	let ran = r.end(&s, r.cell(&s, &code).await).await;
	assert_eq!(ran.ended, Some(Ended::Done), "{}", ran.stderr);
	assert_eq!(
		ran.stdout,
		"hello from the conductor\nelsewhere NotCapable\nread NotCapable\nenv NotCapable\nrun NotCapable\nwrite NotCapable\n"
	);
	assert_eq!(r.heard.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_cell_sees_what_the_cells_before_it_declared() {
	let r = Runner::start(1, SLACK).await;
	let s = r.open().await;
	let ran = r
		.end(&s, r.cell(&s, "const n = 41;\ninterface P { x: number }\nfunction twice(v: number) { return v * 2; }").await)
		.await;
	assert_eq!(ran.ended, Some(Ended::Done), "{}", ran.stderr);
	let ran = r.end(&s, r.cell(&s, "const p: P = { x: n + 1 };\nconsole.log(p.x, twice(p.x));").await).await;
	assert_eq!((ran.ended, ran.stdout.as_str()), (Some(Ended::Done), "42 84\n"), "{}", ran.stderr);

	let ran = r.end(&s, r.cell(&s, "const lost = 1;\nthrow new Error(\"on purpose\");").await).await;
	assert_eq!(ran.ended, Some(Ended::Threw));
	assert!(ran.stderr.contains("on purpose"), "{}", ran.stderr);
	let ran = r.cell(&s, &format!("{HELLO};\nconsole.log(lost);")).await;
	assert_eq!(ran.ended, Some(Ended::Rejected), "what a cell that threw declared is not seen");
	assert!(ran.stderr.contains("TS2304"), "{}", ran.stderr);
	assert!(ran.stderr.contains("cell_3.ts:2"), "{}", ran.stderr);
	assert_eq!(r.heard.load(Ordering::SeqCst), 0, "a type error runs nothing");

	let ran = r.end(&s, r.cell(&s, "const n = 1;\nconsole.log(n, twice(n));").await).await;
	assert_eq!(ran.stdout, "1 2\n", "declared again, a name is the later cell's: {}", ran.stderr);
}

#[tokio::test]
async fn a_session_never_sees_another_sessions_declarations() {
	let r = Runner::start(2, SLACK).await;
	let (a, b) = (r.open().await, r.open().await);
	let ran = r
		.end(
			&a,
			r.cell(&a, "declare global { var leak: string }\ninterface Shared { x: number }\nconst s: Shared = { x: 1 };")
				.await,
		)
		.await;
	assert_eq!(ran.ended, Some(Ended::Done), "{}", ran.stderr);
	for code in ["console.log(leak);", "const s: Shared = { x: 1 };\nconsole.log(s);"] {
		let ran = r.cell(&b, code).await;
		assert_eq!(ran.ended, Some(Ended::Rejected), "{code}: {}", ran.stderr);
		assert!(ran.stderr.contains("TS2304"), "{code}: {}", ran.stderr);
	}
}

#[tokio::test]
async fn a_cell_sees_only_what_it_imports() {
	let r = Runner::start(1, SLACK).await;
	let s = r.open().await;
	let ran = r.end(&s, r.cell(&s, "export const g = 1;\ndeclare global { var hidden: string }").await).await;
	assert_eq!(ran.ended, Some(Ended::Done), "{}", ran.stderr);
	let ran = r.end(&s, r.cell(&s, "console.log(g, typeof hidden);").await).await;
	assert_eq!((ran.ended, ran.stdout.as_str()), (Some(Ended::Done), "1 undefined\n"), "{}", ran.stderr);
	let ran = r.cell(&s, "const g = 2;\nconsole.log(g, hidden);").await;
	assert_eq!(ran.ended, Some(Ended::Rejected), "imports nothing of cell 0: {}", ran.stderr);
	assert!(ran.stderr.contains("TS2304"), "{}", ran.stderr);
}

#[tokio::test]
async fn output_is_kept_up_to_its_cap() {
	let r = Runner::start(1, SLACK).await;
	let s = r.open().await;
	let ran = r.end(&s, r.cell(&s, &format!("console.log(\"x\".repeat({}));", OUTPUT * 2)).await).await;
	assert_eq!((ran.ended, ran.stdout.len(), ran.truncated), (Some(Ended::Done), OUTPUT, true));
}

#[tokio::test]
async fn a_long_cell_is_answered_as_it_runs_and_holds_its_session() {
	let r = Runner::start(1, SLACK).await;
	let s = r.open().await;
	let ran = r.cell(&s, &format!("console.log(\"early\");\n{GATE};\nconsole.log(\"late\");")).await;
	assert_eq!(ran.ended, None, "{}", ran.stderr);
	let mid = r.poll(&s, &ran, "more").await;
	assert_eq!((mid.ended, format!("{}{}", ran.stdout, mid.stdout).as_str()), (None, "early\n"), "{}", mid.stderr);
	assert_eq!(r.try_cell(&s, "console.log(1)").await.0, 409, "one cell at a time");
	r.release();
	let ran = r.end(&s, mid).await;
	assert_eq!((ran.ended, ran.stdout.as_str()), (Some(Ended::Done), "early\nlate\n"));
}

#[tokio::test]
async fn a_cell_past_its_limit_takes_its_session() {
	let r = Runner::start(1, WAIT).await; // the limit it runs past
	let s = r.open().await;
	let ran = r.end(&s, r.cell(&s, "const kept = 1;\nsetInterval(() => {}, 1000);\nawait new Promise(() => {});").await).await;
	assert_eq!(ran.ended, Some(Ended::Killed));
	assert_eq!(r.try_cell(&s, "console.log(kept)").await.0, 410, "its state went with it, never made anew");
	let fresh = r.open().await;
	assert_eq!(r.end(&fresh, r.cell(&fresh, "console.log(1)").await).await.stdout, "1\n");
}

#[tokio::test]
async fn the_idlest_session_makes_room_but_a_running_one_does_not() {
	let r = Runner::start(1, SLACK).await;
	let busy = r.open().await;
	let ran = r.cell(&busy, GATE).await;
	assert_eq!(ran.ended, None);
	assert_eq!(r.send(reqwest::Method::POST, "/sessions", None).await.0, 503, "its one slot runs a cell");
	r.release();
	r.end(&busy, ran).await;
	let other = r.open().await;
	assert_eq!(r.try_cell(&busy, "console.log(1)").await.0, 410, "let go for the new one");
	assert_eq!(r.end(&other, r.cell(&other, "console.log(1)").await).await.stdout, "1\n");
}
