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
			get(move || async move { Json(json!({ "running": listed.lock().unwrap().iter().map(|g| json!({ "gmail": g, "run": "x" })).collect::<Vec<_>>(), "films": [] })) }),
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
		let browsers = Browsers::stubbed(&gologin_base, "http://ip-api.test/json/");
		let connected = Connected::try_new(browsers, &runner_base, SEED.parse().unwrap()).unwrap();
		let app = http::app(SignIn::new(panel, Concierge::dev(who), config), None, None, Default::default(), None, None, Some(connected));
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self {
			app,
			b,
			gologin: stub,
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

	/// A draft of `gmail`'s in `country`, at a point in Lyon.
	async fn gbp(&mut self, gmail: i64, name: &str, country: &str) -> i64 {
		let (status, draft) = self.call(Method::POST, "/api/v1/gmbs", Some(json!({ "name": name }))).await;
		assert_eq!(status, StatusCode::CREATED, "{draft}");
		let id = draft["id"].as_i64().unwrap();
		let (status, moved) = self.call(Method::PATCH, &format!("/api/v1/gmbs/{id}"), Some(json!({ "gmail_id": gmail }))).await;
		assert_eq!(status, StatusCode::OK, "{moved}");
		let (status, _) = self.call(Method::PUT, &format!("/api/v1/gmbs/{id}/country"), Some(json!({ "country": country }))).await;
		assert_eq!(status, StatusCode::NO_CONTENT);
		let (status, _) = self.call(Method::PUT, &format!("/api/v1/gmbs/{id}/location"), Some(json!({ "lat": 45.75, "lng": 4.85 }))).await;
		assert_eq!(status, StatusCode::NO_CONTENT);
		id
	}

	/// `user`'s proxy through ip-api's stub.
	fn proxy(&self, user: &str) -> String {
		format!("http://{user}:secret-of-{user}@{}", self.proxy)
	}

	async fn set(&mut self, gmail: i64, device: &str, proxy: &str) -> (StatusCode, Value) {
		self.call(
			Method::PUT,
			&format!("/api/v1/gmails/{gmail}/browser"),
			Some(json!({ "provider": "gologin", "device": device, "proxy": proxy })),
		)
		.await
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
	assert_eq!(connections["browser"]["running"], true, "{connections}");
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
