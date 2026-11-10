//! M1: one paid run of one cloud phone, to measure what the free stand-ins cannot. Run by a person
//! who has the user's go for this run, never by a test:
//!
//! ```text
//! MULTILOGIN_TOKEN=… cargo r -p sa_browsers --example phone_once -- <phone id> <out dir> <phone-tools dir> <chromium>
//! ```
//! The phone is stopped [`WATCHDOG`] after `phone/start` whatever is still being measured, and on
//! every way this ends but SIGKILL; the stop is read back. Minutes are read before and after.
//! Measured: start → started, adb + glogin, a screencap, a tap, the zone and a fix read back, the exit
//! from inside, and the start's stream URL opened in headless Chromium alone and in an iframe.

use std::{
	path::{Path, PathBuf},
	time::{Duration, Instant},
};

use eyre::{WrapErr, bail, ensure, eyre};
use serde_json::{Value, json};
use tokio::process::Command;

const API: &str = "https://api.multilogin.com";
const WATCHDOG: Duration = Duration::from_secs(4 * 60);

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
		token: std::env::var("MULTILOGIN_TOKEN").wrap_err("MULTILOGIN_TOKEN")?,
	};
	let before = api.call("GET", "/mobile_profiles/limit", None).await?;
	println!("minutes before: {before}");
	ensure!(api.status(&phone).await? == 2, "{phone} is not stopped: nothing started here");

	let started = Instant::now();
	let measured = tokio::select! {
		m = measure(&api, &phone, &out, &tools, &chromium, started) => m,
		() = tokio::time::sleep(WATCHDOG) => Err(eyre!("the watchdog's {} s: stopped mid-measure", WATCHDOG.as_secs())),
		r = tokio::signal::ctrl_c() => Err(eyre!("interrupted ({r:?})")),
	};
	let stopped = stop(&api, &phone).await;
	println!("ran {} s", started.elapsed().as_secs());
	let after = api.call("GET", "/mobile_profiles/limit", None).await;
	println!("minutes after: {after:?}");
	stopped.wrap_err("THE PHONE MAY STILL RUN: stop it in the Multilogin app")?;
	measured
}

async fn measure(api: &Api, phone: &str, out: &Path, tools: &Path, chromium: &str, at: Instant) -> eyre::Result<()> {
	let start = api.call("POST", "/mobile_profiles/phone/start", Some(json!({ "ids": [phone] }))).await?;
	std::fs::write(out.join("start.json"), serde_json::to_vec_pretty(&start)?)?;
	println!("phone/start answered: {start}");
	while api.status(phone).await? != 0 {
		tokio::time::sleep(Duration::from_secs(2)).await;
	}
	println!("started in {} s", at.elapsed().as_secs());
	api.call("POST", "/mobile_profiles/phone/adb/set", Some(json!({ "ids": [phone], "enabled": true }))).await?;
	let info = loop {
		let v = api.call("POST", "/mobile_profiles/phone/adb/info", Some(json!({ "ids": [phone] }))).await?;
		if v["items"][0]["status"] == "active" {
			break v["items"][0].clone();
		}
		tokio::time::sleep(Duration::from_secs(2)).await;
	};
	let serial = format!("{}:{}", info["ip"].as_str().unwrap_or_default(), info["port"].as_str().unwrap_or_default());
	let adb = tools.join("adb");
	let t = Instant::now();
	run(&adb, &["connect", &serial]).await?;
	let login = run(
		&adb,
		&["-s", &serial, "shell", "glogin", info["pwd"].as_str().ok_or_else(|| eyre!("adb/info without pwd: {info}"))?],
	)
	.await?;
	println!("adb + glogin in {} ms: {login}", t.elapsed().as_millis());
	let sh = |line: &'static str| {
		let (adb, serial) = (adb.clone(), serial.clone());
		async move { run(&adb, &["-s", &serial, "shell", line]).await }
	};
	println!("id: {}; wm size: {}", sh("id").await?, sh("wm size").await?);
	let t = Instant::now();
	let png = Command::new(&adb).args(["-s", &serial, "exec-out", "screencap", "-p"]).output().await?;
	std::fs::write(out.join("screen.png"), &png.stdout)?;
	println!("screencap: {} bytes in {} ms", png.stdout.len(), t.elapsed().as_millis());
	let t = Instant::now();
	sh("input tap 540 1200").await?;
	println!("tap in {} ms", t.elapsed().as_millis());
	for line in [
		"appops set com.android.shell android:mock_location allow",
		"cmd location providers add-test-provider gps",
		"cmd location providers set-test-provider-enabled gps true",
		"cmd location providers set-test-provider-location gps --location 45.764,4.8357",
		"settings put global auto_time_zone 0",
		"cmd alarm set-timezone Europe/Paris",
	] {
		sh(line).await?;
	}
	println!(
		"zone: {}; gps: {}",
		sh("getprop persist.sys.timezone").await?,
		sh("dumpsys location | grep -m2 -i 'gps.*45.76'").await.unwrap_or_default()
	);
	run(&adb, &["-s", &serial, "push", tools.join("busybox").to_str().expect("UTF-8"), "/data/local/tmp/busybox"]).await?;
	sh("chmod 755 /data/local/tmp/busybox").await?;
	println!(
		"exit from inside: {}",
		sh("/data/local/tmp/busybox wget -qO- http://ip-api.com/json/?fields=countryCode,city,isp,mobile,query").await?
	);
	match first_url(&start) {
		Some(url) => {
			std::fs::write(
				out.join("iframe.html"),
				format!("<iframe src=\"{url}\" style=\"width:100%;height:95vh;border:0\" allow=\"camera;microphone\"></iframe>"),
			)?;
			for (name, page) in [("stream", url.clone()), ("iframe", format!("file://{}", out.join("iframe.html").display()))] {
				let shot = out.join(format!("{name}.png"));
				let done = Command::new(chromium)
					.env_remove("WAYLAND_DISPLAY")
					.env_remove("DISPLAY")
					.args([
						"--headless=new",
						"--virtual-time-budget=15000",
						"--window-size=1280,900",
						&format!("--screenshot={}", shot.display()),
						&page,
					])
					.output()
					.await?;
				println!("{name}: {} → {}", done.status, shot.display());
			}
		}
		None => println!("phone/start named no url"),
	}
	Ok(())
}

/// Stopped and read so, within 2 minutes.
async fn stop(api: &Api, phone: &str) -> eyre::Result<()> {
	if api.status(phone).await? == 2 {
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

async fn run(bin: &Path, args: &[&str]) -> eyre::Result<String> {
	let o = tokio::time::timeout(Duration::from_secs(30), Command::new(bin).args(args).kill_on_drop(true).output()).await??;
	ensure!(o.status.success(), "{} {}: {}", bin.display(), args.first().unwrap_or(&""), String::from_utf8_lossy(&o.stderr));
	Ok(String::from_utf8_lossy(&o.stdout).trim().to_owned())
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
