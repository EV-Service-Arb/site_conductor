//! Whom the assistant is asked of, against scripted providers: the asker's own models or the panel's,
//! who stands in and when not, what is counted and charged, and what is said when none answers.

use std::{
	collections::VecDeque,
	sync::{Arc, Mutex},
};

use axum::{
	Json, Router,
	extract::State,
	http::{HeaderMap, HeaderName, Method, StatusCode},
	response::IntoResponse,
	routing::post,
};
use jiff::Timestamp;
use sa_core::prices::{Item, OpenAiTokens, Provider};
use sa_panel::{
	Panel,
	import::Export,
	testing::{TestDb, operator, panel, priced},
};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	model::Model,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN, origins},
};
use serde_json::{Value, json};
use tracing::Level;
use tracing_subscriber::layer::SubscriberExt as _;
use uuid::Uuid;

const EMAIL: &str = "asker@x.com";

type Reply = (StatusCode, Vec<(&'static str, &'static str)>, Value);

type Events = Arc<Mutex<Vec<(Level, &'static str)>>>;

/// Both providers, each answering with the next of its script.
#[derive(Clone, Default)]
struct Providers {
	claude: Arc<Mutex<VecDeque<Reply>>>,
	openai: Arc<Mutex<VecDeque<Reply>>>,
	/// Each request's provider and bearer.
	asked: Arc<Mutex<Vec<(&'static str, String)>>>,
}
impl Providers {
	fn answer(&self, which: &'static str, script: &Mutex<VecDeque<Reply>>, headers: &HeaderMap) -> axum::response::Response {
		let bearer = headers["authorization"].to_str().unwrap().strip_prefix("Bearer ").unwrap().to_owned();
		self.asked.lock().unwrap().push((which, bearer));
		let (status, headers, body) = script.lock().unwrap().pop_front().unwrap_or_else(|| panic!("{which}'s script ran out"));
		let headers: HeaderMap = headers.into_iter().map(|(n, v)| (HeaderName::from_static(n), v.parse().unwrap())).collect();
		(status, headers, Json(body)).into_response()
	}

	fn bearers(&self) -> Vec<(&'static str, String)> {
		self.asked.lock().unwrap().drain(..).collect()
	}
}

async fn claude(State(p): State<Providers>, headers: HeaderMap) -> axum::response::Response {
	p.answer("claude", &p.claude, &headers)
}

async fn openai(State(p): State<Providers>, headers: HeaderMap) -> axum::response::Response {
	p.answer("openai", &p.openai, &headers)
}

fn claude_says(stop: &str, content: Value) -> Reply {
	let usage = json!({ "input_tokens": 100, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0, "output_tokens": 10 });
	(
		StatusCode::OK,
		vec![],
		json!({ "model": "claude-opus-5-5", "stop_reason": stop, "content": content, "usage": usage }),
	)
}

fn claude_text() -> Reply {
	claude_says("end_turn", json!([{ "type": "text", "text": "Done." }]))
}

fn claude_calls() -> Reply {
	claude_says("tool_use", json!([{ "type": "tool_use", "id": "t1", "name": "search", "input": { "query": "categories" } }]))
}

fn openai_says(output: Value) -> Reply {
	let usage = json!({ "input_tokens": 100_000, "input_tokens_details": { "cached_tokens": 0 }, "output_tokens": 1_000, "total_tokens": 101_000 });
	(
		StatusCode::OK,
		vec![],
		json!({ "id": "resp_1", "status": "completed", "model": "gpt-5.6-terra", "output": output, "usage": usage }),
	)
}

fn openai_text() -> Reply {
	openai_says(json!([{ "type": "message", "role": "assistant", "content": [{ "type": "output_text", "text": "Done." }] }]))
}

fn refuses(status: u16, kind: &str) -> Reply {
	(
		StatusCode::from_u16(status).unwrap(),
		vec![],
		json!({ "type": "error", "error": { "type": kind, "code": kind, "message": "the provider's words" } }),
	)
}

fn at_limit() -> Reply {
	let (status, _, body) = refuses(429, "rate_limit_error");
	(
		status,
		vec![("anthropic-ratelimit-unified-status", "rejected"), ("anthropic-ratelimit-unified-reset", "1793606681")],
		body,
	)
}

/// Our events and `ask_llm`'s while the guard lives, on this thread.
pub(crate) fn events() -> (Events, tracing::subscriber::DefaultGuard) {
	struct Kept(Events);
	impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for Kept {
		fn on_event(&self, event: &tracing::Event<'_>, _: tracing_subscriber::layer::Context<'_, S>) {
			let m = event.metadata();
			if ["ask_llm", "sa_server"].iter().any(|t| m.target().starts_with(t)) {
				self.0.lock().unwrap().push((*m.level(), m.name()));
			}
		}
	}
	let events = Events::default();
	let guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(Kept(events.clone())));
	(events, guard)
}

pub(crate) fn count(events: &Events, level: Level) -> usize {
	events.lock().unwrap().iter().filter(|(l, _)| *l == level).count()
}

struct World {
	panel: Panel,
	app: Router,
	b: Browser,
	p: Providers,
	_db: TestDb,
}
impl World {
	/// The panel's credentials: `ours`, each its provider's name.
	async fn new(ours: &[Provider]) -> Self {
		let db = TestDb::create().await;
		let panel = panel(&db).await;
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap().unwrap();
		let p = Providers::default();
		let api = Router::new().route("/v1/messages", post(claude)).route("/v1/responses", post(openai)).with_state(p.clone());
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let base = format!("http://{}", listener.local_addr().unwrap());
		tokio::spawn(async move { axum::serve(listener, api).await.unwrap() });
		let who = DevIdentity {
			user_id: DevIdentity::named(EMAIL),
			permissions: operator(),
			email: EMAIL.to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let model = Model::new(origins(&base), ours.iter().map(|o| (*o, format!("our {o}").into())).collect());
		let app = http::app(
			SignIn::new(panel.clone(), Concierge::dev(who), config),
			None,
			None,
			Default::default(),
			None,
			None,
			Some(model),
			sa_server::testing::sandbox(),
			None,
		);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		let mut w = Self { panel, app, b, p, _db: db };
		w.call(Method::GET, "/me", None).await; // the gate's GetMe records the user a balance is kept for
		w
	}

	fn claude(&self, script: impl IntoIterator<Item = Reply>) {
		self.p.claude.lock().unwrap().extend(script);
	}

	fn openai(&self, script: impl IntoIterator<Item = Reply>) {
		self.p.openai.lock().unwrap().extend(script);
	}

	async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
		self.b.call(&self.app, method, &format!("/api/v1{path}"), body).await
	}

	/// The answer, as the ask answers it and as the chat keeps it.
	async fn ask(&mut self) -> Value {
		let chat = Uuid::now_v7();
		let (status, made) = self.call(Method::POST, "/assistant/chats", Some(json!({ "id": chat, "title": null, "turns": [] }))).await;
		assert_eq!(status, StatusCode::CREATED, "{made}");
		let q = json!({ "from": 0, "text": "hi", "mentions": [], "files": [], "url": "/", "bindings": [], "approval": "ask" });
		let (status, answer) = self.call(Method::POST, &format!("/assistant/chats/{chat}/ask"), Some(q)).await;
		assert_eq!(status, StatusCode::OK, "{answer}");
		let (_, kept) = self.call(Method::GET, &format!("/assistant/chats/{chat}"), None).await;
		assert_eq!(kept["turns"], answer["turns"]);
		answer["turns"][1].clone()
	}
}

#[tokio::test]
async fn a_round_that_fails_ends_the_answer_with_the_rounds_done() {
	let mut w = World::new(&[Provider::Claude, Provider::OpenAi]).await;
	w.claude([claude_calls(), refuses(529, "overloaded_error")]);
	let said = w.ask().await;
	assert_eq!((&said["state"], &said["connect"]), (&json!("stopped"), &json!(false)), "{said}");
	assert_eq!(said["text"], "\n\nI stopped here. Claude is not answering now: try again in a minute.");
	assert_eq!(said["steps"], json!([{ "label": "Searched: categories", "done": true }]));
	assert_eq!(said["cost"]["tokens"]["output"], 10, "the round done is told");
	assert_eq!(
		w.p.bearers(),
		[("claude", "our Claude".to_owned()), ("claude", "our Claude".to_owned())],
		"never another mid-answer"
	);
}

#[tokio::test]
async fn the_panel_s_spent_credits_are_said_outright_and_told_of_once() {
	let mut w = World::new(&[Provider::Claude]).await;
	w.claude([at_limit(), at_limit(), claude_text(), at_limit()]);
	let (events, _guard) = events();
	for _ in 0..2 {
		let said = w.ask().await;
		assert_eq!((&said["state"], &said["connect"]), (&json!("stopped"), &json!(true)), "{said}");
		assert_eq!(
			said["text"],
			"I can't answer now. The panel's Claude credits have run out until Nov 2, 08:04 UTC. Connect your own Claude or OpenAI and it costs nothing here."
		);
	}
	assert_eq!(count(&events, Level::ERROR), 1, "{:?}", events.lock().unwrap());
	assert_eq!(w.ask().await["state"], "done");
	w.ask().await;
	assert_eq!(count(&events, Level::ERROR), 2, "again, once it had answered");
}

#[tokio::test]
async fn the_panel_s_next_stands_in_on_the_first_step_and_is_charged_as_itself() {
	let mut w = World::new(&[Provider::Claude, Provider::OpenAi]).await;
	priced(&w.panel, &[(Item::from(OpenAiTokens::Input), 20), (Item::from(OpenAiTokens::Output), 1000)]).await;
	let call = json!([{ "type": "function_call", "call_id": "t1", "name": "search", "arguments": "{\"query\":\"categories\"}" }]);
	w.claude([refuses(401, "authentication_error")]);
	w.openai([openai_says(call), openai_text()]);
	let (events, _guard) = events();
	let said = w.ask().await;
	assert_eq!((&said["state"], &said["text"]), (&json!("done"), &json!("Done.")), "{said}");
	assert_eq!(
		w.p.bearers(),
		[("claude", "our Claude".to_owned()), ("openai", "our OpenAI".to_owned()), ("openai", "our OpenAI".to_owned())],
		"the first step alone is asked of both"
	);
	assert_eq!(
		said["cost"],
		json!({
			"bill": "ours",
			"model": "gpt-5.6-terra",
			"tokens": { "input": 200_000, "cache_write": 0, "cache_read": 0, "output": 2_000 },
			"usd_micros": 424_000,
			"charged": 6,
		}),
		"200k × 20 + 2k × 1000 per million, rounded up; what it lists for is not what is charged"
	);
	assert_eq!(count(&events, Level::ERROR), 1);
	let used = w.panel.used(Timestamp::from_second(0).unwrap(), Timestamp::now()).await.unwrap();
	let used: Vec<(String, i64)> = used
		.into_iter()
		.filter(|d| ["claude", "openai"].contains(&d.item.tool()))
		.map(|d| (d.item.to_string(), d.units))
		.collect();
	assert_eq!(used, [("openai.input".to_owned(), 200_000), ("openai.output".to_owned(), 2_000)]);
}

#[tokio::test]
async fn a_person_s_own_openai_key_answers_alone_and_its_refusal_is_theirs() {
	let mut w = World::new(&[Provider::Claude]).await;
	let start = w.panel.tokens(DevIdentity::named(EMAIL), Timestamp::now()).await.unwrap().unwrap().0;
	let key = || Some(json!({ "key": "sk-proj-mine-4242" }));
	for (refusal, why) in [
		(refuses(401, "invalid_api_key"), "OpenAI refuses it"),
		(refuses(429, "insufficient_quota"), "out of credit"),
		(refuses(404, "model_not_found"), "no access"),
	] {
		w.openai([refusal]);
		let (status, refused) = w.call(Method::PUT, "/me/openai", key()).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
		assert!(refused["error"].as_str().unwrap().contains(why), "{refused}");
		assert!(!refused.to_string().contains("the provider's words"), "{refused}");
	}
	assert_eq!(w.call(Method::GET, "/me/openai", None).await.1, Value::Null, "kept only once it answers");

	w.openai([openai_text()]);
	let (status, set) = w.call(Method::PUT, "/me/openai", key()).await;
	assert_eq!((status, &set["last4"]), (StatusCode::OK, &json!("4242")), "{set}");
	assert!(!w.call(Method::GET, "/me/openai", None).await.1.to_string().contains("sk-proj"));
	w.p.bearers();

	w.openai([openai_text()]);
	let said = w.ask().await;
	assert_eq!(w.p.bearers(), [("openai", "sk-proj-mine-4242".to_owned())]);
	assert_eq!(
		said["cost"],
		json!({
			"bill": "theirs",
			"model": "gpt-5.6-terra",
			"tokens": { "input": 100_000, "cache_write": 0, "cache_read": 0, "output": 1_000 },
			"usd_micros": 212_000,
			"charged": 0,
		})
	);

	w.openai([refuses(401, "invalid_api_key")]);
	let (events, _guard) = events();
	let said = w.ask().await;
	assert_eq!(w.p.bearers(), [("openai", "sk-proj-mine-4242".to_owned())], "never the panel's in its place");
	assert_eq!(
		(&said["state"], &said["connect"], &said["text"]),
		(
			&json!("stopped"),
			&json!(true),
			&json!("I can't answer now. Your OpenAI connection was refused: reconnect it, or disconnect it to ask on the panel's.")
		)
	);
	assert_eq!((count(&events, Level::ERROR), count(&events, Level::WARN)), (0, 1), "theirs to fix: {:?}", events.lock().unwrap());
	assert_eq!(
		w.call(Method::GET, "/me/openai", None).await.1["checked"]["error"],
		"OpenAI no longer takes it: connect it again."
	);
	assert_eq!(w.panel.tokens(DevIdentity::named(EMAIL), Timestamp::now()).await.unwrap().unwrap().0, start);
	assert_eq!(w.call(Method::DELETE, "/me/openai", None).await.0, StatusCode::NO_CONTENT);
	assert_eq!(w.call(Method::DELETE, "/me/openai", None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn with_no_model_at_all_the_answer_offers_to_connect_one() {
	let mut w = World::new(&[]).await;
	let said = w.ask().await;
	assert_eq!((&said["state"], &said["connect"]), (&json!("stopped"), &json!(true)), "{said}");
	assert!(said["text"].as_str().unwrap().contains("connect your own Claude or OpenAI"), "{said}");
}
