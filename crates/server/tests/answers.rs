//! The answers store, through the real router and over a socket as its clients reach it: an answer
//! bought by one key is served to another, two clients asking at once pay once, and only an
//! `answers` key gets in — nor can one write events.

use std::{
	collections::BTreeSet,
	sync::{Arc, Mutex},
};

use axum::{Router, body::Body, extract::State, http::Request, routing::get};
use jiff::{SignedDuration, Timestamp};
use sa_auth::SourceKind;
use sa_core::{
	answers::{self, Claim, Key, Put, Scheme},
	ids::BrandId,
};
use sa_panel::{
	Panel,
	testing::{TestDb, admin, panel, sign},
};
use sa_places::{Answers, Ask, Google, Keys, Remote};
use serde_json::json;
use sha2::{Digest, Sha256};
use tower::ServiceExt;

const PROCOPE: &str = "ChIJprocopeprocopeproc";

async fn key(panel: &Panel, key_id: &str) -> String {
	panel.add_source(key_id, SourceKind::Answers, BTreeSet::new(), None).await.unwrap().unwrap().secret.to_string()
}

/// The conductor on a socket, its answers store's URL.
async fn conductor(panel: Panel) -> String {
	let app = sa_server::testing::app(panel, admin(), "http://127.0.0.1:9");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let url = format!("http://{}/api/answers", listener.local_addr().unwrap());
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	url
}

/// Places answering details slowly, so two askers overlap; how many times it was asked.
async fn places() -> (String, Arc<Mutex<usize>>) {
	async fn details(State(asked): State<Arc<Mutex<usize>>>) -> axum::Json<serde_json::Value> {
		*asked.lock().unwrap() += 1;
		tokio::time::sleep(std::time::Duration::from_millis(300)).await;
		axum::Json(json!({ "id": PROCOPE, "displayName": { "text": "Le Procope" } }))
	}
	let asked = Arc::<Mutex<usize>>::default();
	let app = Router::new().route("/v1/places/{id}", get(details)).with_state(asked.clone());
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let base = format!("http://{}/v1/places", listener.local_addr().unwrap());
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(base, asked)
}

#[tokio::test]
async fn two_machines_asking_at_once_pay_once_and_a_third_reads_it_for_free() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let (laptop, cluster) = (key(&panel, "laptop").await, key(&panel, "review-archive").await);
	let origin = conductor(panel).await;
	let (base, asked) = places().await;
	let on = |key_id: &str, secret: &str| {
		let remote = Remote::try_new(&origin, key_id, secret.to_owned().into()).unwrap();
		Google::try_new(Keys::new(vec!["k".into()]), Arc::new(remote)).unwrap().at(&base)
	};
	let (a, b) = (on("laptop", &laptop), on("review-archive", &cluster));
	let (x, y) = tokio::join!(a.details(PROCOPE, Ask::Cached), b.details(PROCOPE, Ask::Cached));
	assert_eq!(x.unwrap(), y.unwrap());
	assert_eq!(*asked.lock().unwrap(), 1, "the second waited on the first's claim");

	let keyless = Google::try_new(Keys::new(vec![]), Arc::new(Remote::try_new(&origin, "laptop", laptop.into()).unwrap()))
		.unwrap()
		.at(&base);
	assert_eq!(keyless.details(PROCOPE, Ask::Cached).await.unwrap().name.as_deref(), Some("Le Procope"));
	assert_eq!(*asked.lock().unwrap(), 1);
}

#[tokio::test]
async fn a_claim_holds_until_its_lease_lapses_and_the_newest_answer_stays() {
	let db = TestDb::create().await;
	let store = db.store().await;
	let (scheme, k) = (Scheme::Gop, Key([7; 32]));
	let t0 = Timestamp::now();
	assert!(matches!(store.claim_answer(scheme, k, None, t0).await.unwrap(), Claim::Granted { .. }));
	assert!(matches!(store.claim_answer(scheme, k, None, t0).await.unwrap(), Claim::Held { .. }));
	let lapsed = t0 + answers::LEASE + SignedDuration::from_secs(1);
	assert!(matches!(store.claim_answer(scheme, k, None, lapsed).await.unwrap(), Claim::Granted { .. }), "its holder died");

	store.put_answer(scheme, k, r#"{"v":2}"#, None, t0).await.unwrap();
	store.put_answer(scheme, k, r#"{"v":1}"#, None, t0 - SignedDuration::from_secs(60)).await.unwrap();
	let Claim::Stored(kept) = store.claim_answer(scheme, k, None, lapsed).await.unwrap() else {
		panic!("an answer is kept")
	};
	assert_eq!(kept.answer, json!({"v": 2}), "an older answer never replaces a newer one");
	assert!(
		matches!(
			store.claim_answer(scheme, k, Some(t0 + SignedDuration::from_secs(1)), lapsed).await.unwrap(),
			Claim::Granted { .. }
		),
		"an answer older than the reader takes is as good as none"
	);
	assert_eq!(store.answer(Scheme::Places, k).await.unwrap(), None, "schemes are apart");
}

#[tokio::test]
async fn only_an_answers_key_reaches_the_store_and_it_writes_no_events() {
	let db = TestDb::create().await;
	let panel = panel(&db).await;
	let answers_secret = key(&panel, "laptop").await;
	let site_secret = panel
		.add_source(
			"aquafix-site",
			SourceKind::Site,
			[BrandId::parse("aquafix").unwrap()].into(),
			Some(sa_core::ids::SiteOrigin::parse("https://aquafix.top").unwrap()),
		)
		.await
		.unwrap()
		.unwrap()
		.secret
		.to_string();
	let origin = conductor(panel.clone()).await;
	let k = Key(Sha256::digest("GET x").into());
	let put = Put {
		answer: json!({}),
		request: "GET x".into(),
	};

	let site = Remote::try_new(&origin, "aquafix-site", site_secret.into()).unwrap();
	let refused = site.get(k).await.unwrap_err();
	assert!(refused.to_string().contains("401"), "{refused:#}");

	let laptop = Remote::try_new(&origin, "laptop", answers_secret.clone().into()).unwrap();
	let mismatched = laptop.put(Key([0; 32]), put.clone()).await.unwrap_err();
	assert!(mismatched.to_string().contains("400"), "{mismatched:#}");
	laptop.put(k, put).await.unwrap();
	assert_eq!(laptop.get(k).await.unwrap().unwrap().answer, json!({}));

	let app = sa_server::testing::app(panel, admin(), "http://127.0.0.1:9");
	let batch = sign("laptop", &answers_secret, br#"{"events":[]}"#, Timestamp::now());
	let req = Request::post("/api/ingest/v1/events")
		.header("content-type", "application/json")
		.header("x-sa-key-id", &batch.key_id)
		.header("x-sa-timestamp", &batch.timestamp)
		.header("x-sa-signature", &batch.signature)
		.body(Body::from(batch.body.clone()))
		.unwrap();
	assert_eq!(app.oneshot(req).await.unwrap().status(), 401);
}
