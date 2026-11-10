//! The runner as the conductor drives it, stand-ins in the browsers' and phones' places (Orbita,
//! the Multilogin app, Multilogin's API, adb): a launch is up once its devtools (a phone: adb)
//! answer, one run per gmail, bounded, stopped with what it brought scrubbed, and nobody but the
//! conductor heard. A phone is recorded before it starts and stopped on every way its run ends.

use std::{
	collections::HashMap,
	path::{Path, PathBuf},
	process::Stdio,
	sync::{Arc, Mutex},
	time::Duration,
};

use axum::{Json, extract::State, routing::post};
use futures::{SinkExt, StreamExt};
use sa_auth::{Assertion, Principal, Service, Signer};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};

const SEED: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

struct Runner {
	base: String,
	dir: tempfile::TempDir,
	signer: Signer,
	http: reqwest::Client,
	child: tokio::process::Child,
	multilogin: Multilogin,
}

impl Runner {
	/// At most `max` browsers; the stand-in writes its devtools port and sleeps, as Orbita runs.
	async fn start(max: usize) -> Self {
		Self::with(max, &[]).await
	}

	/// `left` recorded as running phones before it starts, as a runner that died would leave them.
	async fn with(max: usize, left: &[&str]) -> Self {
		let dir = tempfile::tempdir().unwrap();
		let orbita = script(
			dir.path(),
			"orbita",
			"echo \"$@\" > args\nprintf '9\\n/devtools/browser/stand-in\\n' > DevToolsActivePort\nexec sleep 600",
		);
		let mlx = script(dir.path(), "mlx", "exec sleep 600");
		let adb = stand_in_adb(dir.path());
		let sessions = dir.path().join("sessions");
		std::fs::create_dir_all(&sessions).unwrap();
		let multilogin = Multilogin::serve(sessions.clone()).await;
		if !left.is_empty() {
			let records: Vec<Value> = left.iter().map(|p| json!({ "phone": p, "token": "run-token" })).collect();
			std::fs::write(sessions.join("phones.json"), serde_json::to_vec(&records).unwrap()).unwrap();
			for p in left {
				multilogin.0.lock().unwrap().status.insert((*p).to_owned(), 0);
			}
		}
		let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
		let signer: Signer = SEED.parse().unwrap();
		let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sa_browsers"))
			.arg("serve")
			.env("BROWSERS_BIND", format!("127.0.0.1:{port}"))
			.env("BROWSERS_DIR", &sessions)
			.env("BROWSERS_MAX", max.to_string())
			.env("BROWSERS_ORBITA", &orbita)
			.env("BROWSERS_FFMPEG", ffmpeg())
			.env("BROWSERS_MLX", &mlx)
			.env("BROWSERS_MLX_LAUNCHER", "http://127.0.0.1:9")
			.env("BROWSERS_MULTILOGIN_API", &multilogin.1)
			.env("BROWSERS_ADB", &adb)
			.env("BROWSERS_BUSYBOX", &orbita)
			.env("BROWSERS_ADB_KEYBOARD", &orbita)
			.env("PANEL_ASSERTION_KEYS", signer.public())
			.stdout(Stdio::null())
			.kill_on_drop(true)
			.spawn()
			.unwrap();
		let r = Self {
			base: format!("http://127.0.0.1:{port}"),
			dir,
			signer,
			http: reqwest::Client::new(),
			child,
			multilogin,
		};
		for _ in 0..100 {
			if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
				return r;
			}
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
		panic!("the runner never listened");
	}

	fn token(&self, method: &str, path: &str) -> String {
		self.signer.sign(&Assertion {
			aud: Service::Browsers,
			principal: Principal::Conductor,
			method: method.to_owned(),
			path: path.to_owned(),
			exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
		})
	}

	async fn call(&self, method: reqwest::Method, path: &str, body: Option<Value>, signed: bool) -> (u16, String) {
		let mut req = self.http.request(method.clone(), format!("{}{path}", self.base));
		if signed {
			req = req.header(sa_auth::HEADER, self.token(method.as_str(), path));
		}
		if let Some(b) = body {
			req = req.json(&b);
		}
		let res = req.send().await.unwrap();
		(res.status().as_u16(), res.text().await.unwrap())
	}

	async fn launch(&self, gmail: i64, run: &str) -> (u16, String) {
		let launch = json!({ "run": run, "browser": { "provider": "orbita",
			"preferences": { "gologin": { "proxy": { "username": "u", "password": "secret-of-u" }, "profile_token": "three-minute-token", "name": "a@gmail.com" } },
			"config": { "gologin": { "profile_token": "three-minute-token" } },
			"proxy": "http://203.0.113.9:8080", "zone": "Europe/Paris", "locale": "fr-FR", "screen": [412, 915] } });
		self.call(reqwest::Method::POST, &format!("/profiles/{gmail}/launch"), Some(launch), true).await
	}

	async fn running(&self) -> Vec<Value> {
		let (status, body) = self.call(reqwest::Method::GET, "/profiles", None, true).await;
		assert_eq!(status, 200, "{body}");
		serde_json::from_str::<Value>(&body).unwrap()["running"].as_array().unwrap().clone()
	}

	async fn launch_phone(&self, gmail: i64, phone: &str) -> (u16, String) {
		let launch = json!({ "run": "gmb:7", "browser": { "provider": "multilogin_phone", "token": "run-token", "phone": phone,
			"geo": [45.764, 4.8357], "zone": "Europe/Paris", "country": "FR" } });
		self.call(reqwest::Method::POST, &format!("/profiles/{gmail}/launch"), Some(launch), true).await
	}

	fn records(&self) -> String {
		std::fs::read_to_string(self.dir.path().join("sessions/phones.json")).unwrap_or_default()
	}

	fn adb_log(&self) -> String {
		std::fs::read_to_string(self.dir.path().join("adb.log")).unwrap_or_default()
	}

	async fn viewer(&self, gmail: i64) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
		let path = format!("/profiles/{gmail}/view");
		let mut req = format!("{}{path}", self.base.replacen("http", "ws", 1)).into_client_request().unwrap();
		req.headers_mut().insert(sa_auth::HEADER, self.token("GET", &path).parse().unwrap());
		tokio_tungstenite::connect_async(req).await.unwrap().0
	}
}

#[tokio::test]
async fn one_run_per_gmail_bounded_stopped_and_scrubbed() {
	let r = Runner::start(1).await;
	assert_eq!(r.launch(1, "Plombier Lyon 7").await.0, 204);
	assert_eq!(r.running().await, [json!({ "gmail": 1, "run": "Plombier Lyon 7", "stream": null })]);
	assert_eq!(r.launch(1, "Plombier Lyon 7").await.0, 204, "the same run: up already");
	let (status, refused) = r.launch(1, "the gmail alone").await;
	assert_eq!((status, refused.contains("Plombier Lyon 7")), (409, true), "another run, never folded in: {refused}");
	assert_eq!(r.launch(2, "Plombier Lyon 8").await.0, 503, "BROWSERS_MAX");

	let session = r.dir.path().join("sessions/1");
	assert!(session.join("orbita.config").exists(), "read once at start");
	assert_eq!(r.call(reqwest::Method::POST, "/profiles/1/stop", None, true).await.0, 204);
	assert_eq!(r.running().await, Vec::<Value>::new());
	assert!(!session.join("orbita.config").exists(), "the profile token goes with the run");
	let prefs = std::fs::read_to_string(session.join("Default/Preferences")).unwrap();
	assert!(!prefs.contains("secret-of-u"), "{prefs}");
	assert!(!prefs.contains("three-minute-token"), "{prefs}");
	assert!(prefs.contains("a@gmail.com"), "the rest of the session stays: {prefs}");

	assert_eq!(r.launch(2, "Plombier Lyon 8").await.0, 204, "room again");
	assert_eq!(r.call(reqwest::Method::DELETE, "/profiles/2", None, true).await.0, 204);
	assert!(!r.dir.path().join("sessions/2").exists(), "its session gone");
}

/// The machine's own: the film is cut by it.
fn ffmpeg() -> String {
	let found = std::process::Command::new("sh").args(["-c", "command -v ffmpeg"]).output().unwrap();
	assert!(found.status.success(), "ffmpeg on PATH: the runner's films need it");
	String::from_utf8(found.stdout).unwrap().trim().to_owned()
}

/// A second of film: a test pattern, with or without its sound.
fn film(dir: &Path, sound: bool) -> Vec<u8> {
	let out = dir.join(if sound { "with.mp4" } else { "without.mp4" });
	let mut args = vec!["-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=320x240:rate=30:duration=1"];
	if sound {
		args.extend(["-f", "lavfi", "-i", "sine=duration=1"]);
	}
	let done = std::process::Command::new(ffmpeg()).args(args).arg(&out).output().unwrap();
	assert!(done.status.success(), "{}", String::from_utf8_lossy(&done.stderr));
	std::fs::read(out).unwrap()
}

#[tokio::test]
async fn a_film_is_the_next_launchs_camera_and_its_sound_the_mic() {
	let r = Runner::start(1).await;
	let put = |body: Vec<u8>| {
		r.http
			.put(format!("{}/profiles/1/film", r.base))
			.header(sa_auth::HEADER, r.token("PUT", "/profiles/1/film"))
			.body(body)
			.send()
	};
	let refused = put(film(r.dir.path(), false)).await.unwrap();
	assert_eq!(refused.status().as_u16(), 400, "a phone's mic is never silent: {}", refused.text().await.unwrap());
	assert_eq!(put(film(r.dir.path(), true)).await.unwrap().status().as_u16(), 204);
	let (_, listed) = r.call(reqwest::Method::GET, "/profiles", None, true).await;
	assert_eq!(serde_json::from_str::<Value>(&listed).unwrap()["films"], json!([1]));

	assert_eq!(r.launch(1, "Plombier Lyon 7").await.0, 204);
	let args = std::fs::read_to_string(r.dir.path().join("sessions/1/args")).unwrap();
	assert!(args.contains("--use-file-for-fake-video-capture=camera 0, facing back.mjpeg"), "{args}");
	assert!(args.contains("--use-file-for-fake-audio-capture=film.wav"), "{args}");
	for _ in 0..50 {
		if r.running().await.is_empty() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(100)).await;
	}
	panic!("a browser whose facing.js cannot attach runs on: its pages would sit paused");
}

#[tokio::test]
async fn nobody_but_the_conductor_is_heard() {
	let r = Runner::start(1).await;
	assert_eq!(r.call(reqwest::Method::GET, "/profiles", None, false).await.0, 401);
	let other: Signer = "k1:AQECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".parse().unwrap();
	let token = other.sign(&Assertion {
		aud: Service::Browsers,
		principal: Principal::Conductor,
		method: "GET".to_owned(),
		path: "/profiles".to_owned(),
		exp: jiff::Timestamp::now().as_second() + sa_auth::TTL,
	});
	let res = r.http.get(format!("{}/profiles", r.base)).header(sa_auth::HEADER, token).send().await.unwrap();
	assert_eq!(res.status().as_u16(), 401, "another key");
	let (status, _) = r.call(reqwest::Method::GET, "/profiles/1/cdp", None, true).await;
	assert_eq!(status, 404, "nothing runs: no devtools to pass");
}

/// An executable `name` in `dir`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
	let path = dir.join(name);
	std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
	std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
	path
}

/// adb as a cloud phone answers it: every call logged to `adb.log`; the exit's country is
/// `country`'s, FR without it.
fn stand_in_adb(dir: &Path) -> PathBuf {
	let png = dir.join("screen.png");
	let made = std::process::Command::new(ffmpeg())
		.args(["-loglevel", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=108x240", "-frames:v", "1"])
		.arg(&png)
		.output()
		.unwrap();
	assert!(made.status.success(), "{}", String::from_utf8_lossy(&made.stderr));
	let d = dir.display();
	script(
		dir,
		"adb",
		&format!(
			r#"echo "$*" >> {d}/adb.log
case "$*" in
  connect*) echo "connected to $2" ;;
  *exec-out*screencap*) cat {d}/screen.png ;;
  *glogin*"'new'"*) echo "glogin success" ;;
  *glogin*) echo "glogin failed:password error" ;;
  *"'wm' 'size'"*) echo "Physical size: 1080x2400" ;;
  *persist.sys.timezone*) echo Europe/Paris ;;
  *wget*) echo "{{\"countryCode\":\"$(cat {d}/country 2>/dev/null || echo FR)\",\"query\":\"92.184.1.1\",\"mobile\":true}}" ;;
  *"'pm' 'list'"*) echo package:com.android.adbkeyboard ;;
esac"#
		),
	)
}

/// Multilogin's API as the runner calls it: its calls in order, each phone's status.
#[derive(Default)]
struct Calls {
	calls: Vec<(String, Value)>,
	status: HashMap<String, u64>,
	/// At each `phone/start`: whether the phone was on the runner's records then.
	recorded_at_start: Vec<bool>,
}

struct Multilogin(Arc<Mutex<Calls>>, String);

impl Multilogin {
	async fn serve(records: PathBuf) -> Self {
		let calls = Arc::new(Mutex::new(Calls::default()));
		let ok = |data: Value| Json(json!({ "data": data, "status": { "error_code": "", "http_code": 200, "message": "" } }));
		let app = axum::Router::new()
			.route(
				"/mobile_profiles/phone/{op}",
				post(
					move |State((calls, records)): State<(Arc<Mutex<Calls>>, PathBuf)>, axum::extract::Path(op): axum::extract::Path<String>, Json(body): Json<Value>| async move {
						let mut c = calls.lock().unwrap();
						c.calls.push((op.clone(), body.clone()));
						let id = body["ids"][0].as_str().unwrap_or_default().to_owned();
						match op.as_str() {
							"start" => {
								let recorded = std::fs::read_to_string(records.join("phones.json")).is_ok_and(|r| r.contains(&id));
								c.recorded_at_start.push(recorded);
								c.status.insert(id.clone(), 0);
								ok(json!({ "fail_amount": 0, "success_details": [{ "id": id, "url": format!("https://stream.example/{id}") }] }))
							}
							"stop" => {
								c.status.insert(id, 2);
								ok(json!({ "fail_amount": 0 }))
							}
							"statuses" => ok(json!({ "fail_amount": 0, "success_details": [{ "id": id, "status": c.status.get(&id).copied().unwrap_or(2) }] })),
							"adb" => unreachable!(),
							_ => ok(json!({ "fail_amount": 0 })),
						}
					},
				),
			)
			.route(
				"/mobile_profiles/phone/adb/{op}",
				post(
					move |State((calls, _)): State<(Arc<Mutex<Calls>>, PathBuf)>, axum::extract::Path(op): axum::extract::Path<String>, Json(body): Json<Value>| async move {
						// as adb left on by the run before: active at once, its first password already replaced
						let mut c = calls.lock().unwrap();
						c.calls.push((format!("adb/{op}"), body));
						let pwd = if c.calls.iter().filter(|(o, _)| o == "adb/info").count() == 1 { "old" } else { "new" };
						ok(json!({ "fail_amount": 0, "items": [{ "status": "active", "ip": "127.0.0.1", "port": "5555", "pwd": pwd }] }))
					},
				),
			)
			.with_state((calls.clone(), records));
		let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
		let base = format!("http://{}", listener.local_addr().unwrap());
		tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
		Self(calls, base)
	}

	fn ops(&self) -> Vec<String> {
		self.0.lock().unwrap().calls.iter().map(|(op, _)| op.clone()).collect()
	}

	fn status(&self, phone: &str) -> Option<u64> {
		self.0.lock().unwrap().status.get(phone).copied()
	}
}

/// The next frame of `type` from the viewer's socket.
async fn next_of(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, kind: &str) -> Value {
	loop {
		let m = tokio::time::timeout(Duration::from_secs(10), ws.next()).await.expect("a frame within 10 s").unwrap().unwrap();
		if let tungstenite::Message::Text(t) = m {
			let v: Value = serde_json::from_str(&t).unwrap();
			if v["type"] == kind {
				return v;
			}
		}
	}
}

async fn eventually(what: &str, mut done: impl FnMut() -> bool) {
	for _ in 0..100 {
		if done() {
			return;
		}
		tokio::time::sleep(Duration::from_millis(100)).await;
	}
	panic!("never: {what}");
}

// r[verify browsers.phone-stops]
#[tokio::test]
async fn a_phone_is_recorded_before_it_starts_driven_and_stopped_confirmed() {
	let r = Runner::start(1).await;
	assert_eq!(r.launch_phone(1, "p1").await, (204, String::new()));
	assert_eq!(r.multilogin.0.lock().unwrap().recorded_at_start, [true], "on disk before it bills");
	assert_eq!(r.running().await, [json!({ "gmail": 1, "run": "gmb:7", "stream": "https://stream.example/p1" })]);
	let log = r.adb_log();
	for placed in [
		"'set-test-provider-location' 'gps' '--location' '45.764,4.8357'",
		"'cmd' 'alarm' 'set-timezone' 'Europe/Paris'",
		"'ime' 'set' 'com.android.adbkeyboard/.AdbIME'",
		"'wget'",
	] {
		assert!(log.contains(placed), "{placed} in {log}");
	}

	let mut ws = r.viewer(1).await;
	assert_eq!(next_of(&mut ws, "tabs").await["tabs"], json!([]), "a phone has none");
	let frame = next_of(&mut ws, "frame").await;
	assert_eq!((frame["width"].as_f64(), frame["height"].as_f64()), (Some(1080.0), Some(2400.0)), "points in the screen's px");
	for input in [
		json!({ "type": "touch", "phase": "start", "x": 100.4, "y": 200.0 }),
		json!({ "type": "touch", "phase": "end", "x": 100.4, "y": 200.0 }),
		json!({ "type": "touch", "phase": "start", "x": 500, "y": 1800 }),
		json!({ "type": "touch", "phase": "move", "x": 500, "y": 600 }),
		json!({ "type": "touch", "phase": "end", "x": 500, "y": 600 }),
		json!({ "type": "text", "text": "Plombier à Lyon 'sûr'" }),
		json!({ "type": "navigate", "url": "business.google.com" }),
		json!({ "type": "history", "delta": -1 }),
	] {
		ws.send(tungstenite::Message::text(input.to_string())).await.unwrap();
	}
	let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "Plombier à Lyon 'sûr'");
	for driven in [
		"'input' 'tap' '100' '200'".to_owned(),
		"'input' 'swipe' '500' '1800' '500' '600'".to_owned(),
		format!("'ADB_INPUT_B64' '--es' 'msg' '{b64}'"),
		"'-d' 'https://business.google.com/'".to_owned(),
		"'input' 'keyevent' '4'".to_owned(),
	] {
		eventually(&driven, || r.adb_log().contains(&driven)).await;
	}

	assert_eq!(r.call(reqwest::Method::POST, "/profiles/1/stop", None, true).await.0, 204);
	assert_eq!(r.multilogin.status("p1"), Some(2));
	assert!(
		r.multilogin.ops().ends_with(&["stop".to_owned(), "statuses".to_owned()]),
		"read stopped after: {:?}",
		r.multilogin.ops()
	);
	assert!(!r.records().contains("p1"), "forgotten once stopped: {}", r.records());
	assert_eq!(r.running().await, Vec::<Value>::new());
}

// r[verify browsers.phone-stops]
#[tokio::test]
async fn a_phone_whose_exit_is_elsewhere_is_stopped() {
	let r = Runner::start(1).await;
	std::fs::write(r.dir.path().join("country"), "DE").unwrap();
	let (status, why) = r.launch_phone(1, "p1").await;
	assert_eq!((status, why.contains("not FR")), (502, true), "{why}");
	assert_eq!(r.multilogin.status("p1"), Some(2));
	assert!(!r.records().contains("p1"), "{}", r.records());
}

// r[verify browsers.phone-stops]
#[tokio::test]
async fn sigterm_stops_every_phone_before_the_runner_exits() {
	let mut r = Runner::start(2).await;
	assert_eq!(r.launch_phone(1, "p1").await.0, 204);
	assert_eq!(r.launch_phone(2, "p2").await.0, 204);
	let pid = rustix::process::Pid::from_raw(r.child.id().unwrap().try_into().unwrap()).unwrap();
	rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();
	let exited = tokio::time::timeout(Duration::from_secs(20), r.child.wait()).await.expect("it exits").unwrap();
	assert!(exited.success(), "{exited}");
	assert_eq!((r.multilogin.status("p1"), r.multilogin.status("p2")), (Some(2), Some(2)));
	assert_eq!(serde_json::from_str::<Value>(&r.records()).unwrap(), json!([]));
}

// r[verify browsers.phone-stops]
#[tokio::test]
async fn a_phone_left_running_is_stopped_before_the_runner_serves() {
	let r = Runner::with(1, &["p9"]).await;
	assert_eq!(r.multilogin.status("p9"), Some(2), "stopped by the time it listens");
	assert!(!r.multilogin.ops().contains(&"start".to_owned()));
	assert_eq!(serde_json::from_str::<Value>(&r.records()).unwrap(), json!([]));
}
