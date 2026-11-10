//! M1: one paid run of one cloud phone, to measure what the free stand-ins cannot. Run by a person
//! who has the user's go for this run, never by a test:
//!
//! ```text
//! MULTILOGIN_TOKEN=… cargo r -p sa_browsers --example phone_once -- <phone id> <out dir> <phone-tools dir> <chromium> | tee <out dir>/log
//! ```
//! Every step has the time it is expected to take and what it is expected to show; past the one or
//! without the other the run is aborted there and the phone stopped (read back), [`WATCHDOG`] the
//! backstop over all of it. Each finding is printed and its file written as soon as it is had.

use std::{
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

use eyre::{bail, ensure, eyre};
use serde_json::{Value, json};
use tokio::process::Command;

const API: &str = "https://api.multilogin.com";
const WATCHDOG: Duration = Duration::from_secs(4 * 60);
const BUSYBOX: &str = "/data/local/tmp/busybox";
const ADB_KEYBOARD: &str = "com.android.adbkeyboard/.AdbIME";
/// A shell command on the phone: ~350 ms each (M1).
const COMMAND: Duration = Duration::from_secs(10);
/// Headless Chromium on the stream page: told to stop loading at 12 s.
const LOOK: Duration = Duration::from_secs(25);

#[tokio::main]
async fn main() -> eyre::Result<()> {
	color_eyre::install()?;
	let [phone, out, tools, chromium] = std::env::args()
		.skip(1)
		.collect::<Vec<_>>()
		.try_into()
		.map_err(|_| eyre!("<phone id> <out dir> <phone-tools dir> <chromium>"))?;
	let (out, tools) = (PathBuf::from(out), PathBuf::from(tools));
	std::fs::create_dir_all(&out)?;
	let api = Api {
		http: reqwest::Client::builder().timeout(Duration::from_secs(20)).build()?,
		token: std::env::var("MULTILOGIN_TOKEN").map_err(|_| eyre!("MULTILOGIN_TOKEN"))?,
	};
	println!("minutes before: {}", api.call("GET", "/mobile_profiles/limit", None).await?);
	ensure!(api.status(&phone).await? == 2, "{phone} is not stopped: nothing started here");

	let started = Instant::now();
	let measured = tokio::select! {
		m = measure(&api, &phone, &out, &tools, &chromium, started) => m,
		() = tokio::time::sleep(WATCHDOG) => Err(eyre!("the watchdog's {} s", WATCHDOG.as_secs())),
		r = tokio::signal::ctrl_c() => Err(eyre!("interrupted ({r:?})")),
	};
	match &measured {
		Ok(()) => println!("[{:>3}s] every step done", started.elapsed().as_secs()),
		Err(e) => println!("[{:>3}s] ABORTED: {e:#}", started.elapsed().as_secs()),
	}
	let stopped = stop(&api, &phone).await;
	println!("ran {} s from phone/start to the stop read back", started.elapsed().as_secs());
	println!("minutes after: {:?}", api.call("GET", "/mobile_profiles/limit", None).await);
	stopped.map_err(|e| eyre!("THE PHONE MAY STILL RUN: stop it in the Multilogin app: {e:#}"))
}

/// `fut` within `expected`, else the run aborts: a billed phone never waits past what a step should
/// take.
async fn within<T>(what: &str, expected: Duration, fut: impl Future<Output = eyre::Result<T>>) -> eyre::Result<T> {
	let t = Instant::now();
	let done = tokio::time::timeout(expected, fut)
		.await
		.map_err(|_| eyre!("{what}: nothing within the {} s expected", expected.as_secs()))?;
	let v = done.map_err(|e| e.wrap_err(what.to_owned()))?;
	println!("-- {what}: {} ms", t.elapsed().as_millis());
	Ok(v)
}

/// `probe` every 2 s until it answers, within `expected`.
async fn until<T, F: Future<Output = eyre::Result<Option<T>>>>(what: &str, expected: Duration, mut probe: impl FnMut() -> F) -> eyre::Result<T> {
	within(what, expected, async {
		//LOOP: bounded by `within`
		loop {
			if let Some(t) = probe().await? {
				return Ok(t);
			}
			tokio::time::sleep(Duration::from_secs(2)).await;
		}
	})
	.await
}

async fn measure(api: &Api, phone: &str, out: &Path, tools: &Path, chromium: &str, at: Instant) -> eyre::Result<()> {
	let start = within(
		"phone/start",
		Duration::from_secs(20),
		api.call("POST", "/mobile_profiles/phone/start", Some(json!({ "ids": [phone] }))),
	)
	.await?;
	std::fs::write(out.join("start.json"), serde_json::to_vec_pretty(&start)?)?;
	println!("[{:>3}s] phone/start answered: {start}", at.elapsed().as_secs());
	// 44 s at M1
	let statuses = until("started", Duration::from_secs(90), || async {
		let v = api.call("POST", "/mobile_profiles/phone/statuses", Some(json!({ "ids": [phone] }))).await?;
		match v["success_details"][0]["status"].as_u64() {
			Some(0) => Ok(Some(v)),
			Some(3) => bail!("{phone} is expired"),
			_ => Ok(None),
		}
	})
	.await?;
	println!("[{:>3}s] started; statuses: {statuses}", at.elapsed().as_secs());
	let url = first_url(&start).ok_or_else(|| eyre!("phone/start named no url"))?;
	tokio::try_join!(look_at_stream(&url, out, chromium), drive(api, phone, out, tools, at)).map(drop)
}

/// The stream URL: its headers (whether a frame may embed it), and what headless Chromium shows of
/// it alone and inside an iframe, and its DOM, each cut at 12 s of loading.
async fn look_at_stream(url: &str, out: &Path, chromium: &str) -> eyre::Result<()> {
	std::fs::write(out.join("stream_url.txt"), url)?;
	let r = within("stream GET", Duration::from_secs(15), async { Ok(reqwest::get(url).await?) }).await?;
	let headers: Vec<String> = r.headers().iter().map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("?"))).collect();
	println!("stream GET {}: {}", r.status(), headers.join(" | "));
	std::fs::write(out.join("stream_body.html"), r.text().await?)?;
	std::fs::write(
		out.join("iframe.html"),
		format!("<iframe src=\"{url}\" style=\"width:100%;height:95vh;border:0\" allow=\"camera;microphone;clipboard-read;clipboard-write\"></iframe>"),
	)?;
	let iframe = format!("file://{}", out.join("iframe.html").display());
	let chrome = |what: &'static str, args: Vec<String>| {
		let mut c = Command::new(chromium);
		c.env_remove("WAYLAND_DISPLAY")
			.env_remove("DISPLAY")
			.args([
				"--headless=new",
				"--timeout=12000",
				"--window-size=1280,900",
				"--use-fake-ui-for-media-stream",
				"--use-fake-device-for-media-stream",
			])
			.args(args)
			.kill_on_drop(true);
		within(what, LOOK, async move {
			let o = c.output().await?;
			ensure!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or_default());
			Ok(o.stdout)
		})
	};
	let shot = |name: &str| format!("--screenshot={}", out.join(format!("{name}.png")).display());
	let (_, _, dom) = tokio::try_join!(
		chrome("stream screenshot", vec![shot("stream"), url.to_owned()]),
		chrome("iframe screenshot", vec![shot("iframe"), iframe]),
		chrome("stream dom", vec!["--dump-dom".to_owned(), url.to_owned()]),
	)?;
	std::fs::write(out.join("stream_dom.html"), &dom)?;
	let text = String::from_utf8_lossy(&dom).to_lowercase();
	let seen: Vec<&str> = ["camera", "library", "microphone", "upload", "video", "canvas", "sign in", "login", "error"]
		.into_iter()
		.filter(|w| text.contains(w))
		.collect();
	println!("stream dom: {} bytes, mentions {seen:?}", dom.len());
	Ok(())
}

/// The phone over adb, each step expected to answer in time and as it should.
async fn drive(api: &Api, phone: &str, out: &Path, tools: &Path, at: Instant) -> eyre::Result<()> {
	println!(
		"adb/info before adb/set: {}",
		api.call("POST", "/mobile_profiles/phone/adb/info", Some(json!({ "ids": [phone] }))).await?
	);
	api.call("POST", "/mobile_profiles/phone/adb/set", Some(json!({ "ids": [phone], "enabled": true }))).await?;
	// adb left on by the run before reads active at once with a password adb/set replaces (M1)
	let p = until("adb active and glogin taking its password", Duration::from_secs(60), || async {
		let v = api.call("POST", "/mobile_profiles/phone/adb/info", Some(json!({ "ids": [phone] }))).await?;
		let i = &v["items"][0];
		if i["status"] != "active" {
			return Ok(None);
		}
		let p = Phone {
			adb: tools.join("adb"),
			serial: format!("{}:{}", i["ip"].as_str().unwrap_or_default(), i["port"].as_str().unwrap_or_default()),
			pwd: i["pwd"].as_str().ok_or_else(|| eyre!("adb/info without pwd: {i}"))?.to_owned(),
		};
		p.adb(&["connect", &p.serial]).await?;
		let login = p.adb(&["-s", &p.serial, "shell", &format!("glogin '{}'", p.pwd)]).await?;
		println!("glogin: {login}");
		Ok((login.contains("success") || login.contains("already")).then_some(p))
	})
	.await?;
	println!("[{:>3}s] adb logged in at {}", at.elapsed().as_secs(), p.serial);

	for (name, line) in [
		("id", "id"),
		(
			"ident",
			"for k in ro.product.model ro.product.brand ro.product.manufacturer ro.build.version.release ro.build.fingerprint ro.product.cpu.abi ro.build.type ro.build.tags ro.debuggable persist.sys.timezone persist.sys.locale gsm.sim.operator.alpha gsm.sim.operator.iso-country gsm.network.type ro.boot.verifiedbootstate net.dns1; do echo \"$k=$(getprop $k)\"; done",
		),
		("screen", "wm size; wm density"),
		("ime", "settings get secure default_input_method; ime list -s"),
		(
			"settings",
			"for k in 'global auto_time_zone' 'global wifi_on' 'global mobile_data' 'secure location_mode' 'secure show_ime_with_hard_keyboard'; do echo \"$k=$(settings get $k)\"; done",
		),
		("packages", "pm list packages | grep -iE 'chrome|gms|vending|adbkeyboard|geelark|multilogin|magisk'"),
		("resolver", "ls -l /etc/resolv.conf 2>&1; cat /etc/resolv.conf 2>&1; getprop net.dns1"),
	] {
		p.step(name, line, |_| true).await?;
	}
	for i in 0..3 {
		let png = within(&format!("screencap {i}"), COMMAND, p.exec_out(&["screencap", "-p"])).await?;
		ensure!(png.starts_with(b"\x89PNG"), "screencap {i} is no PNG: {}", String::from_utf8_lossy(&png[..png.len().min(200)]));
		println!("screencap {i}: {} bytes", png.len());
		if i == 0 {
			std::fs::write(out.join("screen.png"), png)?;
		}
	}
	p.step("tap", "input tap 540 1200", |_| true).await?;

	p.step(
		"mock appops",
		"appops set com.android.shell android:mock_location allow && appops get com.android.shell android:mock_location",
		|s| s.contains("allow"),
	)
	.await?;
	p.step(
		"mock providers",
		"for p in gps network fused; do cmd location providers add-test-provider $p && cmd location providers set-test-provider-enabled $p true && cmd location providers set-test-provider-location $p --location 45.764,4.8357 || exit 1; done; echo done",
		|s| s.ends_with("done"),
	)
	.await?;
	p.step("location read back", "dumpsys location | grep -iE 'last location|mock' | head -8", |s| s.contains("45.76"))
		.await?;
	p.step(
		"zone",
		"settings put global auto_time_zone 0; cmd alarm set-timezone Europe/Paris; getprop persist.sys.timezone",
		|s| s.ends_with("Europe/Paris"),
	)
	.await?;

	within(
		"busybox push",
		COMMAND,
		p.adb(&["-s", &p.serial, "push", tools.join("busybox").to_str().expect("UTF-8"), BUSYBOX]),
	)
	.await?;
	p.step(
		"exit from inside",
		&format!("chmod 755 {BUSYBOX}; {BUSYBOX} wget -qO- 'http://ip-api.com/json/?fields=countryCode,city,isp,mobile,proxy,hosting,query,timezone'"),
		|s| s.contains("\"countryCode\":\"FR\""),
	)
	.await?;

	let said = within(
		"ADBKeyBoard install",
		Duration::from_secs(30),
		p.adb(&["-s", &p.serial, "install", "-r", tools.join("ADBKeyboard.apk").to_str().expect("UTF-8")]),
	)
	.await?;
	ensure!(said.contains("Success"), "ADBKeyBoard install: {said}");
	p.step(
		"ime set",
		&format!("ime enable {ADB_KEYBOARD}; ime set {ADB_KEYBOARD}; settings put secure show_ime_with_hard_keyboard 1; settings get secure default_input_method"),
		|s| s.ends_with(ADB_KEYBOARD),
	)
	.await?;
	p.step(
		"chrome",
		"am start -a android.intent.action.VIEW -d 'https://www.google.com/search?q=plombier' com.android.chrome",
		|s| s.contains("Starting"),
	)
	.await?;
	tokio::time::sleep(Duration::from_secs(6)).await;
	save(&p, out, "chrome").await?;
	let ui = p
		.step("ui dump", "uiautomator dump /sdcard/ui.xml >/dev/null && cat /sdcard/ui.xml", |s| s.contains("<hierarchy"))
		.await?;
	std::fs::write(out.join("ui.xml"), &ui)?;
	let (x, y) = edit_text_center(&ui).ok_or_else(|| eyre!("no text field on Chrome's screen to type into: see chrome.png, ui.xml"))?;
	p.step("tap the field", &format!("input tap {x} {y}"), |_| true).await?;
	tokio::time::sleep(Duration::from_secs(2)).await;
	let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "Électricien à Besançon");
	p.step("type", &format!("am broadcast -a ADB_INPUT_B64 --es msg {b64}"), |s| s.contains("Broadcast completed"))
		.await?;
	tokio::time::sleep(Duration::from_secs(2)).await;
	save(&p, out, "typed").await?;
	println!(
		"statuses at the end: {}",
		api.call("POST", "/mobile_profiles/phone/statuses", Some(json!({ "ids": [phone] }))).await?
	);
	Ok(())
}

async fn save(p: &Phone, out: &Path, name: &str) -> eyre::Result<()> {
	let png = within(&format!("screencap {name}"), COMMAND, p.exec_out(&["screencap", "-p"])).await?;
	ensure!(png.starts_with(b"\x89PNG"), "screencap {name} is no PNG");
	std::fs::write(out.join(format!("{name}.png")), png)?;
	Ok(())
}

/// The centre of the first EditText's bounds in a uiautomator dump.
fn edit_text_center(ui: &str) -> Option<(i64, i64)> {
	let rest = &ui[ui.find("android.widget.EditText")?..];
	let b = rest.find("bounds=\"[")? + 9;
	let nums: Vec<i64> = rest[b..rest[b..].find('"')? + b].split(['[', ']', ',']).filter_map(|n| n.parse().ok()).collect();
	match nums[..] {
		[x0, y0, x1, y1] => Some((x0.midpoint(x1), y0.midpoint(y1))),
		_ => None,
	}
}

struct Phone {
	adb: PathBuf,
	serial: String,
	pwd: String,
}

impl Phone {
	async fn adb(&self, args: &[&str]) -> eyre::Result<String> {
		let o = Command::new(&self.adb).args(args).kill_on_drop(true).output().await?;
		let said = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
		ensure!(o.status.success(), "adb {}: {said}", args.join(" ").chars().take(80).collect::<String>());
		Ok(said.trim().to_owned())
	}

	/// `line` in the phone's shell, logged in on the same connection first; within [`COMMAND`], its
	/// answer what `expect` takes, else the run aborts.
	async fn step(&self, name: &str, line: &str, expect: impl Fn(&str) -> bool) -> eyre::Result<String> {
		let said = within(name, COMMAND, self.adb(&["-s", &self.serial, "shell", &format!("glogin '{}' >/dev/null 2>&1; {line}", self.pwd)])).await?;
		println!("{}", said.chars().take(4000).collect::<String>());
		ensure!(!said.contains("run glogin to login first"), "{name}: not logged in");
		ensure!(expect(&said), "{name}: not what was expected");
		Ok(said)
	}

	async fn exec_out(&self, args: &[&str]) -> eyre::Result<Vec<u8>> {
		let mut a = vec!["-s", &self.serial, "exec-out"];
		a.extend_from_slice(args);
		let o = Command::new(&self.adb).args(&a).kill_on_drop(true).output().await?;
		ensure!(o.status.success(), "exec-out: {}", String::from_utf8_lossy(&o.stderr));
		Ok(o.stdout)
	}
}

/// Stopped and read so, within 2 minutes.
async fn stop(api: &Api, phone: &str) -> eyre::Result<()> {
	if api.status(phone).await? == 2 {
		println!("already stopped");
		return Ok(());
	}
	api.call("POST", "/mobile_profiles/phone/stop", Some(json!({ "ids": [phone] }))).await?;
	for _ in 0..60 {
		if api.status(phone).await? == 2 {
			println!("stopped, read back");
			return Ok(());
		}
		tokio::time::sleep(Duration::from_secs(2)).await;
	}
	bail!("{phone} does not read stopped 2 minutes after phone/stop")
}

fn first_url(v: &Value) -> Option<String> {
	match v {
		Value::Object(m) => m.get("url").and_then(Value::as_str).map(str::to_owned).or_else(|| m.values().find_map(first_url)),
		Value::Array(a) => a.iter().find_map(first_url),
		_ => None,
	}
}

struct Api {
	http: reqwest::Client,
	token: String,
}

impl Api {
	async fn call(&self, method: &str, path: &str, body: Option<Value>) -> eyre::Result<Value> {
		let mut req = self.http.request(method.parse()?, format!("{API}{path}")).bearer_auth(&self.token);
		if let Some(b) = body {
			req = req.json(&b);
		}
		let v: Value = req.send().await?.json().await?;
		ensure!(
			v["status"]["http_code"].as_u64().is_some_and(|c| c < 400) && v["data"]["fail_amount"].as_u64().is_none_or(|n| n == 0),
			"{method} {path}: {v}"
		);
		Ok(v["data"].clone())
	}

	async fn status(&self, phone: &str) -> eyre::Result<u64> {
		let v = self.call("POST", "/mobile_profiles/phone/statuses", Some(json!({ "ids": [phone] }))).await?;
		v["success_details"][0]["status"].as_u64().ok_or_else(|| eyre!("no status for {phone}: {v}"))
	}
}
