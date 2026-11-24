//! The in-page assistant against the real Claude API on a subscription: `CLAUDE_CODE_OAUTH_TOKEN=… cargo r --example ask_live -- "<question>"`.

use axum::{body::Body, http::Method};
use sa_panel::testing::{TestDb, operator, panel};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	model::{Model, Origins},
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};
use uuid::Uuid;

#[tokio::main]
async fn main() {
	let question = std::env::args().nth(1).expect("the question is the first argument");
	let token = std::env::var("CLAUDE_CODE_OAUTH_TOKEN").expect("CLAUDE_CODE_OAUTH_TOKEN is the subscription's token");
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let empty = sa_panel::import::Export {
		people: vec![],
		gmails: vec![],
		gmbs: vec![],
		ledger: vec![],
		kickbacks: vec![],
	};
	p.import_review_archive(empty).await.unwrap().expect("an empty import opens tokens");
	let prices: Vec<_> = sa_core::prices::Item::all().filter(|i| i.tool() == "claude").map(|i| (i, 1)).collect();
	p.set_prices(&prices, &[], "admin@localhost", jiff::Timestamp::now())
		.await
		.expect("the store answers")
		.expect("valid prices");
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
	let model = Model::new(
		Origins {
			claude: "https://api.anthropic.com".into(),
			claude_sign_in: "https://platform.claude.com".into(),
			openai: "https://api.openai.com".into(),
		},
		vec![(sa_core::prices::Provider::Claude, token.into())],
	);
	let app = http::app(
		SignIn::new(p, Concierge::dev(who), config),
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
	let json = [("content-type", "application/json")];
	let chat = Uuid::now_v7();
	let made = json!({ "id": chat, "title": null, "turns": [] });
	let (status, ..) = b.send_body(&app, Method::POST, "/api/v1/assistant/chats", &json, true, Body::from(made.to_string())).await;
	assert_eq!(status, 201);
	let asked = json!({ "from": 0, "text": question, "mentions": [], "files": [], "url": "/", "bindings": [] });
	let (status, _, answer) = b
		.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), &json, true, Body::from(asked.to_string()))
		.await;
	println!("{status} {}", String::from_utf8_lossy(&answer));
	let answer: Value = serde_json::from_slice(&answer).expect("the ask answers JSON");
	println!("{}", serde_json::to_string_pretty(&answer["turns"][1]).expect("JSON serializes"));
}
