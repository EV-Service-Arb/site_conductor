//! Multilogin's Mimic, started and stopped through the launcher of the Multilogin app `serve`
//! keeps (on Xvfb: headed, a phone-wide window; headless draws a 225 px one). Its fingerprint,
//! proxy and place are the profile's, set by the conductor before the launch.

use std::time::Duration;

use eyre::WrapErr;
use serde_json::Value;

use crate::{
	Runner,
	profile::{Ends, IDLE, Up, up},
};

/// Its first start downloads Mimic itself.
const UP_WITHIN: Duration = Duration::from_secs(180);
const ALIVE_EVERY: Duration = Duration::from_secs(15);

pub(crate) async fn life(runner: &Runner, gmail: i64, token: &str, folder: &str, profile: &str, ends: &mut Ends<'_>) -> eyre::Result<()> {
	let started = tokio::time::timeout(UP_WITHIN, async {
		//LOOP: until the launcher has Mimic and starts it; UP_WITHIN bounds it
		loop {
			let path = format!("/api/v2/profile/f/{folder}/p/{profile}/start?automation_type=puppeteer&headless_mode=false");
			match call(runner, token, &path).await {
				Ok(v) =>
					break v["data"]["port"]
						.as_str()
						.and_then(|p| p.parse::<u16>().ok())
						.ok_or_else(|| eyre::eyre!("the launcher started {profile} on no port: {v}")),
				Err(Launcher::NotYet(why)) => tracing::info!(gmail, why, "Mimic not startable yet"),
				Err(Launcher::Refused(e)) => break Err(e),
			}
			tokio::time::sleep(Duration::from_secs(5)).await;
		}
	})
	.await
	.map_err(|_| eyre::eyre!("Mimic did not start within {}s", UP_WITHIN.as_secs()))??;
	let lived = async {
		let version: Value = runner
			.0
			.http
			.get(format!("http://127.0.0.1:{started}/json/version"))
			.send()
			.await?
			.error_for_status()?
			.json()
			.await?;
		let devtools = version["webSocketDebuggerUrl"]
			.as_str()
			.ok_or_else(|| eyre::eyre!("Mimic's /json/version names no socket: {version}"))?
			.to_owned();
		up(runner, gmail, Up::Devtools(devtools));
		let gone = async {
			//LOOP: until its devtools stop answering
			loop {
				tokio::time::sleep(ALIVE_EVERY).await;
				if runner.0.http.get(format!("http://127.0.0.1:{started}/json/version")).send().await.is_err() {
					return;
				}
			}
		};
		tokio::select! {
			() = gone => tracing::info!(gmail, "Mimic exited"),
			() = ends.any(gmail, IDLE) => {}
		}
		eyre::Ok(())
	}
	.await;
	let stopped = call(runner, token, &format!("/api/v1/profile/stop/p/{profile}"))
		.await
		.map(drop)
		.map_err(|e| eyre::eyre!("stopping Mimic's {profile}: {e}"));
	lived.and(stopped)
}

enum Launcher {
	/// Down, or Mimic downloading: asked again.
	NotYet(String),
	Refused(eyre::Report),
}

impl std::fmt::Display for Launcher {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::NotYet(why) => f.write_str(why),
			Self::Refused(e) => write!(f, "{e:#}"),
		}
	}
}

async fn call(runner: &Runner, token: &str, path: &str) -> Result<Value, Launcher> {
	let res = runner.0.http.get(format!("{}{path}", runner.0.launcher)).bearer_auth(token).send().await;
	let res = match res {
		Ok(r) => r,
		Err(e) if e.is_connect() => return Err(Launcher::NotYet(format!("the launcher: {e}"))),
		Err(e) => return Err(Launcher::Refused(eyre::Report::new(e).wrap_err("the launcher"))),
	};
	let status = res.status();
	let text = res.text().await.wrap_err("the launcher's answer").map_err(Launcher::Refused)?;
	let v: Value = serde_json::from_str(&text).map_err(|_| Launcher::Refused(eyre::eyre!("the launcher answered {status}: {text}")))?;
	match v["status"]["error_code"].as_str() {
		Some("CORE_DOWNLOADING_STARTED") => Err(Launcher::NotYet(v["status"]["message"].to_string())),
		_ if status.is_success() => Ok(v),
		_ => Err(Launcher::Refused(eyre::eyre!("the launcher answered {status}: {text}"))),
	}
}
