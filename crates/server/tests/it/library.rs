//! The library (docs/ARCHITECTURE.md, Library): a category is versions its owner's head moves
//! over; a release is readable by everyone, for good, and adopted into the adopter's own category; a
//! newer release is shown, never taken but by its adopter; a study is published only on a released
//! version, and loads for anyone over the shared queries cache, its pull charged to whoever pulls.

use std::{
	collections::HashMap,
	sync::{
		Arc,
		atomic::{AtomicUsize, Ordering},
	},
};

use axum::{
	Json, Router,
	http::{Method, StatusCode},
	routing::post,
};
use gmaps_optimal_placement::testing;
use sa_auth::PermissionSet;
use sa_core::prices::{Item, PlacementRequest};
use sa_panel::{
	areas::Area,
	import::Export,
	seal::DataKey,
	testing::{TestDb, account, admin, operator, panel_on},
	tokens::Change,
};
use sa_places::{Google, Keys, LatLng};
use sa_server::{concierge::DevIdentity, placement::Placement, testing::Browser};
use serde_json::{Value, json};

const ANN: &str = "ann@localhost";
const BOB: &str = "bob@localhost";
const CAROL: &str = "carol@localhost";
const PLUMBER: &str = "/api/v1/categories/gcid:plumber/FR";
/// A pull's search, in tokens.
const PRICE: i64 = 8;

/// One conductor, placement over its fixture and central Clermont drawn; Ann, Bob and Carol signed
/// in with `permissions`, each holding 1000 tokens. Places answers every page with one plumber.
struct Shelf {
	_db: TestDb,
	_dir: tempfile::TempDir,
	app: Router,
	people: HashMap<&'static str, Browser>,
	searched: Arc<AtomicUsize>,
}

impl Shelf {
	async fn new(permissions: PermissionSet) -> Self {
		let db = TestDb::create().await;
		let searched = Arc::<AtomicUsize>::default();
		let counted = searched.clone();
		let search = move |Json(_): Json<Value>| async move {
			counted.fetch_add(1, Ordering::SeqCst);
			Json(json!({ "places": [{
				"id": "dupont", "displayName": {"text": "Dupont plombier"}, "rating": 4.5, "userRatingCount": 10,
				"location": {"latitude": testing::CENTER.lat, "longitude": testing::CENTER.lng},
			}] }))
		};
		let (places, _) = sa_server::testing::upstream(Router::new().route("/v1/places:searchText", post(search))).await;
		let google = Google::try_new(Keys::new(vec![("house".to_owned(), "k".to_owned())]), Arc::new(sa_places::InOrder), Arc::new(db.store().await))
			.unwrap()
			.spend(true)
			.at(format!("{places}/v1/places"));
		let p = panel_on(
			db.store().await,
			DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(),
			google,
			sa_places::Accounts::new(vec![account("house", Some("k"), None)]),
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
		sa_panel::testing::priced(&p, &[(Item::Placement(PlacementRequest::Search), PRICE)]).await;
		let dir = tempfile::tempdir().unwrap();
		testing::fixture(dir.path()).unwrap();
		let (sign_in, forward) = sa_server::testing::conductor(p.clone(), permissions, ANN, &"http://127.0.0.1:9".parse().unwrap());
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
		let mut people = HashMap::new();
		let now = jiff::Timestamp::now();
		for email in [ANN, BOB, CAROL] {
			let mut b = Browser::default();
			b.sign_in_as(&app, email).await;
			b.call(&app, Method::GET, "/api/v1/tokens", None).await; // the gate records who they are
			p.change_balance(DevIdentity::named(email), Change::Set { n: 1000, note: None }, "admin@localhost", now)
				.await
				.unwrap()
				.unwrap();
			people.insert(email, b);
		}
		let area = Area {
			slug: "clermont".into(),
			name: "Clermont".into(),
			country: "FR".into(),
			center: LatLng {
				lat: testing::CENTER.lat,
				lng: testing::CENTER.lng,
			},
			radius_m: testing::RADIUS_M as i64,
			city: None,
			created_by: DevIdentity::named(ANN),
			created_at: now,
		};
		sa_panel::store::areas::put_area(&mut db.pool().await.acquire().await.unwrap(), &area).await.unwrap();
		Self {
			_db: db,
			_dir: dir,
			app,
			people,
			searched,
		}
	}

	async fn call(&mut self, who: &str, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
		let b = self.people.get_mut(who).unwrap();
		b.call(&self.app, method, uri, (!body.is_null()).then_some(body)).await
	}

	/// `who`'s plumber in FR applied as `settings`: its head version's id.
	async fn apply(&mut self, who: &str, settings: Value) -> i64 {
		let (status, body) = self.call(who, Method::PUT, PLUMBER, settings).await;
		assert_eq!(status, StatusCode::OK, "{body}");
		body["head"]["id"].as_i64().unwrap()
	}

	/// `version` released as a new listing of `who`'s: the listing.
	async fn publish(&mut self, who: &str, version: i64, title: &str) -> Value {
		let (status, body) = self
			.call(who, Method::POST, "/api/v1/library", json!({ "version": version, "title": title, "about": "", "notes": "first" }))
			.await;
		assert_eq!(status, StatusCode::CREATED, "{body}");
		body
	}

	/// `who`'s plumber in FR, as `GET /categories` answers it.
	async fn plumber(&mut self, who: &str) -> Value {
		let (_, body) = self.call(who, Method::GET, "/api/v1/categories", Value::Null).await;
		body["categories"]
			.as_array()
			.unwrap()
			.iter()
			.find(|c| c["trade"]["id"] == "gcid:plumber")
			.cloned()
			.unwrap_or(Value::Null)
	}

	async fn balance(&mut self, who: &str) -> i64 {
		self.call(who, Method::GET, "/api/v1/tokens", Value::Null).await.1["balance"].as_i64().unwrap()
	}
}

/// Plombier's settings, a keyword weighed `w`.
fn settings(w: f64) -> Value {
	json!({
		"keywords": [{ "text": "plombier", "weight": 1.0 }, { "text": "fuite d'eau", "weight": w }],
		"additional": [], "excluded": { "words": [], "types": [] }, "demand": null,
	})
}

fn whys(history: &Value) -> Vec<(String, Value, Value)> {
	history["moves"]
		.as_array()
		.unwrap()
		.iter()
		.map(|m| (m["why"].as_str().unwrap().to_owned(), m["from"].clone(), m["to"].clone()))
		.collect()
}

#[tokio::test]
async fn an_edit_is_a_new_version_and_a_revert_a_move_back() {
	let mut s = Shelf::new(operator()).await;
	let one = s.apply(ANN, settings(0.5)).await;
	let two = s.apply(ANN, settings(0.7)).await;
	assert_ne!(one, two, "an edit writes a version, never over one");
	assert_eq!(s.apply(ANN, settings(0.7)).await, two, "the head's own settings move nothing");
	let (status, back) = s.call(ANN, Method::POST, &format!("{PLUMBER}/head"), json!({ "version": one })).await;
	assert_eq!((status, &back["head"]["id"]), (StatusCode::OK, &json!(one)), "{back}");
	let (_, history) = s.call(ANN, Method::GET, &format!("{PLUMBER}/history"), Value::Null).await;
	assert_eq!(
		whys(&history),
		[
			("edit".to_owned(), Value::Null, json!(one)),
			("edit".to_owned(), json!(one), json!(two)),
			("revert".to_owned(), json!(two), json!(one))
		]
	);
	let (status, _) = s.call(ANN, Method::POST, &format!("{PLUMBER}/head"), json!({ "version": 999 })).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "only to one of its own history");
}

#[tokio::test]
async fn a_release_is_adopted_into_the_adopters_own_category_and_updated_only_by_them() {
	let mut s = Shelf::new(operator()).await;
	let one = s.apply(ANN, settings(0.5)).await;
	let listing = s.publish(ANN, one, "Plombier, Clermont").await;
	let id = listing["id"].as_i64().unwrap();
	let (_, browsed) = s.call(BOB, Method::GET, "/api/v1/library?country=FR&q=plomb&sort=recent", Value::Null).await;
	assert_eq!(browsed["listings"][0]["id"], json!(id), "{browsed}");
	assert_eq!(browsed["listings"][0]["mine"], json!(false));

	let lv1 = listing["latest"]["id"].clone();
	let (status, adopted) = s.call(BOB, Method::POST, &format!("{PLUMBER}/head"), json!({ "version": one, "listing_version": lv1 })).await;
	assert_eq!(status, StatusCode::OK, "{adopted}");
	assert_eq!(
		(&adopted["head"]["id"], &adopted["lineage"]["listing"], &adopted["lineage"]["edited"]),
		(&json!(one), &json!(id), &json!(false)),
		"Bob's own category, standing on Ann's version"
	);
	assert_eq!(s.plumber(ANN).await["lineage"], Value::Null, "Ann's is her own");

	let two = s.apply(ANN, settings(0.9)).await;
	let (status, body) = s
		.call(
			ANN,
			Method::POST,
			&format!("/api/v1/library/{id}/versions"),
			json!({ "version": two, "notes": "drains weigh more" }),
		)
		.await;
	assert_eq!(status, StatusCode::CREATED, "{body}");
	let bobs = s.plumber(BOB).await;
	assert_eq!((&bobs["head"]["id"], &bobs["lineage"]["upstream"]["n"]), (&json!(one), &json!(2)), "shown, not taken: {bobs}");

	let upstream = bobs["lineage"]["upstream"].clone();
	let (status, updated) = s
		.call(
			BOB,
			Method::POST,
			&format!("{PLUMBER}/head"),
			json!({ "version": upstream["version"], "listing_version": upstream["id"] }),
		)
		.await;
	assert_eq!(
		(status, &updated["head"]["id"], &updated["lineage"]["upstream"]),
		(StatusCode::OK, &json!(two), &Value::Null),
		"{updated}"
	);
	let (_, history) = s.call(BOB, Method::GET, &format!("{PLUMBER}/history"), Value::Null).await;
	assert_eq!(whys(&history), [("adopt".to_owned(), Value::Null, json!(one)), ("update".to_owned(), json!(one), json!(two))]);
	let (_, detail) = s.call(CAROL, Method::GET, &format!("/api/v1/library/{id}"), Value::Null).await;
	assert_eq!(detail["listing"]["used_by"], json!(1), "Bob's lineage is it: {detail}");
}

#[tokio::test]
async fn an_unreleased_version_is_its_owners_alone() {
	let mut s = Shelf::new(operator()).await;
	let one = s.apply(ANN, settings(0.5)).await;
	let uri = format!("/api/v1/category-versions/{one}");
	assert_eq!(s.call(ANN, Method::GET, &uri, Value::Null).await.0, StatusCode::OK);
	assert_eq!(s.call(BOB, Method::GET, &uri, Value::Null).await.0, StatusCode::NOT_FOUND);
	let study = format!("/api/v1/placement/study?area=clermont&version={one}");
	assert_eq!(s.call(BOB, Method::GET, &study, Value::Null).await.0, StatusCode::NOT_FOUND, "nor studied by anyone else");
	let (status, _) = s
		.call(BOB, Method::POST, "/api/v1/library", json!({ "version": one, "title": "Mine now", "about": "", "notes": "" }))
		.await;
	assert_eq!(status, StatusCode::NOT_FOUND, "published only by one whose moves include it");
	s.publish(ANN, one, "Plombier").await;
	assert_eq!(s.call(BOB, Method::GET, &uri, Value::Null).await.0, StatusCode::OK, "released: everyone's to read");
}

#[tokio::test]
async fn curating_the_standard_library_needs_its_permission() {
	for (permissions, want) in [(operator(), StatusCode::FORBIDDEN), (admin(), StatusCode::NO_CONTENT)] {
		let mut s = Shelf::new(permissions).await;
		let one = s.apply(ANN, settings(0.5)).await;
		let id = s.publish(ANN, one, "Plombier").await["id"].clone();
		let (status, _) = s.call(BOB, Method::PUT, &format!("/api/v1/library/{id}/standard"), json!({ "rank": 1 })).await;
		assert_eq!(status, want);
		let (_, standard) = s.call(BOB, Method::GET, "/api/v1/library?kind=standard&sort=rank", Value::Null).await;
		assert_eq!(standard["listings"].as_array().unwrap().len(), usize::from(want == StatusCode::NO_CONTENT), "{standard}");
	}
}

#[tokio::test]
async fn a_withdrawn_listing_hides_from_strangers_and_stays_with_its_adopters() {
	let mut s = Shelf::new(operator()).await;
	let one = s.apply(ANN, settings(0.5)).await;
	let listing = s.publish(ANN, one, "Plombier").await;
	let id = listing["id"].as_i64().unwrap();
	s.call(
		BOB,
		Method::POST,
		&format!("{PLUMBER}/head"),
		json!({ "version": one, "listing_version": listing["latest"]["id"] }),
	)
	.await;
	assert_eq!(s.call(ANN, Method::POST, &format!("/api/v1/library/{id}/withdraw"), Value::Null).await.0, StatusCode::NO_CONTENT);

	let (_, browsed) = s.call(CAROL, Method::GET, "/api/v1/library?sort=recent", Value::Null).await;
	assert_eq!(browsed["listings"], json!([]), "hidden from browsing");
	assert_eq!(s.call(CAROL, Method::GET, &format!("/api/v1/library/{id}"), Value::Null).await.0, StatusCode::NOT_FOUND);
	let (status, _) = s
		.call(
			CAROL,
			Method::POST,
			&format!("{PLUMBER}/head"),
			json!({ "version": one, "listing_version": listing["latest"]["id"] }),
		)
		.await;
	assert_eq!(status, StatusCode::NOT_FOUND, "nor adopted by someone new");

	assert_eq!(
		s.call(BOB, Method::GET, &format!("/api/v1/library/{id}"), Value::Null).await.0,
		StatusCode::OK,
		"Bob stands on it"
	);
	assert_eq!(s.plumber(BOB).await["lineage"]["withdrawn"], json!(true));
	assert_eq!(
		s.call(CAROL, Method::GET, &format!("/api/v1/category-versions/{one}"), Value::Null).await.0,
		StatusCode::OK,
		"released is released"
	);
}

#[tokio::test]
async fn a_study_is_published_only_on_a_released_version_or_releasing_it() {
	let mut s = Shelf::new(operator()).await;
	let one = s.apply(ANN, settings(0.5)).await;
	let publish = |release: Value| json!({ "area": "clermont", "version": one, "title": "Clermont plumbers", "release": release });
	let (status, refused) = s.call(ANN, Method::POST, "/api/v1/library/studies", publish(Value::Null)).await;
	assert_eq!((status, &refused["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!("unreleased_category")), "{refused}");
	let (_, listings) = s.call(ANN, Method::GET, "/api/v1/library?kind=mine&sort=recent", Value::Null).await;
	assert_eq!(listings["listings"], json!([]), "nothing released by the refusal");

	let release = json!({ "as": "new", "title": "Plombier", "about": "", "notes": "", "forked_from": null });
	let (status, study) = s.call(ANN, Method::POST, "/api/v1/library/studies", publish(release)).await;
	assert_eq!(status, StatusCode::CREATED, "{study}");
	assert_eq!(study["release"]["version"], json!(one), "released in the same write");
	let (_, listed) = s.call(BOB, Method::GET, "/api/v1/library/studies?trade=gcid:plumber&country=FR&area=clermont", Value::Null).await;
	assert_eq!(listed["studies"][0]["sha"], study["sha"], "{listed}");
}

#[tokio::test]
async fn a_published_study_loads_for_anyone_and_their_pull_is_theirs() {
	let mut s = Shelf::new(admin()).await; // a pull spends: sa:placement:study:spend
	let one = s.apply(ANN, settings(0.5)).await;
	let release = json!({ "as": "new", "title": "Plombier", "about": "", "notes": "", "forked_from": null });
	let (_, study) = s
		.call(
			ANN,
			Method::POST,
			"/api/v1/library/studies",
			json!({ "area": "clermont", "version": one, "title": "Clermont", "release": release }),
		)
		.await;
	let uri = format!("/api/v1/placement/studies/{}", study["sha"].as_str().unwrap());

	let (status, payload) = s.call(BOB, Method::GET, &uri, Value::Null).await;
	assert_eq!(status, StatusCode::OK, "{payload}");
	assert_eq!((s.searched.load(Ordering::SeqCst), s.balance(BOB).await), (0, 1000), "a read buys nothing");

	let (status, pulled) = s.call(BOB, Method::POST, &format!("{uri}/pull"), Value::Null).await;
	assert_eq!(status, StatusCode::OK, "{pulled}");
	let bought = s.searched.load(Ordering::SeqCst);
	assert!(bought > 0, "the sweep was lacking");
	assert_eq!(
		(s.balance(BOB).await, s.balance(ANN).await),
		(1000 - PRICE * i64::try_from(bought).unwrap(), 1000),
		"charged to whoever pulls"
	);

	let (status, _) = s.call(CAROL, Method::GET, &uri, Value::Null).await;
	assert_eq!((status, s.searched.load(Ordering::SeqCst)), (StatusCode::OK, bought), "held: bought once, for everyone");
	let (status, _) = s.call(CAROL, Method::POST, &format!("{uri}/pull"), Value::Null).await;
	let asked = i64::try_from(s.searched.load(Ordering::SeqCst) - bought).unwrap();
	assert_eq!(
		(status, s.balance(CAROL).await, s.balance(BOB).await),
		(StatusCode::OK, 1000 - PRICE * asked, 1000 - PRICE * i64::try_from(bought).unwrap()),
		"a pull asks again, at whoever pulls"
	);
}
