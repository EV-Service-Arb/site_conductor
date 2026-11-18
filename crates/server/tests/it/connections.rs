//! Who pays for a Places request: the GBP's gmail's own key, its owner's other gmails' keys, then
//! the shared ones at the admin's price in tokens, refused before buying below it; and a gmail's
//! secrets, which never come back.

use std::{
	collections::HashSet,
	sync::{Arc, Mutex},
};

use axum::{
	Json, Router,
	extract::{Path, State},
	http::{HeaderMap, Method, StatusCode},
	response::{IntoResponse, Response},
	routing::get,
};
use sa_core::prices::{Item, MapLoad, PlacesRequest};
use sa_panel::{
	Panel,
	import::Export,
	seal::DataKey,
	testing::{TestDb, admin, operator, panel_on},
};
use sa_places::{Google, Keys};
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
		Self::holding(operator()).await
	}

	async fn holding(permissions: sa_auth::PermissionSet) -> Self {
		let stub = Stub::default();
		let places = Router::new().route("/v1/places/{id}", get(details)).with_state(stub.clone());
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let base = format!("http://{}/v1/places", listener.local_addr().unwrap());
		tokio::spawn(async move { axum::serve(listener, places).await.unwrap() });

		let db = TestDb::create().await;
		let store = db.store().await;
		let google = Google::try_new(Keys::new(vec![("house".to_owned(), SHARED.to_owned())]), Arc::new(sa_places::InOrder), Arc::new(store.clone()))
			.unwrap()
			.spend(true)
			.at(base);
		let panel = panel_on(
			store,
			DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(),
			google,
			sa_places::Accounts::new(vec![sa_panel::testing::account("house", Some(SHARED), Some("browser-key"))]),
		);
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap().unwrap();
		let who = DevIdentity {
			user_id: DevIdentity::named("dev@localhost"),
			permissions,
			email: "dev@localhost".to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let app = http::app(
			SignIn::new(panel.clone(), Concierge::dev(who), config),
			None,
			None,
			Default::default(),
			None,
			None,
			None,
			None,
			None,
		);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self { stub, panel, app, b, db }
	}

	async fn call(&mut self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
		self.b.call(&self.app, method, uri, body).await
	}

	async fn gmail(&mut self, gmail: &str) -> i64 {
		let (status, added) = self.call(Method::POST, "/api/v1/gmails", Some(json!({ "gmail": gmail }))).await;
		assert_eq!(status, StatusCode::CREATED, "{added}");
		added["id"].as_i64().unwrap()
	}

	async fn price(&self, details: i64) {
		sa_panel::testing::priced(
			&self.panel,
			&[
				(Item::Places(PlacesRequest::Details), details),
				(Item::Places(PlacesRequest::Search), 2 * details),
				(Item::Places(PlacesRequest::Autocomplete), details),
			],
		)
		.await;
	}

	/// Of the caller's trade, added the first time.
	async fn add_gbp(&mut self, gmail: i64, place: &str) -> (StatusCode, Value) {
		let (_, trades) = self.call(Method::GET, "/api/v1/trades", None).await;
		let trade = match &trades["trades"][0]["id"] {
			Value::Null => self.call(Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await.1["id"].clone(),
			id => id.clone(),
		};
		self.call(
			Method::POST,
			&format!("/api/v1/gmails/{gmail}/gmbs"),
			Some(json!({ "place_id": place, "state": "verified", "trade": trade })),
		)
		.await
	}

	/// The keys the billed requests went out with since asked last.
	async fn price_map(&self, tokens: i64) {
		sa_panel::testing::priced(&self.panel, &[(Item::Map(MapLoad::Map), tokens)]).await;
	}

	async fn load_map(&mut self) -> (StatusCode, Value) {
		self.call(Method::POST, "/api/v1/map/load", Some(json!({ "account": "house" }))).await
	}

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
async fn below_the_price_nothing_is_bought() {
	let mut w = World::new().await;
	let gmail = w.gmail("a@gmail.com").await;
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
async fn a_map_is_bought_before_it_is_built() {
	let mut w = World::new().await;
	let balance = w.balance().await;
	let (status, free) = w.load_map().await;
	assert_eq!((status, &free["balance"]), (StatusCode::OK, &json!(balance)), "never priced: free, not refused: {free}");

	w.price_map(balance + 1).await;
	let (status, refused) = w.load_map().await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");
	assert_eq!(w.balance().await, balance);

	w.price_map(3).await;
	let (status, bought) = w.load_map().await;
	assert_eq!((status, &bought["balance"]), (StatusCode::OK, &json!(balance - 3)), "{bought}");
	let (_, ledger) = w.call(Method::GET, "/api/v1/tokens/ledger?limit=1", None).await;
	let charge = &ledger["entries"][0];
	assert_eq!((charge["delta"].as_i64(), charge["service"].as_str()), (Some(-3), Some("map")), "{ledger}");
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

#[tokio::test]
async fn every_priced_item_is_the_admins_to_price_and_the_archive_is_told() {
	let mut w = World::holding(admin()).await;
	let (status, read) = w.call(Method::GET, "/api/v1/admin/tokens/prices", None).await;
	assert_eq!(status, StatusCode::OK, "{read}");
	let listed: Vec<_> = read["prices"].as_array().unwrap().iter().map(|p| p["item"].as_str().unwrap().to_owned()).collect();
	assert_eq!(listed, Item::all().map(|i| i.to_string()).collect::<Vec<_>>(), "the catalog, whole and in order");
	let row = |read: &Value, item: &str| read["prices"].as_array().unwrap().iter().find(|p| p["item"] == item).unwrap().clone();
	let of = |read: &Value, item: &str| row(read, item)["price"].clone();
	assert_eq!(
		(of(&read, "places.details"), of(&read, "walk.open"), row(&read, "walk.open")["multiplier"]["multiplier"].clone()),
		(Value::Null, json!(7), json!(1.0)),
		"Places unsold, a walk as the archive metered it: its price its cost, at ×1"
	);

	let (status, set) = w
		.call(
			Method::PUT,
			"/api/v1/admin/tokens/prices",
			Some(json!({ "multipliers": { "places.details": 1.5, "walk.step": 0 }, "costs": { "places.details": 2, "map.map": 7 } })),
		)
		.await;
	assert_eq!(status, StatusCode::OK, "{set}");
	assert_eq!(
		(of(&set, "places.details"), of(&set, "walk.step"), of(&set, "walk.open")),
		(json!(3), json!(0), json!(7)),
		"2 tokens × 1.5; only those given change"
	);
	let map = row(&set, "map.map");
	assert_eq!((&map["price"], &map["cost"]["tokens"]), (&json!(7), &json!(7.0)), "a cost at the first multiplier, ×1: {map}");
	let told = w.panel.snapshot(jiff::Timestamp::now()).await.unwrap().unwrap().prices;
	assert_eq!(told.get(&"walk.open".parse().unwrap()), Some(&7));
	assert_eq!(told.get(&"walk.step".parse().unwrap()), Some(&0), "×0: free");

	for refused in [
		json!({ "multipliers": { "walk.fly": 1 }, "costs": {} }),
		json!({ "multipliers": { "walk.open": -1 }, "costs": {} }),
		json!({ "multipliers": {}, "costs": { "map.map": -1 } }),
	] {
		let (status, body) = w.call(Method::PUT, "/api/v1/admin/tokens/prices", Some(refused.clone())).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}: {body}");
	}
	let (_, after) = w.call(Method::GET, "/api/v1/admin/tokens/prices", None).await;
	assert_eq!(of(&after, "walk.open"), json!(7), "a refusal changes nothing");

	let mut op = World::new().await;
	let (status, _) = op.call(Method::GET, "/api/v1/admin/tokens/prices", None).await;
	assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn every_paid_answer_and_map_is_counted_once_on_whose_bill() {
	let mut w = World::holding(admin()).await;
	w.price(0).await;
	let gmail = w.gmail("a@gmail.com").await;
	let (status, gbp) = w.add_gbp(gmail, PROCOPE).await;
	assert_eq!(status, StatusCode::CREATED, "{gbp}");
	let (status, set) = w.call(Method::PUT, &format!("/api/v1/gmails/{gmail}/maps_key"), Some(json!({ "key": "own-key-of-a" }))).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let (status, gbp) = w.add_gbp(gmail, FLORE).await;
	assert_eq!(status, StatusCode::CREATED, "{gbp}");
	let (status, kept) = w.call(Method::GET, &format!("/api/v1/gmbs/resolve?place={FLORE}&gmail={gmail}"), None).await;
	assert_eq!(status, StatusCode::OK, "{kept}");
	w.price_map(0).await;
	for _ in 0..2 {
		assert_eq!(w.load_map().await.0, StatusCode::OK);
	}

	let now = jiff::Timestamp::now().as_millisecond();
	let (status, usage) = w.call(Method::GET, &format!("/api/v1/admin/usage?from={}&to={}", now - 3_600_000, now + 60_000), None).await;
	assert_eq!(status, StatusCode::OK, "{usage}");
	let units = |item: &str, bill: &str| -> i64 {
		usage["days"]
			.as_array()
			.unwrap()
			.iter()
			.filter(|d| d["item"] == item && d["bill"] == bill)
			.map(|d| d["units"].as_i64().unwrap())
			.sum()
	};
	assert_eq!(units("places.details", "ours"), 1, "the shared key's buy: {usage}");
	assert_eq!(units("places.details", "theirs"), 1, "the gmail's own key, and the kept answer not again: {usage}");
	assert_eq!(units("map.map", "ours"), 2, "{usage}");

	let mut op = World::new().await;
	assert_eq!(
		op.call(Method::GET, &format!("/api/v1/admin/usage?from={}&to={now}", now - 1), None).await.0,
		StatusCode::FORBIDDEN
	);
}

#[tokio::test]
async fn a_use_is_costed_at_the_cost_it_had_when_done() {
	let mut w = World::holding(admin()).await;
	let load = async |w: &mut World| w.load_map().await.0;
	let cost = async |w: &mut World, tokens: Value| {
		let (status, set) = w
			.call(
				Method::PUT,
				"/api/v1/admin/tokens/prices",
				Some(json!({ "multipliers": { "map.map": 0 }, "costs": { "map.map": tokens } })),
			)
			.await;
		assert_eq!(status, StatusCode::OK, "{set}");
	};
	cost(&mut w, json!(7)).await;
	assert_eq!(load(&mut w).await, StatusCode::OK);
	cost(&mut w, json!(14)).await;
	assert_eq!(load(&mut w).await, StatusCode::OK);
	cost(&mut w, Value::Null).await;
	assert_eq!(load(&mut w).await, StatusCode::CONFLICT, "without a cost, not sold");

	let now = jiff::Timestamp::now().as_millisecond();
	let (status, usage) = w.call(Method::GET, &format!("/api/v1/admin/usage?from={}&to={}", now - 3_600_000, now + 60_000), None).await;
	assert_eq!(status, StatusCode::OK, "{usage}");
	let maps: Vec<&Value> = usage["days"].as_array().unwrap().iter().filter(|d| d["item"] == "map.map").collect();
	let sum = |field: &str| maps.iter().map(|d| d[field].as_f64().unwrap_or(0.)).sum::<f64>();
	assert_eq!(sum("units"), 2.);
	assert!((sum("usd") - 0.021).abs() < 1e-9, "the first at 7 tokens, the second at 14, free to the person (×0): {usage}");
	assert_eq!(sum("uncosted"), 0., "{usage}");
}

#[tokio::test]
async fn a_spend_is_its_cost_times_the_multiplier_and_keeps_what_it_cost_us() {
	let mut w = World::holding(admin()).await;
	let (status, set) = w
		.call(
			Method::PUT,
			"/api/v1/admin/tokens/prices",
			Some(json!({ "multipliers": { "map.map": 2.5 }, "costs": { "map.map": 7 } })),
		)
		.await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let (_, before) = w.call(Method::GET, "/api/v1/tokens", None).await;
	assert_eq!((&before["prices"]["map.map"], &before["token_usd"]), (&json!(18), &json!(0.001)), "17.5, rounded up: {before}");
	assert_eq!(w.load_map().await.0, StatusCode::OK);
	let (_, after) = w.call(Method::GET, "/api/v1/tokens", None).await;
	assert_eq!(before["balance"].as_i64().unwrap() - after["balance"].as_i64().unwrap(), 18);

	let now = jiff::Timestamp::now().as_millisecond();
	let (status, flows) = w
		.call(Method::GET, &format!("/api/v1/admin/tokens/flows?from={}&to={}", now - 3_600_000, now + 60_000), None)
		.await;
	assert_eq!(status, StatusCode::OK, "{flows}");
	let spent: Vec<_> = flows["flows"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|f| f["kind"] == "spend")
		.map(|f| (f["tokens"].clone(), f["at_cost"].clone()))
		.collect();
	assert_eq!(spent, [(json!(-18), json!(-7.0))], "charged, and at cost: {flows}");
}
