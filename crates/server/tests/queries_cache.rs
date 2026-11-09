//! The queries cache, through the real router and over a socket as its clients reach it: an answer
//! bought by one machine is served to the next, two asking at once pay once, and a write must be
//! of the query its key names.

use std::sync::{Arc, Mutex};

use axum::{Router, extract::State, routing::get};
use jiff::{SignedDuration, Timestamp};
use sa_core::queries_cache::{self, Claim, Key, Put, Scheme};
use sa_panel::{
	Panel,
	testing::{TestDb, admin, panel},
};
use sa_places::{Ask, Google, Keys, Payer, QueriesCache, Remote};
use serde_json::json;
use sha2::{Digest, Sha256};

const PROCOPE: &str = "ChIJprocopeprocopeproc";

/// The conductor on a socket, its queries cache's URL.
async fn conductor(panel: Panel) -> String {
	let app = sa_server::testing::app(panel, admin(), "http://127.0.0.1:9");
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let url = format!("http://{}/api/queries_cache", listener.local_addr().unwrap());
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
	let url = conductor(panel(&db).await).await;
	let (base, asked) = places().await;
	let on = |keys: Vec<String>| Google::try_new(Keys::new(keys), Arc::new(Remote::try_new(&url).unwrap())).unwrap().at(&base);
	let (a, b) = (on(vec!["k".into()]), on(vec!["k".into()]));
	let house = Payer::house();
	let (x, y) = tokio::join!(a.details(PROCOPE, Ask::Cached, &house), b.details(PROCOPE, Ask::Cached, &house));
	assert_eq!(x.unwrap(), y.unwrap());
	assert_eq!(*asked.lock().unwrap(), 1, "the second waited on the first's claim");

	assert_eq!(
		on(vec![]).details(PROCOPE, Ask::Cached, &Payer::house()).await.unwrap().name.as_deref(),
		Some("Le Procope"),
		"keyless, it reads"
	);
	assert_eq!(*asked.lock().unwrap(), 1);
}

#[tokio::test]
async fn a_claim_holds_until_its_lease_lapses_and_the_newest_answer_stays() {
	let db = TestDb::create().await;
	let store = db.store().await;
	let (scheme, k) = (Scheme::Gop, Key([7; 32]));
	let t0 = Timestamp::now();
	assert!(matches!(store.claim_query(scheme, k, None, t0).await.unwrap(), Claim::Granted { .. }));
	assert!(matches!(store.claim_query(scheme, k, None, t0).await.unwrap(), Claim::Held { .. }));
	let lapsed = t0 + queries_cache::LEASE + SignedDuration::from_secs(1);
	assert!(matches!(store.claim_query(scheme, k, None, lapsed).await.unwrap(), Claim::Granted { .. }), "its holder died");

	store.keep_query(scheme, k, r#"{"v":2}"#, None, t0).await.unwrap();
	store.keep_query(scheme, k, r#"{"v":1}"#, None, t0 - SignedDuration::from_secs(60)).await.unwrap();
	let Claim::Stored(kept) = store.claim_query(scheme, k, None, lapsed).await.unwrap() else {
		panic!("an answer is kept")
	};
	assert_eq!(kept.answer, json!({"v": 2}), "an older answer never replaces a newer one");
	assert!(
		matches!(
			store.claim_query(scheme, k, Some(t0 + SignedDuration::from_secs(1)), lapsed).await.unwrap(),
			Claim::Granted { .. }
		),
		"an answer older than the reader takes is as good as none"
	);
	assert_eq!(store.cached_query(Scheme::Places, k).await.unwrap(), None, "schemes are apart");
}

#[tokio::test]
async fn a_write_is_kept_only_under_its_querys_digest() {
	let db = TestDb::create().await;
	let cache = Remote::try_new(&conductor(panel(&db).await).await).unwrap();
	let k = Key(Sha256::digest("GET x").into());
	let put = Put {
		answer: json!({}),
		request: "GET x".into(),
	};
	let mismatched = cache.put(Key([0; 32]), put.clone()).await.unwrap_err();
	assert!(mismatched.to_string().contains("400"), "{mismatched:#}");
	cache.put(k, put).await.unwrap();
	assert_eq!(cache.get(k).await.unwrap().unwrap().answer, json!({}));
}
