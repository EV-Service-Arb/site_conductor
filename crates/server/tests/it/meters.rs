//! A metered buy counts the vendor's reading of its account plus ours on that account after it. A
//! Street View panorama is bought before the browser opens it: at the admin's price, never on
//! credit, and none past its meter's free ones there nor without a reading of it, whoever asks. A
//! page loads on the account with room; Places buys go to it too, up to its allowance.

use std::sync::Arc;

use axum::{
	Router,
	http::{Method, StatusCode},
};
use jiff::{SignedDuration, Timestamp};
use sa_core::prices::{Item, MapLoad, PlacesRequest, StreetViewLoad};
use sa_panel::{
	Panel,
	import::Export,
	meters::ByStanding,
	seal::DataKey,
	testing::{TestDb, Told, account, admin, panel_on},
};
use sa_places::{Account, Accounts, Google, InOrder, Keys, Rank};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};

const EMAIL: &str = "dev@localhost";

struct World {
	panel: Panel,
	app: Router,
	b: Browser,
	db: TestDb,
}
impl World {
	/// Signed in as an admin, tokens open, account `a` alone with both keys; no meter read.
	async fn new() -> Self {
		Self::on(vec![both("a", 0.0)]).await
	}

	/// As [`Self::new`], on `accounts`.
	async fn on(accounts: Vec<Account>) -> Self {
		let db = TestDb::create().await;
		let store = db.store().await;
		let keyless = Google::try_new(Keys::new(vec![]), Arc::new(InOrder), Arc::new(store.clone())).unwrap();
		let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
		let panel = panel_on(store, key, keyless, Accounts::new(accounts));
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap().unwrap();
		let who = DevIdentity {
			user_id: DevIdentity::named(EMAIL),
			permissions: admin(),
			email: EMAIL.to_owned(),
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
		Self { panel, app, b, db }
	}

	/// Google counts `units` of each meter on `a` at `now`, its record reaching `through`.
	async fn read(&self, units: i64, through: Timestamp, now: Timestamp) {
		self.read_on("a", units, 0, through, now).await;
	}

	/// Google counts `units` of each meter on `account`, each `net` after credits (millionths), at
	/// `now`, its record reaching `through`.
	async fn read_on(&self, account: &'static str, units: i64, net: i64, through: Timestamp, now: Timestamp) {
		self.panel.sync_meters(&Told { account, units, net, through }, now).await.unwrap();
	}

	async fn price(&self, tokens: i64) {
		sa_panel::testing::priced(&self.panel, &[(Item::from(StreetViewLoad::Panorama), tokens)]).await;
	}

	async fn call(&mut self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
		self.b.call(&self.app, method, uri, body).await
	}

	async fn open(&mut self) -> (StatusCode, Value) {
		self.open_on("a").await
	}

	async fn open_on(&mut self, account: &str) -> (StatusCode, Value) {
		self.call(Method::POST, "/api/v1/map/street_view", Some(json!({ "lat": 45.75, "lng": 4.85, "account": account })))
			.await
	}

	/// The account `GET /map` loads the page on.
	async fn map(&mut self) -> Value {
		let (status, map) = self.call(Method::GET, "/api/v1/map", None).await;
		assert_eq!(status, StatusCode::OK, "{map}");
		map["account"].clone()
	}

	/// `n` panoramas the caller opened on `a` at `at`, as if then.
	async fn opened(&mut self, n: i64, at: Timestamp) {
		self.balance().await; // the caller is a user once their first call is seen
		let done = sqlx::query("WITH RECURSIVE k(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM k WHERE i < $1) INSERT INTO usage (at, item, units, bill, account, source, user_id) SELECT $3, 'street_view.panorama', 1, 'ours', 'a', 'conductor', u.user_id FROM k, users u WHERE u.email = $2")
			.bind(n)
			.bind(EMAIL)
			.bind(at.as_microsecond())
			.execute(&self.db.pool().await)
			.await
			.unwrap();
		assert_eq!(done.rows_affected(), n as u64);
	}

	/// Street View's meter on `a`, with its total beside it.
	async fn street_view_meter(&mut self) -> Value {
		let m = self.street_view().await;
		let mut on_a = m["accounts"][0].clone();
		on_a["stop"] = m["stop"].clone();
		on_a["period_start"] = m["period_start"].clone();
		on_a
	}

	async fn street_view(&mut self) -> Value {
		let (status, meters) = self.call(Method::GET, "/api/v1/admin/meters", None).await;
		assert_eq!(status, StatusCode::OK, "{meters}");
		meters["meters"].as_array().unwrap().iter().find(|m| m["meter"] == "google:658E-F885-E11A").unwrap().clone()
	}

	async fn balance(&mut self) -> i64 {
		let (status, tokens) = self.call(Method::GET, "/api/v1/tokens", None).await;
		assert_eq!(status, StatusCode::OK, "{tokens}");
		tokens["balance"].as_i64().unwrap()
	}
}

/// An account with both keys, `allow` a month past its free units.
fn both(name: &str, allow: f64) -> Account {
	Account {
		allow: [("maps".to_owned(), allow)].into(),
		..account(name, Some(&format!("places-{name}")), Some(&format!("browser-{name}")))
	}
}

#[tokio::test]
async fn a_panorama_is_charged_at_its_price_and_refused_on_credit() {
	let mut w = World::new().await;
	let now = Timestamp::now();
	w.read(0, now, now).await;
	let start = w.balance().await;
	w.price(7).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "{bought}");
	assert_eq!(bought["balance"], start - 7);
	assert_eq!(w.balance().await, start - 7);

	w.price(start).await;
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "the balance below the price: {refused}");
	assert_eq!(w.balance().await, start - 7, "nothing charged when refused");

	let (status, _) = w.call(Method::POST, "/api/v1/map/street_view", Some(json!({ "lat": 91.0, "lng": 0.0 }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_free_ones_are_googles_count_plus_ours_after_it() {
	let mut w = World::new().await;
	w.price(0).await;
	let now = Timestamp::now();
	let through = now - SignedDuration::from_hours(5);
	w.opened(3, through - SignedDuration::from_hours(1)).await; // in Google's count already
	w.read(4990, through, now).await;
	w.opened(8, through + SignedDuration::from_mins(1)).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "4990 + 9: {bought}");
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "the 5000th: {bought}");
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");

	let m = w.street_view_meter().await;
	assert_eq!(
		(m["baseline"].as_i64(), m["delta"].as_i64(), m["used"].as_i64(), m["left"].as_i64()),
		(Some(4990), Some(10), Some(5000), Some(0)),
		"{m}"
	);
	assert_eq!(m["stop"], "at_free");
	assert_eq!(m["reading"]["units"], 4990);
}

#[tokio::test]
async fn a_reading_of_last_month_counts_for_nothing_this_month() {
	let mut w = World::new().await;
	w.price(0).await;
	let now = Timestamp::now();
	let (start, _) = sa_core::meters::Vendor::Google.period(now);
	let last_month = start - SignedDuration::from_hours(24);
	w.read(5000, last_month, last_month).await;
	w.opened(2, last_month).await;
	w.opened(4, start + SignedDuration::from_secs(1)).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "last month's 5000 are not this month's: {bought}");

	let m = w.street_view_meter().await;
	assert_eq!((m["baseline"].as_i64(), m["delta"].as_i64()), (Some(0), Some(5)), "ours from the month's start: {m}");
	assert_eq!(m["since"], m["period_start"]);
}

#[tokio::test]
async fn nothing_is_bought_on_a_stopping_meter_without_a_reading() {
	let mut w = World::new().await;
	w.price(0).await;
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");
	sa_panel::testing::priced(&w.panel, &[(Item::from(MapLoad::Map), 0)]).await;
	let (status, _) = w.call(Method::POST, "/api/v1/map/load", Some(json!({ "account": "a" }))).await;
	assert_eq!(status, StatusCode::OK, "a map load is only watched");

	let m = w.street_view_meter().await;
	assert_eq!(m["reading"], Value::Null);
	let now = Timestamp::now();
	w.read(0, now, now).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "{bought}");
}

#[tokio::test]
async fn a_page_loads_on_the_account_with_free_panoramas_and_a_reload_moves_on() {
	let mut w = World::on(vec![both("a", 0.0), both("b", 0.0)]).await;
	w.price(0).await;
	let now = Timestamp::now();
	w.read_on("a", 4999, 0, now, now).await;
	w.read_on("b", 0, 0, now, now).await;
	assert_eq!(w.map().await, "a", "the first in the config's order with room");
	let (status, bought) = w.open_on("a").await;
	assert_eq!(status, StatusCode::OK, "a's 5000th: {bought}");
	let (status, refused) = w.open_on("a").await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");
	assert!(refused["error"].as_str().unwrap().contains("reload the page: it then loads on b"), "{refused}");
	assert_eq!(w.map().await, "b");
	let (status, bought) = w.open_on("b").await;
	assert_eq!(status, StatusCode::OK, "{bought}");

	let m = w.street_view().await;
	assert_eq!(m["total"], json!({ "baseline": 4999, "delta": 2, "used": 5001, "free": 10000, "left": 4999 }), "{m}");
	assert_eq!((m["accounts"][0]["used"].as_i64(), m["accounts"][1]["used"].as_i64()), (Some(5000), Some(1)));

	let (status, refused) = w.open_on("nowhere").await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
	let (status, refused) = w.call(Method::POST, "/api/v1/map/load", Some(json!({ "account": "nowhere" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
}

#[tokio::test]
async fn an_account_without_its_reading_refuses_its_own_panoramas_only() {
	let mut w = World::on(vec![both("a", 0.0), both("b", 0.0)]).await;
	w.price(0).await;
	let now = Timestamp::now();
	w.read_on("b", 0, 0, now, now).await;
	let (status, refused) = w.open_on("a").await;
	assert_eq!(status, StatusCode::CONFLICT, "a's reader never answered: {refused}");
	let (status, bought) = w.open_on("b").await;
	assert_eq!(status, StatusCode::OK, "{bought}");
}

#[tokio::test]
async fn places_go_to_free_units_first_then_within_the_allowance_then_nowhere() {
	let w = World::on(vec![both("a", 2.0), both("b", 0.0), both("c", 5.0)]).await;
	let now = Timestamp::now();
	let rank = ByStanding {
		store: w.db.store().await,
		accounts: Arc::new(w.panel.accounts().clone()),
	};
	let names = ["a", "b", "c"].map(str::to_owned);
	let ranks = async || rank.rank(PlacesRequest::Details, &names).await.unwrap();
	assert_eq!(ranks().await, [Some(0); 3], "nothing read: ours alone, none yet");
	w.read_on("a", 5000, 100_000, now, now).await; // 0.6 over its 6 meters
	w.read_on("b", 5000, 0, now, now).await;
	w.read_on("c", 4999, 0, now, now).await;
	assert_eq!(ranks().await, [Some(1), None, Some(0)], "a past its free units but 0.6 of its 2 spent; b allowed none");
	w.read_on("a", 5000, 400_000, now, now).await;
	assert_eq!(ranks().await, [None, None, Some(0)], "a past its 2");
}
