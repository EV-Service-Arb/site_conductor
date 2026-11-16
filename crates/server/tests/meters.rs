//! A metered buy counts the vendor's reading plus ours after it. A Street View panorama is bought
//! before the browser opens it: at the admin's price, never on credit, and none past its meter's
//! free ones nor without a reading of it, whoever asks.

use axum::{
	Router,
	body::Body,
	http::{Method, StatusCode},
};
use jiff::{SignedDuration, Timestamp};
use sa_core::prices::{Item, StreetViewLoad};
use sa_panel::{
	Panel,
	import::Export,
	testing::{TestDb, Told, admin, panel},
};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	gmbs::MapDto,
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
	/// Signed in as an admin, tokens open, the browser's Maps key set; no meter read.
	async fn new() -> Self {
		let db = TestDb::create().await;
		let panel = panel(&db).await;
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap();
		let who = DevIdentity {
			user_id: DevIdentity::named(EMAIL),
			permissions: admin(),
			email: EMAIL.to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let map = MapDto { key: "browser-key".to_owned() };
		let app = http::app(
			SignIn::new(panel.clone(), Concierge::dev(who), config),
			None,
			None,
			Default::default(),
			None,
			Some(map),
			None,
			None,
		);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self { panel, app, b, db }
	}

	/// Google counts `units` of each meter at `now`, its record reaching `through`.
	async fn read(&self, units: i64, through: Timestamp, now: Timestamp) {
		self.panel.sync_meters(&Told { units, through }, now).await.unwrap();
	}

	async fn price(&self, tokens: Option<i64>) {
		self.panel
			.set_prices(&[(Item::from(StreetViewLoad::Panorama), tokens)], &[], "admin@localhost", jiff::Timestamp::now())
			.await
			.unwrap()
			.unwrap();
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

	async fn open(&mut self) -> (StatusCode, Value) {
		self.call(Method::POST, "/api/v1/map/street_view", Some(json!({ "lat": 45.75, "lng": 4.85 }))).await
	}

	/// `n` panoramas the caller opened at `at`, as if then.
	async fn opened(&mut self, n: i64, at: Timestamp) {
		self.balance().await; // the caller is a user once their first call is seen
		let done = sqlx::query("WITH RECURSIVE k(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM k WHERE i < $1) INSERT INTO usage (at, item, units, bill, source, user_id) SELECT $3, 'street_view.panorama', 1, 'ours', 'conductor', u.user_id FROM k, users u WHERE u.email = $2")
			.bind(n)
			.bind(EMAIL)
			.bind(at.as_microsecond())
			.execute(&self.db.pool().await)
			.await
			.unwrap();
		assert_eq!(done.rows_affected(), n as u64);
	}

	async fn street_view_meter(&mut self) -> Value {
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

#[tokio::test]
async fn a_panorama_is_charged_at_its_price_and_refused_unpriced_or_on_credit() {
	let mut w = World::new().await;
	let now = Timestamp::now();
	w.read(0, now, now).await;
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "unpriced, not sold: {refused}");

	let start = w.balance().await;
	w.price(Some(7)).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "{bought}");
	assert_eq!(bought["balance"], start - 7);
	assert_eq!(w.balance().await, start - 7);

	w.price(Some(start)).await;
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "the balance below the price: {refused}");
	assert_eq!(w.balance().await, start - 7, "nothing charged when refused");

	let (status, _) = w.call(Method::POST, "/api/v1/map/street_view", Some(json!({ "lat": 91.0, "lng": 0.0 }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_free_ones_are_googles_count_plus_ours_after_it() {
	let mut w = World::new().await;
	w.price(Some(0)).await;
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
	w.price(Some(0)).await;
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
	w.price(Some(0)).await;
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");
	let (status, _) = w.call(Method::POST, "/api/v1/map/loaded", None).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "a map load is only watched");

	let m = w.street_view_meter().await;
	assert_eq!(m["reading"], Value::Null);
	let now = Timestamp::now();
	w.read(0, now, now).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "{bought}");
}
