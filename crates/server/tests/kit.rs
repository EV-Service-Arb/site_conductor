//! The review kit's offer, through the landings' signed routes: no draft and no photo goes twice
//! to one GBP, none is offered while another view holds it, the less used come first.

use std::{
	io::Cursor,
	sync::{Arc, Mutex},
};

use axum::{
	Json, Router,
	body::{Body, to_bytes},
	extract::Path,
	http::{Method, Request, StatusCode},
	routing::get,
};
use jiff::{SignedDuration, Timestamp};
use sa_auth::SourceKind;
use sa_core::{
	gbp::{Country, OnGoogle},
	ids::{BrandId, SiteOrigin},
};
use sa_panel::{
	Panel,
	gmbs::{Category, Existing},
	import::Export,
	kit::Target,
	seal::DataKey,
	store::users::User,
	testing::{TestDb, operator, panel_on, sign, sign_get},
};
use sa_core::prices::{Item, PlacesRequest};
use sa_places::{Google, Keys};
use sa_server::{
	concierge::DevIdentity,
	http,
	testing::{Browser, app_with, upstream},
};
use serde_json::{Value, json};
use tower::ServiceExt;
use uuid::Uuid;

const KEY: &str = "aquafix-site";
const SEVEN: &str = "ChIJlyon7lyon7lyon7lyon";
const THREE: &str = "ChIJlyon3lyon3lyon3lyon";

/// Sites (which page is which GBP) and Places (each place, its review link), at one address.
async fn stand_ins(gmbs: Arc<Mutex<Vec<(i64, &'static str)>>>) -> String {
	let places = move || {
		let gmbs = gmbs.clone();
		async move {
			let places: Vec<Value> = gmbs.lock().unwrap().iter().map(|(id, slug)| json!({"gmb_id": id, "brand": "aquafix", "slug": slug})).collect();
			Json(json!({ "places": places }))
		}
	};
	let details = |Path(id): Path<String>| async move {
		Json(
			json!({ "id": id, "displayName": { "text": id }, "formattedAddress": "Lyon", "location": { "latitude": 45.75, "longitude": 4.84 },
			"addressComponents": [{ "shortText": "FR", "types": ["country"] }],
			"googleMapsLinks": { "writeAReviewUri": format!("https://www.google.com/maps/place//data=!4m3!3m2!1s{id}!12e1") } }),
		)
	};
	let (base, _) = upstream(Router::new().route("/api/conductor/places", get(places)).route("/v1/places/{id}", get(details))).await;
	base
}

struct Kit {
	app: Router,
	panel: Panel,
	secret: String,
	/// Lyon 7, the GBP the checks are about; Lyon 3 shares its category.
	seven: i64,
	three: i64,
	drafts: Vec<i64>,
	photos: Vec<i64>,
}
impl Kit {
	async fn send(&self, req: Request<Body>) -> (StatusCode, Vec<u8>) {
		let res = self.app.clone().oneshot(req).await.unwrap();
		let status = res.status();
		(status, to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec())
	}

	async fn get(&self, path: &str) -> (StatusCode, Vec<u8>) {
		let s = sign_get(KEY, &self.secret, path, Timestamp::now());
		let req = Request::get(path)
			.header("x-sa-key-id", &s.key_id)
			.header("x-sa-timestamp", &s.timestamp)
			.header("x-sa-signature", &s.signature);
		self.send(req.body(Body::empty()).unwrap()).await
	}

	/// The page of `slug` opened: its view and the drafts offered, each with its photos.
	async fn offer(&self, slug: &str) -> (Uuid, Vec<(i64, Vec<i64>)>) {
		let (status, body) = self.get(&format!("/api/kit/v1/offer?brand=aquafix&slug={slug}&qr=1")).await;
		assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
		let o: Value = serde_json::from_slice(&body).unwrap();
		assert!(o["review_uri"].as_str().unwrap().ends_with("!12e1"), "{o}");
		let drafts = o["drafts"]
			.as_array()
			.unwrap()
			.iter()
			.map(|d| (d["id"].as_i64().unwrap(), d["photos"].as_array().unwrap().iter().map(|p| p["id"].as_i64().unwrap()).collect()))
			.collect();
		(o["view"].as_str().unwrap().parse().unwrap(), drafts)
	}

	async fn event(&self, body: Value) -> StatusCode {
		let body = body.to_string().into_bytes();
		let s = sign(KEY, &self.secret, &body, Timestamp::now());
		let req = Request::post("/api/kit/v1/events")
			.header("content-type", "application/json")
			.header("x-sa-key-id", &s.key_id)
			.header("x-sa-timestamp", &s.timestamp)
			.header("x-sa-signature", &s.signature);
		self.send(req.body(Body::from(body)).unwrap()).await.0
	}

	/// Draft numbers (1-based) of `offered`.
	fn numbers(&self, offered: &[(i64, Vec<i64>)]) -> Vec<usize> {
		offered.iter().map(|(id, _)| self.drafts.iter().position(|d| d == id).unwrap() + 1).collect()
	}
}

fn png(shade: u8) -> Vec<u8> {
	let mut out = Vec::new();
	image::RgbImage::from_pixel(8, 6, image::Rgb([shade, 0, 0]))
		.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
		.unwrap();
	out
}

/// Two GBPs of Plumbing · FR on aquafix's landing, the kit on with drafts; 8 drafts, the fourth
/// selecting two photos.
async fn kit(db: &TestDb) -> Kit {
	let pages = Arc::new(Mutex::new(Vec::new()));
	let base = stand_ins(pages.clone()).await;
	let google = Google::try_new(Keys::new(vec!["k".to_owned()]), Arc::new(db.store().await))
		.unwrap()
		.at(format!("{base}/v1/places"));
	let panel = panel_on(db.store().await, DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(), google);
	let now = Timestamp::now();
	panel
		.import_review_archive(Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		})
		.await
		.unwrap();
	panel
		.set_prices(&[PlacesRequest::Details].map(|r| (Item::Places(r), Some(0))), "admin@localhost", now)
		.await
		.unwrap()
		.unwrap();
	let owner = DevIdentity::named("dev@localhost");
	panel
		.user_seen(
			&User {
				user_id: owner,
				email: "dev@localhost".to_owned(),
				name: "Dev".to_owned(),
			},
			now,
		)
		.await
		.unwrap();
	let secret = panel
		.add_source(
			KEY,
			SourceKind::Site,
			[BrandId::parse("aquafix").unwrap()].into(),
			Some(SiteOrigin::parse("https://aquafix.top").unwrap()),
		)
		.await
		.unwrap()
		.unwrap()
		.secret
		.to_string();
	let trade = panel.add_trade(owner, "Plumbing", now).await.unwrap().unwrap().id;
	let fr = Category {
		country: Country::FR,
		keywords: vec![],
		google_primary: None,
		google_additional: vec![],
		study: None,
	};
	panel.set_up(owner, trade, fr).await.unwrap().unwrap();
	let gmail = panel.add_gmail(owner, "lyon@gmail.com", now).await.unwrap().unwrap().id;
	let gmb = async |place: &str, name: &str| {
		let e = Existing {
			gmail_id: gmail,
			place: place.to_owned(),
			state: OnGoogle::Verified,
			name: Some(name.to_owned()),
			country: Some(Country::FR),
			trade,
		};
		panel.add_gmb(Some(owner), e, now).await.unwrap().unwrap().id
	};
	let (seven, three) = (gmb(SEVEN, "Lyon 7").await, gmb(THREE, "Lyon 3").await);
	pages.lock().unwrap().extend([(seven, "lyon-7"), (three, "lyon-3")]);
	let photos = vec![
		panel.add_photo(owner, trade, Country::FR, png(10), now).await.unwrap().unwrap(),
		panel.add_photo(owner, trade, Country::FR, png(20), now).await.unwrap().unwrap(),
	];
	let mut drafts = Vec::new();
	for n in 1..=8 {
		let selects = if n == 4 { photos.clone() } else { vec![] };
		drafts.push(
			panel
				.add_draft(owner, trade, Country::FR, &format!("Draft {n}: quick and clean."), &selects, now)
				.await
				.unwrap()
				.unwrap(),
		);
	}
	let app = app_with(panel.clone(), operator(), "dev@localhost", &base, http::Limits::default());
	let mut browser = Browser::default();
	browser.sign_in(&app).await;
	for g in [seven, three] {
		let (status, _, body) = browser
			.send_body(
				&app,
				Method::PUT,
				&format!("/api/v1/gmbs/{g}/kit"),
				&[("content-type", "application/json")],
				true,
				Body::from(r#"{"target":"drafts"}"#),
			)
			.await;
		assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
	}
	Kit {
		app,
		panel,
		secret,
		seven,
		three,
		drafts,
		photos,
	}
}

#[tokio::test]
async fn the_offer_skips_what_went_there_and_what_another_view_holds_the_less_used_first() {
	let db = TestDb::create().await;
	let k = kit(&db).await;

	let (on_three, offered) = k.offer("lyon-3").await;
	assert_eq!(k.numbers(&offered), [1, 2, 3, 4, 5, 6], "none used yet: the oldest first");
	for d in [1, 2] {
		assert_eq!(k.event(json!({ "view": on_three, "copied": k.drafts[d - 1] })).await, StatusCode::NO_CONTENT);
	}

	let (seven, offered) = k.offer("lyon-7").await;
	assert_eq!(k.numbers(&offered), [3, 4, 5, 6, 7, 8], "copied elsewhere ranks lower");
	let (_, offered) = k.offer("lyon-7").await;
	assert_eq!(k.numbers(&offered), [1, 2], "the first view holds the rest for 15 minutes");
	let (_, offered) = k.offer("lyon-3").await;
	assert_eq!(k.numbers(&offered), [7, 8], "a GBP's views hold its drafts only: 1, 2 went there, 3 to 6 are held there");

	assert_eq!(k.event(json!({ "view": seven, "copied": k.drafts[2] })).await, StatusCode::NO_CONTENT);
	assert_eq!(
		k.event(json!({ "view": seven, "copied": k.drafts[2] })).await,
		StatusCode::CONFLICT,
		"a copy goes to one GBP once"
	);
	assert_eq!(k.event(json!({ "view": seven, "copied": k.drafts[0] })).await, StatusCode::NOT_FOUND, "not offered to that view");

	let later = Timestamp::now() + SignedDuration::from_mins(16);
	let lapsed: Vec<i64> = k.panel.kit_offer(k.seven, true, later).await.unwrap().unwrap().drafts.iter().map(|d| d.id).collect();
	assert_eq!(lapsed, [4, 5, 6, 7, 8, 1].map(|n| k.drafts[n - 1]), "every lease lapsed; 3 went there, 1 and 2 were copied once");
}

#[tokio::test]
async fn a_photo_saved_on_a_gbp_drops_out_there_and_its_draft_stays() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let fourth = k.drafts[3];

	let (view, offered) = k.offer("lyon-7").await;
	let photos = &offered.iter().find(|(id, _)| *id == fourth).unwrap().1;
	assert_eq!(photos.len(), 2);
	let (status, jpeg) = k.get(&format!("/api/kit/v1/photos/{}.jpg?view={view}", photos[0])).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(image::guess_format(&jpeg).unwrap(), image::ImageFormat::Jpeg, "re-encoded");
	let (held_out, offered) = k.offer("lyon-7").await;
	assert!(offered.iter().all(|(id, _)| *id != fourth));
	let (status, _) = k.get(&format!("/api/kit/v1/photos/{}.jpg?view={held_out}", photos[0])).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "no draft offered to that view selects it");

	assert_eq!(k.event(json!({ "view": view, "saved": [k.photos[0]] })).await, StatusCode::NO_CONTENT);
	assert_eq!(
		k.event(json!({ "view": view, "saved": [k.photos[0]] })).await,
		StatusCode::CONFLICT,
		"a photo goes to one GBP once"
	);

	let later = Timestamp::now() + SignedDuration::from_mins(16);
	let again = k.panel.kit_offer(k.seven, true, later).await.unwrap().unwrap();
	let fourth = again.drafts.iter().find(|d| d.id == fourth).expect("its draft is still offered");
	assert_eq!(fourth.photos.iter().map(|p| p.id).collect::<Vec<_>>(), [k.photos[1]], "the saved photo dropped out there");
	let three = k.panel.kit(DevIdentity::named("dev@localhost"), k.three, later).await.unwrap().unwrap();
	let order: Vec<i64> = three.drafts.iter().map(|d| d.id).collect();
	assert_eq!(order, [1, 2, 3, 5, 6, 7, 8, 4].map(|n| k.drafts[n - 1]), "elsewhere, a draft whose photos were saved ranks lower");
	assert_eq!(
		three.drafts[7].photos.iter().map(|p| p.id).collect::<Vec<_>>(),
		[k.photos[1], k.photos[0]],
		"and the more used photo last"
	);
}

#[tokio::test]
async fn the_kit_off_and_another_brands_key_find_nothing() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let owner = DevIdentity::named("dev@localhost");

	k.panel.set_kit_target(owner, k.seven, None, Timestamp::now()).await.unwrap().unwrap();
	let (status, _) = k.get("/api/kit/v1/offer?brand=aquafix&slug=lyon-7").await;
	assert_eq!(status, StatusCode::NOT_FOUND, "the kit is off");
	let (status, _) = k.get("/api/kit/v1/offer?brand=vifnet&slug=lyon-3").await;
	assert_eq!(status, StatusCode::NOT_FOUND, "the key writes for aquafix only");
	k.panel.set_kit_target(owner, k.seven, Some(Target::OneTap), Timestamp::now()).await.unwrap().unwrap();
	let (_, offered) = k.offer("lyon-7").await;
	assert_eq!(k.numbers(&offered), [1], "one tap: the top draft");
}
