//! The queries cache, through the real router and over a socket as its clients reach it: an answer
//! bought by one machine is served to the next, two asking at once pay once, and a write must be
//! of the query its key names. Every version of an answer is kept, and two conductors' databases
//! synced by the binary's `queries-cache` commands drop none.

use std::{
	collections::BTreeSet,
	sync::{Arc, Mutex},
};

use axum::{Router, extract::State, routing::get};
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	prices::{DataForSeoTask, Item, PlacesRequest},
	queries_cache::{self, Claim, Key, Put, Scheme, Stored},
	usage::{Bill, Billed},
};
use sa_panel::{
	Panel,
	seal::DataKey,
	testing::{TestDb, account, admin, panel_on},
};
use sa_places::{Accounts, Ask, Google, InOrder, Keys, Payer, QueriesCache, Remote};
use serde_json::json;

const PROCOPE: &str = "ChIJprocopeprocopeproc";

/// A panel whose one Google account is `house`.
async fn housed(db: &TestDb) -> Panel {
	let store = db.store().await;
	let keyless = Google::try_new(Keys::new(vec![]), Arc::new(InOrder), Arc::new(store.clone())).unwrap();
	let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
	panel_on(store, key, keyless, Accounts::new(vec![account("house", Some("k"), None)], sa_panel::testing::slices()))
}

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
	let url = conductor(housed(&db).await).await;
	let (base, asked) = places().await;
	let on = |keys: Vec<(String, String)>| {
		Google::try_new(Keys::new(keys), Arc::new(InOrder), Arc::new(Remote::try_new(&url).unwrap()))
			.unwrap()
			.spend(true)
			.at(&base)
	};
	let house = || vec![("house".to_owned(), "k".to_owned())];
	let (a, b) = (on(house()), on(house()));
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
	let request = "GET claimed";
	let (scheme, k) = (Scheme::Gop, Key::of(request));
	let t0 = Timestamp::now();
	assert!(matches!(store.claim_query(scheme, k, None, t0).await.unwrap(), Claim::Granted { .. }));
	assert!(matches!(store.claim_query(scheme, k, None, t0).await.unwrap(), Claim::Held { .. }));
	let lapsed = t0 + queries_cache::LEASE + SignedDuration::from_secs(1);
	assert!(matches!(store.claim_query(scheme, k, None, lapsed).await.unwrap(), Claim::Granted { .. }), "its holder died");

	store.keep_query(scheme, k, r#"{"v":2}"#, request, None, t0).await.unwrap().unwrap();
	store
		.keep_query(scheme, k, r#"{"v":1}"#, request, None, t0 - SignedDuration::from_secs(60))
		.await
		.unwrap()
		.unwrap();
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
	let cache = Remote::try_new(&conductor(housed(&db).await).await).unwrap();
	let k = Key::of("GET x");
	let put = Put {
		answer: json!({}),
		request: "GET x".into(),
		bought: None,
	};
	let mismatched = cache.put(Scheme::Places, Key([0; 32]), put.clone()).await.unwrap_err();
	assert!(mismatched.to_string().contains("400"), "{mismatched:#}");
	let billed = |bill, account: Option<&str>| Put {
		bought: Some(Billed {
			item: Item::Places(PlacesRequest::Details),
			bill,
			account: account.map(str::to_owned),
		}),
		..put.clone()
	};
	for (bill, account) in [(Bill::Ours, Some("elsewhere")), (Bill::Ours, None), (Bill::Theirs, Some("house"))] {
		let refused = cache.put(Scheme::Places, k, billed(bill, account)).await.unwrap_err();
		assert!(refused.to_string().contains("400"), "{bill:?} on {account:?}: {refused:#}");
	}
	assert_eq!(cache.get(Scheme::Places, k).await.unwrap(), None, "nothing refused is kept");
	cache.put(Scheme::Places, k, put).await.unwrap();
	assert_eq!(cache.get(Scheme::Places, k).await.unwrap().unwrap().answer, json!({}));
}

#[tokio::test]
async fn a_buy_off_the_google_meters_names_no_account() {
	let db = TestDb::create().await;
	let cache = Remote::try_new(&conductor(housed(&db).await).await).unwrap();
	let k = Key::of("POST volumes");
	let bought = |account: Option<&str>| Put {
		answer: json!({}),
		request: "POST volumes".into(),
		bought: Some(Billed {
			item: Item::DataForSeo(DataForSeoTask::SearchVolume),
			bill: Bill::Ours,
			account: account.map(str::to_owned),
		}),
	};
	let refused = cache.put(Scheme::Gop, k, bought(Some("house"))).await.unwrap_err();
	assert!(refused.to_string().contains("400"), "{refused:#}");
	cache.put(Scheme::Gop, k, bought(None)).await.unwrap();
	assert_eq!(cache.get(Scheme::Gop, k).await.unwrap().unwrap().answer, json!({}));
}

/// Versions of one answer arriving in any order: the newest is served, every older one is held
/// beside it, the same one twice is held once, and none is kept under a key that is not its
/// request's digest.
#[tokio::test]
async fn every_version_of_an_answer_is_held_and_the_newest_served() {
	let db = TestDb::create().await;
	let store = db.store().await;
	let request = "GET versions";
	let k = Key::of(request);
	let t = |minute: i64| Timestamp::from_second(1_800_000_000 + minute * 60).unwrap();
	for (v, at) in [(1, t(0)), (3, t(2)), (2, t(1)), (3, t(2))] {
		store.keep_query(Scheme::Gop, k, &json!({ "v": v }).to_string(), request, None, at).await.unwrap().unwrap();
	}
	assert_eq!(
		store.cached_query(Scheme::Gop, k).await.unwrap(),
		Some(Stored {
			answer: json!({ "v": 3 }),
			fetched_at: t(2)
		})
	);
	assert_eq!(store.queries_index().await.unwrap(), [0, 1, 2].map(|m| (Scheme::Gop, k, t(m))).into());
	for (v, at) in [(1, t(0)), (2, t(1))] {
		let held = store.query_version(Scheme::Gop, k, at).await.unwrap().unwrap();
		assert_eq!((held.answer, held.request.as_str()), (json!({ "v": v }), request), "an older version keeps its request");
	}
	assert!(
		store.keep_query(Scheme::Gop, Key([0; 32]), "{}", request, None, t(3)).await.unwrap().is_err(),
		"a key is its request's SHA-256"
	);
	assert_eq!(store.queries_index().await.unwrap().len(), 3);
}

/// `panel queries-cache <args>` on `db`, `stdin` fed to it; its stdout.
fn panel(db: &TestDb, args: &[&str], stdin: &[u8]) -> String {
	use std::io::Write;
	let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_panel"))
		.arg("queries-cache")
		.args(args)
		.env_clear()
		.env("PANEL_DB_PATH", db.path())
		.env("PANEL_HISTORY_DB_PATH", db.history())
		.stdin(std::process::Stdio::piped())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::piped())
		.spawn()
		.expect("running the panel binary");
	child.stdin.take().unwrap().write_all(stdin).unwrap();
	let out = child.wait_with_output().unwrap();
	assert!(out.status.success(), "queries-cache {args:?}: {}", String::from_utf8_lossy(&out.stderr));
	String::from_utf8(out.stdout).unwrap()
}

/// What one conductor holds that another lacks, exported against the other's index and imported
/// there, as `scripts/queries-cache-sync.sh` runs it: the other ends with every version either
/// held, as of when each was fetched, its own newer ones still served.
#[tokio::test]
async fn a_sync_leaves_the_target_with_every_version_either_held() {
	let (here, there) = (TestDb::create().await, TestDb::create().await);
	let t = |minute: i64| Timestamp::from_second(1_800_000_000 + minute * 60).unwrap();
	let keep = async |db: &TestDb, request: &str, v: i64, at: Timestamp| {
		db.store()
			.await
			.keep_query(Scheme::Gop, Key::of(request), &json!({ "v": v }).to_string(), request, None, at)
			.await
			.unwrap()
			.unwrap();
	};
	keep(&here, "both", 1, t(0)).await;
	keep(&there, "both", 1, t(0)).await;
	keep(&here, "here", 1, t(0)).await;
	keep(&here, "here", 2, t(1)).await;
	keep(&there, "newer there", 2, t(5)).await;
	keep(&here, "newer there", 1, t(1)).await;
	let union: BTreeSet<_> = here
		.store()
		.await
		.queries_index()
		.await
		.unwrap()
		.into_iter()
		.chain(there.store().await.queries_index().await.unwrap())
		.collect();

	let index = panel(&there, &["index"], b"");
	let file = std::env::temp_dir().join(format!("queries_index_{}", uuid::Uuid::now_v7().simple()));
	std::fs::write(&file, &index).unwrap();
	let lacking = panel(&here, &["export", "--except", file.to_str().unwrap()], b"");
	std::fs::remove_file(&file).unwrap();
	assert_eq!(lacking.lines().count(), 3, "what both hold is not sent: {lacking}");
	panel(&there, &["import"], lacking.as_bytes());

	let there = there.store().await;
	assert_eq!(there.queries_index().await.unwrap(), union);
	assert_eq!(there.cached_query(Scheme::Gop, Key::of("newer there")).await.unwrap().unwrap().answer, json!({ "v": 2 }));
	assert_eq!(
		there.cached_query(Scheme::Gop, Key::of("here")).await.unwrap(),
		Some(Stored {
			answer: json!({ "v": 2 }),
			fetched_at: t(1)
		})
	);
}
