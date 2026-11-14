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
use sa_core::{
	gbp::{Country, OnGoogle},
	prices::{Item, PlacesRequest},
};
use sa_panel::{
	Panel,
	gmbs::{Existing, Gmb, Located, Unplaced},
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
	async fn autocomplete(State(asked): State<Arc<Mutex<Vec<String>>>>, Json(body): Json<Value>) -> Json<Value> {
		asked.lock().unwrap().push(format!("autocomplete {}", body["input"]));
		Json(json!({ "suggestions": [
			{ "placePrediction": { "placeId": FLORE, "structuredFormat": { "mainText": { "text": "Café de Flore" }, "secondaryText": { "text": "Paris, France" } } } },
			{ "placePrediction": { "placeId": ASSIST, "structuredFormat": { "mainText": { "text": "Mold Assist" } } } }
		] }))
	}
	let asked = Arc::<Mutex<Vec<String>>>::default();
	let app = Router::new()
		.route("/v1/places/{id}", get(details))
		.route("/v1/places:autocomplete", axum::routing::post(autocomplete))
		.with_state(asked.clone());
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let base = format!("http://{}/v1/places", listener.local_addr().unwrap());
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(asked, base)
}

/// A panel on `db` asking `base` with a key (`None`: keyless), its answers in `db`, its requests free.
async fn panel(db: &TestDb, base: Option<&str>) -> Panel {
	let keys = Keys::new(base.map(|_| vec!["k".to_owned()]).unwrap_or_default());
	let google = Google::try_new(keys, Arc::new(db.store().await)).unwrap();
	let google = match base {
		Some(b) => google.at(b),
		None => google,
	};
	let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
	let p = panel_on(db.store().await, key, google);
	// what a point costs is tests/connections.rs's
	p.set_prices(
		&[PlacesRequest::Details, PlacesRequest::Search, PlacesRequest::Autocomplete].map(|r| (Item::Places(r), Some(0))),
		"admin@localhost",
		jiff::Timestamp::now(),
	)
	.await
	.unwrap()
	.unwrap();
	p
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
	http::app(SignIn::new(panel, Concierge::dev(who), config), None, None, Default::default(), None, map, None, None)
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
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let add = json!({ "place_id": PROCOPE, "state": "verified", "trade": trade["id"] });
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
		let gmb = add(&keyless, owner, gmail.id, place).await;
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

/// On Google already, named and put in France by hand: what Places, keyless, cannot say. Of the
/// owner's trade, added the first time.
async fn add(p: &Panel, owner: uuid::Uuid, gmail_id: i64, place: &str) -> Gmb {
	let trade = match p.trades(owner).await.unwrap().first() {
		Some(t) => t.id,
		None => p.add_trade(owner, "Plumbing", jiff::Timestamp::now()).await.unwrap().unwrap().id,
	};
	let existing = Existing {
		gmail_id,
		place: place.to_owned(),
		state: OnGoogle::Verified,
		name: Some(place.to_owned()),
		country: Some(Country::FR),
		trade,
	};
	p.add_gmb(Some(owner), existing, jiff::Timestamp::now()).await.unwrap().unwrap()
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
	add(&keyless, owner, gmail.id, FLORE).await;

	let keyed = panel(&db, Some(&base)).await;
	let app = app(keyed.clone(), None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, assist) = call(
		&mut b,
		&app,
		Method::POST,
		&format!("/api/v1/gmails/{}/gmbs", gmail.id),
		Some(json!({ "place_id": ASSIST, "state": "verified", "country": "CA", "trade": keyed.trades(owner).await.unwrap()[0].id })),
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
	let (status, _) = call(&mut b, &app, Method::DELETE, &location, None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
	assert_eq!(point(&listed), Value::Null, "a point placed by hand is taken back");
	let (status, _) = call(&mut b, &app, Method::PUT, &location, Some(json!({ "lat": 43.6, "lng": -79.65 }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(keyed.locate().await.unwrap(), located(0, &[], &[ASSIST]), "placed, still without a country");
	assert_eq!(*asked.lock().unwrap(), [ASSIST, FLORE], "a place is not paid for again");
}

#[tokio::test]
async fn a_point_is_placed_and_taken_back_only_on_the_callers_gmb_without_googles_and_on_earth() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let p = panel(&db, Some(&base)).await;
	opened(&p).await;
	let other = seen(&p, "other@localhost").await;
	let theirs = p.add_gmail(other, "other@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	let theirs = add(&p, other, theirs.id, FLORE).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let mut mine = Vec::new();
	for place in [PROCOPE, ASSIST] {
		let add = json!({ "place_id": place, "state": "verified", "country": "FR", "trade": trade["id"] });
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
	for (gmb, want) in [(mine[0], StatusCode::CONFLICT), (theirs.id, StatusCode::NOT_FOUND), (mine[1], StatusCode::CONFLICT)] {
		let (status, body) = call(&mut b, &app, Method::DELETE, &format!("/api/v1/gmbs/{gmb}/location"), None).await;
		assert_eq!(status, want, "GMB {gmb} taken off the map: {body}");
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

	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let new = json!({ "name": "Café de Flore", "trade": trade["id"], "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] });
	let (status, draft) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new.clone())).await;
	assert_eq!(
		(status, &draft["state"], &draft["gmail_id"], &draft["place_id"]),
		(StatusCode::CREATED, &json!("draft"), &Value::Null, &Value::Null)
	);
	let gbp = format!("/api/v1/gmbs/{}", draft["id"]);
	let (status, _) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	assert_eq!(status, StatusCode::CONFLICT);
	let (status, body) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Nowhere", "trade": trade["id"], "state": "draft" })),
	)
	.await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a draft is somewhere: {body}");

	let spots = json!([
		{ "lat": 48.85, "lng": 2.33, "label": "Saint-Germain", "note": "near the café" },
		{ "lat": 45.76, "lng": 4.84, "label": null, "note": null }
	]);
	let (status, set) = call(&mut b, &app, Method::PUT, &format!("{gbp}/spots"), Some(json!({ "spots": spots }))).await;
	assert_eq!(
		(status, &set["spots"], &set["location"]),
		(StatusCode::OK, &spots, &json!({ "lat": 48.85, "lng": 2.33, "placed": true })),
		"a draft is where its first spot is"
	);
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("{gbp}/spots"), Some(json!({ "spots": [] }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
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
	assert_eq!(verifying["spots"], json!([]), "spots are a draft's");
	let (status, _) = call(&mut b, &app, Method::PATCH, &gbp, Some(json!({ "name": "Flore" }))).await;
	assert_eq!(status, StatusCode::CONFLICT, "its name is Google's now");
	let (status, verified) = call(&mut b, &app, Method::PUT, &format!("{gbp}/state"), Some(json!({ "state": "verified" }))).await;
	assert_eq!((status, &verified["state"]), (StatusCode::OK, &json!("verified")));
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("{gbp}/state"), Some(json!({ "state": "draft" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_gbp_off_maps_is_added_without_its_place_and_given_it_once_verified() {
	let (asked, base) = places().await;
	let db = TestDb::create().await;
	let p = panel(&db, Some(&base)).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;

	let suggested = json!({ "suggestions": [
		{ "place_id": FLORE, "main": "Café de Flore", "secondary": "Paris, France" },
		{ "place_id": ASSIST, "main": "Mold Assist", "secondary": null }
	] });
	for input in ["Caf%C3%A9+de+Flore", "+Caf%C3%A9++de+Flore"] {
		let got = call(&mut b, &app, Method::GET, &format!("/api/v1/gmbs/suggest?input={input}&gmail={}", gmail["id"]), None).await;
		assert_eq!(got, (StatusCode::OK, suggested.clone()), "{input}");
	}
	assert_eq!(
		*asked.lock().unwrap(),
		[r#"autocomplete "Café de Flore""#],
		"the same input, spaced otherwise, is not paid for again"
	);
	let (status, found) = call(&mut b, &app, Method::GET, &format!("/api/v1/gmbs/resolve?place={FLORE}"), None).await;
	assert_eq!(
		(status, &found["candidates"][0]["location"], &found["candidates"][0]["country"]),
		(StatusCode::OK, &json!({ "lat": 48.854, "lng": 2.3326 }), &json!("FR")),
		"{found}"
	);

	let new = json!({ "name": "Flore", "trade": trade["id"], "state": "verifying", "gmail_id": gmail["id"], "country": "FR", "location": { "lat": 48.85, "lng": 2.33 } });
	for (field, value) in [("state", json!("verified")), ("trade", Value::Null), ("gmail_id", Value::Null), ("country", Value::Null)] {
		let mut refused = new.clone();
		refused[field] = value;
		let (status, body) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(refused)).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{field}: {body}");
	}
	let (status, gbp) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	assert_eq!(status, StatusCode::CREATED, "{gbp}");
	assert_eq!(
		(&gbp["state"], &gbp["place_id"], &gbp["location"], &gbp["country"]),
		(
			&json!("verifying"),
			&Value::Null,
			&json!({ "lat": 48.85, "lng": 2.33, "placed": true }),
			&json!({ "code": "FR", "by_hand": true })
		)
	);

	let state = format!("/api/v1/gmbs/{}/state", gbp["id"]);
	let (status, body) = call(&mut b, &app, Method::PUT, &state, Some(json!({ "state": "verified" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "verified with its place: {body}");
	let (status, verified) = call(&mut b, &app, Method::PUT, &state, Some(json!({ "state": "verified", "place": FLORE }))).await;
	assert_eq!(status, StatusCode::OK, "{verified}");
	assert_eq!(
		(&verified["state"], &verified["place_id"], &verified["location"], &verified["country"]),
		(
			&json!("verified"),
			&json!(FLORE),
			&json!({ "lat": 48.854, "lng": 2.3326, "placed": false }),
			&json!({ "code": "FR", "by_hand": false })
		),
		"Google's over what was set by hand"
	);
	assert_eq!(asked.lock().unwrap().iter().filter(|a| *a == FLORE).count(), 1, "the place picked is asked once");
}

#[tokio::test]
async fn a_drafts_must_haves_are_patched_and_a_phone_is_one_gbps() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, heating) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Heating" }))).await;
	let (_, first) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Lyon 7", "trade": heating["id"], "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
	)
	.await;
	let (_, second) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Lyon 8", "trade": heating["id"], "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
	)
	.await;
	let first = format!("/api/v1/gmbs/{}", first["id"]);
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;

	let must_haves = json!({ "phone": "04 78 00 00 01", "trade": trade["id"], "areas": ["Lyon 7e"], "hidden_address": "12 rue de Marseille",
		"customers_visit": false, "film": "verif_vid:42" });
	let (status, edited) = call(&mut b, &app, Method::PATCH, &first, Some(must_haves.clone())).await;
	assert_eq!(status, StatusCode::OK, "{edited}");
	for (field, want) in must_haves.as_object().unwrap() {
		let (field, want) = match field.as_str() {
			"phone" => ("phone", &json!("0478000001")),
			"trade" => ("trade_id", want),
			f => (f, want),
		};
		assert_eq!(&edited[field], want, "{field}");
	}
	let (_, cleared) = call(&mut b, &app, Method::PATCH, &first, Some(json!({ "areas": null, "customers_visit": null }))).await;
	assert_eq!((&cleared["areas"], &cleared["customers_visit"], &cleared["trade_id"]), (&json!([]), &Value::Null, &trade["id"]));
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
async fn a_gmails_role_is_optional() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	assert_eq!(gmail["role"], Value::Null, "unknown until said");
	let uri = format!("/api/v1/gmails/{}", gmail["id"]);
	for role in [json!("owner"), json!("manager"), Value::Null] {
		let (status, set) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": role }))).await;
		assert_eq!((status, &set["role"]), (StatusCode::OK, &role));
	}
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
	assert_eq!(listed["gmails"][0]["role"], Value::Null, "{listed}");
	for bad in [json!({ "role": "admin" }), json!({})] {
		let (status, _) = call(&mut b, &app, Method::PATCH, &uri, Some(bad)).await;
		assert_eq!(status, StatusCode::BAD_REQUEST);
	}
	let (status, _) = call(&mut b, &app, Method::PATCH, "/api/v1/gmails/999", Some(json!({ "role": "owner" }))).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Signed in as dev@localhost, beside a trade and a GBP of other@localhost's.
async fn beside_another(db: &TestDb) -> (Router, Browser, i64, i64) {
	let p = panel(db, None).await;
	opened(&p).await;
	let other = seen(&p, "other@localhost").await;
	let trade = p.add_trade(other, "Plumbing", jiff::Timestamp::now()).await.unwrap().unwrap();
	let unplaced = Unplaced {
		name: "Theirs".to_owned(),
		trade: trade.id,
		state: sa_core::gbp::State::Draft,
		gmail_id: None,
		country: None,
		at: None,
		spots: vec![sa_panel::gmbs::Spot {
			at: sa_places::LatLng { lat: 45.76, lng: 4.84 },
			label: None,
			note: None,
		}],
	};
	let gbp = p.create_gmb(other, unplaced, jiff::Timestamp::now()).await.unwrap().unwrap();
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	(app, b, trade.id, gbp.id)
}

#[tokio::test]
async fn a_trade_is_its_owners() {
	let db = TestDb::create().await;
	let (app, mut b, theirs, _) = beside_another(&db).await;
	let (status, plumbing) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	assert_eq!(status, StatusCode::CREATED, "a name is one owner's");
	let (_, mine) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Lyon 7", "trade": plumbing["id"], "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
	)
	.await;
	let set_up = json!({ "keywords": [], "google_primary": null, "google_additional": [], "study": null });
	for (method, uri, body) in [
		(Method::PATCH, format!("/api/v1/gmbs/{}", mine["id"]), Some(json!({ "trade": theirs }))),
		(
			Method::POST,
			"/api/v1/gmbs".to_owned(),
			Some(json!({ "name": "Lyon 8", "trade": theirs, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
		),
		(Method::PATCH, format!("/api/v1/trades/{theirs}"), Some(json!({ "name": "Mine" }))),
		(Method::PUT, format!("/api/v1/trades/{theirs}/FR"), Some(set_up)),
		(Method::DELETE, format!("/api/v1/trades/{theirs}"), None),
	] {
		let (status, body) = call(&mut b, &app, method.clone(), &uri, body).await;
		assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {body}");
	}
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/trades", None).await;
	assert_eq!(listed["trades"].as_array().unwrap().len(), 1, "{listed}");
}

#[tokio::test]
async fn a_trade_a_live_gbp_holds_is_not_deleted() {
	let db = TestDb::create().await;
	let (app, mut b, _, _) = beside_another(&db).await;
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let uri = format!("/api/v1/trades/{}", trade["id"]);
	let set_up = json!({ "keywords": ["plombier"], "google_primary": "gcid:plumber", "google_additional": [], "study": "plumbing" });
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("{uri}/FR"), Some(set_up)).await;
	assert_eq!(status, StatusCode::OK);
	let (_, gbp) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Lyon 7", "trade": trade["id"], "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
	)
	.await;
	assert_eq!(gbp["trade_id"], trade["id"], "{gbp}");

	let (status, body) = call(&mut b, &app, Method::DELETE, &uri, None).await;
	assert_eq!(status, StatusCode::CONFLICT, "{body}");
	let (status, _) = call(&mut b, &app, Method::DELETE, &format!("/api/v1/gmbs/{}", gbp["id"]), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (status, body) = call(&mut b, &app, Method::DELETE, &uri, None).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "a removed GBP gives it up: {body}");
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/trades", None).await;
	assert_eq!(listed, json!({ "trades": [] }));
}

#[tokio::test]
async fn keywords_are_one_spelling_each_and_none_twice() {
	let db = TestDb::create().await;
	let (app, mut b, _, _) = beside_another(&db).await;
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let (status, _) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": " PLUMBING " }))).await;
	assert_eq!(status, StatusCode::CONFLICT, "a name whatever its case");
	let uri = format!("/api/v1/trades/{}/FR", trade["id"]);
	let keywords = |k: Value| Some(json!({ "keywords": k, "google_primary": null, "google_additional": [], "study": null }));

	let (status, body) = call(&mut b, &app, Method::PUT, &uri, keywords(json!(["Plombier", " plombier  "]))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	let (status, body) = call(&mut b, &app, Method::PUT, &uri, keywords(json!(["  Fuite \t d'eau ", "Plombier"]))).await;
	assert_eq!((status, &body["categories"][0]["keywords"]), (StatusCode::OK, &json!(["fuite d'eau", "plombier"])), "{body}");
}

#[tokio::test]
async fn google_categories_are_googles_in_the_country_in_order() {
	let db = TestDb::create().await;
	let (app, mut b, _, _) = beside_another(&db).await;
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let uri = format!("/api/v1/trades/{}/FR", trade["id"]);
	let google = |primary: Value, additional: Value| Some(json!({ "keywords": [], "google_primary": primary, "google_additional": additional, "study": null }));

	let (status, body) = call(
		&mut b,
		&app,
		Method::PUT,
		&uri,
		google(json!("gcid:plumber"), json!(["gcid:heating_contractor", "gcid:drainage_service"])),
	)
	.await;
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(body["categories"][0]["google_primary"]["id"], "gcid:plumber");
	assert_eq!(body["categories"][0]["google_primary"]["names"]["fr"], "Plombier");
	let names: Vec<&str> = body["categories"][0]["google_additional"]
		.as_array()
		.unwrap()
		.iter()
		.map(|g| g["names"]["fr"].as_str().unwrap())
		.collect();
	assert_eq!(names, ["Chauffagiste", "Service d'évacuation des eaux"]);

	let ten: Vec<Value> = [
		"electrician", "roofing_contractor", "painter", "carpenter", "locksmith", "glazier", "general_contractor", "tile_contractor", "handyman", "gardener",
	]
	.map(|g| json!(format!("gcid:{g}")))
	.into();
	for (primary, additional, why) in [
		(json!("gcid:no_such_thing"), json!([]), "not Google's"),
		(json!("gcid:chinatown"), json!([]), "not Google's in FR"),
		(json!(null), json!(["gcid:plumber"]), "additional without a primary"),
		(json!("gcid:plumber"), json!(["gcid:plumber"]), "the primary twice"),
		(json!("gcid:plumber"), json!(["gcid:electrician", "gcid:electrician"]), "an additional twice"),
		(json!("gcid:plumber"), json!(ten), "10 additional"),
	] {
		let (status, body) = call(&mut b, &app, Method::PUT, &uri, google(primary, additional)).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
	}

	let (status, listed) = call(&mut b, &app, Method::GET, "/api/v1/google-categories/FR", None).await;
	assert_eq!(status, StatusCode::OK);
	let plumber = listed["categories"]
		.as_array()
		.unwrap()
		.iter()
		.find(|g| g["id"] == "gcid:plumber")
		.expect("Google lists plumbers in FR");
	assert_eq!(plumber["names"]["fr"], "Plombier");
}

async fn trade_ids(b: &mut Browser, app: &Router) -> Vec<Value> {
	let (_, listed) = call(b, app, Method::GET, "/api/v1/gmails", None).await;
	listed["gmbs"].as_array().unwrap().iter().map(|g| g["trade_id"].clone()).collect()
}

#[tokio::test]
async fn gbps_are_given_a_trade_every_one_or_none() {
	let db = TestDb::create().await;
	let (app, mut b, _, theirs) = beside_another(&db).await;
	let (_, heating) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Heating" }))).await;
	let (_, trade) = call(&mut b, &app, Method::POST, "/api/v1/trades", Some(json!({ "name": "Plumbing" }))).await;
	let mut mine = Vec::new();
	for name in ["Lyon 7", "Lyon 8"] {
		let (_, gbp) = call(
			&mut b,
			&app,
			Method::POST,
			"/api/v1/gmbs",
			Some(json!({ "name": name, "trade": heating["id"], "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
		)
		.await;
		mine.push(gbp["id"].as_i64().unwrap());
	}
	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/gmbs/trade", Some(json!({ "gmbs": [mine[0], theirs], "trade": trade["id"] }))).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
	assert_eq!(trade_ids(&mut b, &app).await, [heating["id"].clone(), heating["id"].clone()], "none given it");

	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/gmbs/trade", Some(json!({ "gmbs": mine, "trade": trade["id"] }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(trade_ids(&mut b, &app).await, [trade["id"].clone(), trade["id"].clone()]);

	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/gmbs/trade", Some(json!({ "gmbs": [mine[1]], "trade": null }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(trade_ids(&mut b, &app).await, [trade["id"].clone(), Value::Null]);
}

#[tokio::test]
async fn a_removed_gmail_is_restored_from_the_trash_and_only_an_admin_keeps_it_forever() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, None);
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	call(&mut b, &app, Method::DELETE, &format!("/api/v1/gmails/{}", gmail["id"]), None).await;

	let (_, trash) = call(&mut b, &app, Method::GET, "/api/v1/trash", None).await;
	assert_eq!(trash["settings"], json!({ "keep_days": 30, "never_purge": true }), "dev sign-in keeps everything: {trash}");
	assert_eq!((&trash["gmails"][0]["gmail"], &trash["gmails"][0]["purge_at"]), (&json!("a@gmail.com"), &Value::Null));

	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/trash/settings", Some(json!({ "keep_days": 7, "never_purge": false }))).await;
	assert_eq!(status, StatusCode::FORBIDDEN, "an operator holds no sa:admin:trash:keep");
	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/trash/settings", Some(json!({ "keep_days": 10 }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "only 1, 7, 30 or 90 days");
	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/trash/settings", Some(json!({ "keep_days": 7 }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);

	let (status, _) = call(&mut b, &app, Method::POST, &format!("/api/v1/trash/gmails/{}/restore", gmail["id"]), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/gmails", None).await;
	assert_eq!(listed["gmails"][0]["id"], gmail["id"], "{listed}");
	let (status, _) = call(&mut b, &app, Method::POST, &format!("/api/v1/trash/gmails/{}/restore", gmail["id"]), None).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "out of the trash");
}
