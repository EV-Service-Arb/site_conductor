//! A Multilogin cloud phone's run: billed per minute from `phone/start` until it reads stopped,
//! so the runner, not the conductor, owns its life (a dead conductor leaves none running).
//!
//! ```text
//! recorded ─▶ phone/start ─▶ started ─▶ adb + glogin ─▶ geo, zone, exit from inside ─▶ Up ─▶ ends
//!    │             └──────────────── any step failing ───────────────────┐                 │
//!    └── forgotten ◀── statuses reads stopped ◀── phone/stop ◀────────────┴─────────────────┘
//! ```
//! Ends: told to, the gmail removed, [`IDLE`] without a viewer, [`CAP`] since its start, the
//! runner stopping. A stop is retried every [`RETRY`] until it is confirmed.

use std::{path::Path, sync::Arc, time::Duration};

use eyre::WrapErr;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
	Runner, adb,
	profile::{Ends, Up, up},
};

/// Without a viewer.
const IDLE: Duration = Duration::from_secs(3 * 60);
/// Since its start, whoever watches.
const CAP: Duration = Duration::from_secs(30 * 60);
const STARTED_WITHIN: Duration = Duration::from_secs(180);
const ADB_WITHIN: Duration = Duration::from_secs(120);
const STOPPED_WITHIN: Duration = Duration::from_secs(120);
const RETRY: Duration = Duration::from_secs(30);
const RECORDS: &str = "phones.json";
const START: &str = "/mobile_profiles/phone/start";
const BUSYBOX_AT: &str = "/data/local/tmp/busybox";

// phone/statuses' `status`
const STARTED: u64 = 0;
const SHUT_DOWN: u64 = 2;
const EXPIRED: u64 = 3;

/// What alone opens `phone/start`: minted once, by `serve`.
pub(crate) struct MobileMinutes(());

impl MobileMinutes {
	// r[impl browsers.mobile-minutes]
	pub(crate) fn of_serve() -> Self {
		Self(())
	}
}

pub(crate) struct Run {
	pub(crate) token: String,
	pub(crate) phone: String,
	pub(crate) geo: Option<(f64, f64)>,
	pub(crate) zone: String,
	pub(crate) country: String,
}

/// A phone this runner may have started: on disk from before `phone/start` until it reads stopped.
#[derive(Clone, Deserialize, Serialize)]
struct Record {
	phone: String,
	/// What stops it: a run token, 3 h.
	token: String,
}

// r[impl browsers.phone-stops]
pub(crate) async fn life(runner: &Runner, gmail: i64, run: Run, ends: &mut Ends<'_>) -> eyre::Result<()> {
	let record = Record {
		phone: run.phone.clone(),
		token: run.token.clone(),
	};
	remember(runner, &record)?;
	let ran = started(runner, gmail, &run, ends).await;
	if let Err(e) = &ran {
		tracing::error!(gmail, phone = run.phone, error = format!("{e:#}"), "phone failed: stopping it");
	}
	stop(runner, &record).await;
	forget(runner, &record.phone)?;
	ran
}

async fn started(runner: &Runner, gmail: i64, run: &Run, ends: &mut Ends<'_>) -> eyre::Result<()> {
	let api = &runner.0.multilogin;
	let at = tokio::time::Instant::now();
	let start = api.start(&run.token, &run.phone, &runner.0.minutes).await?;
	// shortcut: the stream URL's shape is unmeasured until M1; the first `url` in the answer
	let stream = first_url(&start);
	until(&format!("{} to start", run.phone), STARTED_WITHIN, || async {
		match api.status(&run.token, &run.phone).await? {
			EXPIRED => eyre::bail!("{} is expired", run.phone),
			s => Ok((s == STARTED).then_some(())),
		}
	})
	.await?;
	tracing::info!(gmail, phone = run.phone, secs = at.elapsed().as_secs(), "phone started");
	api.call(
		&run.token,
		reqwest::Method::POST,
		"/mobile_profiles/phone/adb/set",
		json!({ "ids": [run.phone], "enabled": true }),
	)
	.await?;
	let info = until(&format!("adb on {}", run.phone), ADB_WITHIN, || async {
		let v = api
			.call(&run.token, reqwest::Method::POST, "/mobile_profiles/phone/adb/info", json!({ "ids": [run.phone] }))
			.await?;
		let i = &v["items"][0];
		let s = |k: &str| i[k].as_str().map(str::to_owned);
		Ok((i["status"] == "active").then(|| (s("ip"), s("port"), s("pwd"))))
	})
	.await?;
	let (Some(ip), Some(port), Some(pwd)) = info else {
		eyre::bail!("adb/info is active for {} without its ip, port or pwd", run.phone);
	};
	let phone = adb::Phone::connect(runner.0.adb.clone(), runner.0.ffmpeg.clone(), format!("{ip}:{port}"), pwd).await?;
	placed(runner, &phone, run).await?;
	up(runner, gmail, Up::Phone { adb: Arc::new(phone), stream });
	tokio::select! {
		() = ends.any(gmail, IDLE) => {}
		() = tokio::time::sleep_until(at + CAP) => tracing::warn!(gmail, phone = run.phone, "phone ran {}s: stopped", CAP.as_secs()),
	}
	Ok(())
}

/// The GBP's point on every location provider, the zone, ADBKeyBoard the IME; then the exit, read
/// from inside, in the GBP's country.
async fn placed(runner: &Runner, phone: &adb::Phone, run: &Run) -> eyre::Result<()> {
	if let Some((lat, lng)) = run.geo {
		phone.sh(&["appops", "set", "com.android.shell", "android:mock_location", "allow"]).await?;
		// latitude first, as Android's set-test-provider-location takes it
		let at = format!("{lat},{lng}");
		for p in ["gps", "network", "fused"] {
			phone.sh(&["cmd", "location", "providers", "add-test-provider", p]).await?;
			phone.sh(&["cmd", "location", "providers", "set-test-provider-enabled", p, "true"]).await?;
			phone.sh(&["cmd", "location", "providers", "set-test-provider-location", p, "--location", &at]).await?;
		}
	}
	phone.sh(&["settings", "put", "global", "auto_time_zone", "0"]).await?;
	phone.sh(&["cmd", "alarm", "set-timezone", &run.zone]).await?;
	let zone = phone.sh(&["getprop", "persist.sys.timezone"]).await?;
	eyre::ensure!(zone == run.zone, "set the zone to {}, and the phone reads back {zone:?}", run.zone);
	if phone.sh(&["pm", "list", "packages", "com.android.adbkeyboard"]).await?.is_empty() {
		phone.install(&runner.0.adb_keyboard).await?;
	}
	// set before a field is focused: switched under a focused one, its view never shows to hear the text
	phone.sh(&["ime", "enable", adb::ADB_KEYBOARD]).await?;
	phone.sh(&["ime", "set", adb::ADB_KEYBOARD]).await?;
	phone.sh(&["settings", "put", "secure", "show_ime_with_hard_keyboard", "1"]).await?;
	phone.push(&runner.0.busybox, BUSYBOX_AT).await?;
	phone.sh(&["chmod", "755", BUSYBOX_AT]).await?;
	let seen = phone.sh(&[BUSYBOX_AT, "wget", "-qO-", "http://ip-api.com/json/?fields=countryCode,query,mobile"]).await?;
	let exit: Value = serde_json::from_str(&seen).wrap_err_with(|| format!("the exit from inside: {seen}"))?;
	eyre::ensure!(exit["countryCode"] == run.country.as_str(), "the phone leaves from {seen}, not {}", run.country);
	tracing::info!(phone = run.phone, exit = seen, zone, fix = ?run.geo, "phone placed");
	Ok(())
}

/// Stopped, and read so: retried every [`RETRY`] until it is, however long.
async fn stop(runner: &Runner, r: &Record) {
	let api = &runner.0.multilogin;
	//LOOP: until the phone reads stopped; each failure reported
	loop {
		let tried = async {
			match api.status(&r.token, &r.phone).await? {
				SHUT_DOWN | EXPIRED => return Ok(()),
				_ => {}
			}
			api.call(&r.token, reqwest::Method::POST, "/mobile_profiles/phone/stop", json!({ "ids": [r.phone] })).await?;
			until(&format!("{} to stop", r.phone), STOPPED_WITHIN, || async {
				Ok(matches!(api.status(&r.token, &r.phone).await?, SHUT_DOWN | EXPIRED).then_some(()))
			})
			.await
		};
		match tried.await {
			Ok(()) => {
				tracing::info!(phone = r.phone, "phone stopped");
				return;
			}
			Err(e) => tracing::error!(
				phone = r.phone,
				error = format!("{e:#}"),
				"a phone not stopped: billed until it is; again in {}s",
				RETRY.as_secs()
			),
		}
		tokio::time::sleep(RETRY).await;
	}
}

/// Every phone recorded and not stopped, stopped, before the runner serves; a record whose token
/// is refused names the phone to stop by hand.
// r[impl browsers.phone-stops]
pub(crate) async fn reconcile(runner: &Runner) -> eyre::Result<()> {
	for r in recorded(&runner.0.dir)? {
		let status = runner.0.multilogin.status(&r.token, &r.phone).await.wrap_err_with(|| {
			format!(
				"phone {} was recorded running, and its status is unreadable: stop it by hand in Multilogin, then remove it from {}",
				r.phone,
				runner.0.dir.join(RECORDS).display()
			)
		})?;
		if !matches!(status, SHUT_DOWN | EXPIRED) {
			tracing::warn!(phone = r.phone, status, "a phone left running: stopping it");
		}
		stop(runner, &r).await;
		forget(runner, &r.phone)?;
	}
	Ok(())
}

fn recorded(dir: &Path) -> eyre::Result<Vec<Record>> {
	let path = dir.join(RECORDS);
	match std::fs::read(&path) {
		Ok(b) => serde_json::from_slice(&b).wrap_err_with(|| format!("{} is not the runner's", path.display())),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
		Err(e) => Err(eyre::Report::new(e).wrap_err(format!("reading {}", path.display()))),
	}
}

fn write(dir: &Path, records: &[Record]) -> eyre::Result<()> {
	let path = dir.join(RECORDS);
	let part = dir.join(format!("{RECORDS}.part"));
	std::fs::write(&part, serde_json::to_vec(records)?).wrap_err_with(|| format!("writing {}", part.display()))?;
	std::fs::rename(&part, &path).wrap_err_with(|| format!("putting {} in place", path.display()))
}

fn remember(runner: &Runner, r: &Record) -> eyre::Result<()> {
	let _held = runner.0.records.lock().expect("never held across a panic");
	let mut all = recorded(&runner.0.dir)?;
	all.retain(|x| x.phone != r.phone);
	all.push(r.clone());
	write(&runner.0.dir, &all)
}

fn forget(runner: &Runner, phone: &str) -> eyre::Result<()> {
	let _held = runner.0.records.lock().expect("never held across a panic");
	let mut all = recorded(&runner.0.dir)?;
	all.retain(|x| x.phone != phone);
	write(&runner.0.dir, &all)
}

/// `probe` until it answers, at most `within`.
async fn until<T, F: Future<Output = eyre::Result<Option<T>>>>(what: &str, within: Duration, mut probe: impl FnMut() -> F) -> eyre::Result<T> {
	let deadline = tokio::time::Instant::now() + within;
	//LOOP: every 3 s until done or the deadline
	loop {
		if let Some(t) = probe().await? {
			return Ok(t);
		}
		eyre::ensure!(tokio::time::Instant::now() < deadline, "no {what} within {}s", within.as_secs());
		tokio::time::sleep(Duration::from_secs(3)).await;
	}
}

fn first_url(v: &Value) -> Option<String> {
	match v {
		Value::Object(m) => m.get("url").and_then(Value::as_str).map(str::to_owned).or_else(|| m.values().find_map(first_url)),
		Value::Array(a) => a.iter().find_map(first_url),
		_ => None,
	}
}

/// Multilogin's API, as the runner calls it: with the run's token, never a long-lived one.
pub(crate) struct Api {
	pub(crate) base: String,
	pub(crate) http: reqwest::Client,
}

impl Api {
	/// The one call that bills.
	async fn start(&self, token: &str, phone: &str, _minutes: &MobileMinutes) -> eyre::Result<Value> {
		self.send(token, reqwest::Method::POST, START, json!({ "ids": [phone] })).await
	}

	async fn call(&self, token: &str, method: reqwest::Method, path: &str, body: Value) -> eyre::Result<Value> {
		assert_ne!(path, START, "a start takes MobileMinutes");
		self.send(token, method, path, body).await
	}

	/// `data` of a call that succeeded whole: a batch answers 200 with its failures in `fail_amount`.
	async fn send(&self, token: &str, method: reqwest::Method, path: &str, body: Value) -> eyre::Result<Value> {
		let res = self
			.http
			.request(method.clone(), format!("{}{path}", self.base))
			.bearer_auth(token)
			.json(&body)
			.send()
			.await
			.wrap_err_with(|| format!("Multilogin's {method} {path}"))?;
		let status = res.status();
		let text = res.text().await.wrap_err_with(|| format!("Multilogin's answer to {method} {path}"))?;
		let v: Value = serde_json::from_str(&text).map_err(|_| eyre::eyre!("Multilogin answered {method} {path} with {status}: {text}"))?;
		if !status.is_success() || v["status"]["http_code"].as_u64().is_none_or(|c| c >= 400) || v["data"]["fail_amount"].as_u64().is_some_and(|n| n > 0) {
			eyre::bail!("Multilogin refused {method} {path}: {text}");
		}
		Ok(v["data"].clone())
	}

	async fn status(&self, token: &str, phone: &str) -> eyre::Result<u64> {
		let v = self.call(token, reqwest::Method::POST, "/mobile_profiles/phone/statuses", json!({ "ids": [phone] })).await?;
		v["success_details"][0]["status"]
			.as_u64()
			.ok_or_else(|| eyre::eyre!("phone/statuses has no status for {phone}: {v}"))
	}
}
