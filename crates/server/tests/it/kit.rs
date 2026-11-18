//! The review kit's offer, through the landings' signed routes: no draft and no photo goes twice
//! to one GBP, no draft outside its places, none is offered while another view holds it, the less
//! used come first. A kit tallies its GBPs since each joined; a review posted from a copy is matched
//! to it.

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
	prices::{Item, PlacesRequest, Spender},
};
use sa_panel::{
	Panel,
	brands::NewBrand,
	gmbs::{Category, Existing},
	import::Export,
	kit::Target,
	seal::DataKey,
	store::users::User,
	testing::{SourceKey, TestDb, operator, panel_on, sign, sign_get},
};
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

pub(super) struct Kit {
	app: Router,
	browser: Browser,
	pub(super) panel: Panel,
	secret: String,
	pub(super) owner: Uuid,
	pub(super) trade: i64,
	/// Aquafix, on aquafix, the brand of both GBPs.
	pub(super) brand: i64,
	/// Lyon 7, the GBP the checks are about; Lyon 3 shares its category and brand.
	pub(super) seven: i64,
	pub(super) three: i64,
	pub(super) drafts: Vec<i64>,
	/// The brand's.
	pub(super) photos: Vec<i64>,
}
impl Kit {
	async fn send(&self, req: Request<Body>) -> (StatusCode, Vec<u8>) {
		let res = self.app.clone().oneshot(req).await.unwrap();
		let status = res.status();
		(status, to_bytes(res.into_body(), usize::MAX).await.unwrap().to_vec())
	}

	pub(super) async fn get(&self, path: &str) -> (StatusCode, Vec<u8>) {
		let s = sign_get(SourceKey { id: KEY, secret: &self.secret }, path, Timestamp::now());
		let req = Request::get(path)
			.header("x-sa-key-id", &s.key_id)
			.header("x-sa-timestamp", &s.timestamp)
			.header("x-sa-signature", &s.signature);
		self.send(req.body(Body::empty()).unwrap()).await
	}

	/// The page of `slug` opened: its view and the drafts offered, each with its photos.
	pub(super) async fn offer(&self, slug: &str) -> (Uuid, Vec<(i64, Vec<i64>)>) {
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

	pub(super) async fn event(&self, body: Value) -> StatusCode {
		let body = body.to_string().into_bytes();
		let s = sign(SourceKey { id: KEY, secret: &self.secret }, &body, Timestamp::now());
		let req = Request::post("/api/kit/v1/events")
			.header("content-type", "application/json")
			.header("x-sa-key-id", &s.key_id)
			.header("x-sa-timestamp", &s.timestamp)
			.header("x-sa-signature", &s.signature);
		self.send(req.body(Body::from(body)).unwrap()).await.0
	}

	async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (StatusCode, Value) {
		let app = self.app.clone();
		self.browser.call(&app, method, path, body).await
	}

	/// Draft numbers (1-based) of `offered`.
	fn numbers(&self, offered: &[(i64, Vec<i64>)]) -> Vec<usize> {
		offered.iter().map(|(id, _)| self.drafts.iter().position(|d| d == id).unwrap() + 1).collect()
	}
}

pub(super) fn png(shade: u8) -> Vec<u8> {
	let mut out = Vec::new();
	image::RgbImage::from_pixel(8, 6, image::Rgb([shade, 0, 0]))
		.write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
		.unwrap();
	out
}

/// Two GBPs of Plumbing · FR on aquafix's landing, of brand Aquafix with two photos, the kit on
/// with 8 drafts.
pub(super) async fn kit(db: &TestDb) -> Kit {
	let pages = Arc::new(Mutex::new(Vec::new()));
	let base = stand_ins(pages.clone()).await;
	let store = db.store().await;
	let google = Google::try_new(Keys::new(vec![("house".to_owned(), "k".to_owned())]), Arc::new(sa_places::InOrder), Arc::new(store.clone()))
		.unwrap()
		.spend(true)
		.at(format!("{base}/v1/places"));
	let panel = panel_on(
		store,
		DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap(),
		google,
		sa_places::Accounts::new(vec![sa_panel::testing::account("house", Some("k"), None)]),
	);
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
		.unwrap()
		.unwrap();
	sa_panel::testing::priced(&panel, &[PlacesRequest::Details].map(|r| (Item::Places(r), 0))).await;
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
			trade: Some(trade),
		};
		panel.add_gmb(Some(Spender::new(owner)), e, now).await.unwrap().unwrap().id
	};
	let (seven, three) = (gmb(SEVEN, "Lyon 7").await, gmb(THREE, "Lyon 3").await);
	pages.lock().unwrap().extend([(seven, "lyon-7"), (three, "lyon-3")]);
	let brand = panel
		.add_brand(
			owner,
			NewBrand {
				gmail_id: gmail,
				trade_id: trade,
				country: Country::FR,
				name: "Aquafix".to_owned(),
				site: BrandId::parse("aquafix").unwrap(),
			},
			now,
		)
		.await
		.unwrap()
		.unwrap()
		.id;
	panel.set_brands(owner, &[seven, three], Some(brand)).await.unwrap().unwrap();
	let photos = vec![
		panel.add_photo(owner, brand, png(10), now).await.unwrap().unwrap(),
		panel.add_photo(owner, brand, png(20), now).await.unwrap().unwrap(),
	];
	let mut drafts = Vec::new();
	for n in 1..=8 {
		drafts.push(
			panel
				.add_draft(owner, trade, Country::FR, &format!("Draft {n}: quick and clean."), &[], now)
				.await
				.unwrap()
				.unwrap(),
		);
	}
	let app = app_with(panel.clone(), operator(), "dev@localhost", &base.parse().unwrap(), http::Limits::default());
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
		browser,
		panel,
		secret,
		owner,
		trade,
		brand,
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
	let lapsed: Vec<i64> = k.panel.kit_offer(k.seven, true, None, later).await.unwrap().unwrap().drafts.iter().map(|d| d.id).collect();
	assert_eq!(lapsed, [4, 5, 6, 7, 8, 1].map(|n| k.drafts[n - 1]), "every lease lapsed; 3 went there, 1 and 2 were copied once");
}

#[tokio::test]
async fn a_photo_saved_on_a_gbp_drops_out_there_and_its_draft_stays() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let fourth = k.drafts[3];

	let (view, offered) = k.offer("lyon-7").await;
	assert!(offered.iter().all(|(_, photos)| *photos == k.photos), "each draft its brand's two least used: {offered:?}");
	let (status, jpeg) = k.get(&format!("/api/kit/v1/photos/{}.jpg?view={view}", k.photos[0])).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(image::guess_format(&jpeg).unwrap(), image::ImageFormat::Jpeg, "re-encoded");

	assert_eq!(k.event(json!({ "view": view, "saved": [k.photos[0]] })).await, StatusCode::NO_CONTENT);
	assert_eq!(
		k.event(json!({ "view": view, "saved": [k.photos[0]] })).await,
		StatusCode::CONFLICT,
		"a photo goes to one GBP once"
	);

	let later = Timestamp::now() + SignedDuration::from_mins(16);
	let again = k.panel.kit_offer(k.seven, true, None, later).await.unwrap().unwrap();
	let fourth = again.drafts.iter().find(|d| d.id == fourth).expect("its draft is still offered");
	assert_eq!(fourth.photos.iter().map(|p| p.id).collect::<Vec<_>>(), [k.photos[1]], "the saved photo dropped out there");
	let three = k.panel.kit(k.owner, k.three, Timestamp::UNIX_EPOCH, later).await.unwrap().unwrap();
	assert_eq!(
		three.drafts[0].photos.iter().map(|p| p.id).collect::<Vec<_>>(),
		[k.photos[1], k.photos[0]],
		"elsewhere, the more used photo last"
	);
}

#[tokio::test]
async fn the_kit_off_and_another_brands_key_find_nothing() {
	let db = TestDb::create().await;
	let k = kit(&db).await;
	let owner = DevIdentity::named("dev@localhost");

	k.panel.set_kit_target(Spender::new(owner), k.seven, None, Timestamp::now()).await.unwrap().unwrap();
	let (status, _) = k.get("/api/kit/v1/offer?brand=aquafix&slug=lyon-7").await;
	assert_eq!(status, StatusCode::NOT_FOUND, "the kit is off");
	let (status, _) = k.get("/api/kit/v1/offer?brand=vifnet&slug=lyon-3").await;
	assert_eq!(status, StatusCode::NOT_FOUND, "the key writes for aquafix only");
	k.panel
		.set_kit_target(Spender::new(owner), k.seven, Some(Target::OneTap), Timestamp::now())
		.await
		.unwrap()
		.unwrap();
	let (_, offered) = k.offer("lyon-7").await;
	assert_eq!(k.numbers(&offered), [1], "one tap: the top draft");
}

/// Areas by Lyon 7 and Lyon 3 (both at 45.75, 4.84): `lyon` holds them, `bron` does not.
async fn areas(db: &TestDb) {
	let pool = db.pool().await;
	for (slug, name, lat, lng, r) in [("lyon", "Lyon", 45.75, 4.84, 5000), ("bron", "Bron", 45.73, 4.91, 2000)] {
		sqlx::query("INSERT INTO areas (slug, name, country, lat, lng, radius_m, created_by, created_at) VALUES ($1, $2, 'FR', $3, $4, $5, $6, 0)")
			.bind(slug)
			.bind(name)
			.bind(lat)
			.bind(lng)
			.bind(r)
			.bind(DevIdentity::named("dev@localhost"))
			.execute(&pool)
			.await
			.unwrap();
	}
}

#[tokio::test]
async fn a_draft_tagged_with_a_place_goes_only_to_a_gbp_in_it() {
	let db = TestDb::create().await;
	let mut k = kit(&db).await;
	areas(&db).await;
	let (d1, d2) = (k.drafts[0], k.drafts[1]);
	let trade = k.trade;
	let tag = |d: i64| format!("/api/v1/trades/{trade}/FR/kit/drafts/{d}");
	let (status, _) = k.call(Method::PATCH, &tag(d1), Some(json!({ "places": ["bron"] }))).await;
	assert_eq!(status, StatusCode::OK);
	let (status, pool) = k.call(Method::PATCH, &tag(d2), Some(json!({ "places": ["lyon"] }))).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(pool["drafts"].as_array().unwrap().iter().find(|d| d["id"] == d1).unwrap()["places"], json!(["bron"]));

	let (_, offered) = k.offer("lyon-7").await;
	assert_eq!(k.numbers(&offered), [2, 3, 4, 5, 6, 7], "Bron's draft never; Lyon's as any");
	let (status, _) = k.call(Method::PATCH, &tag(d1), Some(json!({ "places": ["nowhere"] }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a place is an area");
}

#[tokio::test]
async fn a_kit_tallies_its_gbps_since_each_joined_and_holds_only_the_callers() {
	let db = TestDb::create().await;
	let mut k = kit(&db).await;
	let (before, _) = k.offer("lyon-3").await;
	assert_eq!(k.event(json!({ "view": before, "copied": k.drafts[0] })).await, StatusCode::NO_CONTENT);

	let (status, body) = k
		.call(
			Method::PUT,
			"/api/v1/kits/lyon-east",
			Some(json!({ "name": "Lyon east", "area": null, "gmbs": [k.seven, k.three] })),
		)
		.await;
	assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
	let (status, _) = k.call(Method::PUT, "/api/v1/kits/new", Some(json!({ "name": "New", "area": null, "gmbs": [] }))).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "new is the editor's");
	let (status, _) = k
		.call(Method::PUT, "/api/v1/kits/other", Some(json!({ "name": "Other", "area": null, "gmbs": [k.seven, 9999] })))
		.await;
	assert_eq!(status, StatusCode::CONFLICT, "a GBP not the caller's");

	let (after, offered) = k.offer("lyon-7").await;
	assert_eq!(k.event(json!({ "view": after, "copied": offered[0].0 })).await, StatusCode::NO_CONTENT);

	let (status, kits) = k.call(Method::GET, "/api/v1/kits", None).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(kits["kits"], json!([{ "slug": "lyon-east", "name": "Lyon east", "area": null, "members": 2 }]));
	let (status, kit) = k.call(Method::GET, "/api/v1/kits/lyon-east?since=0", None).await;
	assert_eq!(status, StatusCode::OK, "{kit}");
	assert_eq!(
		(kit["tally"]["opened"].as_i64(), kit["tally"]["copied"].as_i64()),
		(Some(1), Some(1)),
		"the copy before it joined is not the kit's: {kit}"
	);
	assert_eq!(
		kit["gmbs"].as_array().unwrap().iter().map(|g| g["blocked"].clone()).collect::<Vec<_>>(),
		[Value::Null, Value::Null]
	);
	let later = Timestamp::now().as_second() + 60;
	let (_, kit) = k.call(Method::GET, &format!("/api/v1/kits/lyon-east?since={later}"), None).await;
	assert_eq!(kit["tally"]["copied"], 0, "within the period only");

	let (status, _) = k.call(Method::DELETE, "/api/v1/kits/lyon-east", None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (status, _) = k.call(Method::GET, "/api/v1/kits/lyon-east?since=0", None).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_review_carrying_a_copys_words_is_of_it_and_its_going_is_followed() {
	let db = TestDb::create().await;
	let mut k = kit(&db).await;
	let (status, _) = k
		.call(Method::PUT, "/api/v1/kits/lyon-east", Some(json!({ "name": "Lyon east", "area": null, "gmbs": [k.seven] })))
		.await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (view, offered) = k.offer("lyon-7").await;
	assert_eq!(k.event(json!({ "view": view, "copied": offered[0].0 })).await, StatusCode::NO_CONTENT);

	let seen = Timestamp::now() + SignedDuration::from_mins(5);
	let review = |seq: i64, id: &str, text: &str, gone_at: Option<Timestamp>| sa_core::feeds::Review {
		seq,
		review_id: id.to_owned(),
		gmb_id: k.seven,
		rating: 5,
		text: text.to_owned(),
		first_seen: seen,
		gone_at,
	};
	let page = sa_core::feeds::ReviewPage {
		reviews: vec![
			review(1, "r-copy", "Quick and CLEAN work, the draft was right!", None),
			review(2, "r-other", "Terrible service", None),
		],
		next: 2,
	};
	assert_eq!(k.panel.take_reviews(page.clone(), 0).await.unwrap(), 2);
	assert!(k.panel.take_reviews(page, 0).await.is_err(), "a page is taken from where the cursor stands");
	let gone = sa_core::feeds::ReviewPage {
		reviews: vec![review(3, "r-copy", "Quick and CLEAN work, the draft was right!", Some(seen + SignedDuration::from_mins(1)))],
		next: 3,
	};
	k.panel.take_reviews(gone, 2).await.unwrap();

	let (_, kit) = k.call(Method::GET, "/api/v1/kits/lyon-east?since=0", None).await;
	let t = &kit["tally"];
	assert_eq!((t["on_google"].as_i64(), t["gone"].as_i64(), t["other_reviews"].as_i64()), (Some(1), Some(1), Some(1)), "{kit}");
	let kinds: Vec<&str> = kit["latest"].as_array().unwrap().iter().map(|e| e["kind"].as_str().unwrap()).take(3).collect();
	assert_eq!(kinds, ["gone", "posted", "reviewed"], "{kit}");
}
#[tokio::test]
async fn what_a_brands_photo_shows_rides_to_the_landing() {
	let db = TestDb::create().await;
	let mut k = kit(&db).await;
	let (status, photos) = k
		.call(
			Method::PATCH,
			&format!("/api/v1/brands/{}/photos/{}.jpg", k.brand, k.photos[0]),
			Some(json!({ "shows": "  The new tap  " })),
		)
		.await;
	assert_eq!(status, StatusCode::OK, "{photos}");
	let (_, body) = k.get("/api/kit/v1/offer?brand=aquafix&slug=lyon-7&qr=1").await;
	let o: Value = serde_json::from_slice(&body).unwrap();
	let shown: Vec<&Value> = o["drafts"]
		.as_array()
		.unwrap()
		.iter()
		.flat_map(|d| d["photos"].as_array().unwrap())
		.filter(|p| p["id"] == k.photos[0])
		.map(|p| &p["shows"])
		.collect();
	assert!(!shown.is_empty() && shown.iter().all(|s| *s == "The new tap"), "{o}");
}

#[tokio::test]
async fn a_kits_qr_and_shows_tag_what_they_bring_with_it_and_its_removal_keeps_them() {
	let db = TestDb::create().await;
	let mut k = kit(&db).await;
	let put = json!({ "name": "Lyon east", "area": null, "gmbs": [k.seven] });
	let (status, _) = k.call(Method::PUT, "/api/v1/kits/lyon-east", Some(put.clone())).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (_, kit) = k.call(Method::GET, "/api/v1/kits/lyon-east?since=0", None).await;
	let url = kit["gmbs"][0]["url"].as_str().unwrap().to_owned();
	let id: i64 = url.split("&kit=").nth(1).expect("a kit's QR carries it").parse().unwrap();

	let (status, _) = k.call(Method::POST, &format!("/api/v1/gmbs/{}/kit/shown?kit=lyon-east", k.seven), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (status, _) = k.call(Method::POST, &format!("/api/v1/gmbs/{}/kit/shown?kit=lyon-east", k.three), None).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "Lyon 3 is not in it");
	let (status, _) = k.get(&format!("/api/kit/v1/offer?brand=aquafix&slug=lyon-7&qr=1&kit={}", id + 100)).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "no such kit");
	let (status, body) = k.get(&format!("/api/kit/v1/offer?brand=aquafix&slug=lyon-7&qr=1&kit={id}")).await;
	assert_eq!(status, StatusCode::OK);
	let o: Value = serde_json::from_slice(&body).unwrap();
	assert_eq!(k.event(json!({ "view": o["view"], "copied": o["drafts"][0]["id"] })).await, StatusCode::NO_CONTENT);

	let (status, _) = k.call(Method::DELETE, "/api/v1/kits/lyon-east", None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let (status, _) = k.call(Method::PUT, "/api/v1/kits/lyon-east", Some(put)).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "a removed kit's slug is free");

	let pool = db.pool().await;
	let shown: Vec<Option<i64>> = sqlx::query_scalar("SELECT kit_id FROM kit_shown ORDER BY at").fetch_all(&pool).await.unwrap();
	assert_eq!(shown, [Some(id)]);
	let copied: Option<i64> = sqlx::query_scalar("SELECT v.kit_id FROM kit_uses u JOIN kit_views v ON v.id = u.view_id")
		.fetch_one(&pool)
		.await
		.unwrap();
	assert_eq!(copied, Some(id), "a copy is its view's kit's");
	let removed: Option<i64> = sqlx::query_scalar("SELECT removed_at FROM kits WHERE id = $1").bind(id).fetch_one(&pool).await.unwrap();
	assert!(removed.is_some(), "kept, removed");
}
