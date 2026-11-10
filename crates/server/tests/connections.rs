//! Who pays for a Places request: the GBP's gmail's own key, its owner's other gmails' keys, then
//! the shared ones at the admin's price in tokens, refused before buying below it; and a gmail's
//! secrets, which never come back.

use std::{
	collections::HashSet,
	sync::{Arc, Mutex},
};

use axum::{
	Json, Router,
	body::Body,
	extract::{Path, State},
	http::{HeaderMap, Method, StatusCode},
	response::{IntoResponse, Response},
	routing::get,
};
use sa_panel::{
	Panel,
	import::Export,
	seal::DataKey,
	testing::{TestDb, operator, panel_on},
};
use sa_places::{Google, Keys, Sku};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};

const PROCOPE: &str = "ChIJprocopeprocopeproc";
const FLORE: &str = "ChIJfloreflorefloreflo";
const SHARED: &str = "shared-rotation-key";

/// Places, as it answers each key: a spent window, a key it refuses, else the place.
#[derive(Clone, Default)]
struct Stub {
	/// The key of each billed request, in order; probes are not billed.
	billed: Arc<Mutex<Vec<String>>>,
	spent: Arc<Mutex<HashSet<String>>>,
}

async fn details(State(s): State<Stub>, headers: HeaderMap, Path(id): Path<String>) -> Response {
	let key = headers["x-goog-api-key"].to_str().unwrap().to_owned();
	if key.starts_with("bad") {
		let refused = json!({"error": {"code": 400, "status": "INVALID_ARGUMENT", "message": "API key not valid. Please pass a valid API key.",
			"details": [{"reason": "API_KEY_INVALID"}]}});
		return (StatusCode::BAD_REQUEST, Json(refused)).into_response();
	}
	if headers["x-goog-fieldmask"] == "id" {
		return Json(json!({ "id": id })).into_response();
	}
	s.billed.lock().unwrap().push(key.clone());
	if s.spent.lock().unwrap().contains(&key) {
		let reset = jiff::Timestamp::now().as_second() as u64;
		let refusal = json!({"error": {"code": 429, "status": "RESOURCE_EXHAUSTED", "details": [{"reason": "RATE_LIMIT_EXCEEDED", "metadata": {
			"window_start_time": reset.to_string(), "quota_limit": "GetPlaceRequestPerDayPerProject"}}]}});
		return (StatusCode::TOO_MANY_REQUESTS, Json(refusal)).into_response();
	}
	Json(
		json!({ "id": id, "displayName": { "text": id }, "formattedAddress": "Paris", "location": { "latitude": 48.85, "longitude": 2.33 },
		"addressComponents": [{ "shortText": "FR", "types": ["country", "political"] }] }),
	)
	.into_response()
}

struct World {
	stub: Stub,
	panel: Panel,
	app: Router,
	b: Browser,
	db: TestDb,
}

impl World {
	/// Signed in, tokens open (the caller's first balance 500), the shared rotation one key.
	async fn new() -> Self {
		let stub = Stub::default();
		let places = Router::new().route("/v1/places/{id}", get(details)).with_state(stub.clone());
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let base = format!("http://{}/v1/places", listener.local_addr().unwrap());
		tokio::spawn(async move { axum::serve(listener, places).await.unwrap() });

		let db = TestDb::create().await;
		let google = Google::try_new(Keys::new(vec![SHARED.to_owned()]), Arc::new(db.store().await)).unwrap().at(base);
		let panel = panel_on(db.store().await, DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(), google);
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap();
		let who = DevIdentity {
			user_id: DevIdentity::named("dev@localhost"),
			permissions: operator(),
			email: "dev@localhost".to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let app = http::app(SignIn::new(panel.clone(), Concierge::dev(who), config), None, None, Default::default(), None, None, None);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self { stub, panel, app, b, db }
	}

	async fn call(&mut self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
		let json = [("content-type", "application/json")];
		let (status, _, bytes) = self
			.b
			.send_body(
				&self.app,
				method,
				uri,
				if body.is_some() { &json } else { &[] },
				true,
				body.map_or(Body::empty(), |b| Body::from(b.to_string())),
			)
			.await;
		(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
	}

	async fn gmail(&mut self, gmail: &str) -> i64 {
		let (status, added) = self.call(Method::POST, "/api/v1/gmails", Some(json!({ "gmail": gmail }))).await;
		assert_eq!(status, StatusCode::CREATED, "{added}");
		added["id"].as_i64().unwrap()
	}

	async fn price(&self, details: i64) {
		self.panel
			.set_prices(&[(Sku::Details, details), (Sku::Search, 2 * details)], "admin@localhost", jiff::Timestamp::now())
			.await
			.unwrap()
			.unwrap();
	}

	async fn add_gbp(&mut self, gmail: i64, place: &str) -> (StatusCode, Value) {
		self.call(Method::POST, &format!("/api/v1/gmails/{gmail}/gmbs"), Some(json!({ "place_id": place, "state": "verified" })))
			.await
	}

	/// The keys the billed requests went out with since asked last.
	fn billed(&self) -> Vec<String> {
		std::mem::take(&mut *self.stub.billed.lock().unwrap())
	}

	async fn balance(&mut self) -> i64 {
		let (status, tokens) = self.call(Method::GET, "/api/v1/tokens", None).await;
		assert_eq!(status, StatusCode::OK, "{tokens}");
		tokens["balance"].as_i64().unwrap()
	}
}

#[tokio::test]
async fn the_gmails_key_goes_first_then_the_owners_others_then_the_shared_ones_paid_in_tokens() {
	let mut w = World::new().await;
	w.price(3).await;
	let (a, b) = (w.gmail("a@gmail.com").await, w.gmail("b@gmail.com").await);
	for (gmail, key) in [(a, "own-key-of-a"), (b, "own-key-of-b")] {
		let (status, set) = w.call(Method::PUT, &format!("/api/v1/gmails/{gmail}/maps_key"), Some(json!({ "key": key }))).await;
		assert_eq!(status, StatusCode::OK, "{set}");
	}
	let start = w.balance().await;

	w.stub.spent.lock().unwrap().insert("own-key-of-a".to_owned());
	let (status, gbp) = w.add_gbp(a, PROCOPE).await;
	assert_eq!(status, StatusCode::CREATED, "{gbp}");
	assert_eq!(w.billed(), ["own-key-of-a", "own-key-of-b"], "a's own, spent; then b's, the same owner's");
	assert_eq!(w.balance().await, start, "an own key is free");

	w.stub.spent.lock().unwrap().insert("own-key-of-b".to_owned());
	let (status, gbp) = w.add_gbp(b, FLORE).await;
	assert_eq!(status, StatusCode::CREATED, "{gbp}");
	assert_eq!(w.billed(), ["own-key-of-b", SHARED], "a's sits out from before; b's spent now; then the shared key");
	assert_eq!(w.balance().await, start - 3, "the shared key at its price");
	let (_, ledger) = w.call(Method::GET, "/api/v1/tokens/ledger?limit=10", None).await;
	let charge = &ledger["entries"][0];
	assert_eq!((charge["delta"].as_i64(), charge["service"].as_str()), (Some(-3), Some("places")), "{ledger}");
	assert_eq!(charge["label"], format!("Places details · {FLORE}"), "{ledger}");

	let (status, found) = w.call(Method::GET, &format!("/api/v1/gmbs/resolve?place={FLORE}&gmail={a}"), None).await;
	assert_eq!(status, StatusCode::OK, "{found}");
	assert_eq!(w.billed(), Vec::<String>::new(), "kept: Google is not asked again");
	assert_eq!(w.balance().await, start - 3, "and nothing is charged for it");
}

#[tokio::test]
async fn below_the_price_or_unpriced_nothing_is_bought() {
	let mut w = World::new().await;
	let gmail = w.gmail("a@gmail.com").await;
	let (status, refused) = w.add_gbp(gmail, PROCOPE).await;
	assert_eq!(status, StatusCode::CONFLICT, "not priced: {refused}");

	let balance = w.balance().await;
	w.price(balance + 1).await;
	let (status, refused) = w.add_gbp(gmail, PROCOPE).await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");
	assert!(refused["error"].as_str().unwrap().contains("Maps key"), "the remedy named: {refused}");
	assert_eq!(w.billed(), Vec::<String>::new(), "Google never asked");
	assert_eq!(w.balance().await, balance);

	w.price(balance).await;
	let (status, gbp) = w.add_gbp(gmail, PROCOPE).await;
	assert_eq!(status, StatusCode::CREATED, "the whole balance is enough: {gbp}");
	assert_eq!(w.balance().await, 0);
}

#[tokio::test]
async fn a_gmails_secrets_are_checked_before_kept_and_never_come_back() {
	let mut w = World::new().await;
	let gmail = w.gmail("a@gmail.com").await;
	let key = "own-key-never-shown-1234";

	let (status, refused) = w.call(Method::PUT, &format!("/api/v1/gmails/{gmail}/maps_key"), Some(json!({ "key": "bad-key-of-a" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
	assert!(refused["error"].as_str().unwrap().contains("API key not valid"), "Google's reason: {refused}");
	let imap = json!({ "host": "localhost", "username": "a@gmail.com", "password": "app-password-never-shown" });
	let (status, refused) = w.call(Method::PUT, &format!("/api/v1/gmails/{gmail}/imap"), Some(imap)).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "only a public host is dialled: {refused}");
	let (_, none) = w.call(Method::GET, &format!("/api/v1/gmails/{gmail}/connections"), None).await;
	assert_eq!((&none["imap"], &none["maps_key"]), (&Value::Null, &Value::Null), "a refusal stores nothing: {none}");

	let (status, set) = w.call(Method::PUT, &format!("/api/v1/gmails/{gmail}/maps_key"), Some(json!({ "key": key }))).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	assert_eq!(set["maps_key"]["last4"], "1234");
	for uri in [
		format!("/api/v1/gmails/{gmail}/connections"),
		"/api/v1/gmails".to_owned(),
		format!("/api/v1/gmails/{gmail}/maps_key/test"),
	] {
		let method = if uri.ends_with("/test") { Method::POST } else { Method::GET };
		let (status, read) = w.call(method, &uri, None).await;
		assert_eq!(status, StatusCode::OK, "{uri}: {read}");
		assert!(!read.to_string().contains(key), "{uri} answers the key: {read}");
	}
	for suffix in ["", "-wal"] {
		let file = std::fs::read(format!("{}{suffix}", w.db.path().display())).unwrap_or_default();
		assert!(!file.windows(key.len()).any(|w| w == key.as_bytes()), "the key in the clear in the database{suffix}");
	}

	let (status, _) = w.call(Method::DELETE, &format!("/api/v1/gmails/{gmail}/maps_key"), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (_, gone) = w.call(Method::GET, &format!("/api/v1/gmails/{gmail}/connections"), None).await;
	assert_eq!(gone["maps_key"], Value::Null, "{gone}");
}
