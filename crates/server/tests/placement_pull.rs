//! A study's pull is sold per Places search (docs/ARCHITECTURE.md, Tokens and GBPs): placement is
//! told what the person affords, and what it bought is charged to them.

use axum::{
	Json, Router,
	body::Body,
	extract::{Path, Query},
	http::{Method, StatusCode},
	response::IntoResponse,
	routing::post,
};
use sa_core::prices::{Item, PlacementRequest};
use sa_panel::{
	import::Export,
	testing::{TestDb, admin, panel},
	tokens::Change,
};
use sa_server::{
	concierge::DevIdentity,
	testing::{Browser, Log, app_with, upstream},
};
use serde_json::{Value, json};

/// Searches the stub's sweep needs.
const NEED: u64 = 38;
const PULL: &str = "/api/placement/studies/abc/pull";

#[derive(serde::Deserialize)]
struct Budget {
	budget: u64,
}

/// gmaps_optimal_placement's pull: refused before buying past the budget, else all of it bought;
/// study `fails` fails after 5.
async fn pull(Path(sha): Path<String>, Query(Budget { budget }): Query<Budget>) -> axum::response::Response {
	match budget < NEED {
		_ if sha == "fails" => (StatusCode::INTERNAL_SERVER_ERROR, [("x-billed", "5")], "the day's quota ran out").into_response(),
		true => (StatusCode::PAYMENT_REQUIRED, [("x-billed", "0")], Json(json!({ "need": NEED, "budget": budget }))).into_response(),
		false => ([("x-billed", NEED.to_string())], Json(json!({}))).into_response(),
	}
}

async fn setup(price: Option<i64>, balance: i64) -> (TestDb, Router, Log, Browser) {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	p.import_review_archive(Export {
		people: vec![],
		gmails: vec![],
		gmbs: vec![],
		ledger: vec![],
		kickbacks: vec![],
	})
	.await
	.unwrap();
	let now = jiff::Timestamp::now();
	p.set_prices(&[(Item::Placement(PlacementRequest::Search), price)], &[], "admin@localhost", now)
		.await
		.unwrap()
		.unwrap();
	let email = "dev-admin@localhost";
	let (base, seen) = upstream(Router::new().route("/studies/{sha}/pull", post(pull))).await;
	let app = app_with(p.clone(), admin(), email, &base, Default::default());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	get(&mut b, &app, "/api/v1/tokens").await; // the gate records who they are
	p.change_balance(DevIdentity::named(email), Change::Set { n: balance, note: None }, "admin@localhost", now)
		.await
		.unwrap()
		.unwrap();
	(db, app, seen, b)
}

async fn get(b: &mut Browser, app: &Router, uri: &str) -> Value {
	let (status, _, body) = b.send_body(app, Method::GET, uri, &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::OK, "{uri}");
	serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn a_pull_is_told_what_the_balance_affords_and_charged_what_it_bought() {
	let (_db, app, seen, mut b) = setup(Some(8), 1000).await;

	let (status, headers) = b.send(&app, Method::POST, &format!("{PULL}?budget=99999"), &[], true).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(headers["x-billed"], "38");
	assert_eq!(
		seen.lock().unwrap().pop().unwrap().path_and_query,
		"/studies/abc/pull?budget=125",
		"the caller's own budget is not heard"
	);

	assert_eq!(get(&mut b, &app, "/api/v1/tokens").await["balance"], 1000 - 38 * 8);
	let ledger = get(&mut b, &app, "/api/v1/tokens/ledger?limit=1").await;
	let charge = &ledger["entries"][0];
	assert_eq!(
		(&charge["delta"], &charge["kind"], &charge["service"], &charge["key"]),
		(&json!(-304), &json!("charge"), &json!("placement"), &json!("abc")),
		"{charge}"
	);
}

#[tokio::test]
async fn a_balance_below_the_sweep_buys_nothing_and_is_told_the_need() {
	let (_db, app, seen, mut b) = setup(Some(8), 100).await;

	let (status, _, body) = b.send_body(&app, Method::POST, PULL, &[], true, Body::empty()).await;
	assert_eq!(status, StatusCode::PAYMENT_REQUIRED);
	assert_eq!(String::from_utf8_lossy(&body), "this pull needs at least 304 tokens (38 searches at 8), and you hold 100");
	assert_eq!(seen.lock().unwrap().pop().unwrap().path_and_query, "/studies/abc/pull?budget=12");
	assert_eq!(get(&mut b, &app, "/api/v1/tokens").await["balance"], 100);
}

#[tokio::test]
async fn a_failed_pull_is_charged_what_it_bought_and_owes_it_all_back() {
	let (_db, app, _, mut b) = setup(Some(8), 1000).await;

	let (status, headers) = b.send(&app, Method::POST, "/api/placement/studies/fails/pull", &[], true).await;
	assert_eq!((status, &headers["x-billed"]), (StatusCode::INTERNAL_SERVER_ERROR, &"5".parse().unwrap()));
	let tokens = get(&mut b, &app, "/api/v1/tokens").await;
	assert_eq!(tokens["balance"], 1000 - 5 * 8);
	let owed = &tokens["kickbacks"][0];
	assert_eq!((&owed["amount"], &owed["service"], &owed["key"]), (&json!(40), &json!("placement"), &json!("fails")), "{tokens}");
}

#[tokio::test]
async fn an_unpriced_pull_is_refused_before_placement_is_asked() {
	let (_db, app, seen, mut b) = setup(None, 1000).await;

	assert_eq!(b.send(&app, Method::POST, PULL, &[], true).await.0, StatusCode::CONFLICT);
	assert!(seen.lock().unwrap().is_empty());
	assert_eq!(get(&mut b, &app, "/api/v1/tokens").await["balance"], 1000);
}

#[tokio::test]
async fn a_person_reads_the_prices_and_sets_above_what_they_are_asked() {
	let (_db, app, _, mut b) = setup(Some(8), 1000).await;

	let tokens = get(&mut b, &app, "/api/v1/tokens").await;
	assert_eq!((&tokens["prices"]["placement.search"], &tokens["confirm_above"]), (&json!(8), &json!(100)), "{tokens}");
	assert!(tokens["prices"].get("map.map").is_none(), "a metered item is not sold");

	let mut put = async |n: i64| {
		let body = Body::from(json!({ "confirm_above": n }).to_string());
		b.send_body(&app, Method::PUT, "/api/v1/tokens/settings", &[("content-type", "application/json")], true, body)
			.await
			.0
	};
	assert_eq!(put(250).await, StatusCode::NO_CONTENT);
	assert_eq!(put(-1).await, StatusCode::BAD_REQUEST);
	assert_eq!(get(&mut b, &app, "/api/v1/tokens").await["confirm_above"], 250);
}
