//! A GMB's point: Places asked once for it, carried by `GET /gmails`, filled in later by
//! `locate`, placed by hand where Places has none; and the browser's map.

use std::sync::{Arc, Mutex};

use axum::{
	Json, Router,
	body::Body,
	extract::{Path, State},
	http::{Method, StatusCode},
	response::{IntoResponse, Response},
	routing::get,
};
use sa_core::gbp::{Country, OnGoogle};
use sa_panel::{
	Panel,
	gmbs::{Existing, Located},
	import::Export,
	seal::DataKey,
	testing::{TestDb, operator, panel_on},
};
use sa_places::{Google, Keys};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	gmbs::MapDto,
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};

const PROCOPE: &str = "ChIJprocopeprocopeproc";
const FLORE: &str = "ChIJfloreflorefloreflo";
/// A service-area business: Places gives it no point.
const ASSIST: &str = "ChIJassistassistassis";

/// Places, counting what it is asked.
async fn places() -> (Arc<Mutex<Vec<String>>>, String) {
	async fn details(State(asked): State<Arc<Mutex<Vec<String>>>>, Path(id): Path<String>) -> Response {
		asked.lock().unwrap().push(id.clone());
		let (lat, lng) = match id.as_str() {
			PROCOPE => (48.853, 2.3389),
			FLORE => (48.854, 2.3326),
			ASSIST => return Json(json!({ "id": id, "displayName": { "text": "Mold Assist" } })).into_response(),
			_ => return (StatusCode::NOT_FOUND, "not found").into_response(),
		};
		Json(
			json!({ "id": id, "displayName": { "text": id }, "formattedAddress": "Paris", "location": { "latitude": lat, "longitude": lng },
			"addressComponents": [{ "shortText": "FR", "types": ["country", "political"] }] }),
		)
		.into_response()
	}
	let asked = Arc::<Mutex<Vec<String>>>::default();
	let app = Router::new().route("/v1/places/{id}", get(details)).with_state(asked.clone());
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let base = format!("http://{}/v1/places", listener.local_addr().unwrap());
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(asked, base)
}

/// A panel on `db` asking `base` with a key (`None`: keyless), its answers in `db`, GMBs open.
async fn panel(db: &TestDb, base: Option<&str>) -> Panel {
	let keys = Keys::new(base.map(|_| vec!["k".to_owned()]).unwrap_or_default());
	let google = Google::try_new(keys, Arc::new(db.store().await)).unwrap();
	let google = match base {
		Some(b) => google.at(b),
		None => google,
	};
	let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
	panel_on(db.store().await, key, google)
}

async fn opened(p: &Panel) {
	let empty = Export {
		people: vec![],
		gmails: vec![],
		gmbs: vec![],
		ledger: vec![],
		kickbacks: vec![],
	};
	p.import_review_archive(empty).await.unwrap();
}

fn app(panel: Panel, map: Option<MapDto>) -> Router {
	let who = DevIdentity {
		user_id: DevIdentity::named("dev@localhost"),
		permissions: operator(),
		email: "dev@localhost".to_owned(),
	};
	let config = SignInConfig {
		panel_origin: ORIGIN.to_owned(),
		concierge_origin: ORIGIN.to_owned(),
	};
	http::app(SignIn::new(panel, Concierge::dev(who), config), None, None, Default::default(), None, map)
}

async fn call(b: &mut Browser, app: &Router, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
	let json = [("content-type", "application/json")];
	let (status, _, bytes) = b
		.send_body(
			app,
			method,
			uri,
			if body.is_some() { &json } else { &[] },
			true,
			body.map_or(Body::empty(), |b| Body::from(b.to_string())),
		)
		.await;
	(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

#[tokio::test]
async fn a_gmb_is_located_when_added_and_its_place_asked_once() {
	let (asked, base) = places().await;
	let db = TestDb::create().await;
	let p = panel(&db, Some(&base)).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;

	let mut gmails = Vec::new();
	for gmail in ["a@gmail.com", "b@gmail.com"] {
		let (status, added) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": gmail }))).await;
		assert_eq!(status, StatusCode::CREATED, "{added}");
		gmails.push(added["id"].as_i64().unwrap());
	}
	let add = json!({ "place_id": PROCOPE, "state": "verified" });
	let (status, gmb) = call(&mut b, &app, Method::POST, &format!("/api/v1/gmails/{}/gmbs", gmails[0]), Some(add.clone())).await;
	assert_eq!(status, StatusCode::CREATED, "{gmb}");
	assert_eq!(gmb["location"], json!({ "lat": 48.853, "lng": 2.3389, "placed": false }));
	assert_eq!(gmb["country"], json!({ "code": "FR", "by_hand": false }));
	let (status, again) = call(&mut b, &app, Method::POST, &format!("/api/v1/gmails/{}/gmbs", gmails[1]), Some(add)).await;
	assert_eq!(status, StatusCode::CONFLICT, "a place is one GBP's: {again}");
	assert_eq!(asked.lock().unwrap().len(), 1, "the place is not asked again");

	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
	assert_eq!(listed["gmbs"][0]["location"], json!({ "lat": 48.853, "lng": 2.3389, "placed": false }), "{listed}");
	assert_eq!(listed["gmbs"][0]["gmail_id"], gmails[0], "{listed}");
}

#[tokio::test]
async fn locate_fills_only_the_missing_points_and_pays_for_each_once() {
	let (asked, base) = places().await;
	let db = TestDb::create().await;
	let keyless = panel(&db, None).await;
	opened(&keyless).await;
	let owner = seen(&keyless, "dev@localhost").await;
	let gmail = keyless.add_gmail(owner, "a@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	for place in [PROCOPE, FLORE] {
		let gmb = keyless.add_gmb(Some(owner), existing(gmail.id, place), jiff::Timestamp::now()).await.unwrap().unwrap();
		assert_eq!(gmb.location, None, "no key, no point");
	}

	let keyed = panel(&db, Some(&base)).await;
	assert_eq!(keyed.locate().await.unwrap(), located(2, &[], &[]));
	let gbps = keyed.gmails(owner).await.unwrap().1;
	assert!(
		gbps.iter().all(|b| b.location.is_some() && b.country.is_some_and(|c| !c.by_hand)),
		"Google's country over one by hand"
	);
	assert_eq!(keyed.locate().await.unwrap(), located(0, &[], &[]));
	assert_eq!(asked.lock().unwrap().len(), 2, "a second run asks nothing");
}

/// On Google already, named and put in France by hand: what Places, keyless, cannot say.
fn existing(gmail_id: i64, place: &str) -> Existing {
	Existing {
		gmail_id,
		place: place.to_owned(),
		state: OnGoogle::Verified,
		name: Some(place.to_owned()),
		country: Some(Country::FR),
	}
}

fn located(n: usize, no_point: &[&str], no_country: &[&str]) -> Located {
	let owned = |places: &[&str]| places.iter().map(|p| p.to_string()).collect();
	Located {
		located: n,
		no_point: owned(no_point),
		no_country: owned(no_country),
		refused: vec![],
	}
}

async fn seen(p: &Panel, email: &str) -> uuid::Uuid {
	let user_id = DevIdentity::named(email);
	let user = sa_panel::tokens::User {
		user_id,
		email: email.into(),
		name: email.into(),
	};
	p.user_seen(&user, jiff::Timestamp::now()).await.unwrap();
	user_id
}

#[tokio::test]
async fn a_service_area_gmb_has_no_point_until_placed_on_the_map() {
	let (asked, base) = places().await;
	let db = TestDb::create().await;
	let keyless = panel(&db, None).await;
	opened(&keyless).await;
	let owner = seen(&keyless, "dev@localhost").await;
	let gmail = keyless.add_gmail(owner, "a@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	keyless.add_gmb(Some(owner), existing(gmail.id, FLORE), jiff::Timestamp::now()).await.unwrap().unwrap();

	let keyed = panel(&db, Some(&base)).await;
	let app = app(keyed.clone(), None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, assist) = call(
		&mut b,
		&app,
		Method::POST,
		&format!("/api/v1/gmails/{}/gmbs", gmail.id),
		Some(json!({ "place_id": ASSIST, "state": "verified", "country": "CA" })),
	)
	.await;
	assert_eq!((status, &assist["location"]), (StatusCode::CREATED, &Value::Null), "{assist}");
	assert_eq!(keyed.locate().await.unwrap(), located(1, &[ASSIST], &[ASSIST]), "listed, and the run goes on to the next place");

	let location = format!("/api/v1/gmbs/{}/location", assist["id"]);
	let point = |gmails: &Value| gmails["gmbs"].as_array().unwrap().iter().find(|g| g["id"] == assist["id"]).unwrap()["location"].clone();
	for (lat, lng) in [(43.589, -79.644), (43.6, -79.65)] {
		let (status, _) = call(&mut b, &app, Method::PUT, &location, Some(json!({ "lat": lat, "lng": lng }))).await;
		assert_eq!(status, StatusCode::NO_CONTENT);
		let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
		assert_eq!(point(&listed), json!({ "lat": lat, "lng": lng, "placed": true }), "placed, then moved");
	}
	assert_eq!(keyed.locate().await.unwrap(), located(0, &[], &[ASSIST]), "placed, still without a country");
	assert_eq!(*asked.lock().unwrap(), [ASSIST, FLORE], "a place is not paid for again");
}

#[tokio::test]
async fn a_point_is_placed_only_on_the_callers_gmb_without_googles_and_on_earth() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let p = panel(&db, Some(&base)).await;
	opened(&p).await;
	let other = seen(&p, "other@localhost").await;
	let theirs = p.add_gmail(other, "other@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	let theirs = p.add_gmb(Some(other), existing(theirs.id, FLORE), jiff::Timestamp::now()).await.unwrap().unwrap();
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let mut mine = Vec::new();
	for place in [PROCOPE, ASSIST] {
		let add = json!({ "place_id": place, "state": "verified", "country": "FR" });
		let (_, gmb) = call(&mut b, &app, Method::POST, &format!("/api/v1/gmails/{}/gmbs", gmail["id"]), Some(add)).await;
		mine.push(gmb["id"].as_i64().unwrap());
	}

	let paris = json!({ "lat": 48.86, "lng": 2.35 });
	for (gmb, at, want) in [
		(mine[0], &paris, StatusCode::CONFLICT),
		(theirs.id, &paris, StatusCode::NOT_FOUND),
		(mine[1], &json!({ "lat": 91.0, "lng": 2.35 }), StatusCode::BAD_REQUEST),
		(mine[1], &json!({ "lat": 48.86, "lng": -180.5 }), StatusCode::BAD_REQUEST),
	] {
		let (status, body) = call(&mut b, &app, Method::PUT, &format!("/api/v1/gmbs/{gmb}/location"), Some(at.clone())).await;
		assert_eq!(status, want, "GMB {gmb} at {at}: {body}");
	}
}

#[tokio::test]
async fn the_map_is_told_only_when_configured() {
	let db = TestDb::create().await;
	let configured = MapDto { key: "browser-key".into() };
	for (map, want) in [
		(None, (StatusCode::NOT_FOUND, json!({ "error": "not found" }))),
		(Some(configured), (StatusCode::OK, json!({ "key": "browser-key" }))),
	] {
		let app = app(panel(&db, None).await, map);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		assert_eq!(call(&mut b, &app, Method::GET, "/api/v1/map", None).await, want);
	}
}

#[tokio::test]
async fn a_draft_goes_onto_google_and_is_verified() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let p = panel(&db, Some(&base)).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;

	let (status, draft) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(json!({ "name": "Café de Flore" }))).await;
	assert_eq!(
		(status, &draft["state"], &draft["gmail_id"], &draft["place_id"]),
		(StatusCode::CREATED, &json!("draft"), &Value::Null, &Value::Null)
	);
	let gbp = format!("/api/v1/gmbs/{}", draft["id"]);
	let (status, _) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(json!({ "name": "Café de Flore" }))).await;
	assert_eq!(status, StatusCode::CONFLICT);
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
	assert_eq!(listed["gmbs"][0]["id"], draft["id"], "a draft is listed with no gmail: {listed}");

	let to_google = json!({ "state": "verifying", "place": FLORE });
	let (status, body) = call(&mut b, &app, Method::PUT, &format!("{gbp}/state"), Some(to_google.clone())).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "no gmail yet: {body}");
	let (status, edited) = call(&mut b, &app, Method::PATCH, &gbp, Some(json!({ "gmail_id": gmail["id"], "phone": " +33 1 45 48 55 26 " }))).await;
	assert_eq!((status, &edited["gmail_id"], &edited["phone"]), (StatusCode::OK, &gmail["id"], &json!("+33145485526")));
	let (_, cleared) = call(&mut b, &app, Method::PATCH, &gbp, Some(json!({ "phone": null }))).await;
	assert_eq!(cleared["phone"], Value::Null);

	let (status, verifying) = call(&mut b, &app, Method::PUT, &format!("{gbp}/state"), Some(to_google)).await;
	assert_eq!(
		(status, &verifying["state"], &verifying["place_id"]),
		(StatusCode::OK, &json!("verifying"), &json!(FLORE)),
		"{verifying}"
	);
	assert_eq!(verifying["country"], json!({ "code": "FR", "by_hand": false }), "Google's");
	let (status, _) = call(&mut b, &app, Method::PATCH, &gbp, Some(json!({ "name": "Flore" }))).await;
	assert_eq!(status, StatusCode::CONFLICT, "its name is Google's now");
	let (status, verified) = call(&mut b, &app, Method::PUT, &format!("{gbp}/state"), Some(json!({ "state": "verified" }))).await;
	assert_eq!((status, &verified["state"]), (StatusCode::OK, &json!("verified")));
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("{gbp}/state"), Some(json!({ "state": "draft" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_drafts_must_haves_are_patched_and_a_phone_is_one_gbps() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, first) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(json!({ "name": "Lyon 7" }))).await;
	let (_, second) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(json!({ "name": "Lyon 8" }))).await;
	let first = format!("/api/v1/gmbs/{}", first["id"]);

	let must_haves = json!({ "phone": "04 78 00 00 01", "category": "Plombier", "areas": ["Lyon 7e"], "hidden_address": "12 rue de Marseille",
		"customers_visit": false, "film": "verif_vid:42", "profile": "mlx:lyon-7" });
	let (status, edited) = call(&mut b, &app, Method::PATCH, &first, Some(must_haves.clone())).await;
	assert_eq!(status, StatusCode::OK, "{edited}");
	for (field, want) in must_haves.as_object().unwrap() {
		let want = if field == "phone" { &json!("0478000001") } else { want };
		assert_eq!(&edited[field], want, "{field}");
	}
	let (_, cleared) = call(&mut b, &app, Method::PATCH, &first, Some(json!({ "areas": null, "customers_visit": null }))).await;
	assert_eq!(
		(&cleared["areas"], &cleared["customers_visit"], &cleared["category"]),
		(&json!([]), &Value::Null, &json!("Plombier"))
	);
	let (status, _) = call(
		&mut b,
		&app,
		Method::PATCH,
		&first,
		Some(json!({ "areas": (0..21).map(|i| format!("Lyon {i}")).collect::<Vec<_>>() })),
	)
	.await;
	assert_eq!(status, StatusCode::BAD_REQUEST);

	let (status, body) = call(&mut b, &app, Method::PATCH, &format!("/api/v1/gmbs/{}", second["id"]), Some(json!({ "phone": "04.78.00.00.01" }))).await;
	assert_eq!(status, StatusCode::CONFLICT, "{body}");
}

#[tokio::test]
async fn a_gmails_cap_follows_its_role() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	assert_eq!((&gmail["role"], &gmail["cap"]), (&Value::Null, &Value::Null), "unknown until said");
	let uri = format!("/api/v1/gmails/{}", gmail["id"]);
	for (role, cap) in [("owner", 3), ("manager", 6)] {
		let (status, set) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": role }))).await;
		assert_eq!((status, &set["role"], &set["cap"]), (StatusCode::OK, &json!(role), &json!(cap)));
	}
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
	assert_eq!(listed["gmails"][0]["cap"], json!(6), "{listed}");
	let (status, _) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": "admin" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
	let (status, _) = call(&mut b, &app, Method::PATCH, "/api/v1/gmails/999", Some(json!({ "role": "owner" }))).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
}
