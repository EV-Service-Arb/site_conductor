//! `execute` end to end: the in-page assistant asks a scripted Claude, whose code runs in the runner over
//! the generated client (crates/sandbox/js) and comes back through the conductor's `/api/run` as the
//! asker; an MCP client's the same. A chat's session keeps what its cells declared; a call the level
//! holds pauses the cell until the person decides; a session ends with its caller's.

use std::{
	process::Stdio,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{Json, Router, http::Method, routing::post};
use jiff::Timestamp;
use sa_panel::{
	Panel,
	import::Export,
	testing::{TestDb, operator, panel},
};
use sa_server::{
	claude::{Model, Origins},
	concierge::{Concierge, DevIdentity},
	execute::Sandbox,
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, Mcp, ORIGIN, SEED, mcp_grant, tool_result},
};
use serde_json::{Value, json};
use uuid::Uuid;

const EMAIL: &str = "asker@x.com";

const ESCAPES: &str = r#"const at = Deno.env.get("SA_RUN_URL")!;
const out = await fetch(`${at}/auth/logout`, { method: "POST" });
console.log(out.status, out.headers.has("x-sa-run-refused"));"#;

/// Spends, then removes: what each level holds of it.
const SPENDS_THEN_REMOVES: &str = r#"const spent = await api.POST("/api/v1/map/load", { body: { account: "nope" } });
console.log("spent", spent.error?.code ?? "ok");
if (spent.error?.code === "refused") throw new Error(spent.error.error);
const gone = await api.DELETE("/api/v1/trades/{id}", { params: { path: { id: 7 } } });
console.log("removed", gone.error?.code ?? "ok");"#;

const REMOVES: &str = r#"const gone = await api.DELETE("/api/v1/trades/{id}", { params: { path: { id: 7 } } });
console.log("removed", gone.error?.code ?? "ok");
if (gone.error) console.error(gone.error.error);"#;

/// A Claude API that answers each request with the next of `script`, and keeps what it was sent.
async fn claude(script: Vec<Value>) -> (String, Arc<Mutex<Vec<Value>>>) {
	let sent = Arc::new(Mutex::new(Vec::new()));
	let script = Arc::new(Mutex::new(script.into_iter()));
	let seen = sent.clone();
	let app = Router::new().route(
		"/v1/messages",
		post(move |Json(req): Json<Value>| {
			let (seen, script) = (seen.clone(), script.clone());
			async move {
				seen.lock().unwrap().push(req);
				let mut reply = script.lock().unwrap().next().expect("the script ran out");
				reply["model"] = json!("claude-opus-5-5");
				reply["usage"] = json!({ "input_tokens": 100, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 900, "output_tokens": 40 });
				Json(reply)
			}
		}),
	);
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(format!("http://{addr}"), sent)
}

fn tool(id: &str, name: &str, input: Value) -> Value {
	json!({ "type": "tool_use", "id": id, "name": name, "input": input })
}

fn execute(id: &str, code: &str) -> Value {
	tool(id, "execute", json!({ "code": code }))
}

fn round(uses: Vec<Value>) -> Value {
	json!({ "stop_reason": "tool_use", "content": uses })
}

fn end(text: &str) -> Value {
	json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": text }] })
}

/// The runner, sending back to `conductor`, answering within a second.
async fn runner(conductor: &str) -> (String, tokio::process::Child) {
	let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
	let deno = std::env::split_paths(&std::env::var_os("PATH").unwrap())
		.map(|d| d.join("deno"))
		.find(|p| p.is_file())
		.expect("deno on PATH: the dev shell has it");
	let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sa_sandbox"))
		.arg("serve")
		.env("SANDBOX_BIND", format!("127.0.0.1:{port}"))
		.env("SANDBOX_MAX", "4")
		.env("SANDBOX_WAIT_SECS", "1")
		.env("SANDBOX_DENO", deno)
		.env("SANDBOX_JS", crate::js())
		.env("SANDBOX_CONDUCTOR_URL", format!("{conductor}/api/run"))
		.env("PANEL_ASSERTION_KEYS", SEED.parse::<sa_auth::Signer>().unwrap().public())
		.stdout(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.unwrap();
	let base = format!("http://{}", format_args!("127.0.0.1:{port}"));
	for _ in 0..100 {
		if reqwest::get(format!("{base}/health")).await.is_ok() {
			return (base, child);
		}
		tokio::time::sleep(Duration::from_millis(50)).await;
	}
	panic!("the runner never listened");
}

/// The conductor with a runner behind it, served (a cell's calls come back over the net), its asker
/// signed in with a chat and their own Claude, trades open.
struct Stack {
	app: Router,
	b: Browser,
	chat: Uuid,
	sent: Arc<Mutex<Vec<Value>>>,
	_db: TestDb,
	_runner: tokio::process::Child,
}

impl Stack {
	async fn new(script: Vec<Value>) -> Self {
		let db = TestDb::create().await;
		let p: Panel = panel(&db).await;
		let (model, sent) = claude(script).await;
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let conductor = format!("http://{}", listener.local_addr().unwrap());
		let (sandbox, runner) = runner(&conductor).await;
		let who = DevIdentity {
			user_id: DevIdentity::named(EMAIL),
			permissions: operator(),
			email: EMAIL.to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let app = http::app(
			SignIn::new(p.clone(), Concierge::dev(who), config),
			None,
			None,
			Default::default(),
			None,
			None,
			Some(Model::new(Origins { api: model.clone(), oauth: model }, Some("owner".to_owned().into()))),
			Some(Sandbox::try_new(&sandbox, SEED.parse().unwrap()).unwrap()),
		);
		let served = app.clone();
		tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
		p.import_review_archive(Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		})
		.await
		.unwrap()
		.unwrap();
		let mut b = Browser::default();
		b.sign_in(&app).await;
		let chat = Uuid::now_v7();
		let made = json!({ "id": chat, "title": null, "turns": [] });
		assert_eq!(b.call(&app, Method::POST, "/api/v1/assistant/chats", Some(made)).await.0, 201);
		let year = Timestamp::now() + jiff::SignedDuration::from_hours(24 * 365);
		p.set_claude(DevIdentity::named(EMAIL), "theirs", year, Timestamp::now()).await.unwrap();
		Self {
			app,
			b,
			chat,
			sent,
			_db: db,
			_runner: runner,
		}
	}

	/// Asks at `approval` after the chat's turns: the answer.
	async fn ask(&mut self, approval: &str) -> Value {
		let (_, chat) = self.b.call(&self.app, Method::GET, &format!("/api/v1/assistant/chats/{}", self.chat), None).await;
		let from = chat["turns"].as_array().unwrap().len();
		let question = json!({ "from": from, "text": "go on", "mentions": [], "files": [], "url": "/", "bindings": [], "approval": approval });
		let (status, answer) = self.b.call(&self.app, Method::POST, &format!("/api/v1/assistant/chats/{}/ask", self.chat), Some(question)).await;
		assert_eq!(status, 200, "{answer}");
		answer["turns"][from + 1].clone()
	}

	/// What tool use `i` of the model's `n`th round was answered, parsed.
	fn result(&self, n: usize, i: usize) -> Value {
		let sent = self.sent.lock().unwrap();
		let results = sent[n]["messages"].as_array().unwrap().last().unwrap()["content"].as_array().unwrap().clone();
		serde_json::from_str(results[i]["content"].as_str().unwrap()).unwrap()
	}

	/// The operations an answer's actions name, and their states.
	fn actions(said: &Value) -> Vec<(String, String)> {
		said["actions"]
			.as_array()
			.unwrap()
			.iter()
			.map(|a| (a["target"]["call"]["operation"].as_str().unwrap().to_owned(), a["state"].as_str().unwrap().to_owned()))
			.collect()
	}
}

fn pairs(of: &[(&str, &str)]) -> Vec<(String, String)> {
	of.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

#[tokio::test]
async fn code_reaches_only_operations_as_the_asker() {
	let mut s = Stack::new(vec![
		round(vec![
			execute("e1", "const me = await api.GET(\"/api/v1/me\");\nconsole.log(me.data!.email);"),
			execute("e2", ESCAPES),
		]),
		end("Done."),
	])
	.await;
	s.ask("ask").await;
	assert!(
		s.sent.lock().unwrap()[0]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "poll"),
		"offered with a sandbox"
	);
	let read = s.result(1, 0);
	assert_eq!((&read["state"], &read["stdout"]), (&json!({ "ended": "done" }), &json!("asker@x.com\n")), "{read}");
	let escaped = s.result(1, 1);
	assert_eq!(escaped["stdout"], "403 true\n", "nothing but a live cell's token, and an operation, is let by: {escaped}");
	let (status, _) = s.b.call(&s.app, Method::GET, "/api/v1/me", None).await;
	assert_eq!(status, 200, "the asker's session outlived the cell");
}

/// The level holds in code what it holds in a call: a held call pauses the cell, a new question
/// declines it and the code sees that, and what runs on its own is an action already done.
#[tokio::test]
async fn the_level_holds_in_code_what_it_holds_in_a_call() {
	let mut s = Stack::new(vec![
		round(vec![execute("e1", SPENDS_THEN_REMOVES)]),
		end("Agree to the map?"),
		round(vec![tool("p1", "poll", json!({ "cell": 0 })), execute("e2", SPENDS_THEN_REMOVES)]),
		end("Agree to the removal?"),
		round(vec![tool("p2", "poll", json!({ "cell": 1 })), execute("e3", SPENDS_THEN_REMOVES)]),
		end("Done."),
	])
	.await;
	let asked = s.ask("ask").await;
	assert_eq!(Stack::actions(&asked), pairs(&[("buy_map", "proposed")]));
	let paused = s.result(1, 0);
	assert_eq!(paused["state"]["paused"]["operation"], "buy_map", "{paused}");

	let spent = s.ask("spend").await;
	let declined = s.result(3, 0);
	assert_eq!(declined["state"], json!({ "ended": "threw" }), "a new question declines what waited: {declined}");
	assert!(declined["stderr"].as_str().unwrap().contains("declined"), "{declined}");
	let paused = s.result(3, 1);
	assert_eq!(paused["state"]["paused"]["operation"], "delete_trade", "{paused}");
	assert!(paused["stdout"].as_str().unwrap().starts_with("spent "), "spending ran: {paused}");
	assert_eq!(Stack::actions(&spent), pairs(&[("buy_map", "done"), ("delete_trade", "proposed")]));

	let acted = s.ask("act").await;
	let ran = s.result(5, 1);
	assert_eq!(ran["state"], json!({ "ended": "done" }), "{ran}");
	assert!(ran["stdout"].as_str().unwrap().contains("removed "), "{ran}");
	assert_eq!(Stack::actions(&acted), pairs(&[("buy_map", "done"), ("delete_trade", "done")]));
}

/// A chat's session keeps what its cells declared; a name none declared runs nothing; a long cell is
/// answered by its number and `poll` follows it to its end.
#[tokio::test]
async fn a_session_keeps_what_its_cells_declared() {
	let mut s = Stack::new(vec![
		round(vec![
			execute("e1", "const me = (await api.GET(\"/api/v1/me\")).data!;"),
			execute("e2", "console.log(me.email);"),
			execute("e3", "console.log(nobody);"),
			execute("e4", "await new Promise((r) => setTimeout(r, 1500));\nconsole.log(\"late\");"),
		]),
		round(vec![tool("p1", "poll", json!({ "cell": 3 })), tool("p2", "poll", json!({ "cell": 3 }))]),
		end("Done."),
	])
	.await;
	s.ask("ask").await;
	assert_eq!(s.result(1, 1)["stdout"], "asker@x.com\n", "{}", s.result(1, 1));
	let undeclared = s.result(1, 2);
	assert_eq!(undeclared["state"], json!({ "ended": "rejected" }));
	assert!(undeclared["stderr"].as_str().unwrap().contains("TS2304"), "{undeclared}");
	let long = s.result(1, 3);
	assert_eq!((&long["cell"], &long["state"]), (&json!(3), &json!("running")), "{long}");
	let (first, second) = (s.result(2, 0), s.result(2, 1));
	assert_eq!(format!("{}{}", first["stdout"].as_str().unwrap(), second["stdout"].as_str().unwrap()), "late\n");
	assert_eq!(second["state"], json!({ "ended": "done" }), "{second}");
}

/// Signing out ends a running cell, its session with it: the next cell is told its state is gone.
#[tokio::test]
async fn signing_out_ends_the_session() {
	let mut s = Stack::new(vec![
		round(vec![execute(
			"e1",
			"const kept = 1;\nfor (;;) { await api.GET(\"/api/v1/me\"); await new Promise((r) => setTimeout(r, 100)); }",
		)]),
		end("It runs."),
		round(vec![execute("e2", "console.log(kept);")]),
		end("Gone."),
	])
	.await;
	s.ask("ask").await;
	assert_eq!(s.result(1, 0)["state"], "running");
	let (status, _) = s.b.call(&s.app, Method::POST, "/auth/logout", None).await;
	assert_eq!(status, 204);
	tokio::time::sleep(Duration::from_millis(500)).await;
	s.b.sign_in(&s.app).await;
	s.ask("ask").await;
	let gone = s.result(3, 0);
	assert!(gone["error"].as_str().unwrap().contains("no longer holds this session"), "{gone}");
}

/// A held call waits in its cell: the person's agreeing from the answer runs it there, and the answer
/// resumes with what the code did next.
#[tokio::test]
async fn a_paused_cell_goes_on_once_the_person_agrees() {
	let mut s = Stack::new(vec![round(vec![execute("e1", REMOVES)]), end("Agree to remove it?"), end("It is removed.")]).await;
	let said = s.ask("ask").await;
	assert_eq!(Stack::actions(&said), pairs(&[("delete_trade", "proposed")]));
	let act = json!({ "turn": said["id"], "action": said["actions"][0]["id"] });
	let (status, chat) = s.b.call(&s.app, Method::POST, &format!("/api/v1/assistant/chats/{}/act", s.chat), Some(act)).await;
	assert_eq!(status, 200, "{chat}");
	let turns = chat["turns"].as_array().unwrap();
	assert_eq!(turns[1]["actions"][0]["state"], "done");
	assert_eq!(turns.last().unwrap()["text"], "It is removed.", "the answer resumed");
	let sent = s.sent.lock().unwrap();
	let resumed = sent[2]["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_owned();
	assert!(resumed.contains("<resumed>"), "the call ran in the code: {resumed}");
	assert!(resumed.contains("removed not_found"), "the call ran in the code: {resumed}");
}

/// An MCP client that elicits is asked through its stream, and the cell goes on with its user's yes;
/// one that cannot sees the held call refused at once, its proposal in the error.
#[tokio::test]
async fn an_mcp_cell_waits_for_the_users_word() {
	let s = Stack::new(vec![]).await;
	let (bearer, _, _) = mcp_grant(&s.app, "ask").await;
	let (_, asks) = Mcp::initialize(&s.app, &bearer, json!({ "elicitation": {} })).await;
	let mut events = asks.tool_streamed(&s.app, "execute", json!({ "code": REMOVES })).await;
	let asked = events.next().await.unwrap();
	assert_eq!(asked["method"], "elicitation/create", "{asked}");
	assert_eq!(asks.answer(&s.app, &asked["id"], json!({ "action": "accept", "content": { "approve": true } })).await, 202);
	let (ran, _) = tool_result(&events.next().await.unwrap());
	assert_eq!((&ran["state"], &ran["stdout"]), (&json!({ "ended": "done" }), &json!("removed not_found\n")), "{ran}");

	let (_, cannot) = Mcp::initialize(&s.app, &bearer, json!({})).await;
	let (refused, _) = cannot.tool(&s.app, "execute", json!({ "code": REMOVES })).await;
	assert_eq!(refused["stdout"], "removed refused\n", "{refused}");
	assert!(refused["stderr"].as_str().unwrap().contains("\"confirmed\": true"), "{refused}");
}
