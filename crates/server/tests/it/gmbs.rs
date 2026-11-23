//! A GMB's point: Places asked once for it, carried by `GET /gmails`, overwritten by a Sync,
//! placed by hand where Places has none; and the browser's map.

use std::sync::{Arc, Mutex};

use axum::{
	Json, Router,
	body::Body,
	extract::{Path, State},
	http::{Method, StatusCode},
	response::{IntoResponse, Response},
	routing::get,
};
use sa_auth::PermissionSet;
use sa_core::{
	gbp::{Country, OnGoogle},
	prices::{Item, PlacesRequest, Spender},
};
use sa_panel::{
	Panel,
	categories::{Body as Settings, Key},
	gmbs::{Existing, Gmb, Unplaced},
	import::Export,
	seal::DataKey,
	testing::{TestDb, account, admin, operator, panel_on},
};
use sa_places::{Google, Keys};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};

const PROCOPE: &str = "ChIJprocopeprocopeproc";
const FLORE: &str = "ChIJfloreflorefloreflo";
/// A service-area business: Places gives it no point.
const ASSIST: &str = "ChIJassistassistassis";
/// A place Google has merged into Procope: it answers under Procope's id.
const MERGED: &str = "ChIJmergedmergedmerged";
const PLUMBER: &str = "gcid:plumber";
const HEATING: &str = "gcid:heating_contractor";
const PHOTO: &[u8] = b"a photo of the shop";

/// Places, counting what it is asked: `<id>` its details, `profile <id>` its profile, `photo <ref>`
/// a photo's link. A profile's photos are `places/<id>/photos/front` and `…/back`, each at
/// `/img/<ref>`.
async fn places() -> (Arc<Mutex<Vec<String>>>, String) {
	async fn details(State(asked): State<Arc<Mutex<Vec<String>>>>, Path(id): Path<String>, headers: axum::http::HeaderMap) -> Response {
		let profile = headers["x-goog-fieldmask"].to_str().unwrap().contains("photos");
		asked.lock().unwrap().push(if profile { format!("profile {id}") } else { id.clone() });
		let (lat, lng) = match id.as_str() {
			PROCOPE => (48.853, 2.3389),
			FLORE => (48.854, 2.3326),
			ASSIST => return Json(json!({ "id": id, "displayName": { "text": "Mold Assist" } })).into_response(),
			MERGED => return Json(json!({ "id": PROCOPE, "displayName": { "text": "Le Procope" } })).into_response(),
			_ => return (StatusCode::NOT_FOUND, "not found").into_response(),
		};
		let mut place = json!({ "id": id, "displayName": { "text": id }, "formattedAddress": "Paris", "location": { "latitude": lat, "longitude": lng },
			"addressComponents": [{ "shortText": "FR", "types": ["country", "political"] }] });
		if profile {
			place["displayName"] = json!({ "text": format!("{id} on Google") });
			place["formattedAddress"] = json!("13 Rue de l'Ancienne Comédie, 75006 Paris");
			place["internationalPhoneNumber"] = json!(if id == PROCOPE { "+33 1 40 46 79 00" } else { "+33 1 45 48 55 26" });
			place["websiteUri"] = json!("https://procope.example");
			place["regularOpeningHours"] = json!({ "weekdayDescriptions": ["Monday: 9:00 AM – 5:00 PM", "Tuesday: Closed"] });
			place["primaryType"] = json!("plumber");
			place["photos"] = json!([
				{ "name": format!("places/{id}/photos/front"), "authorAttributions": [{ "displayName": "Ann" }] },
				{ "name": format!("places/{id}/photos/back") }
			]);
		}
		Json(place).into_response()
	}
	async fn media(State(asked): State<Arc<Mutex<Vec<String>>>>, Path((_, photo)): Path<(String, String)>, headers: axum::http::HeaderMap) -> Json<Value> {
		asked.lock().unwrap().push(format!("photo {photo}"));
		let host = headers["host"].to_str().unwrap();
		Json(json!({ "name": photo, "photoUri": format!("http://{host}/img/{photo}") }))
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
		.route("/v1/places/{id}/photos/{photo}/media", get(media))
		.route("/img/{photo}", get(|| async { PHOTO }))
		.route("/v1/places:autocomplete", axum::routing::post(autocomplete))
		.with_state(asked.clone());
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let base = format!("http://{}/v1/places", listener.local_addr().unwrap());
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(asked, base)
}

/// A panel on `db` asking `base` with a key (`None`: keyless), its answers in `db`, its requests free.
async fn panel(db: &TestDb, base: Option<&str>) -> Panel {
	let keys = Keys::new(base.map(|_| vec![("house".to_owned(), "k".to_owned())]).unwrap_or_default());
	let store = db.store().await;
	let google = Google::try_new(keys, Arc::new(sa_places::InOrder), Arc::new(store.clone())).unwrap().spend(true);
	let google = match base {
		Some(b) => google.at(b),
		None => google,
	};
	let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
	let accounts = base.map(|_| vec![sa_panel::testing::account("house", Some("k"), None)]).unwrap_or_default();
	let p = panel_on(store, key, google, sa_places::Accounts::new(accounts, sa_panel::testing::slices()));
	// what a point costs is tests/connections.rs's
	sa_panel::testing::priced(&p, &[PlacesRequest::Details, PlacesRequest::Search, PlacesRequest::Autocomplete].map(|r| (Item::Places(r), 0))).await;
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
	p.import_review_archive(empty).await.unwrap().unwrap();
}

fn app(panel: Panel, permissions: PermissionSet) -> Router {
	let who = DevIdentity {
		user_id: DevIdentity::named("dev@localhost"),
		permissions,
		email: "dev@localhost".to_owned(),
	};
	let config = SignInConfig {
		panel_origin: ORIGIN.to_owned(),
		concierge_origin: ORIGIN.to_owned(),
	};
	http::app(
		SignIn::new(panel, Concierge::dev(who), config),
		None,
		None,
		Default::default(),
		None,
		None,
		None,
		sa_server::testing::sandbox(),
		None,
	)
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
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;

	let mut gmails = Vec::new();
	for gmail in ["a@gmail.com", "b@gmail.com"] {
		let (status, added) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": gmail }))).await;
		assert_eq!(status, StatusCode::CREATED, "{added}");
		gmails.push(added["id"].as_i64().unwrap());
	}
	let add = json!({ "place_id": PROCOPE, "state": "verified", "trade": PLUMBER });
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
async fn a_sync_overwrites_a_verified_gbp_with_googles_profile_asked_afresh() {
	let (asked, base) = places().await;
	let db = TestDb::create().await;
	let keyless = panel(&db, None).await;
	opened(&keyless).await;
	let owner = seen(&keyless, "dev@localhost").await;
	let gmail = keyless.add_gmail(owner, "a@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	let gbp = add(&keyless, owner, gmail.id, PROCOPE).await;
	assert_eq!((gbp.location, gbp.country.map(|c| c.by_hand)), (None, Some(true)), "no key, no point, a country by hand");

	let keyed = panel(&db, Some(&base)).await;
	let app = app(keyed.clone(), admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("/api/v1/gmbs/{}/location", gbp.id), Some(json!({ "lat": 1.0, "lng": 1.0 }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let synced = sync(&mut b, &app, gbp.id).await;
	let fields = ["name", "address", "phone", "website", "hours", "google_primary", "country", "location"].map(|f| synced[f].clone());
	assert_eq!(
		fields,
		[
			json!(format!("{PROCOPE} on Google")),
			json!("13 Rue de l'Ancienne Comédie, 75006 Paris"),
			json!("+33140467900"),
			json!("https://procope.example"),
			json!(["Monday: 9:00 AM – 5:00 PM", "Tuesday: Closed"]),
			json!("gcid:plumber"),
			json!({ "code": "FR", "by_hand": false }),
			json!({ "lat": 48.853, "lng": 2.3389, "placed": false }),
		],
		"Google's over the name, the country and the point by hand"
	);
	let photos = synced["photos"].as_array().unwrap();
	assert_eq!(photos.iter().map(|p| p["author"].clone()).collect::<Vec<_>>(), [json!("Ann"), Value::Null]);
	let (status, _, bytes) = b
		.send_body(&app, Method::GET, &format!("/api/v1/gmbs/{}/photos/{}", gbp.id, photos[0]["id"]), &[], true, Body::empty())
		.await;
	assert_eq!((status, &bytes[..]), (StatusCode::OK, PHOTO));

	let again = sync(&mut b, &app, gbp.id).await;
	assert_ne!(again["photos"][0]["id"], photos[0]["id"], "a photo synced again is another, its address served immutable");
	let profiles = asked.lock().unwrap().iter().filter(|a| a.starts_with("profile")).count();
	assert_eq!(profiles, 2, "each Sync asks Google afresh");
}

#[tokio::test]
async fn a_sync_is_refused_off_verified_over_another_gbps_phone_and_for_a_place_google_renamed() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let keyless = panel(&db, None).await;
	opened(&keyless).await;
	let owner = seen(&keyless, "dev@localhost").await;
	let gmail = keyless.add_gmail(owner, "a@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	let merged = add(&keyless, owner, gmail.id, MERGED).await;
	let procope = add(&keyless, owner, gmail.id, PROCOPE).await;
	let flore = add(&keyless, owner, gmail.id, FLORE).await;
	let keyed = panel(&db, Some(&base)).await;
	let app = app(keyed.clone(), admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _) = call(&mut b, &app, Method::PATCH, &format!("/api/v1/gmbs/{}", flore.id), Some(json!({ "phone": "+33140467900" }))).await;
	assert_eq!(status, StatusCode::OK);
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("/api/v1/gmbs/{}/state", flore.id), Some(json!({ "state": "suspended" }))).await;
	assert_eq!(status, StatusCode::OK);

	for (gbp, why) in [(flore.id, "suspended"), (procope.id, "Flore has its phone"), (merged.id, "Google names it Procope now")] {
		let (status, body) = call(&mut b, &app, Method::POST, &format!("/api/v1/gmbs/{gbp}/sync"), None).await;
		assert_eq!(status, StatusCode::CONFLICT, "{why}: {body}");
	}
}

#[tokio::test]
async fn a_sync_keeps_the_point_placed_by_hand_where_google_has_none() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let keyless = panel(&db, None).await;
	opened(&keyless).await;
	let owner = seen(&keyless, "dev@localhost").await;
	let gmail = keyless.add_gmail(owner, "a@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	let assist = add(&keyless, owner, gmail.id, ASSIST).await;
	let keyed = panel(&db, Some(&base)).await;
	let app = app(keyed.clone(), admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _) = call(
		&mut b,
		&app,
		Method::PUT,
		&format!("/api/v1/gmbs/{}/location", assist.id),
		Some(json!({ "lat": 43.6, "lng": -79.65 })),
	)
	.await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let synced = sync(&mut b, &app, assist.id).await;
	assert_eq!(
		(&synced["name"], &synced["location"], &synced["country"]),
		(
			&json!("Mold Assist"),
			&json!({ "lat": 43.6, "lng": -79.65, "placed": true }),
			&json!({ "code": "FR", "by_hand": true })
		),
	);
}

async fn sync(b: &mut Browser, app: &Router, gbp: i64) -> Value {
	let (status, synced) = call(b, app, Method::POST, &format!("/api/v1/gmbs/{gbp}/sync"), None).await;
	assert_eq!(status, StatusCode::OK, "{synced}");
	synced
}

/// On Google already, named and put in France by hand: what Places, keyless, cannot say. Of the
/// plumber.
async fn add(p: &Panel, owner: uuid::Uuid, gmail_id: i64, place: &str) -> Gmb {
	let existing = Existing {
		gmail_id,
		place: place.to_owned(),
		state: OnGoogle::Verified,
		name: Some(place.to_owned()),
		country: Some(Country::FR),
		trade: Some(PLUMBER.to_owned()),
	};
	p.add_gmb(Some(Spender::new(owner)), existing, jiff::Timestamp::now()).await.unwrap().unwrap()
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
	let app = app(keyed.clone(), admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, assist) = call(
		&mut b,
		&app,
		Method::POST,
		&format!("/api/v1/gmails/{}/gmbs", gmail.id),
		Some(json!({ "place_id": ASSIST, "state": "verified", "country": "CA", "trade": PLUMBER })),
	)
	.await;
	assert_eq!((status, &assist["location"]), (StatusCode::CREATED, &Value::Null), "{assist}");

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
	assert_eq!(*asked.lock().unwrap(), [ASSIST], "a place is not paid for again");
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
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let mut mine = Vec::new();
	for place in [PROCOPE, ASSIST] {
		let add = json!({ "place_id": place, "state": "verified", "country": "FR", "trade": PLUMBER });
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
	for (accounts, want) in [
		(
			vec![account("server", Some("k"), None)],
			(StatusCode::NOT_FOUND, json!({ "code": "not_found", "error": "not found" })),
		),
		(
			vec![account("server", Some("k"), None), account("browser", None, Some("browser-key"))],
			(StatusCode::OK, json!({ "key": "browser-key", "account": "browser" })),
		),
	] {
		let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
		let store = db.store().await;
		let keyless = Google::try_new(Keys::new(vec![]), Arc::new(sa_places::InOrder), Arc::new(store.clone())).unwrap();
		let names: Vec<String> = accounts.iter().map(|a| a.name.clone()).collect();
		let panel = panel_on(store, key, keyless, sa_places::Accounts::new(accounts, sa_panel::testing::slices()));
		let none = sa_core::google::Hold {
			allow: [(sa_core::meters::SLICE.to_owned(), sa_core::google::Allow { day: 0.0, month: 0.0 })].into(),
			unguarded: vec![],
		};
		for name in &names {
			panel.set_hold(name, &none, "test", jiff::Timestamp::now()).await.unwrap().unwrap();
		}
		let app = app(panel, admin());
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
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let new = json!({ "name": "Café de Flore", "trade": PLUMBER, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] });
	let (status, draft) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new.clone())).await;
	assert_eq!(
		(status, &draft["state"], &draft["gmail_id"], &draft["place_id"]),
		(StatusCode::CREATED, &json!("draft"), &Value::Null, &Value::Null)
	);
	let gbp = format!("/api/v1/gmbs/{}", draft["id"]);
	let (status, _) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	assert_eq!(status, StatusCode::CONFLICT);
	let (status, body) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(json!({ "name": "Nowhere", "trade": PLUMBER, "state": "draft" }))).await;
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
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;

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

	let new = json!({ "name": "Flore", "trade": PLUMBER, "state": "verifying", "gmail_id": gmail["id"], "country": "FR", "location": { "lat": 48.85, "lng": 2.33 } });
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
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, first) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Lyon 7", "trade": HEATING, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
	)
	.await;
	let (_, second) = call(
		&mut b,
		&app,
		Method::POST,
		"/api/v1/gmbs",
		Some(json!({ "name": "Lyon 8", "trade": HEATING, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
	)
	.await;
	assert_eq!(
		first["tabs"],
		json!([
			{ "tab": "launch", "closed": null },
			{ "tab": "stats", "closed": "until_verified" },
			{ "tab": "ranking", "closed": "until_verified" },
			{ "tab": "reviews", "closed": "until_verified" },
			{ "tab": "connections", "closed": null },
			{ "tab": "profile", "closed": null },
			{ "tab": "log", "closed": null },
		]),
		"a draft's window, under no gmail yet"
	);
	assert_eq!(
		first["readiness"],
		json!({ "missing": ["category", "areas", "phone", "hidden_address", "film", "gmail", "profile"], "unknown": ["site"] }),
		"its trade has no country yet, and with no sites to ask a landing page is unknown"
	);
	let first = format!("/api/v1/gmbs/{}", first["id"]);

	let must_haves = json!({ "phone": "04 78 00 00 01", "trade": PLUMBER, "areas": ["Lyon 7e"], "hidden_address": "12 rue de Marseille",
		"customers_visit": false, "film": "verif_vid:42" });
	let (status, edited) = call(&mut b, &app, Method::PATCH, &first, Some(must_haves.clone())).await;
	assert_eq!(status, StatusCode::OK, "{edited}");
	for (field, want) in must_haves.as_object().unwrap() {
		let (field, want) = match field.as_str() {
			"phone" => ("phone", &json!("0478000001")),
			f => (f, want),
		};
		assert_eq!(&edited[field], want, "{field}");
	}
	assert_eq!(edited["readiness"], json!({ "missing": ["category", "gmail", "profile"], "unknown": ["site"] }), "{edited}");
	let (_, cleared) = call(&mut b, &app, Method::PATCH, &first, Some(json!({ "areas": null, "customers_visit": null }))).await;
	assert_eq!((&cleared["areas"], &cleared["customers_visit"], &cleared["trade"]), (&json!([]), &Value::Null, &json!(PLUMBER)));
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

/// A GBP's role is its gmail's on it: none without one, a transfer clears it, an owner gmail only a manager's.
#[tokio::test]
async fn a_gbps_role_is_its_gmails() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let (_, other) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "b@gmail.com" }))).await;
	let new = json!({ "name": "Café de Flore", "trade": PLUMBER, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] });
	let (_, draft) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	let uri = format!("/api/v1/gmbs/{}", draft["id"]);
	assert_eq!(draft["role"], Value::Null, "unknown until said");
	let (status, _) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": "owner" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "no gmail to have a role on");

	let (_, owned) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "gmail_id": gmail["id"], "role": "owner" }))).await;
	assert_eq!(owned["role"], "owner", "{owned}");
	let (status, _) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "owner_gmail": "c@gmail.com" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "an owner's gmail is its own");
	let (_, managed) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": "manager", "owner_gmail": " C@gmail.com " }))).await;
	assert_eq!((&managed["role"], &managed["owner_gmail"]), (&json!("manager"), &json!("c@gmail.com")), "{managed}");
	let (status, _) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "owner_gmail": "not an address" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
	let (_, owned) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": "owner" }))).await;
	assert_eq!((&owned["role"], &owned["owner_gmail"]), (&json!("owner"), &Value::Null), "{owned}");

	let (_, moved) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "gmail_id": other["id"] }))).await;
	assert_eq!((&moved["gmail_id"], &moved["role"]), (&other["id"], &Value::Null), "{moved}");
	let (status, _) = call(&mut b, &app, Method::PATCH, &uri, Some(json!({ "role": "admin" }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Signed in as dev@localhost, beside a GBP and a category of other@localhost's.
async fn beside_another(db: &TestDb) -> (Router, Browser, i64) {
	let p = panel(db, None).await;
	opened(&p).await;
	let other = seen(&p, "other@localhost").await;
	let key = Key::try_new(other, PLUMBER, Country::FR).unwrap();
	p.apply(key, Settings::empty(key), jiff::Timestamp::now()).await.unwrap().unwrap();
	let unplaced = Unplaced {
		name: "Theirs".to_owned(),
		trade: PLUMBER.to_owned(),
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
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	(app, b, gbp.id)
}

fn settings(keywords: Value, additional: Value) -> Option<Value> {
	Some(json!({ "keywords": keywords, "additional": additional, "excluded": { "words": [], "types": [] }, "demand": null }))
}

#[tokio::test]
async fn a_category_is_its_owners() {
	let db = TestDb::create().await;
	let (app, mut b, theirs) = beside_another(&db).await;
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/categories", None).await;
	assert_eq!(listed, json!({ "categories": [] }), "another's is not listed");
	let uri = "/api/v1/categories/gcid:plumber/FR";
	for (method, uri) in [(Method::GET, format!("{uri}/history")), (Method::DELETE, uri.to_owned())] {
		let (status, body) = call(&mut b, &app, method.clone(), &uri, None).await;
		assert_eq!(status, StatusCode::NOT_FOUND, "{method} {uri}: {body}");
	}
	let (status, body) = call(&mut b, &app, Method::PATCH, &format!("/api/v1/gmbs/{theirs}"), Some(json!({ "phone": "04 78 00 00 01" }))).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "another's GBP: {body}");
}

#[tokio::test]
async fn a_category_a_live_gbp_holds_is_not_deleted() {
	let db = TestDb::create().await;
	let (app, mut b, _) = beside_another(&db).await;
	let uri = "/api/v1/categories/gcid:plumber/FR";
	let (status, set) = call(&mut b, &app, Method::PUT, uri, settings(json!([{ "text": "plombier", "weight": 1.0 }]), json!([]))).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let new = json!({ "name": "Flore", "trade": PLUMBER, "state": "verifying", "gmail_id": gmail["id"], "country": "FR", "location": { "lat": 48.85, "lng": 2.33 } });
	let (status, gbp) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	assert_eq!((status, &gbp["trade"]), (StatusCode::CREATED, &json!(PLUMBER)), "{gbp}");

	let (status, body) = call(&mut b, &app, Method::DELETE, uri, None).await;
	assert_eq!(status, StatusCode::CONFLICT, "{body}");
	let (status, _) = call(&mut b, &app, Method::DELETE, &format!("/api/v1/gmbs/{}", gbp["id"]), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (status, body) = call(&mut b, &app, Method::DELETE, uri, None).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "a removed GBP gives it up: {body}");
	let (_, listed) = call(&mut b, &app, Method::GET, "/api/v1/categories", None).await;
	assert_eq!(listed, json!({ "categories": [] }));
}

#[tokio::test]
async fn keywords_are_one_spelling_each_none_twice_and_the_trades_name_among_them() {
	let db = TestDb::create().await;
	let (app, mut b, _) = beside_another(&db).await;
	let uri = "/api/v1/categories/gcid:plumber/FR";
	let unweighed = |texts: &[&str]| json!(texts.iter().map(|t| json!({ "text": t, "weight": null })).collect::<Vec<_>>());
	let own = json!({ "text": "Plombier", "weight": 1.0 });

	let (status, body) = call(&mut b, &app, Method::PUT, uri, settings(json!([own, { "text": " plombier  ", "weight": null }]), json!([]))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
	let (status, body) = call(&mut b, &app, Method::PUT, uri, settings(json!([{ "text": "  Fuite \t d'eau ", "weight": null }, own]), json!([]))).await;
	assert_eq!(
		(status, &body["head"]["keywords"]),
		(StatusCode::OK, &json!([{ "text": "fuite d'eau", "weight": null }, { "text": "plombier", "weight": 1.0 }])),
		"{body}"
	);

	for (k, why) in [
		(unweighed(&["fuite d'eau", "plombier"]), "the trade's name not at weight 1"),
		(unweighed(&["fuite d'eau"]), "the trade's name missing"),
		(json!([{ "text": "plombier", "weight": 1.0 }, { "text": "fuite d'eau", "weight": 0.0 }]), "a weight not positive"),
	] {
		let (status, body) = call(&mut b, &app, Method::PUT, uri, settings(k, json!([]))).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
	}
}

#[tokio::test]
async fn additional_google_categories_are_googles_in_the_country_in_order() {
	let db = TestDb::create().await;
	let (app, mut b, _) = beside_another(&db).await;
	let uri = "/api/v1/categories/gcid:plumber/FR";
	let keywords = json!([{ "text": "plombier", "weight": 1.0 }]);
	let google = |additional: &[Value]| settings(keywords.clone(), json!(additional.iter().map(|id| json!({ "id": id, "weight": 0.5 })).collect::<Vec<_>>()));

	let (status, body) = call(&mut b, &app, Method::PUT, uri, google(&[json!("gcid:heating_contractor"), json!("gcid:drainage_service")])).await;
	assert_eq!(status, StatusCode::OK, "{body}");
	assert_eq!(body["trade"]["id"], "gcid:plumber");
	assert_eq!(body["trade"]["names"]["fr"], "Plombier");
	let names: Vec<&str> = body["head"]["additional"]
		.as_array()
		.unwrap()
		.iter()
		.map(|g| g["google"]["names"]["fr"].as_str().unwrap())
		.collect();
	assert_eq!(names, ["Chauffagiste", "Service d'évacuation des eaux"]);

	let ten: Vec<Value> = [
		"electrician", "roofing_contractor", "painter", "carpenter", "locksmith", "glazier", "general_contractor", "tile_contractor", "handyman", "gardener",
	]
	.map(|g| json!(format!("gcid:{g}")))
	.into();
	for (additional, why) in [
		(vec![json!("gcid:no_such_thing")], "not Google's"),
		(vec![json!("gcid:chinatown")], "not Google's in FR"),
		(vec![json!("gcid:plumber")], "the trade twice"),
		(vec![json!("gcid:electrician"), json!("gcid:electrician")], "an additional twice"),
		(ten, "10 additional"),
	] {
		let (status, body) = call(&mut b, &app, Method::PUT, uri, google(&additional)).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {body}");
	}
	let (status, body) = call(&mut b, &app, Method::PUT, "/api/v1/categories/gcid:chinatown/FR", settings(keywords.clone(), json!([]))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a trade not Google's in FR: {body}");

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
	listed["gmbs"].as_array().unwrap().iter().map(|g| g["trade"].clone()).collect()
}

#[tokio::test]
async fn gbps_are_given_a_trade_every_one_or_none() {
	let db = TestDb::create().await;
	let (app, mut b, theirs) = beside_another(&db).await;
	let mut mine = Vec::new();
	for name in ["Lyon 7", "Lyon 8"] {
		let (_, gbp) = call(
			&mut b,
			&app,
			Method::POST,
			"/api/v1/gmbs",
			Some(json!({ "name": name, "trade": HEATING, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] })),
		)
		.await;
		mine.push(gbp["id"].as_i64().unwrap());
	}
	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/gmbs/trade", Some(json!({ "gmbs": [mine[0], theirs], "trade": PLUMBER }))).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
	assert_eq!(trade_ids(&mut b, &app).await, [json!(HEATING), json!(HEATING)], "none given it");

	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/gmbs/trade", Some(json!({ "gmbs": mine, "trade": PLUMBER }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(trade_ids(&mut b, &app).await, [json!(PLUMBER), json!(PLUMBER)]);

	let (status, _) = call(&mut b, &app, Method::PUT, "/api/v1/gmbs/trade", Some(json!({ "gmbs": [mine[1]], "trade": null }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(trade_ids(&mut b, &app).await, [json!(PLUMBER), Value::Null]);
}

#[tokio::test]
async fn a_removed_gmail_is_restored_from_the_trash_and_only_an_admin_keeps_it_forever() {
	let db = TestDb::create().await;
	let p = panel(&db, None).await;
	opened(&p).await;
	let app = app(p, operator());
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

async fn log(b: &mut Browser, app: &Router, gbp: i64) -> Vec<Value> {
	let (status, log) = call(b, app, Method::GET, &format!("/api/v1/gmbs/{gbp}/log"), None).await;
	assert_eq!(status, StatusCode::OK, "{log}");
	log["events"].as_array().unwrap().clone()
}

/// A Sync is one `synced` event with each field before and after, one that changed nothing too; a point placed by
/// hand is an edit; newest first.
#[tokio::test]
async fn a_gbps_log_holds_each_sync_with_what_it_changed() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let keyless = panel(&db, None).await;
	opened(&keyless).await;
	let owner = seen(&keyless, "dev@localhost").await;
	let gmail = keyless.add_gmail(owner, "a@gmail.com", jiff::Timestamp::now()).await.unwrap().unwrap();
	let gbp = add(&keyless, owner, gmail.id, PROCOPE).await;
	let app = app(panel(&db, Some(&base)).await, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (status, _) = call(&mut b, &app, Method::PUT, &format!("/api/v1/gmbs/{}/location", gbp.id), Some(json!({ "lat": 1.0, "lng": 1.0 }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	sync(&mut b, &app, gbp.id).await;
	let synced = sync(&mut b, &app, gbp.id).await;
	assert!(synced["synced_at"].is_string(), "{synced}");

	let events = log(&mut b, &app, gbp.id).await;
	let kinds = events.iter().map(|e| e["kind"].as_str().unwrap()).collect::<Vec<_>>();
	assert_eq!(kinds, ["synced", "synced", "edited", "added"], "{events:?}");
	assert_eq!(events[0]["changes"], json!([]), "a Sync that changed nothing is still one");
	let phone = events[1]["changes"].as_array().unwrap().iter().find(|c| c["field"] == "phone").cloned();
	assert_eq!(phone, Some(json!({ "field": "phone", "from": null, "to": "+33140467900" })), "{:?}", events[1]);
	assert_eq!(events[2]["changes"][0]["field"], "point");
	assert!(events[3]["by"].as_str().unwrap().contains("dev@localhost"), "{:?}", events[3]);
	assert_eq!(events[3]["gmail_id"], gmail.id);
}

/// A state's move is logged from and to, in the write that made it.
#[tokio::test]
async fn a_gbps_log_holds_its_state_moves() {
	let (_asked, base) = places().await;
	let db = TestDb::create().await;
	let p = panel(&db, Some(&base)).await;
	opened(&p).await;
	let app = app(p, admin());
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, gmail) = call(&mut b, &app, Method::POST, "/api/v1/gmails", Some(json!({ "gmail": "a@gmail.com" }))).await;
	let new = json!({ "name": "Café de Flore", "trade": PLUMBER, "state": "draft", "spots": [{ "lat": 45.76, "lng": 4.84 }] });
	let (_, draft) = call(&mut b, &app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	let gbp = draft["id"].as_i64().unwrap();
	call(&mut b, &app, Method::PATCH, &format!("/api/v1/gmbs/{gbp}"), Some(json!({ "gmail_id": gmail["id"] }))).await;
	let (status, _) = call(
		&mut b,
		&app,
		Method::PUT,
		&format!("/api/v1/gmbs/{gbp}/state"),
		Some(json!({ "state": "verifying", "place": FLORE })),
	)
	.await;
	assert_eq!(status, StatusCode::OK);

	let events = log(&mut b, &app, gbp).await;
	let state = events.iter().find(|e| e["kind"] == "state").unwrap();
	assert_eq!((&state["from"], &state["to"]), (&json!("draft"), &json!("verifying")));
	let moved = events.iter().find(|e| e["kind"] == "gmail").unwrap();
	assert_eq!(
		(&moved["from"], &moved["to"], &moved["gmail_id"]),
		(&Value::Null, &gmail["id"], &Value::Null),
		"under none until moved"
	);
	let (status, only) = call(&mut b, &app, Method::GET, &format!("/api/v1/gmbs/{gbp}/log?kinds=state,added"), None).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(
		only["events"].as_array().unwrap().iter().map(|e| e["kind"].clone()).collect::<Vec<_>>(),
		[json!("state"), json!("added")]
	);
	let (status, _) = call(&mut b, &app, Method::GET, &format!("/api/v1/gmbs/{gbp}/log?kinds=nope"), None).await;
	assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// The log is append-only; another owner's GBP has none to read.
#[tokio::test]
async fn a_gbps_log_is_append_only_and_its_owners() {
	let db = TestDb::create().await;
	let (app, mut b, theirs) = beside_another(&db).await;
	let (status, _) = call(&mut b, &app, Method::GET, &format!("/api/v1/gmbs/{theirs}/log"), None).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
	let pool = db.store().await.pool().clone();
	for refused in ["UPDATE gmb_events SET at = 0", "DELETE FROM gmb_events"] {
		let err = sqlx::query(refused).execute(&pool).await.unwrap_err();
		assert!(err.to_string().contains("append-only"), "{refused}: {err}");
	}
}
