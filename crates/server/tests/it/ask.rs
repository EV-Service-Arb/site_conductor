//! The in-page assistant's loop against a scripted Claude API: its tools reach the operations
//! through the gate as the asker, a destructive one becomes a proposal, and the answer lands in the
//! chat's turns.

use std::sync::{Arc, Mutex};

use axum::{Json, Router, body::Body, http::Method, routing::post};
use jiff::Timestamp;
use sa_panel::testing::{TestDb, operator, panel};
use sa_server::{
	claude::{Model, Origins},
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
			tool_use("t4", "open", json!({ "view": "home", "params": { "trash": true }, "label": "Open the trash" })),
			tool_use("t5", "open", json!({ "view": "admin_tokens", "params": {}, "label": "Everyone's tokens" })),
			tool_use("t6", "open", json!({ "view": "home", "params": { "nope": 1 }, "label": "Somewhere" })),
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
	let model = Model::new(Origins { api: base.clone(), oauth: base }, Some("owner".to_owned().into()));
	let app = http::app(SignIn::new(p.clone(), Concierge::dev(who), config), None, None, Default::default(), None, None, Some(model), None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let json = [("content-type", "application/json")];
	let chat = Uuid::now_v7();
	let made = json!({ "id": chat, "title": null, "turns": [] });
	let (status, ..) = b.send_body(&app, Method::POST, "/api/v1/assistant/chats", &json, true, Body::from(made.to_string())).await;
	assert_eq!(status, 201);
	let year = Timestamp::now() + jiff::SignedDuration::from_hours(24 * 365);
	p.set_claude(DevIdentity::named(email), "theirs", year, Timestamp::now()).await.unwrap();

	let question = json!({ "from": 0, "text": "delete my trade 7", "mentions": [], "files": [], "url": "/", "bindings": [], "approval": "ask" });
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
	assert_eq!(said["actions"][1]["target"], json!({ "href": "/?trash=" }), "a view by its name, its href the table's");
	assert_eq!(said["actions"].as_array().unwrap().len(), 2, "not a view the asker may not open, nor one of no such key");
	assert_eq!(said["steps"].as_array().unwrap().len(), 6);
	let used: Vec<(String, i64, i64)> = p
		.used(Timestamp::from_second(0).unwrap(), Timestamp::now())
		.await
		.unwrap()
		.into_iter()
		.filter(|d| d.item.tool() == "claude")
		.map(|d| (d.item.to_string(), d.units, d.times))
		.collect();
	assert_eq!(
		used,
		[("claude.cache_read".into(), 2700, 3), ("claude.input".into(), 300, 3), ("claude.output".into(), 120, 3)],
		"every answer's tokens, an empty kind not counted"
	);

	let sent = sent.lock().unwrap().clone(); // the asks below await
	let results = |n: usize| sent[n]["messages"].as_array().unwrap().last().unwrap()["content"].as_array().unwrap().clone();
	let found: Value = serde_json::from_str(results(1)[0]["content"].as_str().unwrap()).unwrap();
	assert!(found["operations"].as_array().unwrap().iter().any(|o| o["id"] == "delete_trade"), "{found}");
	let trades: Value = serde_json::from_str(results(2)[0]["content"].as_str().unwrap()).unwrap();
	assert_eq!(trades, json!({ "trades": [] }), "read through the gate as the asker");
	assert!(results(2)[1]["content"].as_str().unwrap().contains("proposed"));
	assert!(results(2)[3]["content"].as_str().unwrap().contains("not the person's to open"), "{}", results(2)[3]);
	assert!(results(2)[4]["content"].as_str().unwrap().contains("nope"), "{}", results(2)[4]);
	let open = sent[0]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "open").cloned().unwrap();
	assert!(open["description"].as_str().unwrap().contains("- home (/)"), "the views are the tool's: {open}");
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

/// The level the person picked holds what it does not cover, and runs the rest: a call taken on its
/// own is an action already done, its answer the model's in the same turn.
#[tokio::test]
async fn the_approval_level_holds_only_what_it_does_not_cover() {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let round = || {
		json!({ "stop_reason": "tool_use", "content": [
			tool_use("spend", "call", json!({ "operation": "buy_map", "body": { "account": "nope" } })),
			tool_use("destroy", "call", json!({ "operation": "delete_trade", "path": { "id": 7 } })),
		] })
	};
	let end = || json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Done." }] });
	let (base, sent) = claude(vec![round(), end(), round(), end(), round(), end()]).await;
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
	let model = Model::new(Origins { api: base.clone(), oauth: base }, Some("owner".to_owned().into()));
	let app = http::app(SignIn::new(p.clone(), Concierge::dev(who), config), None, None, Default::default(), None, None, Some(model), None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let json = [("content-type", "application/json")];
	let chat = Uuid::now_v7();
	let made = json!({ "id": chat, "title": null, "turns": [] });
	assert_eq!(b.send_body(&app, Method::POST, "/api/v1/assistant/chats", &json, true, Body::from(made.to_string())).await.0, 201);
	let year = Timestamp::now() + jiff::SignedDuration::from_hours(24 * 365);
	p.set_claude(DevIdentity::named(email), "theirs", year, Timestamp::now()).await.unwrap();

	let mut states = Vec::new();
	for (from, approval) in [(0, "ask"), (2, "spend"), (4, "act")] {
		let question = json!({ "from": from, "text": "map it, and delete trade 7", "mentions": [], "files": [], "url": "/", "bindings": [], "approval": approval });
		let (status, _, answer) = b
			.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), &json, true, Body::from(question.to_string()))
			.await;
		assert_eq!(status, 200, "{}", String::from_utf8_lossy(&answer));
		let answer: Value = serde_json::from_slice(&answer).unwrap();
		let said = answer["turns"][from + 1].clone();
		let actions: Vec<(String, String)> = said["actions"]
			.as_array()
			.unwrap()
			.iter()
			.map(|a| (a["target"]["call"]["operation"].as_str().unwrap().to_owned(), a["state"].as_str().unwrap().to_owned()))
			.collect();
		states.push(actions);
	}
	let pair = |a: &str, b: &str| vec![("buy_map".to_owned(), a.to_owned()), ("delete_trade".to_owned(), b.to_owned())];
	assert_eq!(states, [pair("proposed", "proposed"), pair("done", "proposed"), pair("done", "done")]);

	let sent = sent.lock().unwrap().clone();
	let result = |n: usize, i: usize| sent[n]["messages"].as_array().unwrap().last().unwrap()["content"][i]["content"].as_str().unwrap().to_owned();
	let proposed = |s: &str| s.contains("\"proposed\"");
	assert!(proposed(&result(1, 0)), "asking first, both wait: {} {}", result(1, 0), result(1, 1));
	assert!(proposed(&result(1, 1)), "asking first, both wait: {} {}", result(1, 0), result(1, 1));
	assert!(!proposed(&result(3, 0)), "spending on its own, the map's answer is the model's: {}", result(3, 0));
	assert!(result(3, 0).contains("\"status\""), "spending on its own, the map's answer is the model's: {}", result(3, 0));
	assert!(proposed(&result(3, 1)), "and what destroys still waits: {}", result(3, 1));
	assert!(!proposed(&result(5, 0)), "acting on its own, both ran: {} {}", result(5, 0), result(5, 1));
	assert!(!proposed(&result(5, 1)), "acting on its own, both ran: {} {}", result(5, 0), result(5, 1));
	let call = |n: usize| {
		sent[n]["tools"].as_array().unwrap().iter().find(|t| t["name"] == "call").unwrap()["description"]
			.as_str()
			.unwrap()
			.to_owned()
	};
	assert_ne!(call(0), call(4), "the tool says what the level holds");
}
