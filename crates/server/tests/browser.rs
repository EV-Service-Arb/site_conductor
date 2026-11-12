//! A gmail's browser: its proxy checked before it is kept, one gmail's only, out in its GBPs'
//! country; its fingerprint set once; started at a GBP's point on the runner as the conductor;
//! its viewer refused to another origin and another person.

use std::sync::{Arc, Mutex};

use axum::{
	Json, Router,
	body::Body,
	extract::{Path, State},
	http::{HeaderMap, Method, StatusCode, header},
	response::IntoResponse,
	routing::{get, post, put},
};
use base64::Engine;
use sa_auth::{Principal, Service};
use sa_panel::{
	browsers::Browsers,
	import::Export,
	testing::{TestDb, operator, panel},
};
use sa_server::{
	browser::Connected,
	concierge::{Concierge, DEV_CODE, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, Log, ORIGIN, SEED, upstream},
};
use serde_json::{Value, json};

/// ip-api, as each proxy's user comes out: `fr`, `us`, else refused.
async fn ip_api(headers: HeaderMap) -> impl IntoResponse {
	let auth = headers.get(header::PROXY_AUTHORIZATION).map(|v| v.to_str().unwrap().strip_prefix("Basic ").unwrap().to_owned());
	let user = auth.map(|a| String::from_utf8(base64::engine::general_purpose::STANDARD.decode(a).unwrap()).unwrap());
	let user = user.as_deref().and_then(|u| u.split_once(':')).map(|(u, _)| u.to_owned());
	Json(match user.as_deref().and_then(|u| u.split('-').next()) {
		Some("fr") => json!({ "status": "success", "countryCode": "FR", "city": "Lyon", "timezone": "Europe/Paris", "mobile": true, "query": "92.184.1.2" }),
		Some("us") => json!({ "status": "success", "countryCode": "US", "city": "Austin", "timezone": "America/Chicago", "mobile": false, "query": "8.8.4.4" }),
		_ => json!({ "status": "fail", "message": "private range" }),
	})
}

/// GoLogin's API: what it was asked, in order.
#[derive(Clone, Default)]
struct GoLogin(Arc<Mutex<Vec<String>>>);

impl GoLogin {
	fn asked(&self) -> Vec<String> {
		std::mem::take(&mut *self.0.lock().unwrap())
	}
}

fn gologin(stub: GoLogin) -> Router {
	let noted = |what: &'static str| {
		move |State(s): State<GoLogin>| async move {
			s.0.lock().unwrap().push(what.to_owned());
		}
	};
	Router::new()
		.route(
			"/browser/custom",
			post(|State(s): State<GoLogin>| async move {
				s.0.lock().unwrap().push("create".to_owned());
				Json(json!({ "id": "gl-1" }))
			}),
		)
		.route(
			"/browser/{id}",
			get(|State(s): State<GoLogin>, Path(id): Path<String>| async move {
				s.0.lock().unwrap().push(format!("read {id}"));
				Json(json!({ "canvas": { "noise": 0.83 }, "webGL": { "noise": 53.3 }, "clientRects": { "noise": 6.63 }, "audioContext": { "noise": 1.8e-8 } }))
			})
			.delete(noted("delete")),
		)
		.route("/browser/{id}/custom", put(noted("put")))
		.route(
			"/browser/features/{id}/profile-params-for-orbita-token",
			get(|State(s): State<GoLogin>| async move {
				s.0.lock().unwrap().push("token".to_owned());
				Json(json!({ "token": "three-minute-token" }))
			}),
		)
		.with_state(stub)
}

/// Multilogin's API: what it was asked, in order, with the bodies.
#[derive(Clone, Default)]
struct Multilogin(Arc<Mutex<Vec<(String, Value)>>>);

impl Multilogin {
	fn asked(&self) -> Vec<(String, Value)> {
		std::mem::take(&mut *self.0.lock().unwrap())
	}

	fn paths(&self) -> Vec<String> {
		self.asked().into_iter().map(|(p, _)| p).collect()
	}
}

fn multilogin(stub: Multilogin) -> Router {
	let ok = |data: Value| Json(json!({ "data": data, "status": { "error_code": "", "http_code": 200, "message": "" } }));
	let answer = move |path: &str, body: &Value| match path {
		"/workspace/automation_token" => json!({ "token": "three-hour-token" }),
		"/workspace/folders" => json!({ "folders": [{ "folder_id": "f-default", "name": "Default folder" }] }),
		"/workspace/folder_create" => json!({ "id": "f-sc" }),
		"/mobile_profiles/phone/brand/list" => json!([{ "surface_brand_name": "Xiaomi", "surface_model_name": "14" }, { "surface_brand_name": "Google", "surface_model_name": "Pixel 7" }]),
		"/mobile_profiles/phone/create" => json!({ "fail_amount": 0, "details": [{ "id": "ph-1" }] }),
		"/profile/create" => json!({ "ids": ["mimic-1"] }),
		"/mobile_profiles/files/upload" => json!({ "material_id": format!("film-{}", body["n"]) }),
		_ => json!({ "fail_amount": 0 }),
	};
	let handler = move |State(s): State<Multilogin>, uri: axum::http::Uri, body: axum::body::Bytes| async move {
		let path = uri.path().to_owned();
		let body: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({ "n": s.0.lock().unwrap().iter().filter(|(p, _)| p.ends_with("upload")).count() }));
		s.0.lock().unwrap().push((path.clone(), body.clone()));
		ok(answer(&path, &body))
	};
	Router::new().fallback(handler).with_state(stub)
}

/// sa_browsers' runner: what is running, as launched and stopped.
fn runner(running: Arc<Mutex<Vec<i64>>>) -> Router {
	let (launched, stopped, listed) = (running.clone(), running.clone(), running);
	Router::new()
		.route(
			"/profiles/{gmail}/launch",
			post(move |Path(g): Path<i64>| async move {
				launched.lock().unwrap().push(g);
				StatusCode::NO_CONTENT
			}),
		)
		.route(
			"/profiles/{gmail}/stop",
			post(move |Path(g): Path<i64>| async move {
				stopped.lock().unwrap().retain(|r| *r != g);
				StatusCode::NO_CONTENT
			}),
		)
		.route("/profiles/{gmail}", axum::routing::delete(|| async { StatusCode::NO_CONTENT }))
		.route("/profiles/{gmail}/film", put(|| async { StatusCode::NO_CONTENT }).delete(|| async { StatusCode::NO_CONTENT }))
		.route(
			"/profiles",
			get(move || async move { Json(json!({ "running": listed.lock().unwrap().iter().map(|g| json!({ "gmail": g, "run": "gmb:1" })).collect::<Vec<_>>(), "films": [] })) }),
		)
}

async fn serve(routes: Router) -> String {
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
	format!("http://{addr}")
}

struct World {
	app: Router,
	b: Browser,
	gologin: GoLogin,
	multilogin: Multilogin,
	runner: Log,
	/// What ip-api's stub answers through, as a proxy: any user, any password.
	proxy: String,
	db: TestDb,
}

impl World {
	async fn new() -> Self {
		Self::with_runner(runner(Arc::default())).await
	}

	async fn with_runner(runner: Router) -> Self {
		let stub = GoLogin::default();
		let gologin_base = serve(gologin(stub.clone())).await;
		let ml = Multilogin::default();
		let multilogin_base = serve(multilogin(ml.clone())).await;
		let proxy = serve(Router::new().route("/json/", get(ip_api))).await;
		let (runner_base, runner_log) = upstream(runner).await;

		let db = TestDb::create().await;
		let panel = panel(&db).await;
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap();
		let who = DevIdentity {
			user_id: DevIdentity::named("dev@localhost"),
			permissions: operator(),
			email: "dev@localhost".to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let browsers = Browsers::stubbed(&gologin_base, &multilogin_base, "http://ip-api.test/json/");
		let connected = Connected::try_new(browsers, &runner_base, SEED.parse().unwrap()).unwrap();
		let app = http::app(SignIn::new(panel, Concierge::dev(who), config), None, None, Default::default(), None, None, Some(connected));
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self {
			app,
			b,
			gologin: stub,
			multilogin: ml,
			runner: runner_log,
			proxy: proxy.strip_prefix("http://").unwrap().to_owned(),
			db,
		}
	}

	async fn call(&mut self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
		let json = [("content-type", "application/json")];
		let (status, _, bytes) = self
			.b
			.send_body(
				&self.app,
				method,
				uri,
				if body.is_some() { &json } else { &[] },
				true,
				body.map_or(Body::empty(), |b| Body::from(b.to_string())),
			)
			.await;
		(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
	}

	async fn gmail(&mut self, gmail: &str) -> i64 {
		let (status, added) = self.call(Method::POST, "/api/v1/gmails", Some(json!({ "gmail": gmail }))).await;
		assert_eq!(status, StatusCode::CREATED, "{added}");
		added["id"].as_i64().unwrap()
	}

	/// A draft of `gmail`'s in `country`, at a point in Lyon, of a trade named as it.
	async fn gbp(&mut self, gmail: i64, name: &str, country: &str) -> i64 {
		let (status, trade) = self.call(Method::POST, "/api/v1/trades", Some(json!({ "name": name }))).await;
		assert_eq!(status, StatusCode::CREATED, "{trade}");
		let draft = json!({ "name": name, "trade": trade["id"], "state": "draft", "gmail_id": gmail, "country": country, "location": { "lat": 45.75, "lng": 4.85 } });
		let (status, draft) = self.call(Method::POST, "/api/v1/gmbs", Some(draft)).await;
		assert_eq!(status, StatusCode::CREATED, "{draft}");
		draft["id"].as_i64().unwrap()
	}

	/// `user`'s proxy through ip-api's stub.
	fn proxy(&self, user: &str) -> String {
		format!("http://{user}:secret-of-{user}@{}", self.proxy)
	}

	async fn set(&mut self, gmail: i64, device: &str, proxy: &str) -> (StatusCode, Value) {
		self.set_as(gmail, "gologin", Some(device), proxy).await
	}

	async fn set_as(&mut self, gmail: i64, provider: &str, device: Option<&str>, proxy: &str) -> (StatusCode, Value) {
		self.call(
			Method::PUT,
			&format!("/api/v1/gmails/{gmail}/browser"),
			Some(json!({ "provider": provider, "device": device, "proxy": proxy })),
		)
		.await
	}

	/// The runner's launches, as it was told them.
	fn launches(&self) -> Vec<Value> {
		self.runner
			.lock()
			.unwrap()
			.iter()
			.filter(|s| s.path_and_query.ends_with("/launch"))
			.map(|s| serde_json::from_slice(&s.body).unwrap())
			.collect()
	}

	async fn film(&mut self, gmail: i64, mp4: Vec<u8>) -> StatusCode {
		let (status, _, _) = self
			.b
			.send_body(
				&self.app,
				Method::PUT,
				&format!("/api/v1/gmails/{gmail}/browser/film"),
				&[("content-type", "video/mp4")],
				true,
				Body::from(mp4),
			)
			.await;
		status
	}

	async fn listed(&mut self, gmail: i64) -> Value {
		let (_, gmails) = self.call(Method::GET, "/api/v1/gmails", None).await;
		gmails["gmails"].as_array().unwrap().iter().find(|g| g["id"] == gmail).unwrap().clone()
	}
}

#[tokio::test]
async fn a_proxy_is_kept_only_once_it_goes_through_out_in_the_gmails_country_and_no_one_elses() {
	let mut w = World::new().await;
	let (a, b) = (w.gmail("a@gmail.com").await, w.gmail("b@gmail.com").await);
	w.gbp(a, "Plombier Lyon 7", "FR").await;

	for (proxy, why) in [
		("ftp://u:p@example.com:21".to_owned(), "a scheme no browser speaks"),
		("http://example.com:8080".to_owned(), "no credentials"),
		(w.proxy("down"), "ip-api refused it"),
		(w.proxy("us"), "out in the US, the gmail's GBP in France"),
	] {
		let (status, refused) = w.set(a, "pixel_7", &proxy).await;
		assert_eq!(status, StatusCode::BAD_REQUEST, "{why}: {refused}");
	}
	let (_, none) = w.call(Method::GET, &format!("/api/v1/gmails/{a}/connections"), None).await;
	assert_eq!(none["browser"], Value::Null, "a refusal stores nothing: {none}");
	assert_eq!(w.listed(a).await["browser"], false, "the GBPs' profile must-have not ready");

	let (status, set) = w.set(a, "pixel_7", &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let exit = &set["browser"]["exit"];
	assert_eq!((&exit["country"], &exit["city"], &exit["mobile"]), (&json!("FR"), &json!("Lyon"), &json!(true)), "{set}");
	assert_eq!(set["browser"]["endpoint"], w.proxy, "host:port, never the credentials");
	assert!(!set.to_string().contains("secret-of-fr"), "{set}");
	assert_eq!(w.listed(a).await["browser"], true, "the profile must-have follows the gmail");
	for suffix in ["", "-wal"] {
		let file = std::fs::read(format!("{}{suffix}", w.db.path().display())).unwrap_or_default();
		assert!(!file.windows(12).any(|w| w == b"secret-of-fr"), "the proxy in the clear in the database{suffix}");
	}

	let (status, refused) = w.set(b, "pixel_7", &w.proxy("fr").replace("http://", "HTTP://")).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "another gmail's proxy, however spelled: {refused}");

	let (status, refused) = w.set(a, "galaxy_s23", &w.proxy("fr-2")).await;
	assert_eq!(status, StatusCode::CONFLICT, "a fingerprint is set once: {refused}");
	let (status, replaced) = w.set(a, "pixel_7", &w.proxy("fr-2")).await;
	assert_eq!(status, StatusCode::OK, "the proxy alone is replaced: {replaced}");
	let (status, taken) = w.set(b, "redmi_note_14", &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "given up by a, free for b: {taken}");

	let (status, refused) = w.call(Method::DELETE, &format!("/api/v1/gmails/{a}"), None).await;
	assert_eq!(status, StatusCode::CONFLICT, "its browser goes first: {refused}");
}

#[tokio::test]
async fn a_start_is_at_the_gbps_point_its_profile_made_once_and_told_to_the_runner_as_the_conductor() {
	let mut w = World::new().await;
	let a = w.gmail("a@gmail.com").await;
	let gbp = w.gbp(a, "Plombier Lyon 7", "FR").await;
	let (status, set) = w.set(a, "pixel_7", &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");

	let (status, body) = w.call(Method::POST, &format!("/api/v1/gmails/{a}/browser/start"), Some(json!({ "gmb": gbp }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
	assert_eq!(w.gologin.asked(), ["create", "read gl-1", "put", "token"], "made once, its noise read back");
	let launch = w.runner.lock().unwrap().iter().find(|s| s.path_and_query == format!("/profiles/{a}/launch")).unwrap().clone();
	let asserted = launch.assertion(Service::Browsers, None);
	assert_eq!(asserted.principal, Principal::Conductor);
	let launch: Value = serde_json::from_slice(&launch.body).unwrap();
	let orbita = &launch["browser"];
	assert_eq!(
		(&orbita["provider"], &orbita["zone"], &orbita["locale"]),
		(&json!("orbita"), &json!("Europe/Paris"), &json!("fr-FR")),
		"{launch}"
	);
	let prefs = &orbita["preferences"]["gologin"];
	assert_eq!(
		(&prefs["geoLocation"]["latitude"], &prefs["geoLocation"]["longitude"]),
		(&json!(45.75), &json!(4.85)),
		"the GBP's point"
	);
	assert_eq!(orbita["config"]["gologin"]["profile_token"], "three-minute-token");
	assert_eq!(orbita["proxy"], format!("http://{}", w.proxy), "the credentials ride in the preferences only");

	let (_, connections) = w.call(Method::GET, &format!("/api/v1/gmails/{a}/connections"), None).await;
	assert_eq!(connections["browser"]["running"], json!({ "gmb": gbp, "stream": null }), "{connections}");
	let (status, _) = w.call(Method::POST, &format!("/api/v1/gmails/{a}/browser/start"), Some(json!({ "gmb": null }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(w.gologin.asked(), ["put", "token"], "the profile is not made again");
	let launches: Vec<Value> = w
		.runner
		.lock()
		.unwrap()
		.iter()
		.filter(|s| s.path_and_query.ends_with("/launch"))
		.map(|s| serde_json::from_slice(&s.body).unwrap())
		.collect();
	assert_eq!(launches[1]["browser"]["preferences"]["gologin"]["geoLocation"]["mode"], "block", "no GBP, no position");

	let (status, _) = w.call(Method::POST, &format!("/api/v1/gmails/{a}/browser/stop"), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let mp4 = vec![7u8; 3 << 20];
	let (status, _, _) =
		w.b.send_body(
			&w.app,
			Method::PUT,
			&format!("/api/v1/gmails/{a}/browser/film"),
			&[("content-type", "video/mp4")],
			true,
			Body::from(mp4.clone()),
		)
		.await;
	assert_eq!(status, StatusCode::NO_CONTENT, "a film past the API's own body limit");
	let film = w.runner.lock().unwrap().iter().find(|s| s.path_and_query.ends_with("/film")).unwrap().clone();
	assert_eq!((film.body.len(), film.assertion(Service::Browsers, None).principal), (mp4.len(), Principal::Conductor));
	let (status, _, _) = w.b.send_body(&w.app, Method::PUT, &format!("/api/v1/gmails/{a}/browser/film"), &[], false, Body::from("x")).await;
	assert_eq!(status, StatusCode::FORBIDDEN, "the gate's CSRF stands on the film too");
	let (status, _) = w.call(Method::DELETE, &format!("/api/v1/gmails/{a}/browser"), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(w.gologin.asked(), ["delete"], "the provider's profile goes with it");
	let (_, gone) = w.call(Method::GET, &format!("/api/v1/gmails/{a}/connections"), None).await;
	assert_eq!(gone["browser"], Value::Null, "{gone}");
}

#[tokio::test]
async fn the_viewer_is_refused_to_another_origin_and_to_another_persons_gmail() {
	let mut w = World::new().await;
	let a = w.gmail("a@gmail.com").await;
	let (status, set) = w.set(a, "pixel_7", &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let viewer = format!("/api/v1/gmails/{a}/browser");

	let (status, _) = w.b.send(&w.app, Method::GET, &viewer, &[("origin", "https://evil.example")], false).await;
	assert_eq!(status, StatusCode::FORBIDDEN);

	let mut other = Browser::default();
	let (_, headers) = other.send(&w.app, Method::GET, "/auth/login?as=other%40localhost", &[], false).await;
	let location = headers[header::LOCATION].to_str().unwrap().strip_prefix(ORIGIN).unwrap().to_owned();
	assert!(location.starts_with(&format!("/auth/callback?code={DEV_CODE}")));
	other.send(&w.app, Method::GET, &location, &[], false).await;
	let (status, _) = other.send(&w.app, Method::GET, &viewer, &[("origin", ORIGIN)], false).await;
	assert_eq!(status, StatusCode::NOT_FOUND, "another person's gmail is not there");
	let (status, _) = other.send(&w.app, Method::POST, &format!("{viewer}/stop"), &[], true).await;
	assert_eq!(status, StatusCode::NOT_FOUND);
}

/// Chrome's devtools as far as the viewer drives them: one page, attached, cast; what it is told.
async fn devtools(socket: axum::extract::ws::WebSocket, told: Arc<Mutex<Vec<Value>>>) {
	use axum::extract::ws::Message;
	use futures::{SinkExt, StreamExt};
	let (mut tx, mut rx) = socket.split();
	while let Some(Ok(Message::Text(t))) = rx.next().await {
		let m: Value = serde_json::from_str(&t).unwrap();
		told.lock().unwrap().push(m.clone());
		let answer = |result: Value| Message::Text(json!({ "id": m["id"], "result": result }).to_string().into());
		let out: Vec<Message> = match m["method"].as_str().unwrap() {
			"Target.setDiscoverTargets" => vec![
				answer(json!({})),
				Message::Text(
					json!({ "method": "Target.targetCreated", "params": { "targetInfo": { "targetId": "T1", "type": "page", "title": "Google", "url": "https://business.google.com/" } } })
						.to_string()
						.into(),
				),
			],
			"Target.attachToTarget" => vec![answer(json!({ "sessionId": "S1" }))],
			"Page.startScreencast" => vec![
				answer(json!({})),
				Message::Text(
					json!({ "sessionId": "S1", "method": "Page.screencastFrame", "params": { "data": "/9j/frame", "sessionId": 7, "metadata": { "deviceWidth": 412, "deviceHeight": 915 } } })
						.to_string()
						.into(),
				),
			],
			_ => vec![answer(json!({}))],
		};
		for o in out {
			tx.send(o).await.unwrap();
		}
	}
}

/// The viewer's next frame, or how it closed.
async fn next<S: futures::Stream<Item = Result<tokio_tungstenite::tungstenite::Message, tokio_tungstenite::tungstenite::Error>> + Unpin>(ws: &mut S) -> Value {
	use futures::StreamExt;
	use tokio_tungstenite::tungstenite::Message;
	match ws.next().await.unwrap().unwrap() {
		Message::Text(t) => serde_json::from_str::<Value>(&t).unwrap(),
		Message::Close(c) => json!({ "closed": u16::from(c.unwrap().code) }),
		m => panic!("{m:?}"),
	}
}

#[tokio::test]
async fn the_viewer_sees_the_page_and_touches_it_as_a_phone_one_viewer_at_a_time() {
	use futures::{SinkExt, StreamExt};
	use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

	let told: Arc<Mutex<Vec<Value>>> = Arc::default();
	let cdp_told = told.clone();
	let runner = runner(Arc::new(Mutex::new(vec![1]))).route(
		"/profiles/{gmail}/cdp",
		get(move |ws: axum::extract::WebSocketUpgrade| {
			let told = cdp_told.clone();
			async move { ws.on_upgrade(move |s| devtools(s, told)) }
		}),
	);
	let mut w = World::with_runner(runner).await;
	let a = w.gmail("a@gmail.com").await;
	let (status, set) = w.set(a, "pixel_7", &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let conductor = serve(w.app.clone()).await;
	let open = || {
		let mut req = format!("{}/api/v1/gmails/{a}/browser", conductor.replace("http", "ws")).into_client_request().unwrap();
		req.headers_mut().insert("origin", ORIGIN.parse().unwrap());
		req.headers_mut().insert("cookie", w.b.cookie().parse().unwrap());
		tokio_tungstenite::connect_async(req)
	};
	let (mut first, _) = open().await.unwrap();
	let mut seen = Vec::new();
	while !seen.iter().any(|m: &Value| m["type"] == "frame") {
		seen.push(next(&mut first).await);
	}
	let frame = seen.last().unwrap();
	assert_eq!((&frame["data"], &frame["width"], &frame["height"]), (&json!("/9j/frame"), &json!(412.0), &json!(915.0)));
	assert!(seen.iter().any(|m| m["type"] == "tabs" && m["active"] == "T1"), "{seen:?}");

	first
		.send(Message::text(json!({ "type": "touch", "phase": "start", "x": 100.0, "y": 200.0 }).to_string()))
		.await
		.unwrap();
	first.send(Message::text(json!({ "type": "text", "text": "Plombier Lyon 7" }).to_string())).await.unwrap();
	tokio::time::sleep(std::time::Duration::from_millis(300)).await;
	let methods: Vec<Value> = told.lock().unwrap().iter().map(|m| json!([m["method"], m["sessionId"]])).collect();
	for want in [
		json!(["Page.screencastFrameAck", "S1"]),
		json!(["Input.dispatchTouchEvent", "S1"]),
		json!(["Input.insertText", "S1"]),
	] {
		assert!(methods.contains(&want), "{want} in {methods:?}");
	}
	assert!(!methods.iter().any(|m| m[0] == "Runtime.enable"), "what pages detect devtools by: {methods:?}");

	let (_second, _) = open().await.unwrap();
	let mut last = next(&mut first).await;
	while last.get("closed").is_none() {
		last = next(&mut first).await;
	}
	assert_eq!(last["closed"], sa_server::browser::CLOSE_REPLACED, "one viewer per gmail");
}

#[tokio::test]
async fn a_cloud_phone_is_made_at_its_set_up_its_model_drawn_its_proxy_replaced_and_deleted_with_its_film() {
	let mut w = World::new().await;
	let a = w.gmail("a@gmail.com").await;
	let gbp = w.gbp(a, "Plombier Lyon 7", "FR").await;
	let (status, refused) = w.set_as(a, "multilogin", Some("pixel_7"), &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "a phone's model is drawn, never named: {refused}");
	let (status, set) = w.set_as(a, "multilogin", None, &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	assert_eq!(set["browser"]["device"], "Google Pixel 7", "Google or Samsung only: {set}");
	let asked = w.multilogin.asked();
	let create = &asked.iter().find(|(p, _)| p == "/mobile_profiles/phone/create").unwrap().1;
	assert_eq!(create["data"][0]["proxy_information"], w.proxy("fr"), "{create}");
	assert_eq!((&create["mobile_type"], &create["data"][0]["profile_name"]), (&json!("Android 14"), &json!("a@gmail.com")));

	let (status, replaced) = w.set_as(a, "multilogin", None, &w.proxy("fr-2")).await;
	assert_eq!(status, StatusCode::OK, "{replaced}");
	assert_eq!(replaced["browser"]["device"], "Google Pixel 7", "the model stays");
	let asked = w.multilogin.asked();
	assert_eq!(asked.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(), ["/mobile_profiles/phone/update"]);
	assert_eq!((&asked[0].1["id"], &asked[0].1["proxy_config"]["username"]), (&json!("ph-1"), &json!("fr-2")));

	let (status, body) = w.call(Method::POST, &format!("/api/v1/gmails/{a}/browser/start"), Some(json!({ "gmb": gbp }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
	assert_eq!(w.multilogin.paths(), ["/workspace/automation_token"], "a run's own token, nothing else");
	assert_eq!(
		w.launches()[0]["browser"],
		json!({ "provider": "multilogin_phone", "token": "three-hour-token", "phone": "ph-1", "geo": [45.75, 4.85], "zone": "Europe/Paris", "country": "FR" })
	);

	assert_eq!(w.film(a, vec![7u8; 1 << 20]).await, StatusCode::NO_CONTENT);
	assert_eq!(w.film(a, vec![8u8; 1 << 20]).await, StatusCode::NO_CONTENT);
	let asked = w.multilogin.asked();
	assert_eq!(
		asked.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
		["/mobile_profiles/files/upload", "/mobile_profiles/files/upload", "/mobile_profiles/files/delete"],
		"the one before leaves the Library"
	);
	assert_eq!(asked[2].1["ids"], json!(["film-0"]));
	assert!(
		w.runner.lock().unwrap().iter().all(|s| !s.path_and_query.ends_with("/film")),
		"a phone's film is not the runner's"
	);
	let (_, connections) = w.call(Method::GET, &format!("/api/v1/gmails/{a}/connections"), None).await;
	assert_eq!(connections["browser"]["film"], true, "{connections}");

	let (status, _) = w.call(Method::DELETE, &format!("/api/v1/gmails/{a}/browser"), None).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	let asked = w.multilogin.asked();
	assert_eq!(
		asked.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
		["/mobile_profiles/files/delete", "/mobile_profiles/phone/delete"]
	);
	assert_eq!((&asked[0].1["ids"], &asked[1].1["ids"]), (&json!(["film-1"]), &json!(["ph-1"])));
}

#[tokio::test]
async fn multilogins_browser_is_made_in_the_conductors_folder_and_told_the_place_at_each_start() {
	let mut w = World::new().await;
	let a = w.gmail("a@gmail.com").await;
	let gbp = w.gbp(a, "Plombier Lyon 7", "FR").await;
	let (status, set) = w.set_as(a, "multilogin_browser", Some("pixel_7"), &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let asked = w.multilogin.asked();
	assert_eq!(
		asked.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
		["/workspace/folders", "/workspace/folder_create", "/profile/create"]
	);
	let made = &asked[2].1;
	assert_eq!(
		(&made["folder_id"], &made["os_type"], &made["browser_type"]),
		(&json!("f-sc"), &json!("android"), &json!("mimic"))
	);
	assert_eq!(made["parameters"]["fingerprint"]["graphic"]["renderer"], "Mali-G710", "the device's GPU");
	assert_eq!(made["parameters"]["flags"]["navigator_masking"], "mask", "a custom navigator breaks UA-CH");

	let (status, body) = w.call(Method::POST, &format!("/api/v1/gmails/{a}/browser/start"), Some(json!({ "gmb": gbp }))).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
	let asked = w.multilogin.asked();
	assert_eq!(
		asked.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>(),
		["/profile/update", "/workspace/automation_token"],
		"the folder known"
	);
	let fp = &asked[0].1["parameters"]["fingerprint"];
	assert_eq!((&fp["geolocation"]["latitude"], &fp["timezone"]["zone"]), (&json!(45.75), &json!("Europe/Paris")), "{fp}");
	assert_eq!(asked[0].1["profile_id"], "mimic-1");
	assert_eq!(
		w.launches()[0]["browser"],
		json!({ "provider": "multilogin_browser", "token": "three-hour-token", "folder": "f-sc", "profile": "mimic-1" })
	);
	assert_eq!(w.film(a, vec![7u8; 16]).await, StatusCode::CONFLICT, "Mimic plays no film");
}

#[tokio::test]
async fn a_phones_viewer_is_the_runners_its_frames_checked_and_passed_both_ways() {
	use futures::{SinkExt, StreamExt};
	use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

	let heard: Arc<Mutex<Vec<String>>> = Arc::default();
	let view_heard = heard.clone();
	let runner = runner(Arc::new(Mutex::new(vec![1]))).route(
		"/profiles/{gmail}/view",
		get(move |ws: axum::extract::WebSocketUpgrade| {
			let heard = view_heard.clone();
			async move {
				ws.on_upgrade(move |mut s| async move {
					use axum::extract::ws::Message as M;
					s.send(M::Text(json!({ "type": "frame", "data": "/9j/phone", "width": 1080.0, "height": 2400.0 }).to_string().into()))
						.await
						.unwrap();
					while let Some(Ok(M::Text(t))) = s.recv().await {
						heard.lock().unwrap().push(t.to_string());
					}
				})
			}
		}),
	);
	let mut w = World::with_runner(runner).await;
	let a = w.gmail("a@gmail.com").await;
	let (status, set) = w.set_as(a, "multilogin", None, &w.proxy("fr")).await;
	assert_eq!(status, StatusCode::OK, "{set}");
	let conductor = serve(w.app.clone()).await;
	let mut req = format!("{}/api/v1/gmails/{a}/browser", conductor.replace("http", "ws")).into_client_request().unwrap();
	req.headers_mut().insert("origin", ORIGIN.parse().unwrap());
	req.headers_mut().insert("cookie", w.b.cookie().parse().unwrap());
	let (mut viewer, _) = tokio_tungstenite::connect_async(req).await.unwrap();
	assert_eq!(next(&mut viewer).await, json!({ "type": "frame", "data": "/9j/phone", "width": 1080.0, "height": 2400.0 }));
	let touch = json!({ "type": "touch", "phase": "start", "x": 540.0, "y": 1200.0 });
	viewer.send(Message::text(touch.to_string())).await.unwrap();
	viewer.send(Message::text(json!({ "type": "shell", "line": "reboot" }).to_string())).await.unwrap();
	assert_eq!(next(&mut viewer).await, json!({ "closed": 1007 }), "not a frame of the viewer's");
	let heard: Vec<Value> = heard.lock().unwrap().iter().map(|t| serde_json::from_str(t).unwrap()).collect();
	assert_eq!(heard, [touch], "only the viewer's frames reach the runner");
}
