//! A person's own Claude: connected from the code Anthropic's page shows, and the assistant's
//! answers on it, free; on the panel's otherwise, charged at its prices, each answer saying what
//! it cost.

use std::sync::{Arc, Mutex};

use axum::{
	Json, Router,
	body::Body,
	http::{HeaderMap, Method, StatusCode},
	response::IntoResponse,
	routing::post,
};
use jiff::Timestamp;
use sa_core::{
	prices::{Item, ModelTokens},
	usage::Bill,
};
use sa_panel::{
	Panel,
	import::Export,
	session::pkce_challenge,
	testing::{TestDb, operator, panel},
};
use sa_server::{
	claude::{Model, Origins},
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};
use uuid::Uuid;

const EMAIL: &str = "asker@x.com";

/// Claude's sign-in and Messages API: a code `refused` exchanges for a token Claude then refuses.
#[derive(Clone, Default)]
struct Stub {
	exchanged: Arc<Mutex<Vec<Value>>>,
	/// Each Messages request's bearer, and whether it was a probe.
	asked: Arc<Mutex<Vec<(String, bool)>>>,
}

async fn token(axum::extract::State(s): axum::extract::State<Stub>, Json(req): Json<Value>) -> axum::response::Response {
	s.exchanged.lock().unwrap().push(req.clone());
	match req["code"].as_str().unwrap() {
		"bad" => (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant", "error_description": "Invalid code" }))).into_response(),
		code => Json(json!({ "access_token": format!("{code}-token"), "expires_in": req["expires_in"], "token_type": "Bearer" })).into_response(),
	}
}

async fn messages(axum::extract::State(s): axum::extract::State<Stub>, headers: HeaderMap, Json(req): Json<Value>) -> axum::response::Response {
	let bearer = headers["authorization"].to_str().unwrap().strip_prefix("Bearer ").unwrap().to_owned();
	let probe = req["max_tokens"] == 1;
	s.asked.lock().unwrap().push((bearer.clone(), probe));
	if bearer == "refused-token" && !probe {
		return (
			StatusCode::UNAUTHORIZED,
			Json(json!({ "type": "error", "error": { "type": "authentication_error", "message": "OAuth token revoked" } })),
		)
			.into_response();
	}
	Json(json!({
		"model": "claude-opus-5-5",
		"stop_reason": "end_turn",
		"content": [{ "type": "text", "text": "Done." }],
		"usage": { "input_tokens": 100_000, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 900_000, "output_tokens": 40_000 },
	}))
	.into_response()
}

struct World {
	panel: Panel,
	app: Router,
	b: Browser,
	stub: Stub,
	_db: TestDb,
}

impl World {
	async fn new() -> Self {
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
		let stub = Stub::default();
		let api = Router::new().route("/v1/oauth/token", post(token)).route("/v1/messages", post(messages)).with_state(stub.clone());
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
		let model = Model::new(Origins { api: base.clone(), oauth: base }, Some("owner".to_owned().into()));
		let app = http::app(
			SignIn::new(panel.clone(), Concierge::dev(who), config),
			None,
			None,
			Default::default(),
			None,
			None,
			None,
			Some(model),
			None,
		);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self { panel, app, b, stub, _db: db }
	}

	async fn call(&mut self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
		let json = [("content-type", "application/json")];
		let extra: &[(&str, &str)] = if body.is_some() { &json } else { &[] };
		let (status, _, bytes) = self
			.b
			.send_body(&self.app, method, uri, extra, true, body.map_or(Body::empty(), |b| Body::from(b.to_string())))
			.await;
		(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
	}

	/// The authorize URL's PKCE challenge and state.
	async fn connect(&mut self) -> (String, String) {
		let (status, started) = self.call(Method::POST, "/api/v1/me/claude/connect", None).await;
		assert_eq!(status, StatusCode::OK, "{started}");
		let url = url::Url::parse(started["url"].as_str().unwrap()).unwrap();
		assert_eq!(url.origin().ascii_serialization(), "https://claude.com");
		let q = |k: &str| url.query_pairs().find(|(n, _)| n == k).unwrap().1.into_owned();
		assert_eq!(q("client_id"), "9d1c250a-e61b-44d9-88ed-5944d1962f5e");
		assert_eq!(q("code_challenge_method"), "S256");
		(q("code_challenge"), q("state"))
	}

	async fn ask(&mut self) -> Value {
		let chat = Uuid::now_v7();
		let (status, made) = self.call(Method::POST, "/api/v1/assistant/chats", Some(json!({ "id": chat, "title": null, "turns": [] }))).await;
		assert_eq!(status, StatusCode::CREATED, "{made}");
		let q = json!({ "from": 0, "text": "hi", "mentions": [], "files": [], "url": "/", "bindings": [] });
		let (status, answer) = self.call(Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), Some(q)).await;
		assert_eq!(status, StatusCode::OK, "{answer}");
		answer["turns"][1].clone()
	}

	async fn balance(&self) -> i64 {
		self.panel.tokens(DevIdentity::named(EMAIL), Timestamp::now()).await.unwrap().unwrap().0
	}

	/// The last ask's bearers, probes left out.
	fn bearers(&self) -> Vec<String> {
		self.stub.asked.lock().unwrap().drain(..).filter(|(_, probe)| !probe).map(|(b, _)| b).collect()
	}
}

#[tokio::test]
async fn a_person_connects_their_claude_from_the_pasted_code() {
	let mut w = World::new().await;
	let (challenge, state) = w.connect().await;

	let (status, refused) = w.call(Method::PUT, "/api/v1/me/claude", Some(json!({ "code": "mine#not-the-state" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "another Connect's code: {refused}");
	assert_eq!(refused["code"], "bad_request");
	assert!(w.stub.exchanged.lock().unwrap().is_empty(), "refused before Claude is asked");

	let (status, set) = w.call(Method::PUT, "/api/v1/me/claude", Some(json!({ "code": format!("mine#{state}") }))).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	assert_eq!(set["checked"]["error"], Value::Null);
	let exchanged = w.stub.exchanged.lock().unwrap().pop().unwrap();
	assert_eq!(pkce_challenge(exchanged["code_verifier"].as_str().unwrap()), challenge, "the verifier of the URL's challenge");
	assert_eq!(exchanged["expires_in"], 31_536_000);
	assert_eq!(exchanged["grant_type"], "authorization_code");
	assert_eq!(exchanged["state"], state.as_str());
	assert_eq!(
		w.stub.asked.lock().unwrap().drain(..).collect::<Vec<_>>(),
		[("mine-token".to_owned(), true)],
		"proved before it is kept"
	);

	let (status, got) = w.call(Method::GET, "/api/v1/me/claude", None).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(got["set_at"], set["set_at"]);
	assert!(!got.to_string().contains("mine-token"), "the token never comes back: {got}");

	let (status, again) = w.call(Method::PUT, "/api/v1/me/claude", Some(json!({ "code": format!("mine#{state}") }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a Connect is spent once: {again}");
	let (_, state) = w.connect().await;
	let (status, bad) = w.call(Method::PUT, "/api/v1/me/claude", Some(json!({ "code": format!("bad#{state}") }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "Claude's refusal of the code: {bad}");

	let (status, _) = w.call(Method::DELETE, "/api/v1/me/claude", None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(w.call(Method::GET, "/api/v1/me/claude", None).await.1, Value::Null);
	assert_eq!(w.call(Method::DELETE, "/api/v1/me/claude", None).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn an_answer_runs_on_whose_claude_and_says_what_it_cost() {
	let mut w = World::new().await;
	w.call(Method::GET, "/api/v1/me", None).await; // the gate's GetMe records the user a balance is kept for
	let start = w.balance().await;

	let per_million = [(ModelTokens::Input, 10), (ModelTokens::CacheWrite, 12), (ModelTokens::CacheRead, 1), (ModelTokens::Output, 50)];
	let prices: Vec<(Item, i64)> = per_million.iter().map(|&(t, p)| (Item::from(t), p)).collect();
	sa_panel::testing::priced(&w.panel, &prices).await;
	let ours = w.ask().await;
	assert_eq!(w.bearers(), ["owner"]);
	assert_eq!(
		ours["cost"],
		json!({ "bill": "ours", "tokens": { "input": 100_000, "cache_write": 0, "cache_read": 900_000, "output": 40_000 }, "charged": 4 }),
		"(100k × 10 + 900k × 1 + 40k × 50) / 1M, rounded up"
	);
	assert_eq!(w.balance().await, start - 4);

	let (_, state) = w.connect().await;
	w.call(Method::PUT, "/api/v1/me/claude", Some(json!({ "code": format!("mine#{state}") }))).await;
	w.bearers();
	let theirs = w.ask().await;
	assert_eq!(w.bearers(), ["mine-token"]);
	assert_eq!(theirs["cost"]["bill"], "theirs");
	assert_eq!(theirs["cost"]["charged"], 0);
	assert_eq!(w.balance().await, start - 4, "free on their own");
	let bills: Vec<(Bill, i64)> = w
		.panel
		.used(Timestamp::from_second(0).unwrap(), Timestamp::now())
		.await
		.unwrap()
		.into_iter()
		.filter(|d| d.item == Item::from(ModelTokens::Input))
		.map(|d| (d.bill, d.units))
		.collect();
	assert_eq!(bills, [(Bill::Ours, 100_000), (Bill::Theirs, 100_000)]);

	let (_, state) = w.connect().await;
	w.call(Method::PUT, "/api/v1/me/claude", Some(json!({ "code": format!("refused#{state}") }))).await;
	w.bearers();
	let refused = w.ask().await;
	assert_eq!(w.bearers(), ["refused-token"], "never the panel's in its place");
	assert_eq!(refused["state"], "stopped");
	assert!(refused["text"].as_str().unwrap().contains("reconnect"), "{refused}");
	let (_, status) = w.call(Method::GET, "/api/v1/me/claude", None).await;
	assert_eq!(status["checked"]["error"], "OAuth token revoked");
	assert_eq!(w.balance().await, start - 4);
}
