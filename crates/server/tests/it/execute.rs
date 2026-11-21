//! `execute` end to end: the in-page assistant asks a scripted Claude, whose Starlark runs in a sealed worker
//! and calls back up its pipe as the asker; an MCP client's the same. A session keeps what its Done cells
//! bound; a call the level holds pauses the cell until the person decides; a session ends with its
//! caller's.

use std::sync::{Arc, Mutex};

use axum::{Json, Router, http::Method, routing::post};
use jiff::Timestamp;
use sa_panel::{
	Panel,
	import::Export,
	testing::{TestDb, operator, panel},
};
use sa_sandbox::{MAX, OUTPUT, WAIT};
use sa_server::{
	claude::{Model, Origins},
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, Mcp, ORIGIN, mcp_grant, sandbox, tool_result},
};
use serde_json::{Value, json};
use uuid::Uuid;

const EMAIL: &str = "asker@x.com";

/// Spends, then removes: what each level holds of it.
const SPENDS_THEN_REMOVES: &str = r#"spent = call("buy_map", body = {"account": "nope"})
print("spent", spent["body"].get("code", "ok") if type(spent["body"]) == "dict" else "ok")
if spent["status"] == 403 and spent["body"]["code"] == "refused":
    fail(spent["body"]["error"])
gone = call("delete_category", path = {"trade": "gcid:plumber", "country": "FR"})
print("removed", gone["body"]["code"])"#;

const REMOVES: &str = r#"gone = call("delete_category", path = {"trade": "gcid:plumber", "country": "FR"})
print("removed", gone["body"]["code"])
if gone["status"] == 403:
    fail(gone["body"]["error"])"#;

/// Busy past one [`WAIT`] and more on any machine the tests run on.
const LONG: &str = "n = 0\nfor i in range(400000000):\n    n += 1\nprint(\"late\")";

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

/// The conductor, its asker signed in with a chat and their own Claude, GBPs open.
struct Stack {
	app: Router,
	b: Browser,
	chat: Uuid,
	sent: Arc<Mutex<Vec<Value>>>,
	_db: TestDb,
}

impl Stack {
	async fn new(script: Vec<Value>) -> Self {
		let db = TestDb::create().await;
		let p: Panel = panel(&db).await;
		let (model, sent) = claude(script).await;
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
			sandbox(),
			None,
		);
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
		let mut s = Self {
			app,
			b,
			chat: Uuid::nil(),
			sent,
			_db: db,
		};
		s.chat = s.new_chat().await;
		let year = Timestamp::now() + jiff::SignedDuration::from_hours(24 * 365);
		p.set_claude(DevIdentity::named(EMAIL), "theirs", year, Timestamp::now()).await.unwrap();
		s
	}

	async fn new_chat(&mut self) -> Uuid {
		let chat = Uuid::now_v7();
		let made = json!({ "id": chat, "title": null, "turns": [] });
		assert_eq!(self.b.call(&self.app, Method::POST, "/api/v1/assistant/chats", Some(made)).await.0, 201);
		chat
	}

	/// Asks in `chat` at `approval` after its turns: the answer.
	async fn ask_in(&mut self, chat: Uuid, approval: &str) -> Value {
		let (_, read) = self.b.call(&self.app, Method::GET, &format!("/api/v1/assistant/chats/{chat}"), None).await;
		let from = read["turns"].as_array().unwrap().len();
		let question = json!({ "from": from, "text": "go on", "mentions": [], "files": [], "url": "/", "bindings": [], "approval": approval });
		let (status, answer) = self.b.call(&self.app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), Some(question)).await;
		assert_eq!(status, 200, "{answer}");
		answer["turns"][from + 1].clone()
	}

	async fn ask(&mut self, approval: &str) -> Value {
		self.ask_in(self.chat, approval).await
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

	/// An MCP client of the asker's at `approval`, declaring `capabilities`.
	async fn mcp(&self, approval: &str, capabilities: Value) -> Mcp {
		let (bearer, _, _) = mcp_grant(&self.app, approval).await;
		Mcp::initialize(&self.app, &bearer, capabilities).await.1
	}
}

fn pairs(of: &[(&str, &str)]) -> Vec<(String, String)> {
	of.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect()
}

/// `code` as a cell of `client`'s session: its state, what it printed, and why it failed.
async fn run(s: &Stack, client: &Mcp, code: &str) -> (Value, String, String) {
	let (told, _) = client.tool(&s.app, "execute", json!({ "code": code })).await;
	(
		told["state"].clone(),
		told["stdout"].as_str().unwrap_or_default().to_owned(),
		told["stderr"].as_str().unwrap_or_default().to_owned(),
	)
}

fn done() -> Value {
	json!({ "ended": "done" })
}

fn threw() -> Value {
	json!({ "ended": "threw" })
}

#[tokio::test]
async fn code_reaches_only_operations_as_the_asker() {
	let mut s = Stack::new(vec![
		round(vec![
			execute("e1", "print(call(\"me\")[\"body\"][\"email\"])"),
			execute("e2", "out = call(\"logout\")\nprint(out[\"status\"], out[\"body\"][\"code\"])"),
		]),
		end("Done."),
	])
	.await;
	s.ask("ask").await;
	assert!(s.sent.lock().unwrap()[0]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "execute"), "always offered");
	let read = s.result(1, 0);
	assert_eq!((&read["state"], &read["stdout"]), (&done(), &json!("asker@x.com\n")), "{read}");
	let escaped = s.result(1, 1);
	assert_eq!(escaped["stdout"], "403 refused\n", "nothing but an operation of the catalog is reached: {escaped}");
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
	assert_eq!(declined["state"], threw(), "a new question declines what waited: {declined}");
	assert!(declined["stderr"].as_str().unwrap().contains("declined"), "{declined}");
	let paused = s.result(3, 1);
	assert_eq!(paused["state"]["paused"]["operation"], "delete_category", "{paused}");
	assert!(paused["stdout"].as_str().unwrap().starts_with("spent "), "spending ran: {paused}");
	assert_eq!(Stack::actions(&spent), pairs(&[("buy_map", "done"), ("delete_category", "proposed")]));

	let acted = s.ask("act").await;
	let ran = s.result(5, 1);
	assert_eq!(ran["state"], done(), "{ran}");
	assert!(ran["stdout"].as_str().unwrap().contains("removed "), "{ran}");
	assert_eq!(Stack::actions(&acted), pairs(&[("buy_map", "done"), ("delete_category", "done")]));
}

/// A later cell reads what Done cells bound, frozen; one that failed hands on nothing; a name bound again
/// is the later cell's.
#[tokio::test]
async fn a_session_keeps_what_done_cells_bound() {
	let s = Stack::new(vec![]).await;
	let client = s.mcp("ask", json!({})).await;
	assert_eq!(run(&s, &client, "me = call(\"me\")[\"body\"]").await.0, done());
	assert_eq!(run(&s, &client, "print(me[\"email\"])").await.1, "asker@x.com\n");
	let (state, _, why) = run(&s, &client, "half = 1\nfail(\"no\")").await;
	assert_eq!(state, threw());
	assert!(why.contains("no"), "{why}");
	let (state, _, why) = run(&s, &client, "print(half)").await;
	assert_eq!(state, threw());
	assert!(why.contains("`half` not found"), "a failed cell hands on nothing: {why}");
	let (state, _, why) = run(&s, &client, "me[\"email\"] = \"else\"").await;
	assert_eq!(state, threw());
	assert!(why.contains("Immutable"), "what is handed on is frozen: {why}");
	assert_eq!(run(&s, &client, "me = 7").await.0, done());
	assert_eq!(run(&s, &client, "print(me)").await.1, "7\n", "a name bound again is the later cell's");
}

#[tokio::test]
async fn sessions_never_see_each_others_names() {
	let s = Stack::new(vec![]).await;
	let (one, other) = (s.mcp("ask", json!({})).await, s.mcp("ask", json!({})).await);
	assert_eq!(run(&s, &one, "mine = 1").await.0, done());
	let (state, _, why) = run(&s, &other, "print(mine)").await;
	assert_eq!(state, threw());
	assert!(why.contains("`mine` not found"), "{why}");
	assert_eq!(run(&s, &one, "print(mine)").await.1, "1\n");
}

#[tokio::test]
async fn output_is_kept_up_to_its_cap() {
	let s = Stack::new(vec![]).await;
	let client = s.mcp("ask", json!({})).await;
	let lines = OUTPUT.div_ceil(100) + 10;
	let (told, _) = client
		.tool(&s.app, "execute", json!({ "code": format!("for i in range({lines}):\n    print(\"x\" * 99)") }))
		.await;
	assert_eq!(told["state"], done());
	assert_eq!(told["stdout"].as_str().unwrap().len(), OUTPUT);
	assert_eq!(told["truncated"], true);
}

/// A long cell is answered by its number at [`WAIT`], and `poll` follows it to its end.
#[tokio::test]
async fn a_long_cell_is_answered_at_wait_and_followed_by_poll() {
	let s = Stack::new(vec![]).await;
	let client = s.mcp("ask", json!({})).await;
	let started = std::time::Instant::now();
	let (first, _) = client.tool(&s.app, "execute", json!({ "code": LONG })).await;
	assert!(started.elapsed() >= WAIT, "answered at WAIT");
	assert_eq!((&first["cell"], &first["state"]), (&json!(0), &json!("running")), "{first}");
	let mut printed = String::new();
	for _ in 0..30 {
		let (next, _) = client.tool(&s.app, "poll", json!({ "cell": 0 })).await;
		printed.push_str(next["stdout"].as_str().unwrap());
		if next["state"] != "running" {
			assert_eq!(next["state"], done(), "{next}");
			assert_eq!(printed, "late\n");
			return;
		}
	}
	panic!("the cell never ended");
}

/// A call made with credentials signed out since ends the session: its cell is told so, and the next
/// execute that its state is gone.
#[tokio::test]
async fn signing_out_ends_the_session() {
	let mut s = Stack::new(vec![
		round(vec![execute("e1", &format!("kept = 1\n{REMOVES}"))]),
		end("Agree to remove it?"),
		end("It ended."),
		round(vec![execute("e2", "print(kept)")]),
		end("Gone."),
	])
	.await;
	let said = s.ask("ask").await;
	let (status, _) = s.b.call(&s.app, Method::POST, "/auth/logout", None).await;
	assert_eq!(status, 204);
	s.b.sign_in(&s.app).await;
	let act = json!({ "turn": said["id"], "action": said["actions"][0]["id"] });
	let (status, chat) = s.b.call(&s.app, Method::POST, &format!("/api/v1/assistant/chats/{}/act", s.chat), Some(act)).await;
	assert_eq!(status, 200, "{chat}");
	let resumed = s.sent.lock().unwrap()[2]["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().to_owned();
	assert!(resumed.contains("signed_out"), "the held call ran as the signed-out session: {resumed}");
	s.ask("ask").await;
	let gone = s.result(4, 0);
	assert!(gone["error"].as_str().unwrap().contains("no longer holds this session"), "{gone}");
}

/// A held call waits in its cell: the person's agreeing from the answer runs it there, and the answer
/// resumes with what the code did next.
#[tokio::test]
async fn a_paused_cell_goes_on_once_the_person_agrees() {
	let mut s = Stack::new(vec![round(vec![execute("e1", REMOVES)]), end("Agree to remove it?"), end("It is removed.")]).await;
	let said = s.ask("ask").await;
	assert_eq!(Stack::actions(&said), pairs(&[("delete_category", "proposed")]));
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
	let asks = s.mcp("ask", json!({ "elicitation": {} })).await;
	let mut events = asks.tool_streamed(&s.app, "execute", json!({ "code": REMOVES })).await;
	let asked = events.next().await.unwrap();
	assert_eq!(asked["method"], "elicitation/create", "{asked}");
	assert_eq!(asks.answer(&s.app, &asked["id"], json!({ "action": "accept", "content": { "approve": true } })).await, 202);
	let (ran, _) = tool_result(&events.next().await.unwrap());
	assert_eq!((&ran["state"], &ran["stdout"]), (&done(), &json!("removed not_found\n")), "{ran}");

	let cannot = s.mcp("ask", json!({})).await;
	let (refused, _) = cannot.tool(&s.app, "execute", json!({ "code": REMOVES })).await;
	assert_eq!(refused["stdout"], "removed refused\n", "{refused}");
	assert!(refused["stderr"].as_str().unwrap().contains("\"confirmed\": true"), "{refused}");
}

/// Past [`MAX`] sessions, a new one takes the idlest's place, which is then told it is gone; when every
/// one runs a cell, there is no room.
#[tokio::test]
async fn the_idlest_session_makes_room_a_running_one_does_not() {
	let mut script = vec![round(vec![execute("idle", "x = 1")]), end("Bound.")];
	for _ in 1..MAX {
		script.extend([round(vec![execute("held", REMOVES)]), end("Agree?")]);
	}
	script.extend([
		round(vec![execute("new", "y = 2")]),
		end("Bound."),
		round(vec![execute("evicted", "print(x)")]),
		end("Gone."),
		round(vec![execute("held", REMOVES)]),
		end("Agree?"),
		round(vec![execute("busy", "z = 3")]),
		end("Busy."),
	]);
	let mut s = Stack::new(script).await;
	let idle = s.chat;
	s.ask_in(idle, "ask").await;
	for _ in 1..MAX {
		let chat = s.new_chat().await;
		s.ask_in(chat, "ask").await;
	}
	let newest = s.new_chat().await;
	s.ask_in(newest, "ask").await;
	let rounds = 2 * MAX;
	assert_eq!(s.result(rounds + 1, 0)["state"], done(), "the idlest made room");
	s.ask_in(idle, "ask").await;
	let evicted = s.result(rounds + 3, 0);
	assert!(evicted["error"].as_str().unwrap().contains("no longer holds this session"), "{evicted}");
	s.ask_in(newest, "ask").await;
	assert_eq!(s.result(rounds + 5, 0)["state"]["paused"]["operation"], "delete_category");
	s.ask_in(idle, "ask").await;
	let busy = s.result(rounds + 7, 0);
	assert!(busy["error"].as_str().unwrap().contains("busy"), "none idle, none let go: {busy}");
}
