//! A study is defined by the conductor from its own rows (docs/ARCHITECTURE.md, Forward:
//! Placement): an area and one of the caller's categories, PUT at placement under the
//! definition's id, which is all the browser is handed.

use axum::{
	Json, Router,
	body::Body,
	extract::Path,
	http::{Method, StatusCode},
	response::IntoResponse,
	routing::{get, put},
};
use sa_auth::{Principal, Service};
use sa_core::placement::Definition;
use sa_panel::{
	areas::{Area, Shape},
	import::Export,
	testing::{TestDb, operator, panel},
};
use sa_places::{LatLng, Viewport};
use sa_server::{
	concierge::DevIdentity,
	http,
	testing::{Browser, Log, conductor, services, upstream},
};
use serde_json::{Value, json};

const DOCUMENT: &str = include_str!("../../../frontend/src/shared/api/services/placement.json");
const EMAIL: &str = "dev@localhost";

/// Placement's define: every definition is studied, but one whose study is `carpentry`.
async fn define(Path(sha): Path<String>, Json(def): Json<Definition>) -> axum::response::Response {
	assert_eq!(def.id(), sha, "a definition is PUT under its id");
	match def.category.as_ref().map(|c| c.study.as_str()) {
		Some("carpentry") => (StatusCode::UNPROCESSABLE_ENTITY, Json(json!({ "code": "unstudied", "error": "no trade file carpentry" }))).into_response(),
		_ => StatusCode::NO_CONTENT.into_response(),
	}
}

struct Conductor {
	db: TestDb,
	app: Router,
	seen: Log,
	b: Browser,
}

impl Conductor {
	/// Placement documented and up; Lyon drawn.
	async fn new() -> Self {
		let db = TestDb::create().await;
		let p = panel(&db).await;
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
		let lyon = Area {
			slug: "lyon".into(),
			name: "Lyon".into(),
			country: "FR".into(),
			center: LatLng { lat: 45.76, lng: 4.84 },
			shape: Shape::City {
				place_id: "ChIJl4foalHq9EcR8CG75CqrCAQ".into(),
				viewport: Viewport {
					south: 45.7,
					west: 4.77,
					north: 45.81,
					east: 4.9,
				},
			},
			margin_m: 2000,
			created_by: DevIdentity::named(EMAIL),
			created_at: jiff::Timestamp::now(),
		};
		let placement = Router::new()
			.route("/openapi.json", get(|| async { ([("content-type", "application/json")], DOCUMENT) }))
			.route("/studies/{sha}", put(define));
		let (base, seen) = upstream(placement).await;
		let (sign_in, forward) = conductor(p, operator(), EMAIL, &base);
		forward.catalog().follow(services(&base), tokio::sync::watch::channel(true).1).await;
		seen.lock().unwrap().clear();
		let app = http::app(sign_in, Some(forward), None, Default::default(), None, None, None, None, None);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		let mut c = Self { db, app, seen, b };
		c.call(Method::GET, "/api/v1/tokens", Value::Null).await; // the gate records who they are, whom the area is by
		sa_panel::store::areas::put_area(&mut c.db.pool().await.acquire().await.unwrap(), &lyon).await.unwrap();
		c
	}

	async fn call(&mut self, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
		let (status, _, body) = self
			.b
			.send_body(&self.app, method, uri, &[("content-type", "application/json")], true, Body::from(body.to_string()))
			.await;
		(status, serde_json::from_slice(&body).unwrap_or(Value::Null))
	}

	/// A trade of the caller's, set up for `country` as `set_up` says: its id.
	async fn trade(&mut self, name: &str, country: &str, set_up: Value) -> i64 {
		let (_, trade) = self.call(Method::POST, "/api/v1/trades", json!({ "name": name })).await;
		let id = trade["id"].as_i64().unwrap();
		let (status, body) = self.call(Method::PUT, &format!("/api/v1/trades/{id}/{country}"), set_up).await;
		assert_eq!(status, StatusCode::OK, "{body}");
		id
	}

	async fn study(&mut self, category: Value) -> (StatusCode, Value) {
		self.call(Method::POST, "/api/v1/areas/lyon/studies", json!({ "category": category })).await
	}

	/// What placement was PUT since the last look.
	fn defined(&self) -> Vec<(String, Definition)> {
		let seen = std::mem::take(&mut *self.seen.lock().unwrap());
		seen.into_iter()
			.filter(|s| s.method == Method::PUT)
			.map(|s| {
				assert_eq!(s.headers["content-type"], "application/json", "ev_lib's Body asks for it");
				let asserted = s.assertion(Service::Placement, None);
				assert!(
					matches!(asserted.principal, Principal::User(u) if u.sub == DevIdentity::named(EMAIL).to_string()),
					"as the caller, by their uuid"
				);
				(s.path_and_query, serde_json::from_slice(&s.body).unwrap())
			})
			.collect()
	}
}

fn set_up(study: Option<&str>, primary: Option<&str>, keywords: &[&str]) -> Value {
	json!({ "keywords": keywords, "google_primary": primary, "google_additional": [], "study": study })
}

#[tokio::test]
async fn a_category_over_an_area_is_defined_at_placement_and_only_its_id_handed_back() {
	let mut c = Conductor::new().await;
	let plumbing = c.trade("Plumbing", "FR", set_up(Some("plumbing"), Some("gcid:plumber"), &["plombier", "fuite d'eau"])).await;

	let (status, body) = c.study(json!({ "trade": plumbing, "country": "FR" })).await;
	assert_eq!(status, StatusCode::OK, "{body}");
	let sha = body["sha"].as_str().unwrap().to_owned();
	assert_eq!(body, json!({ "sha": sha }));
	let [(path, def)] = c.defined().try_into().unwrap();
	assert_eq!(path, format!("/studies/{sha}"));
	let category = def.category.unwrap();
	assert_eq!(
		(category.study.as_str(), category.google_primary.name.as_str()),
		("plumbing", "Plombier"),
		"named in the country's language"
	);
	assert_eq!(category.trade, u64::try_from(plumbing).unwrap());

	let (status, body) = c.study(Value::Null).await;
	assert_eq!(status, StatusCode::OK, "{body}");
	let [(path, def)] = c.defined().try_into().unwrap();
	assert_eq!((path, def.category, def.area.slug()), (format!("/studies/{}", body["sha"].as_str().unwrap()), None, "lyon"));
}

#[tokio::test]
async fn a_category_not_set_up_to_be_studied_is_refused_by_why() {
	let mut c = Conductor::new().await;
	for (name, country, set_up, code) in [
		("Roofing", "FR", set_up(None, Some("gcid:roofing_contractor"), &["couvreur"]), "no_study"),
		("Painting", "FR", set_up(Some("painting"), None, &["peintre"]), "no_google_category"),
		("Locksmith", "FR", set_up(Some("locksmith"), Some("gcid:locksmith"), &[]), "no_keywords"),
		("Cleaning", "US", set_up(Some("cleaning"), Some("gcid:plumber"), &["cleaner"]), "another_country"),
	] {
		let trade = c.trade(name, country, set_up).await;
		let (status, body) = c.study(json!({ "trade": trade, "country": country })).await;
		assert_eq!((status, &body["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!(code)), "{name}: {body}");
		assert!(c.defined().is_empty(), "{name}: placement is not asked");
	}
	let (status, _) = c.study(json!({ "trade": 9999, "country": "FR" })).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "not a category of the caller's");

	let carpentry = c.trade("Carpentry", "FR", set_up(Some("carpentry"), Some("gcid:plumber"), &["menuisier"])).await;
	let (status, body) = c.study(json!({ "trade": carpentry, "country": "FR" })).await;
	assert_eq!((status, &body["code"]), (StatusCode::UNPROCESSABLE_ENTITY, &json!("unstudied")), "placement's own word: {body}");
}
