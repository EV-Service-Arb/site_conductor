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
use sa_core::{
	google::{Allow, Hold},
	prices::{Item, MapLoad, PlacesRequest, StreetViewLoad},
};
use sa_panel::{
	Panel,
	import::Export,
	meters::{Bill, ByStanding, Day, Draw, Lever, Tier},
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
	/// As [`Self::new`], on `accounts`, each held to its meters' slice's allowance a day and a month, where given.
	async fn on(accounts: Vec<(Account, Option<f64>)>) -> Self {
		let holds: Vec<(String, f64)> = accounts.iter().filter_map(|(a, allow)| Some((a.name.clone(), (*allow)?))).collect();
		let accounts = accounts.into_iter().map(|(a, _)| a).collect();
		let db = TestDb::create().await;
		let store = db.store().await;
		let keyless = Google::try_new(Keys::new(vec![]), Arc::new(InOrder), Arc::new(store.clone())).unwrap();
		let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
		let maps = sa_core::google::Slice {
			name: sa_core::meters::SLICE.to_owned(),
			service: "(?i)maps|places|geocoding|street view".to_owned(),
			sku: None,
		};
		let panel = panel_on(store, key, keyless, Accounts::new(accounts, vec![maps]));
		for (name, allow) in holds {
			panel.set_hold(&name, &hold(allow), "test", Timestamp::now()).await.unwrap().unwrap();
		}
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
			sa_server::testing::sandbox(),
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
		let bill = Bill {
			days: vec![],
			draws: vec![],
			tiers: vec![],
			guard: None,
		};
		let published = Default::default();
		self.panel
			.sync_meters(
				&Told {
					account,
					units,
					net,
					through,
					bill,
					published,
				},
				now,
			)
			.await
			.unwrap();
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

/// An account with both keys, to be held to `allow` a day and a month past its free units.
fn both(name: &str, allow: f64) -> (Account, Option<f64>) {
	(account(name, Some(&format!("places-{name}")), Some(&format!("browser-{name}"))), Some(allow))
}

/// The meters' slice's `allow` a day and a month, nothing unguarded.
fn hold(allow: f64) -> Hold {
	Hold {
		allow: [(sa_core::meters::SLICE.to_owned(), Allow { day: allow, month: allow })].into(),
		unguarded: vec![],
		credits: vec![],
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

/// What an account's export told of its bill, the guard's last tick there among it.
fn bill(day: &str, credit_micros: i64, guard: Option<Vec<Lever>>) -> Bill {
	let day = |service: &str, sku: &str, cost_micros| Day {
		day: day.parse().unwrap(),
		service: service.to_owned(),
		sku: sku.to_owned(),
		currency: "EUR".to_owned(),
		cost_micros,
		credit_micros,
		discount_micros: 0,
	};
	Bill {
		days: vec![day("Maps API", "Dynamic Maps", -credit_micros), day("Vertex AI", "Veo 3 video", 0)],
		draws: vec![Draw {
			credit: "c1".to_owned(),
			hour: Timestamp::now() - SignedDuration::from_hours(24),
			amount_micros: credit_micros,
		}],
		tiers: vec![Tier {
			month: "202611".to_owned(),
			service: "Maps API".to_owned(),
			sku: "Dynamic Maps".to_owned(),
			unit: Some("count".to_owned()),
			units: 114.0,
			paid_units: 0.0,
			free_to: None,
			cost_micros: 0,
			free_micros: 0,
		}],
		guard,
	}
}

fn real_money(list: &str) -> Lever {
	Lever {
		tick: Timestamp::now(),
		slice: Some(sa_core::meters::SLICE.to_owned()),
		project: None,
		lever: "real money".to_owned(),
		allow_day: Some(0.0),
		allow_month: Some(0.0),
		real_day: Some(0.0),
		real_month: Some(0.0),
		setting: None,
		state: Some("within".to_owned()),
		today: None,
		action: None,
		list: Some(list.to_owned()),
	}
}

#[tokio::test]
async fn an_accounts_bill_is_read_whole_by_slice_with_its_credits_and_the_guards_last_tick() {
	let credit = sa_core::google::Credit {
		id: Some("c1".to_owned()),
		name: "trial".to_owned(),
		read: Timestamp::now() - SignedDuration::from_hours(48),
		left: 100.0,
		ends: jiff::civil::date(2099, 1, 1),
	};
	let mut w = World::on(vec![both("a", 0.0)]).await;
	let (status, held) = w
		.call(Method::PUT, "/api/v1/admin/meters/accounts/a/hold", Some(json!(Hold { credits: vec![credit], ..hold(0.0) })))
		.await;
	assert_eq!(status, StatusCode::OK, "a credit is the admin's to set: {held}");
	let now = Timestamp::now();
	let a = w.panel.accounts().get("a").unwrap().clone();
	let digest = w.panel.definition(&a).await.unwrap().unwrap().digest();
	let sync = async |w: &World, bill: Bill| {
		w.panel
			.sync_meters(
				&Told {
					account: "a",
					units: 0,
					net: 0,
					through: now,
					bill,
					published: Default::default(),
				},
				now,
			)
			.await
			.unwrap()
	};
	let read = async |w: &mut World| w.call(Method::GET, "/api/v1/admin/meters", None).await.1["accounts"][0].clone();

	let never = read(&mut w).await;
	assert_eq!(
		(&never["credits"], &never["guard"], &never["bill_read"]),
		(&json!([]), &Value::Null, &Value::Null),
		"before a read: {never}"
	);

	let unguarded = Lever {
		slice: None,
		project: Some("p".to_owned()),
		lever: "unguarded: bigquery.googleapis.com".to_owned(),
		allow_day: None,
		allow_month: None,
		real_day: None,
		real_month: None,
		state: None,
		..real_money(&digest)
	};
	sync(&w, bill("2026-11-10", -7_000_000, Some(vec![real_money(&digest), unguarded]))).await;
	let a = read(&mut w).await;
	assert_eq!(
		(a["credits"][0]["left"].as_f64(), a["credits"][0]["binds"].as_str()),
		(Some(93.0), Some("spent")),
		"less what was drawn since the console's reading: {a}"
	);
	assert_eq!((&a["allowances"][0]["slice"], &a["guard"]["holds_this_list"]), (&json!("maps"), &json!(true)), "{a}");
	assert_eq!(
		(&a["guard"]["unguarded"], &a["guard"]["disabled"], &a["allowances"][0]["levers"]),
		(&json!([{ "project": "p", "api": "bigquery.googleapis.com" }]), &json!([]), &json!([])),
		"an API the guard leaves alone is the account's, not a slice's lever: {a}"
	);
	let window = format!(
		"/api/v1/admin/meters/bill?from={}&to={}",
		(now - SignedDuration::from_hours(24 * 60)).as_millisecond(),
		now.as_millisecond()
	);
	let (status, b) = w.call(Method::GET, &window, None).await;
	assert_eq!(status, StatusCode::OK, "{b}");
	let slices: Vec<(&str, f64, f64)> = b["days"]
		.as_array()
		.unwrap()
		.iter()
		.map(|d| (d["slice"].as_str().unwrap(), d["credit"].as_f64().unwrap(), d["paid"].as_f64().unwrap()))
		.collect();
	assert_eq!(slices, [("maps", 7.0, 0.0), ("Vertex AI", 7.0, -7.0)], "a service no slice draws is its own: {b}");
	assert_eq!(b["tiers"][0]["units"], 114.0);

	sync(&w, bill("2026-11-11", 0, Some(vec![real_money("another list")]))).await;
	let (_, b) = w.call(Method::GET, &window, None).await;
	assert_eq!(b["days"][0]["day"], "2026-11-11T00:00:00Z", "the read before is replaced whole: {b}");
	assert_eq!(read(&mut w).await["guard"]["holds_this_list"], false, "a guard deployed from another definition is told apart");
}

// r[verify google.guarded]
#[tokio::test]
async fn an_account_is_used_once_held_and_its_hold_is_published_beside_its_export_when_it_changes() {
	let unheld = account("u", Some("places-u"), Some("browser-u"));
	let unexported = sa_places::Account {
		export: None,
		..account("x", Some("places-x"), Some("browser-x"))
	};
	let mut w = World::on(vec![both("a", 1.0), (unheld, None), (unexported, None)]).await;
	let now = Timestamp::now();
	let rank = ByStanding {
		store: w.db.store().await,
		accounts: Arc::new(w.panel.accounts().clone()),
	};
	let names = ["a", "u", "x"].map(str::to_owned);
	assert_eq!(
		rank.rank(PlacesRequest::Details, &names).await.unwrap(),
		[Some(0), None, None],
		"u is held by nothing, x has no export"
	);
	let zero = json!({ "allow": { "maps": { "day": 0.0, "month": 0.0 } }, "unguarded": [], "credits": [] });
	let (status, refused) = w.call(Method::PUT, "/api/v1/admin/meters/accounts/x/hold", Some(zero.clone())).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "nothing holds an account without an export: {refused}");
	let (status, _) = w.call(Method::PUT, "/api/v1/admin/meters/accounts/u/hold", Some(zero)).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(rank.rank(PlacesRequest::Details, &names).await.unwrap(), [Some(0), Some(0), None], "u once held");

	let (status, refused) = w
		.call(Method::PUT, "/api/v1/admin/meters/accounts/a/hold", Some(json!({ "allow": {}, "unguarded": [], "credits": [] })))
		.await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a hold short of a slice: {refused}");
	let set = json!({ "allow": { "maps": { "day": 2.0, "month": 10.0 } }, "unguarded": ["run.googleapis.com"], "credits": [] });
	let (status, held) = w.call(Method::PUT, "/api/v1/admin/meters/accounts/a/hold", Some(set.clone())).await;
	assert_eq!((status, &held), (StatusCode::OK, &set));
	let (_, meters) = w.call(Method::GET, "/api/v1/admin/meters", None).await;
	assert_eq!(meters["accounts"][0]["hold"], set, "{meters}");

	let reader = Told {
		account: "a",
		units: 0,
		net: 0,
		through: now,
		bill: bill("2026-11-10", 0, None),
		published: Default::default(),
	};
	let mut last = None;
	w.panel.publish_definition(&reader, &mut last, now).await.unwrap();
	w.panel.publish_definition(&reader, &mut last, now).await.unwrap();
	let published = reader.published.lock().unwrap().clone();
	assert_eq!(published.len(), 1, "published once while unchanged");
	assert_eq!(
		(published[0].held.hold.allow["maps"].day, published[0].held.hold.unguarded.as_slice()),
		(2.0, ["run.googleapis.com".to_owned()].as_slice())
	);
	let (status, _) = w
		.call(
			Method::PUT,
			"/api/v1/admin/meters/accounts/a/hold",
			Some(json!({ "allow": { "maps": { "day": 0.0, "month": 0.0 } }, "unguarded": [], "credits": [] })),
		)
		.await;
	assert_eq!(status, StatusCode::OK);
	w.panel.publish_definition(&reader, &mut last, now).await.unwrap();
	assert_eq!(reader.published.lock().unwrap().len(), 2, "a change is published");
}
