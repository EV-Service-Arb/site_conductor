//! M1: one paid run of one cloud phone, to measure what the free stand-ins cannot. Run by a person
//! who has the user's go for this run, never by a test:
//!
//! ```text
//! MULTILOGIN_TOKEN=… cargo r -p sa_browsers --example phone_once -- <phone id> <out dir> <phone-tools dir> <chromium> | tee <out dir>/log
//! ```
//! The phone is stopped [`WATCHDOG`] after `phone/start` whatever is still being measured, and on
//! every way this ends but SIGKILL; the stop is read back. Minutes are read before and after. Each
//! finding is printed and its file written as soon as it is had: a step that fails is reported and
//! the next runs, so a cut run keeps all it got. The stream URL is looked at beside the adb steps.

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
		http: reqwest::Client::builder().timeout(Duration::from_secs(30)).build()?,
		token: std::env::var("MULTILOGIN_TOKEN").map_err(|_| eyre!("MULTILOGIN_TOKEN"))?,
	};
	println!("minutes before: {}", api.call("GET", "/mobile_profiles/limit", None).await?);
	ensure!(api.status(&phone).await? == 2, "{phone} is not stopped: nothing started here");

	let started = Instant::now();
	let measured = tokio::select! {
		m = measure(&api, &phone, &out, &tools, &chromium, started) => m,
		() = tokio::time::sleep(WATCHDOG) => Err(eyre!("the watchdog's {} s: stopped mid-measure", WATCHDOG.as_secs())),
		r = tokio::signal::ctrl_c() => Err(eyre!("interrupted ({r:?})")),
	};
	if let Err(e) = &measured {
		println!("measuring ended: {e:#}");
	}
	let stopped = stop(&api, &phone).await;
	println!("ran {} s from phone/start to the stop read back", started.elapsed().as_secs());
	println!("minutes after: {:?}", api.call("GET", "/mobile_profiles/limit", None).await);
	stopped.map_err(|e| eyre!("THE PHONE MAY STILL RUN: stop it in the Multilogin app: {e:#}"))
}

async fn measure(api: &Api, phone: &str, out: &Path, tools: &Path, chromium: &str, at: Instant) -> eyre::Result<()> {
	let start = api.call("POST", "/mobile_profiles/phone/start", Some(json!({ "ids": [phone] }))).await?;
	std::fs::write(out.join("start.json"), serde_json::to_vec_pretty(&start)?)?;
	println!("[{:>3}s] phone/start answered: {start}", at.elapsed().as_secs());
	let statuses = loop {
		let v = api.call("POST", "/mobile_profiles/phone/statuses", Some(json!({ "ids": [phone] }))).await?;
		match v["success_details"][0]["status"].as_u64() {
			Some(0) => break v,
			Some(3) => bail!("{phone} is expired"),
			_ => tokio::time::sleep(Duration::from_secs(2)).await,
		}
	};
	println!("[{:>3}s] started; statuses: {statuses}", at.elapsed().as_secs());
	let stream = async {
		match first_url(&start) {
			Some(url) => look_at_stream(&url, out, chromium).await,
			None => println!("phone/start named no url"),
		}
	};
	let (_, driven) = tokio::join!(stream, drive(api, phone, out, tools, at));
	driven
}

/// The stream URL: its answer's headers (whether a frame may embed it), and what headless Chromium
/// shows of it alone and inside an iframe.
async fn look_at_stream(url: &str, out: &Path, chromium: &str) {
	std::fs::write(out.join("stream_url.txt"), url).expect("the out dir is ours");
	println!("stream url: {url}");
	match reqwest::get(url).await {
		Ok(r) => {
			let headers: Vec<String> = r.headers().iter().map(|(k, v)| format!("{k}: {}", v.to_str().unwrap_or("?"))).collect();
			println!("stream GET {}: {}", r.status(), headers.join(" | "));
			let body = r.text().await.unwrap_or_default();
			std::fs::write(out.join("stream_body.html"), &body).expect("the out dir is ours");
		}
		Err(e) => println!("stream GET failed: {e}"),
	}
	std::fs::write(
		out.join("iframe.html"),
		format!("<iframe src=\"{url}\" style=\"width:100%;height:95vh;border:0\" allow=\"camera;microphone;clipboard-read;clipboard-write\"></iframe>"),
	)
	.expect("the out dir is ours");
	let iframe = format!("file://{}", out.join("iframe.html").display());
	let chrome = |args: Vec<String>| {
		let mut c = Command::new(chromium);
		c.env_remove("WAYLAND_DISPLAY")
			.env_remove("DISPLAY")
			.args([
				"--headless=new",
				"--virtual-time-budget=20000",
				"--window-size=1280,900",
				"--use-fake-ui-for-media-stream",
				"--use-fake-device-for-media-stream",
			])
			.args(args)
			.kill_on_drop(true);
		c
	};
	let shot = |name: &str, page: &str| chrome(vec![format!("--screenshot={}", out.join(format!("{name}.png")).display()), page.to_owned()]);
	let dom = chrome(vec!["--dump-dom".to_owned(), url.to_owned()]).output();
	// a live stream page never goes idle: each look is cut at 40 s (M1's first run waited out its watchdog on them)
	let bounded = |f| tokio::time::timeout(Duration::from_secs(40), f);
	let (a, b, d) = tokio::join!(bounded(shot("stream", url).output()), bounded(shot("iframe", &iframe).output()), bounded(dom));
	let (a, b, d) = (
		a.map_err(|_| "cut at 40 s"),
		b.map_err(|_| "cut at 40 s"),
		d.unwrap_or_else(|_| Err(std::io::Error::other("cut at 40 s"))),
	);
	println!("stream screenshot: {:?}; iframe screenshot: {:?}", a.map(|o| o.map(|o| o.status)), b.map(|o| o.map(|o| o.status)));
	match d {
		Ok(o) => {
			std::fs::write(out.join("stream_dom.html"), &o.stdout).expect("the out dir is ours");
			let dom = String::from_utf8_lossy(&o.stdout).to_lowercase();
			let seen: Vec<&str> = ["camera", "library", "microphone", "upload", "webrtc", "video", "canvas", "sign in", "login"]
				.into_iter()
				.filter(|w| dom.contains(w))
				.collect();
			println!("stream dom: {} bytes, mentions {seen:?}", o.stdout.len());
		}
		Err(e) => println!("stream dom failed: {e}"),
	}
}

/// The phone over adb: each step's finding printed, a failed one reported and passed.
async fn drive(api: &Api, phone: &str, out: &Path, tools: &Path, at: Instant) -> eyre::Result<()> {
	println!(
		"adb/info before adb/set: {:?}",
		api.call("POST", "/mobile_profiles/phone/adb/info", Some(json!({ "ids": [phone] }))).await
	);
	api.call("POST", "/mobile_profiles/phone/adb/set", Some(json!({ "ids": [phone], "enabled": true }))).await?;
	let t = Instant::now();
	// adb left on by the run before reads active at once with a password adb/set replaces (M1)
	let p = loop {
		let v = api.call("POST", "/mobile_profiles/phone/adb/info", Some(json!({ "ids": [phone] }))).await?;
		let i = &v["items"][0];
		if i["status"] == "active" {
			let p = Phone {
				adb: tools.join("adb"),
				serial: format!("{}:{}", i["ip"].as_str().unwrap_or_default(), i["port"].as_str().unwrap_or_default()),
				pwd: i["pwd"].as_str().ok_or_else(|| eyre!("adb/info without pwd: {i}"))?.to_owned(),
			};
			p.adb(&["connect", &p.serial]).await?;
			let login = p.sh("glogin", Some(&p.pwd)).await?;
			println!("[{:>3}s] glogin {} ms after adb/set: {login}", at.elapsed().as_secs(), t.elapsed().as_millis());
			if login.contains("success") || login.contains("already") {
				break p;
			}
		}
		tokio::time::sleep(Duration::from_secs(2)).await;
	};

	let steps: &[(&str, &str)] = &[
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
		("resolver", "ls -l /etc/resolv.conf; cat /etc/resolv.conf; getprop net.dns1"),
		("accounts", "dumpsys account | grep -E 'Account \\{' | head -5"),
	];
	for (name, line) in steps {
		p.step(name, line).await;
	}
	for i in 0..3 {
		let t = Instant::now();
		match p.exec_out(&["screencap", "-p"]).await {
			Ok(png) => {
				println!("screencap {i}: {} bytes in {} ms", png.len(), t.elapsed().as_millis());
				if i == 0 {
					std::fs::write(out.join("screen.png"), png)?;
				}
			}
			Err(e) => println!("screencap {i} failed: {e:#}"),
		}
	}
	let t = Instant::now();
	p.step("tap", "input tap 540 1200").await;
	println!("tap round trip {} ms", t.elapsed().as_millis());

	for (name, line) in [
		(
			"mock appops",
			"appops set com.android.shell android:mock_location allow && appops get com.android.shell android:mock_location",
		),
		(
			"mock providers",
			"for p in gps network fused; do cmd location providers add-test-provider $p; cmd location providers set-test-provider-enabled $p true; cmd location providers set-test-provider-location $p --location 45.764,4.8357; done; echo done",
		),
		("location read back", "dumpsys location | grep -iE 'last location|mock' | head -8"),
		("zone", "settings put global auto_time_zone 0; cmd alarm set-timezone Europe/Paris; getprop persist.sys.timezone"),
	] {
		p.step(name, line).await;
	}

	match p.adb(&["-s", &p.serial, "push", tools.join("busybox").to_str().expect("UTF-8"), BUSYBOX]).await {
		Ok(_) =>
			p.step(
				"exit from inside",
				&format!("chmod 755 {BUSYBOX}; {BUSYBOX} wget -qO- 'http://ip-api.com/json/?fields=countryCode,city,isp,mobile,proxy,hosting,query,timezone'"),
			)
			.await,
		Err(e) => println!("busybox push failed: {e:#}"),
	}

	let t = Instant::now();
	match p.adb(&["-s", &p.serial, "install", "-r", tools.join("ADBKeyboard.apk").to_str().expect("UTF-8")]).await {
		Ok(said) => println!("ADBKeyBoard installed in {} ms: {said}", t.elapsed().as_millis()),
		Err(e) => println!("ADBKeyBoard install failed: {e:#}"),
	}
	p.step(
		"ime set",
		&format!("ime enable {ADB_KEYBOARD}; ime set {ADB_KEYBOARD}; settings put secure show_ime_with_hard_keyboard 1; settings get secure default_input_method"),
	)
	.await;
	p.step(
		"chrome",
		"am start -a android.intent.action.VIEW -d 'https://www.google.com/search?q=plombier' com.android.chrome",
	)
	.await;
	tokio::time::sleep(Duration::from_secs(6)).await;
	save(&p, out, "chrome").await;
	p.step("ui dump", "uiautomator dump /sdcard/ui.xml >/dev/null && cat /sdcard/ui.xml").await;
	// the first text field on screen, if any: tapped, then typed into through ADBKeyBoard
	let ui = p.sh("cat /sdcard/ui.xml", None).await.unwrap_or_default();
	std::fs::write(out.join("ui.xml"), &ui)?;
	match edit_text_center(&ui) {
		Some((x, y)) => {
			p.step("tap the field", &format!("input tap {x} {y}")).await;
			tokio::time::sleep(Duration::from_secs(2)).await;
			let b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "Électricien à Besançon");
			p.step("type", &format!("am broadcast -a ADB_INPUT_B64 --es msg {b64}")).await;
			tokio::time::sleep(Duration::from_secs(2)).await;
			save(&p, out, "typed").await;
		}
		None => println!("no text field on screen to type into"),
	}
	println!("statuses: {:?}", api.call("POST", "/mobile_profiles/phone/statuses", Some(json!({ "ids": [phone] }))).await);
	println!("[{:>3}s] every step done", at.elapsed().as_secs());
	Ok(())
}

async fn save(p: &Phone, out: &Path, name: &str) {
	match p.exec_out(&["screencap", "-p"]).await {
		Ok(png) => {
			std::fs::write(out.join(format!("{name}.png")), png).expect("the out dir is ours");
			println!("saved {name}.png");
		}
		Err(e) => println!("screencap {name} failed: {e:#}"),
	}
}

/// The centre of the first EditText's bounds in a uiautomator dump.
fn edit_text_center(ui: &str) -> Option<(i64, i64)> {
	let at = ui.find("android.widget.EditText")?;
	let rest = &ui[at..];
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
		let o = tokio::time::timeout(Duration::from_secs(40), Command::new(&self.adb).args(args).kill_on_drop(true).output()).await??;
		let said = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
		ensure!(o.status.success(), "adb {}: {said}", args.join(" ").chars().take(80).collect::<String>());
		Ok(said.trim().to_owned())
	}

	/// `line` in the phone's shell, glogin'd on the same connection first; `arg` its one argument.
	async fn sh(&self, line: &str, arg: Option<&str>) -> eyre::Result<String> {
		let line = match arg {
			Some(a) => format!("{line} '{a}'"),
			None => format!("glogin '{}' >/dev/null 2>&1; {line}", self.pwd),
		};
		self.adb(&["-s", &self.serial, "shell", &line]).await
	}

	async fn step(&self, name: &str, line: &str) {
		let t = Instant::now();
		match self.sh(line, None).await {
			Ok(said) => println!("== {name} ({} ms):\n{}", t.elapsed().as_millis(), said.chars().take(4000).collect::<String>()),
			Err(e) => println!("== {name} FAILED ({} ms): {e:#}", t.elapsed().as_millis()),
		}
	}

	async fn exec_out(&self, args: &[&str]) -> eyre::Result<Vec<u8>> {
		let mut a = vec!["-s", &self.serial, "exec-out"];
		a.extend_from_slice(args);
		let o = tokio::time::timeout(Duration::from_secs(40), Command::new(&self.adb).args(&a).kill_on_drop(true).output()).await??;
		ensure!(o.status.success() && !o.stdout.is_empty(), "exec-out: {}", String::from_utf8_lossy(&o.stderr));
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
