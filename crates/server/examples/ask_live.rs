//! The in-page assistant against the real Claude API: `ANTHROPIC_API_KEY=… cargo r --example ask_live -- "<question>"`.

use axum::{body::Body, http::Method};
use sa_panel::testing::{TestDb, operator, panel};
use sa_server::{
	ask::Model,
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::main]
async fn main() {
	let question = std::env::args().nth(1).expect("the question is the first argument");
	let key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY is the model's key");
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let email = "asker@x.com";
	let who = DevIdentity {
		user_id: DevIdentity::named(email),
		permissions: operator(),
		email: email.to_owned(),
	};
	let config = SignInConfig {
		panel_origin: ORIGIN.to_owned(),
		concierge_origin: ORIGIN.to_owned(),
	};
	let model = Model::new("https://api.anthropic.com".into(), key);
	let app = http::app(SignIn::new(p, Concierge::dev(who), config), None, None, Default::default(), None, None, None, Some(model));
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let json = [("content-type", "application/json")];
	let chat = Uuid::now_v7();
	let made = json!({ "id": chat, "title": null, "turns": [] });
	let (status, ..) = b.send_body(&app, Method::POST, "/api/v1/assistant/chats", &json, true, Body::from(made.to_string())).await;
	assert_eq!(status, 201);
	let asked = json!({ "from": 0, "text": question, "mentions": [], "files": [], "url": "/", "views": { "trash": "/?trash" }, "bindings": [] });
	let (status, _, answer) = b
		.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), &json, true, Body::from(asked.to_string()))
		.await;
	println!("{status} {}", String::from_utf8_lossy(&answer));
	let answer: Value = serde_json::from_slice(&answer).expect("the ask answers JSON");
	println!("{}", serde_json::to_string_pretty(&answer["turns"][1]).unwrap());
}
