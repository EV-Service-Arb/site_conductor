//! `execute` end to end: the in-page assistant asks a scripted Claude, whose code runs in the
//! runner over the generated client (crates/sandbox/js, its node_modules installed) and comes
//! back through the conductor's `/api/run` as the asker. What reads runs; what destroys stops the
//! run and is proposed; nothing but an operation is reached; the token dies with its run.

use std::{
	process::Stdio,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{Json, Router, body::Body, http::Method, routing::post};
use jiff::Timestamp;
use sa_panel::testing::{TestDb, operator, panel};
use sa_server::{
	claude::Model,
	concierge::{Concierge, DevIdentity},
	execute::Sandbox,
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN, SEED},
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

fn execute(id: &str, code: &str) -> Value {
	json!({ "type": "tool_use", "id": id, "name": "execute", "input": { "code": code } })
}

const READS: &str = r#"const me = await api.GET("/api/v1/me");
if (me.error) throw new Error(me.error.code);
console.log(me.data.email);"#;

const ESCAPES: &str = r#"const at = Deno.env.get("SA_RUN_URL")!, token = Deno.env.get("SA_RUN_TOKEN")!;
const out = await fetch(`${at}/auth/logout`, { method: "POST", headers: { authorization: `Bearer ${token}` } });
console.log(out.status, out.headers.has("x-sa-run-refused"), token);"#;

const DESTROYS: &str = r#"await api.DELETE("/api/v1/trades/{id}", { params: { path: { id: 7 } } });
console.log("after");"#;

/// The runner, sending back to `conductor`.
async fn runner(conductor: &str) -> (String, tokio::process::Child) {
	let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
	let deno = std::env::split_paths(&std::env::var_os("PATH").unwrap())
		.map(|d| d.join("deno"))
		.find(|p| p.is_file())
		.expect("deno on PATH: the dev shell has it");
	let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sa_sandbox"))
		.arg("serve")
		.env("SANDBOX_BIND", format!("127.0.0.1:{port}"))
		.env("SANDBOX_MAX", "2")
		.env("SANDBOX_DENO", deno)
		.env("SANDBOX_JS", concat!(env!("CARGO_MANIFEST_DIR"), "/js"))
		.env("SANDBOX_CONDUCTOR_URL", format!("{conductor}/api/run"))
		.env("PANEL_ASSERTION_KEYS", SEED.parse::<sa_auth::Signer>().unwrap().public())
		.stdout(Stdio::null())
		.kill_on_drop(true)
		.spawn()
		.unwrap();
	let base = format!("http://127.0.0.1:{port}");
	for _ in 0..100 {
		if reqwest::get(format!("{base}/health")).await.is_ok() {
			return (base, child);
		}
		tokio::time::sleep(Duration::from_millis(50)).await;
	}
	panic!("the runner never listened");
}

#[tokio::test]
async fn code_runs_as_the_asker_and_stops_at_what_destroys() {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let (model, sent) = claude(vec![
		json!({ "stop_reason": "tool_use", "content": [execute("e1", READS), execute("e2", ESCAPES)] }),
		json!({ "stop_reason": "tool_use", "content": [execute("e3", DESTROYS)] }),
		json!({ "stop_reason": "end_turn", "content": [{ "type": "text", "text": "Confirm to delete it." }] }),
	])
	.await;
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let conductor = format!("http://{}", listener.local_addr().unwrap());
	let (sandbox, _runner) = runner(&conductor).await;
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
	let app = http::app(
		SignIn::new(p.clone(), Concierge::dev(who), config),
		None,
		None,
		Default::default(),
		None,
		None,
		None,
		Some(Model::new(model.clone(), model, Some("owner".to_owned().into()))),
		Some(Sandbox::try_new(&sandbox, SEED.parse().unwrap()).unwrap()),
	);
	let served = app.clone();
	tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let json = [("content-type", "application/json")];
	let chat = Uuid::now_v7();
	let made = json!({ "id": chat, "title": null, "turns": [] });
	assert_eq!(b.send_body(&app, Method::POST, "/api/v1/assistant/chats", &json, true, Body::from(made.to_string())).await.0, 201);
	let year = Timestamp::now() + jiff::SignedDuration::from_hours(24 * 365);
	p.set_claude(DevIdentity::named(email), "theirs", year, Timestamp::now()).await.unwrap();

	let question = json!({ "from": 0, "text": "delete my trade 7", "mentions": [], "files": [], "url": "/", "bindings": [] });
	let (status, _, answer) = b
		.send_body(&app, Method::POST, &format!("/api/v1/assistant/chats/{chat}/ask"), &json, true, Body::from(question.to_string()))
		.await;
	assert_eq!(status, 200, "{}", String::from_utf8_lossy(&answer));
	let answer: Value = serde_json::from_slice(&answer).unwrap();
	let said = &answer["turns"][1];

	let sent = sent.lock().unwrap().clone();
	assert!(sent[0]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "execute"), "offered with a sandbox");
	let result = |n: usize, i: usize| -> Value {
		let results = sent[n]["messages"].as_array().unwrap().last().unwrap()["content"].as_array().unwrap().clone();
		serde_json::from_str(results[i]["content"].as_str().unwrap()).unwrap()
	};
	let read = result(1, 0);
	assert_eq!((&read["ran"]["ended"], &read["ran"]["stdout"]), (&json!({ "exited": 0 }), &json!("asker@x.com\n")), "{read}");

	let escaped = result(1, 1);
	let stdout = escaped["ran"]["stdout"].as_str().unwrap();
	let (seen, token) = stdout.trim_end().rsplit_once(' ').unwrap();
	assert_eq!(seen, "404 true", "only an operation is reached: {escaped}");
	let (status, ..) = b.send(&app, Method::GET, "/api/v1/me", &[], false).await;
	assert_eq!(status, 200, "the asker's session outlived the run's try at /auth/logout");
	let after = reqwest::Client::new().get(format!("{conductor}/api/run/api/v1/me")).bearer_auth(token).send().await.unwrap();
	assert_eq!(after.status(), 401, "the token died with its run");

	let destroyed = result(2, 0);
	assert_eq!(destroyed["ran"]["ended"], json!({ "exited": 4 }), "{destroyed}");
	assert_eq!(destroyed["ran"]["stdout"], "", "nothing after the proposed call ran");
	assert!(destroyed["proposed"].is_string(), "{destroyed}");
	assert_eq!(
		(&said["actions"][0]["target"], &said["actions"][0]["state"]),
		(
			&json!({ "call": { "operation": "delete_trade", "path": { "id": "7" }, "query": {}, "body": null } }),
			&json!("proposed")
		)
	);
	assert_eq!(said["actions"].as_array().unwrap().len(), 1);
}
