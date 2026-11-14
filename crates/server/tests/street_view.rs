//! A Street View panorama is bought before the browser opens it: at the admin's price, never on
//! credit, and none past Google's free ones in a month, whoever asks.

use axum::{
	Router,
	body::Body,
	http::{Method, StatusCode},
};
use sa_core::prices::{Item, StreetViewLoad};
use sa_panel::{
	Panel,
	import::Export,
	street_view::FREE,
	testing::{TestDb, operator, panel},
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
	/// Signed in, tokens open, the browser's Maps key set.
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
			permissions: operator(),
			email: EMAIL.to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let map = MapDto { key: "browser-key".to_owned() };
		let app = http::app(SignIn::new(panel.clone(), Concierge::dev(who), config), None, None, Default::default(), None, Some(map), None);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self { panel, app, b, db }
	}

	async fn price(&self, tokens: Option<i64>) {
		self.panel
			.set_prices(&[(Item::from(StreetViewLoad::Panorama), tokens)], "admin@localhost", jiff::Timestamp::now())
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

	async fn balance(&mut self) -> i64 {
		let (status, tokens) = self.call(Method::GET, "/api/v1/tokens", None).await;
		assert_eq!(status, StatusCode::OK, "{tokens}");
		tokens["balance"].as_i64().unwrap()
	}

	/// `n` panoramas opened by the caller this month, as if earlier.
	async fn opened(&mut self, n: i64) {
		self.balance().await; // the caller is a user once their first call is seen
		let at = jiff::Timestamp::now().as_microsecond();
		let done = sqlx::query("WITH RECURSIVE k(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM k WHERE i < $1) INSERT INTO street_views (user_id, at, lat, lng) SELECT u.user_id, $3, 0, 0 FROM k, users u WHERE u.email = $2")
			.bind(n)
			.bind(EMAIL)
			.bind(at)
			.execute(&self.db.pool().await)
			.await
			.unwrap();
		assert_eq!(done.rows_affected(), n as u64);
	}
}

#[tokio::test]
async fn a_panorama_is_charged_at_its_price_and_refused_unpriced_or_on_credit() {
	let mut w = World::new().await;
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
async fn none_opens_past_the_months_free_ones_even_free_of_tokens() {
	let mut w = World::new().await;
	w.price(Some(0)).await;
	w.opened(FREE - 1).await;
	let (status, bought) = w.open().await;
	assert_eq!(status, StatusCode::OK, "the last free one: {bought}");
	let (status, refused) = w.open().await;
	assert_eq!(status, StatusCode::CONFLICT, "{refused}");
}
