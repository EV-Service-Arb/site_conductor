//! Placement's studies (docs/ARCHITECTURE.md, Placement): the conductor builds a study's Definition
//! from its own areas and the caller's trades, asks placement for it as them, and streams the payload
//! back unread; a pull is sold per Places search, Ads request and DataForSEO task (Tokens and GBPs):
//! placement is told how many searches the person affords, and what it bought is charged to them.

use axum::{
	Json, Router,
	body::Body,
	extract::Query,
	http::{Method, StatusCode},
	response::IntoResponse,
	routing::{get, post},
};
use sa_auth::{PermissionSet, Principal, Service};
use sa_core::{
	placement::Definition,
	prices::{AdsRequest, DataForSeoTask, Item, PlacementRequest, Tool},
};
use sa_panel::{
	areas::{Area, Shape},
	import::Export,
	testing::{TestDb, admin, operator, panel},
	tokens::Change,
};
use sa_places::{LatLng, Viewport};
use sa_server::{
	concierge::DevIdentity,
	testing::{Browser, Log, app_with, upstream},
};
use serde_json::{Value, json};

const EMAIL: &str = "dev-admin@localhost";
/// Searches the stub's sweep needs.
const NEED: u64 = 38;
/// Each Ads request and DataForSEO task.
const AFTER: i64 = 3;
/// What the stub answers a study with, byte for byte.
const PAYLOAD: &str = r#"{"name":"as placement wrote it"}"#;

#[derive(serde::Deserialize)]
struct Budget {
	budget: u64,
}

/// Placement: every definition is studied but one whose study is `carpentry`.
async fn study(Json(def): Json<Definition>) -> axum::response::Response {
	match def.category.as_ref().map(|c| c.study.as_str()) {
		Some("carpentry") => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "code": "unstudied", "error": "no trade file carpentry" }))).into_response(),
		_ => ([("content-type", "application/json")], PAYLOAD).into_response(),
	}
}

/// Placement's pull: refused before buying past the budget, else all of it bought with two Ads
/// requests; study `fails` fails after 5 searches.
async fn pull(Query(Budget { budget }): Query<Budget>, Json(def): Json<Definition>) -> axum::response::Response {
	match budget < NEED {
		_ if def.category.as_ref().is_some_and(|c| c.study == "fails") => {
			(StatusCode::INTERNAL_SERVER_ERROR, [("x-billed", r#"{"places.search":5}"#)], Json(json!({ "code": "internal", "error": "the day's quota ran out" }))).into_response()
		}
		true => (
			StatusCode::PAYMENT_REQUIRED,
			[("x-billed", "{}".to_owned()), ("x-need", NEED.to_string())],
			Json(json!({ "code": "unaffordable", "error": "needs more" })),
		)
			.into_response(),
		false => ([("x-billed", json!({ "places.search": NEED, "ads.keyword_ideas": 2 }).to_string())], PAYLOAD).into_response(),
	}
}

struct Conductor {
	_db: TestDb,
	app: Router,
	seen: Log,
	b: Browser,
}

impl Conductor {
	/// Placement up, Lyon drawn, a pull's items priced as given, the caller holding `balance`.
	async fn new(permissions: PermissionSet, price: Option<i64>, after: Option<i64>, balance: i64) -> Self {
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
		let prices: Vec<_> = std::iter::once((Item::Placement(PlacementRequest::Search), price))
			.chain(<(AdsRequest, DataForSeoTask)>::items().into_iter().map(|i| (i, after)))
			.collect();
		p.set_prices(&prices, &[], "admin@localhost", now).await.unwrap().unwrap();
		let placement = Router::new()
			.route("/trades.json", get(|| async { Json(json!({ "trades": ["plumbing", "carpentry"], "countries": ["FR"] })) }))
			.route("/studies", post(study))
			.route("/studies/pull", post(pull));
		let (base, seen) = upstream(placement).await;
		let app = app_with(p.clone(), permissions, EMAIL, &base, Default::default());
		let mut b = Browser::default();
		b.sign_in(&app).await;
		let mut c = Self { _db: db, app, seen, b };
		c.call(Method::GET, "/api/v1/tokens", Value::Null).await; // the gate records who they are, whom the area is by
		p.change_balance(DevIdentity::named(EMAIL), Change::Set { n: balance, note: None }, "admin@localhost", now)
			.await
			.unwrap()
			.unwrap();
		let lyon = Area {
			slug: "lyon".into(),
			name: "Lyon".into(),
			country: "FR".into(),
			center: LatLng { lat: 45.76, lng: 4.84 },
			shape: Shape::City {
				place_id: "ChIJl4foalHq9EcR8CG75CqrCAQ".into(),
				viewport: Viewport {
					south: 45.7,
					west: 4.77,
					north: 45.81,
					east: 4.9,
				},
			},
			margin_m: 2000,
			created_by: DevIdentity::named(EMAIL),
			created_at: now,
		};
		sa_panel::store::areas::put_area(&mut c._db.pool().await.acquire().await.unwrap(), &lyon).await.unwrap();
		c
	}

	async fn send(&mut self, method: Method, uri: &str, body: Value) -> (StatusCode, Vec<u8>) {
		let body = match body {
			Value::Null => Body::empty(),
			v => Body::from(v.to_string()),
		};
		let (status, _, bytes) = self.b.send_body(&self.app, method, uri, &[("content-type", "application/json")], true, body).await;
		(status, bytes.to_vec())
	}

	async fn call(&mut self, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
		let (status, bytes) = self.send(method, uri, body).await;
		(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
	}

	/// A trade of the caller's, set up for `country` as `set_up` says: its `<id>.<country>`.
	async fn trade(&mut self, name: &str, country: &str, set_up: Value) -> String {
		let (_, trade) = self.call(Method::POST, "/api/v1/trades", json!({ "name": name })).await;
		let id = trade["id"].as_i64().unwrap();
		let (status, body) = self.call(Method::PUT, &format!("/api/v1/trades/{id}/{country}"), set_up).await;
		assert_eq!(status, StatusCode::OK, "{body}");
		format!("{id}.{country}")
	}

	async fn balance(&mut self) -> Value {
		self.call(Method::GET, "/api/v1/tokens", Value::Null).await.1["balance"].clone()
	}

	/// What placement was asked since the last look: the path and the definition, asserted as the caller.
	fn asked(&self) -> Vec<(String, Definition)> {
		let seen = std::mem::take(&mut *self.seen.lock().unwrap());
		seen.into_iter()
			.filter(|s| s.method == Method::POST)
			.map(|s| {
				let asserted = s.assertion(Service::Placement, None);
				assert!(
					matches!(asserted.principal, Principal::User(u) if u.sub == DevIdentity::named(EMAIL).to_string()),
					"as the caller, by their uuid"
				);
				(s.path_and_query, serde_json::from_slice(&s.body).unwrap())
			})
			.collect()
	}
}

fn set_up(study: Option<&str>, primary: Option<&str>, keywords: &[&str]) -> Value {
	json!({ "keywords": keywords, "google_primary": primary, "google_additional": [], "study": study })
}

fn plumbing() -> Value {
	set_up(Some("plumbing"), Some("gcid:plumber"), &["plombier", "fuite d'eau"])
}

#[tokio::test]
async fn a_study_is_built_from_the_conductors_rows_and_its_payload_streamed_through() {
	let mut c = Conductor::new(operator(), Some(8), Some(AFTER), 1000).await;
	let plumbing = c.trade("Plumbing", "FR", plumbing()).await;

	let (status, body) = c.send(Method::GET, &format!("/api/v1/placement/study?area=lyon&category={plumbing}"), Value::Null).await;
	assert_eq!((status, String::from_utf8(body).unwrap().as_str()), (StatusCode::OK, PAYLOAD), "unread, as placement wrote it");
	let [(path, def)] = c.asked().try_into().unwrap();
	assert_eq!(path, "/studies");
	let category = def.category.unwrap();
	assert_eq!(
		(category.study.as_str(), category.google_primary.name.as_str(), def.area.slug()),
		("plumbing", "Plombier", "lyon"),
		"named in the country's language"
	);

	let (status, _) = c.send(Method::GET, "/api/v1/placement/study?area=lyon", Value::Null).await;
	assert_eq!(status, StatusCode::OK);
	let [(_, def)] = c.asked().try_into().unwrap();
	assert_eq!(def.category, None, "the area's grid alone");

	let (status, catalog) = c.call(Method::GET, "/api/v1/placement/catalog", Value::Null).await;
	assert_eq!((status, catalog), (StatusCode::OK, json!({ "trades": ["plumbing", "carpentry"], "countries": ["FR"] })));
}

#[tokio::test]
async fn a_category_not_set_up_to_be_studied_is_refused_by_why() {
	let mut c = Conductor::new(operator(), Some(8), Some(AFTER), 1000).await;
	for (name, country, set_up, code) in [
		("Roofing", "FR", set_up(None, Some("gcid:roofing_contractor"), &["couvreur"]), "no_study"),
		("Painting", "FR", set_up(Some("painting"), None, &["peintre"]), "no_google_category"),
		("Locksmith", "FR", set_up(Some("locksmith"), Some("gcid:locksmith"), &[]), "no_keywords"),
		("Cleaning", "US", set_up(Some("cleaning"), Some("gcid:plumber"), &["cleaner"]), "another_country"),
	] {
		let category = c.trade(name, country, set_up).await;
		let (status, body) = c.call(Method::GET, &format!("/api/v1/placement/study?area=lyon&category={category}"), Value::Null).await;
		assert_eq!((status, &body["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!(code)), "{name}: {body}");
		assert!(c.asked().is_empty(), "{name}: placement is not asked");
	}
	assert_eq!(c.send(Method::GET, "/api/v1/placement/study?area=lyon&category=9999.FR", Value::Null).await.0, StatusCode::NOT_FOUND, "not a category of the caller's");
	assert_eq!(c.send(Method::GET, "/api/v1/placement/study?area=paris", Value::Null).await.0, StatusCode::NOT_FOUND, "no such area");

	let carpentry = c.trade("Carpentry", "FR", set_up(Some("carpentry"), Some("gcid:plumber"), &["menuisier"])).await;
	let (status, body) = c.call(Method::GET, &format!("/api/v1/placement/study?area=lyon&category={carpentry}"), Value::Null).await;
	assert_eq!((status, &body["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!("unstudied")), "placement's own word: {body}");
}

#[tokio::test]
async fn a_pull_is_told_what_the_balance_affords_and_charged_what_it_bought() {
	let mut c = Conductor::new(admin(), Some(8), Some(AFTER), 1000).await;
	let plumbing = c.trade("Plumbing", "FR", plumbing()).await;

	let (status, body) = c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=lyon&category={plumbing}"), Value::Null).await;
	assert_eq!((status, String::from_utf8(body).unwrap().as_str()), (StatusCode::OK, PAYLOAD));
	let [(path, def)] = c.asked().try_into().unwrap();
	assert_eq!(path, "/studies/pull?budget=125", "1000 tokens at 8 a search");

	assert_eq!(c.balance().await, 1000 - 38 * 8 - 2 * AFTER);
	let (_, ledger) = c.call(Method::GET, "/api/v1/tokens/ledger?limit=1", Value::Null).await;
	let charge = &ledger["entries"][0];
	assert_eq!(
		(&charge["delta"], &charge["kind"], &charge["service"], &charge["key"]),
		(&json!(-310), &json!("charge"), &json!("placement"), &json!(def.id())),
		"{charge}"
	);
}

#[tokio::test]
async fn a_balance_below_the_sweep_buys_nothing_and_is_told_the_need() {
	let mut c = Conductor::new(admin(), Some(8), Some(AFTER), 100).await;
	let plumbing = c.trade("Plumbing", "FR", plumbing()).await;

	let (status, body) = c.call(Method::POST, &format!("/api/v1/placement/study/pull?area=lyon&category={plumbing}"), Value::Null).await;
	assert_eq!(
		(status, body),
		(
			StatusCode::PAYMENT_REQUIRED,
			json!({ "code": "unaffordable", "error": "this pull needs at least 304 tokens (38 searches at 8), and you hold 100" })
		)
	);
	assert_eq!(c.asked()[0].0, "/studies/pull?budget=12");
	assert_eq!(c.balance().await, 100);
}

#[tokio::test]
async fn a_failed_pull_is_charged_what_it_bought_and_owes_it_all_back() {
	let mut c = Conductor::new(admin(), Some(8), Some(AFTER), 1000).await;
	let fails = c.trade("Plumbing", "FR", set_up(Some("fails"), Some("gcid:plumber"), &["plombier"])).await;

	let (status, _) = c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=lyon&category={fails}"), Value::Null).await;
	assert_eq!(status, StatusCode::BAD_GATEWAY);
	let [(_, def)] = c.asked().try_into().unwrap();
	let (_, tokens) = c.call(Method::GET, "/api/v1/tokens", Value::Null).await;
	assert_eq!(tokens["balance"], 1000 - 5 * 8);
	let owed = &tokens["kickbacks"][0];
	assert_eq!((&owed["amount"], &owed["service"], &owed["key"]), (&json!(40), &json!("placement"), &json!(def.id())), "{tokens}");
}

#[tokio::test]
async fn what_the_searches_leave_pays_for_the_rest_and_no_more() {
	let mut c = Conductor::new(admin(), Some(8), Some(AFTER), 38 * 8 + 1).await;
	let plumbing = c.trade("Plumbing", "FR", plumbing()).await;

	assert_eq!(c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=lyon&category={plumbing}"), Value::Null).await.0, StatusCode::OK);
	assert_eq!(c.balance().await, 0, "the Ads requests past it are let go");
}

#[tokio::test]
async fn an_unpriced_pull_is_refused_before_placement_is_asked() {
	for (price, after) in [(None, Some(AFTER)), (Some(8), None)] {
		let mut c = Conductor::new(admin(), price, after, 1000).await;
		let plumbing = c.trade("Plumbing", "FR", plumbing()).await;

		let (status, _) = c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=lyon&category={plumbing}"), Value::Null).await;
		assert_eq!(status, StatusCode::CONFLICT, "{price:?}, {after:?}");
		assert!(c.asked().is_empty());
		assert_eq!(c.balance().await, 1000);
	}
}

#[tokio::test]
async fn reading_studies_is_not_pulling_them() {
	let mut c = Conductor::new(operator(), Some(8), Some(AFTER), 1000).await;
	let plumbing = c.trade("Plumbing", "FR", plumbing()).await;

	let (status, _) = c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=lyon&category={plumbing}"), Value::Null).await;
	assert_eq!(status, StatusCode::FORBIDDEN);
	assert!(c.asked().is_empty());
}

#[tokio::test]
async fn a_person_reads_the_prices_and_sets_above_what_they_are_asked() {
	let mut c = Conductor::new(admin(), Some(8), Some(AFTER), 1000).await;

	let (_, tokens) = c.call(Method::GET, "/api/v1/tokens", Value::Null).await;
	assert_eq!((&tokens["prices"]["placement.search"], &tokens["confirm_above"]), (&json!(8), &json!(100)), "{tokens}");
	assert!(tokens["prices"].get("map.map").is_none(), "an unpriced item is not read");

	assert_eq!(c.send(Method::PUT, "/api/v1/tokens/settings", json!({ "confirm_above": 250 })).await.0, StatusCode::NO_CONTENT);
	assert_eq!(c.send(Method::PUT, "/api/v1/tokens/settings", json!({ "confirm_above": -1 })).await.0, StatusCode::BAD_REQUEST);
	assert_eq!(c.call(Method::GET, "/api/v1/tokens", Value::Null).await.1["confirm_above"], 250);
}

#[tokio::test]
async fn without_placement_its_studies_are_not_connected() {
	let db = TestDb::create().await;
	let (sign_in, _) = sa_server::testing::conductor(panel(&db).await, operator(), EMAIL, "http://127.0.0.1:9");
	let app = sa_server::http::app(sign_in, None, None, Default::default(), None, None, None, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _, body) = b.send_body(&app, Method::GET, "/api/v1/placement/catalog", &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{}", String::from_utf8_lossy(&body));
	assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["error"], "placement not connected");
}
