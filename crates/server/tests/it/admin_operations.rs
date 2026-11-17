//! What an operator once ran as `panel` commands, as `/api/v1` operations: each behind the
//! conductor's admin permissions, and refusing what the commands did.

use axum::{
	Router,
	body::Body,
	http::{Method, StatusCode},
};
use sa_core::queries_cache::{Key, Scheme};
use sa_panel::{
	Panel,
	seal::DataKey,
	testing::{TestDb, admin, operator, panel_with},
	tokens::User,
};
use sa_server::testing::{Browser, app};
use serde_json::{Value, json};

async fn conductor(permissions: sa_auth::PermissionSet) -> (Panel, Router, Browser, TestDb) {
	let db = TestDb::create().await;
	let panel = panel_with(db.store().await, DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap());
	let app = app(panel.clone(), permissions, "http://127.0.0.1:1");
	let mut b = Browser::default();
	b.sign_in(&app).await;
	(panel, app, b, db)
}

async fn call(b: &mut Browser, app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
	let json = [("content-type", "application/json")];
	let (status, _, bytes) = b
		.send_body(
			app,
			method,
			uri,
			if body.is_some() { &json } else { &[] },
			true,
			body.map_or(Body::empty(), |b| Body::from(b.to_string())),
		)
		.await;
	(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn empty_export() -> Value {
	json!({ "people": [], "gmails": [], "gmbs": [], "ledger": [], "kickbacks": [] })
}

#[tokio::test]
async fn an_operator_runs_none_of_them() {
	let (_, app, mut b, _db) = conductor(operator()).await;
	let ops = [
		(Method::PUT, "/api/v1/sources/aquafix-site/origin", json!({ "origin": "https://aquafix.top" })),
		(Method::POST, "/api/v1/admin/gmails", json!({ "email": "a@b.c", "gmail": "a@gmail.com" })),
		(Method::POST, "/api/v1/admin/gmbs/locate", Value::Null),
		(Method::POST, "/api/v1/admin/imports/review-archive", empty_export()),
		(Method::POST, "/api/v1/admin/imports/queries-cache", json!({ "scheme": "gop", "answers": {} })),
	];
	for (method, uri, body) in ops {
		let (status, refused) = call(&mut b, &app, method.clone(), uri, (!body.is_null()).then_some(body)).await;
		assert_eq!(status, StatusCode::FORBIDDEN, "{method} {uri}: {refused}");
	}
}

#[tokio::test]
async fn a_site_key_from_before_origins_gets_its_origin_once() {
	let (panel, app, mut b, _db) = conductor(admin()).await;
	for (key, kind) in [("aquafix-site", "site"), ("aquafix-bot", "bot")] {
		sqlx::query("INSERT INTO sources (key_id, kind, brand_ids, secret_sealed, data_key_fp) VALUES ($1, $2, '[\"aquafix\"]', x'00', zeroblob(32))")
			.bind(key)
			.bind(kind)
			.execute(panel.store().pool())
			.await
			.unwrap();
	}
	let set = |key: &'static str, origin: &'static str| (Method::PUT, format!("/api/v1/sources/{key}/origin"), Some(json!({ "origin": origin })));
	let mut put = async |(key, origin)| {
		let (method, uri, body) = set(key, origin);
		call(&mut b, &app, method, &uri, body).await.0
	};
	assert_eq!(put(("aquafix-site", "https://aquafix.top")).await, StatusCode::NO_CONTENT);
	assert_eq!(put(("aquafix-site", "https://aquafix.top")).await, StatusCode::NO_CONTENT, "the same again is no change");
	assert_eq!(put(("aquafix-site", "https://elsewhere.top")).await, StatusCode::CONFLICT);
	assert_eq!(put(("aquafix-bot", "https://aquafix.top")).await, StatusCode::CONFLICT, "only a site has one");
	assert_eq!(put(("nobody", "https://aquafix.top")).await, StatusCode::CONFLICT);
	assert_eq!(put(("aquafix-site", "aquafix.top")).await, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_review_archive_import_opens_tokens_once_and_a_gmail_is_added_for_a_person_who_signed_in() {
	let (panel, app, mut b, _db) = conductor(admin()).await;
	let import = "/api/v1/admin/imports/review-archive";
	let subless = json!({ "people": [{ "sub": null, "email": "x@y.z", "name": "X", "first_seen": "2026-01-01T00:00:00Z" }], "gmails": [], "gmbs": [], "ledger": [], "kickbacks": [] });
	let (status, refused) = call(&mut b, &app, Method::POST, import, Some(subless)).await;
	assert_eq!((status, refused["error"].as_str().unwrap().contains("x@y.z")), (StatusCode::BAD_REQUEST, true), "{refused}");
	assert_eq!(call(&mut b, &app, Method::POST, import, Some(empty_export())).await.0, StatusCode::NO_CONTENT);
	assert_eq!(call(&mut b, &app, Method::POST, import, Some(empty_export())).await.0, StatusCode::CONFLICT, "imported before");

	let add = json!({ "email": "ann@example.com", "gmail": "ann@gmail.com" });
	let (status, refused) = call(&mut b, &app, Method::POST, "/api/v1/admin/gmails", Some(add.clone())).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "nobody signed in with that email: {refused}");
	let user = User {
		user_id: uuid::Uuid::from_u128(7),
		email: "ann@example.com".into(),
		name: "Ann".into(),
	};
	panel.user_seen(&user, jiff::Timestamp::now()).await.unwrap();
	let (status, added) = call(&mut b, &app, Method::POST, "/api/v1/admin/gmails", Some(add.clone())).await;
	assert_eq!((status, &added["gmail"]), (StatusCode::CREATED, &json!("ann@gmail.com")), "{added}");
	assert_eq!(panel.gmails(user.user_id).await.unwrap().0.len(), 1);
	assert_eq!(call(&mut b, &app, Method::POST, "/api/v1/admin/gmails", Some(add)).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn imported_answers_are_kept_as_of_their_own_time_and_a_newer_one_stays() {
	let (panel, app, mut b, _db) = conductor(admin()).await;
	let (newer, older) = (Key([1; 32]), Key([2; 32]));
	let body = |key: Key, answer: Value, at: &str| json!({ "scheme": "gop", "answers": { key.to_string(): { "answer": answer, "fetched_at": at } } });
	let import = "/api/v1/admin/imports/queries-cache";
	for (key, answer, at) in [(newer, json!({ "v": 2 }), "2026-02-01T00:00:00Z"), (older, json!({ "v": 1 }), "2026-01-01T00:00:00Z")] {
		let (status, done) = call(&mut b, &app, Method::POST, import, Some(body(key, answer, at))).await;
		assert_eq!((status, &done["answers"]), (StatusCode::OK, &json!(1)), "{done}");
	}
	call(&mut b, &app, Method::POST, import, Some(body(newer, json!({ "v": 1 }), "2026-01-15T00:00:00Z"))).await;
	let kept = |key| panel.store().cached_query(Scheme::Gop, key);
	assert_eq!(kept(newer).await.unwrap().unwrap().answer, json!({ "v": 2 }), "an older answer never replaces a newer one");
	assert_eq!(kept(older).await.unwrap().unwrap().answer, json!({ "v": 1 }));
	let (status, _) = call(&mut b, &app, Method::POST, import, Some(json!({ "scheme": "nowhere", "answers": {} }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}
