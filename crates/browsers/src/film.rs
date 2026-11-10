//! A gmail's film as its browser's back camera: the next launch plays it through Chrome's file
//! capture, its first frame still before it and its last held after, the sound beside it; and
//! `facing.js` before every page's own scripts, so the camera is a phone's back one.
//!
//! The file route's tells are known (emulate_phone_cam `tmp/ongoing_debug/fake-camera-tells.md`):
//! the track's label ends in `.mjpeg`, and the picture restarts at every `getUserMedia` while the
//! sound runs on.

use std::path::Path;

use eyre::WrapErr;
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

/// The track's label is the path as given: Chrome on Android's back camera, and the extension
/// its parser needs.
pub(crate) const VIDEO: &str = "camera 0, facing back.mjpeg";
pub(crate) const AUDIO: &str = "film.wav";
const SOURCE: &str = "film.mp4";
/// Before the film: the moment the operator presses record.
const STILL_S: u32 = 5;
/// After it, before the loop starts it again.
const HOLD_S: u32 = 60;

/// Whether `dir` holds a film for the next launch.
pub(crate) fn present(dir: &Path) -> bool {
	dir.join(VIDEO).exists() && dir.join(AUDIO).exists()
}

/// The arguments that play it.
pub(crate) fn args() -> [String; 3] {
	[
		"--use-fake-device-for-media-stream".to_owned(),
		format!("--use-file-for-fake-video-capture={VIDEO}"),
		format!("--use-file-for-fake-audio-capture={AUDIO}"),
	]
}

/// `mp4` into `dir`'s camera and mic files, or what ffmpeg refused of it; `Err` is ours.
pub(crate) async fn set(ffmpeg: &Path, dir: &Path, mp4: &[u8]) -> eyre::Result<Result<(), String>> {
	tokio::fs::create_dir_all(dir).await.wrap_err_with(|| format!("creating {}", dir.display()))?;
	tokio::fs::write(dir.join(SOURCE), mp4).await.wrap_err("writing the film")?;
	let video = format!(
		"scale=720:1280:force_original_aspect_ratio=decrease,pad=720:1280:(ow-iw)/2:(oh-ih)/2,fps=30,\
		 tpad=start_duration={STILL_S}:start_mode=clone:stop_duration={HOLD_S}:stop_mode=clone"
	);
	// a phone's mic is never digital silence: a faint noise under the still and the hold
	let audio = format!(
		"[0:a:0]aresample=48000,adelay={STILL_S}s:all=1,apad=pad_dur={HOLD_S}[f];anoisesrc=color=pink:amplitude=0.002:sample_rate=48000[n];\
		 [f][n]amix=inputs=2:duration=first:normalize=0"
	);
	let runs: [(&str, Vec<&str>); 2] = [
		(VIDEO, vec!["-an", "-vf", &video, "-c:v", "mjpeg", "-q:v", "4", "-f", "mjpeg"]),
		(AUDIO, vec!["-vn", "-filter_complex", &audio, "-c:a", "pcm_s16le", "-f", "wav"]),
	];
	for (out, args) in runs {
		let part = format!("{out}.part");
		let done = tokio::process::Command::new(ffmpeg)
			.current_dir(dir)
			.args(["-hide_banner", "-loglevel", "error", "-y", "-i", SOURCE])
			.args(args)
			.arg(&part)
			.kill_on_drop(true)
			.output()
			.await
			.wrap_err_with(|| format!("running {}", ffmpeg.display()))?;
		if !done.status.success() {
			return Ok(Err(format!("ffmpeg could not make the film's {out}: {}", String::from_utf8_lossy(&done.stderr).trim())));
		}
		tokio::fs::rename(dir.join(&part), dir.join(out)).await.wrap_err("putting the film in place")?;
	}
	Ok(Ok(()))
}

/// The film gone: the next launch has no camera.
pub(crate) async fn remove(dir: &Path) -> eyre::Result<()> {
	for f in [SOURCE, VIDEO, AUDIO] {
		match tokio::fs::remove_file(dir.join(f)).await {
			Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(eyre::Report::new(e).wrap_err(format!("removing {f}"))),
			_ => {}
		}
	}
	Ok(())
}

/// `facing.js` into every page and frame before its own scripts, until the browser's devtools
/// close: the browser-wide auto-attach pauses each new target until it is told to run. `Runtime`
/// is never enabled.
pub(crate) async fn facing(devtools: &str) -> eyre::Result<()> {
	let (ws, _) = tokio_tungstenite::connect_async(devtools).await.wrap_err("the browser's devtools, for facing.js")?;
	let (mut tx, mut rx) = ws.split();
	let script = include_str!("facing.js").replace("__LABEL__", &Value::from(VIDEO).to_string());
	let auto = json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true });
	let mut next = 0u64;
	let mut send = |session: Option<&str>, method: &str, params: Value| {
		next += 1;
		let mut m = json!({ "id": next, "method": method, "params": params });
		if let Some(s) = session {
			m["sessionId"] = s.into();
		}
		Message::text(m.to_string())
	};
	tx.send(send(None, "Target.setAutoAttach", auto.clone())).await?;
	//LOOP: until the browser closes its devtools
	while let Some(m) = rx.next().await {
		let Message::Text(t) = m.wrap_err("the browser's devtools, for facing.js")? else { continue };
		let v: Value = serde_json::from_str(&t)?;
		if let Some(e) = v.get("error") {
			// the target closed between its attach and this
			if e["message"].as_str().is_some_and(|m| m.starts_with("Session with given id not found")) {
				continue;
			}
			eyre::bail!("devtools refused facing.js's command: {e}");
		}
		if v["method"] != "Target.attachedToTarget" {
			continue;
		}
		let p = &v["params"];
		let session = p["sessionId"].as_str().ok_or_else(|| eyre::eyre!("attachedToTarget without a session: {v}"))?.to_owned();
		let kind = p["targetInfo"]["type"].as_str().unwrap_or_default();
		// workers have no getUserMedia, and a paused one may answer nothing but `run`
		if kind == "page" || kind == "iframe" {
			tx.send(send(Some(&session), "Target.setAutoAttach", auto.clone())).await?;
			tx.send(send(Some(&session), "Page.enable", json!({}))).await?;
			tx.send(send(Some(&session), "Page.addScriptToEvaluateOnNewDocument", json!({ "source": script, "runImmediately": true })))
				.await?;
		}
		if p["waitingForDebugger"] == true {
			tx.send(send(Some(&session), "Runtime.runIfWaitingForDebugger", json!({}))).await?;
		}
	}
	Ok(())
}
