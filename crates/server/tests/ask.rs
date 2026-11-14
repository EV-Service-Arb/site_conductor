//! The in-page assistant's loop against a scripted Claude API: its tools reach the operations
//! through the gate as the asker, a destructive one becomes a proposal, and the answer lands in the
//! chat's turns.

use std::sync::{Arc, Mutex};

use axum::{Json, Router, body::Body, http::Method, routing::post};
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
				Json(script.lock().unwrap().next().expect("the script ran out"))
			}
		}),
	);
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(format!("http://{addr}"), sent)
}

fn tool_use(id: &str, name: &str, input: Value) -> Value {
	json!({ "type": "tool_use", "id": id, "name": name, "input": input })
}

#[tokio::test]
async fn the_assistant_calls_as_the_asker_and_proposes_what_destroys() {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let (base, sent) = claude(vec![
		json!({ "stop_reason": "tool_use", "content": [tool_use("t1", "search", json!({ "query": "delete a trade" }))] }),
		json!({ "stop_reason": "tool_use", "content": [
			{ "type": "text", "text": "Reading your trades." },
			tool_use("t2", "call", json!({ "operation": "trades" })),
			tool_use("t3", "call", json!({ "operation": "delete_trade", "path": { "id": 7 } })),
			tool_use("t4", "open", json!({ "url": "/trash", "label": "Open the trash" })),
		] }),
		json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Confirm to delete it." }] }),
	])
	.await;
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
	let model = Model::new(base, "test".into());
	let app = http::app(SignIn::new(p.clone(), Concierge::dev(who), config), None, None, Default::default(), None, None, None, Some(model));
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let json = [("content-type", "application/json")];
	let chat = Uuid::now_v7();
	let made = json!({ "id": chat, "title": null, "turns": [] });
	let (status, ..) = b.send_body(&app, Method::POST, "/api/v1/assistant/chats", &json, true, Body::from(made.to_string())).await;
	assert_eq!(status, 201);

	let question = json!({ "from": 0, "text": "delete my trade 7", "mentions": [], "files": [], "url": "/", "views": { "trash": "/?trash" }, "bindings": [] });
	let (status, _, answer) = b
		.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), &json, true, Body::from(question.to_string()))
		.await;
	assert_eq!(status, 200, "{}", String::from_utf8_lossy(&answer));
	let answer: Value = serde_json::from_slice(&answer).unwrap();
	let turns = answer["turns"].as_array().unwrap();
	assert_eq!(turns.len(), 2);
	assert_eq!(turns[0]["text"], "delete my trade 7");
	let said = &turns[1];
	assert_eq!(said["state"], "done");
	assert_eq!(said["text"], "Confirm to delete it.");
	assert_eq!(
		said["actions"][0]["target"],
		json!({ "call": { "operation": "delete_trade", "path": { "id": 7 }, "query": null, "body": null } })
	);
	assert_eq!(said["actions"][0]["state"], "proposed");
	assert_eq!(said["actions"][1]["target"], json!({ "href": "/trash" }));
	assert_eq!(said["steps"].as_array().unwrap().len(), 4);

	let sent = sent.lock().unwrap();
	let results = |n: usize| sent[n]["messages"].as_array().unwrap().last().unwrap()["content"].as_array().unwrap().clone();
	let found: Value = serde_json::from_str(results(1)[0]["content"].as_str().unwrap()).unwrap();
	assert!(found["operations"].as_array().unwrap().iter().any(|o| o["id"] == "delete_trade"), "{found}");
	let trades: Value = serde_json::from_str(results(2)[0]["content"].as_str().unwrap()).unwrap();
	assert_eq!(trades, json!({ "trades": [] }), "read through the gate as the asker");
	assert!(results(2)[1]["content"].as_str().unwrap().contains("proposed"));
	drop(sent);

	let made = json!({ "name": "Plumbing" });
	let (status, _, trade) = b.send_body(&app, Method::POST, "/api/v1/trades", &json, true, Body::from(made.to_string())).await;
	assert_eq!(status, 503, "trades open with the import: {}", String::from_utf8_lossy(&trade));
	let act = json!({ "turn": said["id"], "action": said["actions"][0]["id"] });
	let (status, _, refused) = b
		.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/act"), &json, true, Body::from(act.to_string()))
		.await;
	assert_eq!(status, 503, "the operation's own refusal, as it refused: {}", String::from_utf8_lossy(&refused));
	let link = json!({ "turn": said["id"], "action": said["actions"][1]["id"] });
	let (status, ..) = b
		.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/act"), &json, true, Body::from(link.to_string()))
		.await;
	assert_eq!(status, 409, "a view is the page's to open");
}
