//! An area carries the country Places says it lies in: a city's from its locality, a circle's from
//! a reverse geocode of its centre; a centre at sea is refused.

use std::sync::Arc;

use axum::{
	Json, Router,
	body::Body,
	extract::Path,
	http::{Method, StatusCode},
	routing::get,
};
use sa_core::prices::{Item, PlacesRequest};
use sa_panel::{
	import::Export,
	seal::DataKey,
	testing::{TestDb, operator, panel_on},
};
use sa_places::{Google, Keys};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN, upstream},
};
use serde_json::{Value, json};

#[tokio::test]
async fn an_area_has_the_country_its_place_or_its_centre_lies_in() {
	let locality = |Path(id): Path<String>| async move {
		Json(json!({ "id": id, "displayName": { "text": "Lyon" }, "location": { "latitude": 45.75, "longitude": 4.84 },
			"viewport": { "low": { "latitude": 45.7, "longitude": 4.78 }, "high": { "latitude": 45.8, "longitude": 4.9 } },
			"addressComponents": [{ "shortText": "FR", "types": ["country", "political"] }] }))
	};
	let country = |Path(point): Path<String>| async move {
		match point.starts_with("0.0000") {
			true => Json(json!({})),
			false => Json(json!({ "results": [{ "addressComponents": [{ "shortText": "DE", "types": ["country", "political"] }] }] })),
		}
	};
	let (base, _) = upstream(Router::new().route("/v1/places/geocode/{point}", get(country)).route("/v1/places/{id}", get(locality))).await;

	let db = TestDb::create().await;
	let store = db.store().await;
	let google = Google::try_new(Keys::new(vec![("house".to_owned(), "k".to_owned())]), Arc::new(sa_places::InOrder), Arc::new(store.clone()))
		.unwrap()
		.spend(true)
		.at(format!("{base}/v1/places"));
	let panel = panel_on(
		store,
		DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(),
		google,
		sa_places::Accounts::new(vec![sa_panel::testing::account("house", Some("k"), None)], vec![]),
	);
	panel
		.import_review_archive(Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		})
		.await
		.unwrap()
		.unwrap();
	sa_panel::testing::priced(&panel, &[PlacesRequest::Details, PlacesRequest::Geocode].map(|r| (Item::Places(r), 0))).await;
	let who = DevIdentity {
		user_id: DevIdentity::named("dev@localhost"),
		permissions: operator(),
		email: "dev@localhost".to_owned(),
	};
	let config = SignInConfig {
		panel_origin: ORIGIN.to_owned(),
		concierge_origin: ORIGIN.to_owned(),
	};
	let app = http::app(
		SignIn::new(panel, Concierge::dev(who), config),
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

	let mut put = async |slug: &str, body: Value| {
		let (status, _, bytes) = b
			.send_body(
				&app,
				Method::PUT,
				&format!("/api/v1/areas/{slug}"),
				&[("content-type", "application/json")],
				true,
				Body::from(body.to_string()),
			)
			.await;
		(status, serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null))
	};
	let (status, city) = put(
		"lyon",
		json!({ "name": "Lyon", "center": { "lat": 45.76, "lng": 4.84 }, "radius_m": 8000, "city": "ChIJlyonlyonlyonlyonly" }),
	)
	.await;
	assert_eq!((status, &city["country"]), (StatusCode::OK, &json!("FR")), "{city}");
	let (status, circle) = put("berlin-5km", json!({ "name": "Berlin", "center": { "lat": 52.52, "lng": 13.4 }, "radius_m": 5000, "city": null })).await;
	assert_eq!((status, &circle["country"]), (StatusCode::OK, &json!("DE")), "{circle}");
	let (status, sea) = put("sea", json!({ "name": "Sea", "center": { "lat": 0.0, "lng": 0.0 }, "radius_m": 5000, "city": null })).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "{sea}");
}
