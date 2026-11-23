//! Placement's studies (docs/ARCHITECTURE.md, Placement): the conductor builds a study's Definition
//! from its own areas and the caller's trades, and placement's library builds it over a work dir of
//! real Filosofi rows (`gmaps_optimal_placement::testing`); a pull is sold per Places search (Tokens
//! and GBPs): the library says what the study lacks, the conductor buys it into the queries cache
//! and charges the person for it.

use std::sync::{
	Arc,
	atomic::{AtomicBool, AtomicUsize, Ordering},
};

use axum::{
	Json, Router,
	body::Body,
	http::{Method, StatusCode},
	routing::post,
};
use gmaps_optimal_placement::testing;
use jiff::Timestamp;
use sa_auth::PermissionSet;
use sa_core::prices::{Item, PlacementRequest};
use sa_panel::{
	Panel,
	areas::Area,
	import::Export,
	seal::DataKey,
	testing::{TestDb, account, admin, operator, panel, panel_on},
	tokens::Change,
};
use sa_places::{Google, Keys, LatLng};
use sa_server::{
	concierge::DevIdentity,
	placement::Placement,
	testing::{Browser, upstream},
};
use serde_json::{Value, json};

const PLUMBER: &str = "gcid:plumber";
const EMAIL: &str = "dev-admin@localhost";
/// The first pages of the sweep over the fixture: two searches over two tiles. One more lies behind
/// the first of them.
const NEED: usize = 4;

struct Conductor {
	_db: TestDb,
	/// Placement's work dir.
	dir: tempfile::TempDir,
	panel: Panel,
	app: Router,
	/// Searches Places answered.
	searched: Arc<AtomicUsize>,
	/// What Places was sent.
	places: sa_server::testing::Log,
	b: Browser,
}

impl Conductor {
	/// Placement over its fixture and Places up, central Clermont drawn, a pull's search priced as
	/// given, the caller holding `balance`. Places answers every first page with one plumber, and the
	/// first of them with a page behind it.
	async fn new(permissions: PermissionSet, price: i64, balance: i64) -> Self {
		let db = TestDb::create().await;
		let searched = Arc::<AtomicUsize>::default();
		let (counted, behind) = (searched.clone(), Arc::new(AtomicBool::new(true)));
		let search = move |Json(body): Json<Value>| async move {
			counted.fetch_add(1, Ordering::SeqCst);
			if body.get("pageToken").is_some() {
				return Json(json!({ "places": [] }));
			}
			let plumber = json!({
				"id": "dupont", "displayName": {"text": "Dupont plombier"}, "rating": 4.5, "userRatingCount": 10,
				"location": {"latitude": testing::CENTER.lat, "longitude": testing::CENTER.lng},
			});
			match behind.swap(false, Ordering::SeqCst) {
				true => Json(json!({ "places": [plumber], "nextPageToken": "t" })),
				false => Json(json!({ "places": [plumber] })),
			}
		};
		let probe = |axum::extract::Path(id): axum::extract::Path<String>| async move { Json(json!({ "id": id })) };
		let (places, log) = upstream(Router::new().route("/v1/places:searchText", post(search)).route("/v1/places/{id}", axum::routing::get(probe))).await;
		let google = Google::try_new(Keys::new(vec![("house".to_owned(), "k".to_owned())]), Arc::new(sa_places::InOrder), Arc::new(db.store().await))
			.unwrap()
			.spend(true)
			.at(format!("{places}/v1/places"));
		let p = panel_on(
			db.store().await,
			DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(),
			google,
			sa_places::Accounts::new(vec![account("house", Some("k"), None)], sa_panel::testing::slices()),
		);
		p.import_review_archive(Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		})
		.await
		.unwrap()
		.unwrap();
		let now = jiff::Timestamp::now();
		sa_panel::testing::priced(&p, &[(Item::Placement(PlacementRequest::Search), price)]).await;
		let dir = tempfile::tempdir().unwrap();
		testing::fixture(dir.path()).unwrap();
		let (sign_in, forward) = sa_server::testing::conductor(p.clone(), permissions, EMAIL, &"http://127.0.0.1:9".parse().unwrap());
		let app = sa_server::http::app(
			sign_in,
			Some(forward),
			None,
			Default::default(),
			None,
			None,
			None,
			sa_server::testing::sandbox(),
			Some(Placement::new(dir.path().to_owned())),
		);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		let mut c = Self {
			_db: db,
			dir,
			panel: p.clone(),
			app,
			searched,
			places: log,
			b,
		};
		c.call(Method::GET, "/api/v1/tokens", Value::Null).await; // the gate records who they are, whom the area is by
		p.change_balance(DevIdentity::named(EMAIL), Change::Set { n: balance, note: None }, "admin@localhost", now)
			.await
			.unwrap()
			.unwrap();
		for (slug, country, lat, lng, radius_m) in [
			("clermont", "FR", testing::CENTER.lat, testing::CENTER.lng, testing::RADIUS_M as i64),
			("london", "GB", 51.5, -0.12, 6000),
		] {
			let area = Area {
				slug: slug.into(),
				name: slug.into(),
				country: country.into(),
				center: LatLng { lat, lng },
				radius_m,
				city: None,
				created_by: DevIdentity::named(EMAIL),
				created_at: now,
			};
			sa_panel::store::areas::put_area(&mut c._db.pool().await.acquire().await.unwrap(), &area).await.unwrap();
		}
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

	/// `trade` set up for `country` as `settings` says: its head's version id.
	async fn category(&mut self, trade: &str, country: &str, settings: Value) -> i64 {
		let (status, body) = self.call(Method::PUT, &format!("/api/v1/categories/{trade}/{country}"), settings).await;
		assert_eq!(status, StatusCode::OK, "{body}");
		body["head"]["id"].as_i64().unwrap()
	}

	async fn balance(&mut self) -> Value {
		self.call(Method::GET, "/api/v1/tokens", Value::Null).await.1["balance"].clone()
	}

	fn searched(&self) -> usize {
		self.searched.load(Ordering::SeqCst)
	}

	/// The ids of the definitions placement studied, as its work dir keeps them for `fit`.
	fn studied(&self) -> Vec<String> {
		let mut ids: Vec<String> = std::fs::read_dir(self.dir.path().join("studies"))
			.unwrap()
			.map(|e| e.unwrap().path().file_stem().unwrap().to_str().unwrap().to_owned())
			.collect();
		ids.sort();
		ids
	}
}

fn settings(keywords: &[(&str, Option<f64>)], demand: Value) -> Value {
	let keywords: Vec<Value> = keywords.iter().map(|(text, weight)| json!({ "text": text, "weight": weight })).collect();
	json!({ "keywords": keywords, "additional": [], "excluded": { "words": [], "types": [] }, "demand": demand })
}

fn plumbing() -> Value {
	settings(&[("plombier", Some(1.)), ("fuite d'eau", Some(0.5))], Value::Null)
}

#[tokio::test]
async fn a_study_is_built_from_the_conductors_rows() {
	let mut c = Conductor::new(operator(), 8, 1000).await;
	let plumbing = c.category(PLUMBER, "FR", plumbing()).await;

	let (status, payload) = c.call(Method::GET, &format!("/api/v1/placement/study?area=clermont&version={plumbing}"), Value::Null).await;
	assert_eq!(status, StatusCode::OK, "{payload}");
	assert_eq!(payload["name"], "Plombier·FR·clermont");
	assert_eq!(payload["notice"]["missing"], NEED, "nothing is held, and a read never buys: {payload}");
	assert_eq!(c.searched(), 0);

	let (status, payload) = c.call(Method::GET, "/api/v1/placement/study?area=clermont", Value::Null).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(
		(&payload["name"], &payload["trade"], &payload["notice"]),
		(&json!("clermont"), &Value::Null, &Value::Null),
		"the area's grid alone"
	);
	assert_eq!(c.studied().len(), 2, "both kept where `fit` harvests them");

	let (status, catalog) = c.call(Method::GET, "/api/v1/placement/catalog", Value::Null).await;
	assert_eq!((status, catalog), (StatusCode::OK, json!({ "countries": ["FR"] })));
}

#[tokio::test]
async fn a_category_not_to_be_studied_is_refused_by_why() {
	let mut c = Conductor::new(operator(), 8, 1000).await;
	let version = c.category(PLUMBER, "US", settings(&[("plumber", Some(1.))], Value::Null)).await;
	let (status, body) = c.call(Method::GET, &format!("/api/v1/placement/study?area=clermont&version={version}"), Value::Null).await;
	assert_eq!((status, &body["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!("another_country")), "{body}");
	assert_eq!(
		c.send(Method::GET, "/api/v1/placement/study?area=clermont&version=999999", Value::Null).await.0,
		StatusCode::NOT_FOUND,
		"not a version of the caller's"
	);
	assert_eq!(
		c.send(Method::GET, "/api/v1/placement/study?area=paris", Value::Null).await.0,
		StatusCode::NOT_FOUND,
		"no such area"
	);
	let (status, body) = c.call(Method::GET, "/api/v1/placement/study?area=london", Value::Null).await;
	assert_eq!(
		(status, &body["code"]),
		(StatusCode::UNPROCESSABLE_ENTITY, &json!("unstudied")),
		"placement holds no base for GB: {body}"
	);
	assert!(!c.dir.path().join("studies").exists(), "nothing was studied");
}

#[tokio::test]
async fn a_pull_buys_what_the_study_lacks_and_is_charged_for_it() {
	let mut c = Conductor::new(admin(), 8, 1000).await;
	let plumbing = c.category(PLUMBER, "FR", plumbing()).await;
	let study = format!("/api/v1/placement/study?area=clermont&version={plumbing}");
	assert_eq!(c.call(Method::GET, &study, Value::Null).await.1["notice"]["missing"], NEED, "read, and kept");

	let (status, payload) = c.call(Method::POST, &format!("/api/v1/placement/study/pull?area=clermont&version={plumbing}"), Value::Null).await;
	assert_eq!(status, StatusCode::OK, "{payload}");
	assert_eq!(payload["notice"], Value::Null, "built of what was bought: {payload}");
	assert_eq!(payload["trade"]["tiers"][0]["name"], "Plombier", "named in the country's language");
	assert_eq!(c.searched(), NEED + 1, "asked again after each round, until none is lacking");
	assert_eq!(c.call(Method::GET, &study, Value::Null).await.1, payload, "the pull's build is the one read since");

	assert_eq!(c.balance().await, 1000 - (NEED as i64 + 1) * 8);
	let (_, ledger) = c.call(Method::GET, "/api/v1/tokens/ledger?limit=1", Value::Null).await;
	let charge = &ledger["entries"][0];
	assert_eq!(
		(&charge["delta"], &charge["kind"], &charge["service"], &charge["key"]),
		(&json!(-(NEED as i64 + 1) * 8), &json!("charge"), &json!("placement"), &json!(c.studied()[0])),
		"{charge}"
	);
}

#[tokio::test]
async fn a_pull_buys_on_its_callers_own_maps_key_first_and_charges_only_the_rotation() {
	let mut c = Conductor::new(admin(), 8, 1000).await;
	let (status, gmail) = c.call(Method::POST, "/api/v1/gmails", json!({ "gmail": "a@gmail.com" })).await;
	assert_eq!(status, StatusCode::CREATED, "{gmail}");
	let (status, set) = c.call(Method::PUT, &format!("/api/v1/gmails/{}/maps_key", gmail["id"]), json!({ "key": "own-key-1234" })).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let plumbing = c.category(PLUMBER, "FR", plumbing()).await;

	let (status, payload) = c.call(Method::POST, &format!("/api/v1/placement/study/pull?area=clermont&version={plumbing}"), Value::Null).await;
	assert_eq!(status, StatusCode::OK, "{payload}");
	let keys: Vec<String> = c
		.places
		.lock()
		.unwrap()
		.iter()
		.filter(|s| s.path_and_query.ends_with(":searchText"))
		.map(|s| s.headers["x-goog-api-key"].to_str().unwrap().to_owned())
		.collect();
	assert_eq!(keys, vec!["own-key-1234"; NEED + 1], "every search on the caller's own key");
	assert_eq!(c.balance().await, 1000, "which is free");
}

#[tokio::test]
async fn a_balance_below_the_first_round_buys_nothing_and_is_told_the_need() {
	let mut c = Conductor::new(admin(), 8, 20).await;
	let plumbing = c.category(PLUMBER, "FR", plumbing()).await;

	let (status, body) = c.call(Method::POST, &format!("/api/v1/placement/study/pull?area=clermont&version={plumbing}"), Value::Null).await;
	assert_eq!(
		(status, body),
		(
			StatusCode::PAYMENT_REQUIRED,
			json!({ "code": "unaffordable", "error": "this pull needs at least 32 tokens (4 searches at 8), and you hold 20" })
		)
	);
	assert_eq!((c.searched(), c.balance().await), (0, json!(20)));
}

/// The budget holds at every search, not only at the first round; a pull stopped part-way is owed
/// back, and its retry buys only what the stopped one did not.
#[tokio::test]
async fn a_pull_past_its_budget_is_stopped_owed_back_and_its_retry_buys_only_the_rest() {
	let mut c = Conductor::new(admin(), 8, NEED as i64 * 8).await;
	let plumbing = c.category(PLUMBER, "FR", plumbing()).await;
	let uri = format!("/api/v1/placement/study/pull?area=clermont&version={plumbing}");

	let (status, body) = c.call(Method::POST, &uri, Value::Null).await;
	assert_eq!((status, &body["code"]), (StatusCode::PAYMENT_REQUIRED, &json!("unaffordable")), "{body}");
	assert_eq!(c.searched(), NEED, "the round's searches, and not the one behind them");
	let (_, tokens) = c.call(Method::GET, "/api/v1/tokens", Value::Null).await;
	assert_eq!((&tokens["balance"], &tokens["kickbacks"][0]["amount"]), (&json!(0), &json!(NEED * 8)), "{tokens}");

	c.panel
		.change_balance(DevIdentity::named(EMAIL), Change::Set { n: 100, note: None }, "admin@localhost", Timestamp::now())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(c.send(Method::POST, &uri, Value::Null).await.0, StatusCode::OK);
	assert_eq!(c.searched(), NEED + 1, "what the stopped pull bought is held");
	assert_eq!(c.balance().await, 100 - 8);

	c.panel
		.change_balance(DevIdentity::named(EMAIL), Change::Set { n: 1000, note: None }, "admin@localhost", Timestamp::now())
		.await
		.unwrap()
		.unwrap();
	assert_eq!(c.send(Method::POST, &uri, Value::Null).await.0, StatusCode::OK);
	assert_eq!(c.searched(), 2 * (NEED + 1) - 1, "a whole pull's answers are what the next one asks again");
}

#[tokio::test]
async fn a_failed_pull_is_charged_what_it_bought_and_owes_it_all_back() {
	let mut c = Conductor::new(admin(), 8, 1000).await;
	let unknown = json!({ "expr": "no_such_column", "columns": [], "layers": [] });
	let fails = c.category(PLUMBER, "FR", settings(&[("plombier", Some(1.))], unknown)).await;

	let (status, _) = c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=clermont&version={fails}"), Value::Null).await;
	assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "a demand over a column the grid lacks builds nothing");
	let (_, tokens) = c.call(Method::GET, "/api/v1/tokens", Value::Null).await;
	let bought = 3; // one search over two tiles, and the page behind the first
	assert_eq!(c.searched(), bought);
	assert_eq!(tokens["balance"], 1000 - bought as i64 * 8);
	let owed = &tokens["kickbacks"][0];
	assert_eq!(
		(&owed["amount"], &owed["service"], &owed["key"]),
		(&json!(bought * 8), &json!("placement"), &json!(c.studied()[0])),
		"{tokens}"
	);
}

#[tokio::test]
async fn reading_studies_is_not_pulling_them() {
	let mut c = Conductor::new(operator(), 8, 1000).await;
	let plumbing = c.category(PLUMBER, "FR", plumbing()).await;

	let (status, _) = c.send(Method::POST, &format!("/api/v1/placement/study/pull?area=clermont&version={plumbing}"), Value::Null).await;
	assert_eq!(status, StatusCode::FORBIDDEN);
	assert_eq!(c.searched(), 0);
}

#[tokio::test]
async fn a_person_reads_the_prices_and_sets_above_what_they_are_asked() {
	let mut c = Conductor::new(admin(), 8, 1000).await;

	let (_, tokens) = c.call(Method::GET, "/api/v1/tokens", Value::Null).await;
	assert_eq!((&tokens["prices"]["placement.search"], &tokens["confirm_above"]), (&json!(8), &json!(100)), "{tokens}");

	assert_eq!(c.send(Method::PUT, "/api/v1/tokens/settings", json!({ "confirm_above": 250 })).await.0, StatusCode::NO_CONTENT);
	assert_eq!(c.send(Method::PUT, "/api/v1/tokens/settings", json!({ "confirm_above": -1 })).await.0, StatusCode::BAD_REQUEST);
	assert_eq!(c.call(Method::GET, "/api/v1/tokens", Value::Null).await.1["confirm_above"], 250);
}

#[tokio::test]
async fn without_placement_its_studies_are_not_connected() {
	let db = TestDb::create().await;
	let (sign_in, _) = sa_server::testing::conductor(panel(&db).await, operator(), EMAIL, &"http://127.0.0.1:9".parse().unwrap());
	let app = sa_server::http::app(sign_in, None, None, Default::default(), None, None, None, sa_server::testing::sandbox(), None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _, body) = b.send_body(&app, Method::GET, "/api/v1/placement/catalog", &[], false, Body::empty()).await;
	assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{}", String::from_utf8_lossy(&body));
	assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["error"], "placement not connected");
}
