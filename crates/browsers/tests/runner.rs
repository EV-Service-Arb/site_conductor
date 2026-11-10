//! The runner as the conductor drives it, a stand-in browser in Orbita's place: a launch is up
//! once its devtools answer, one run per gmail, bounded, stopped with what it brought scrubbed,
//! and nobody but the conductor heard.

use std::{process::Stdio, time::Duration};

use sa_auth::{Assertion, Principal, Service, Signer};
use serde_json::{Value, json};

const SEED: &str = "k1:AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

struct Runner {
	base: String,
	dir: tempfile::TempDir,
	signer: Signer,
	http: reqwest::Client,
	_child: tokio::process::Child,
}

impl Runner {
	/// At most `max` browsers; the stand-in writes its devtools port and sleeps, as Orbita runs.
	async fn start(max: usize) -> Self {
		let dir = tempfile::tempdir().unwrap();
		let orbita = dir.path().join("orbita");
		std::fs::write(
			&orbita,
			"#!/bin/sh\necho \"$@\" > args\nprintf '9\\n/devtools/browser/stand-in\\n' > DevToolsActivePort\nexec sleep 600\n",
		)
		.unwrap();
		std::fs::set_permissions(&orbita, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
		let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
		let signer: Signer = SEED.parse().unwrap();
		let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_sa_browsers"))
			.arg("serve")
			.env("BROWSERS_BIND", format!("127.0.0.1:{port}"))
			.env("BROWSERS_DIR", dir.path().join("sessions"))
			.env("BROWSERS_MAX", max.to_string())
			.env("BROWSERS_ORBITA", &orbita)
			.env("BROWSERS_FFMPEG", ffmpeg())
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
			_child: child,
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
}

#[tokio::test]
async fn one_run_per_gmail_bounded_stopped_and_scrubbed() {
	let r = Runner::start(1).await;
	assert_eq!(r.launch(1, "Plombier Lyon 7").await.0, 204);
	assert_eq!(r.running().await, [json!({ "gmail": 1, "run": "Plombier Lyon 7" })]);
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
fn film(dir: &std::path::Path, sound: bool) -> Vec<u8> {
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
