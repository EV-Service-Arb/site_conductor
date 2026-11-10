//! A cloud phone over adb, and its viewer: the screen polled (`screencap`, made a JPEG by ffmpeg),
//! the viewer's fingers and keys as `input`, text through ADBKeyBoard (`input text` types ASCII
//! only), Android's own UI in the frames: no tabs, no dialogs.
//!
//! Its adbd answers `shell` only after `glogin <pwd>` on that connection; a login is renewed
//! before it is [`LOGIN_FOR`] old.

use std::{
	path::{Path, PathBuf},
	sync::Mutex,
	time::{Duration, Instant},
};

use axum::extract::ws::{self, WebSocket};
use base64::Engine;
use eyre::WrapErr;
use sa_browsers::view::{In, Out, TouchPhase, web_url};
use tokio::process::Command;

/// It held 75 s on one connection, once (M1).
const LOGIN_FOR: Duration = Duration::from_secs(30);
const COMMAND_WITHIN: Duration = Duration::from_secs(30);
/// shortcut: screencap polling (≈2–3 fps at best), upgrade to scrcpy-server's H.264 through ffmpeg
/// if the operator finds it too slow.
const FRAME_EVERY: Duration = Duration::from_millis(300);
/// A finger that moved less is a tap.
const TAP_SLOP: f64 = 10.0;
const ADB_KEYBOARD: &str = "com.android.adbkeyboard/.AdbIME";

/// Where a finger went down, where it is, and since when.
type Finger = ((f64, f64), (f64, f64), Instant);

/// A started phone's adb.
pub(crate) struct Phone {
	pub(crate) adb: PathBuf,
	pub(crate) ffmpeg: PathBuf,
	/// ip:port.
	pub(crate) serial: String,
	pwd: String,
	logged_in: Mutex<Option<Instant>>,
	/// Its screen, px: the frames' space.
	pub(crate) screen: (u32, u32),
}

impl Phone {
	pub(crate) async fn connect(adb: PathBuf, ffmpeg: PathBuf, serial: String, pwd: String) -> eyre::Result<Self> {
		let mut p = Self {
			adb,
			ffmpeg,
			serial,
			pwd,
			logged_in: Mutex::new(None),
			screen: (0, 0),
		};
		let size = p.sh(&["wm", "size"]).await?;
		// "Physical size: WxH", then "Override size: WxH" when one is set: the last stands
		let wh = size.lines().filter_map(|l| l.split_once(": ")).map(|(_, wh)| wh.trim()).next_back();
		p.screen = wh
			.and_then(|wh| wh.split_once('x'))
			.and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
			.ok_or_else(|| eyre::eyre!("`wm size` said {size:?}"))?;
		Ok(p)
	}

	async fn adb(&self, args: &[&str]) -> eyre::Result<std::process::Output> {
		let out = tokio::time::timeout(COMMAND_WITHIN, Command::new(&self.adb).args(args).kill_on_drop(true).output())
			.await
			.map_err(|_| eyre::eyre!("adb {} took over {}s", args.first().unwrap_or(&""), COMMAND_WITHIN.as_secs()))?
			.wrap_err_with(|| format!("running {}", self.adb.display()))?;
		Ok(out)
	}

	/// Connected and logged in, unless it was lately.
	async fn login(&self) -> eyre::Result<()> {
		if self.logged_in.lock().expect("never held across a panic").is_some_and(|at| at.elapsed() < LOGIN_FOR) {
			return Ok(());
		}
		let out = self.adb(&["connect", &self.serial]).await?;
		let said = String::from_utf8_lossy(&out.stdout);
		eyre::ensure!(said.contains("connected to"), "adb connect {}: {said}", self.serial);
		let out = self.adb(&["-s", &self.serial, "shell", "glogin", &quote(&self.pwd)]).await?;
		let said = String::from_utf8_lossy(&out.stdout);
		eyre::ensure!(said.contains("glogin success") || said.contains("already logged"), "glogin on {}: {said}", self.serial);
		*self.logged_in.lock().expect("never held across a panic") = Some(Instant::now());
		Ok(())
	}

	/// `args` run by the phone's shell, each quoted: what a viewer typed never reaches it as syntax.
	pub(crate) async fn sh(&self, args: &[&str]) -> eyre::Result<String> {
		self.login().await?;
		let line = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
		let out = self.adb(&["-s", &self.serial, "shell", &line]).await?;
		let said = String::from_utf8_lossy(&out.stdout).trim().to_owned();
		eyre::ensure!(out.status.success(), "`{line}` on the phone: {said} {}", String::from_utf8_lossy(&out.stderr).trim());
		Ok(said)
	}

	pub(crate) async fn push(&self, from: &Path, to: &str) -> eyre::Result<()> {
		self.login().await?;
		let from = from.to_str().expect("our store paths are UTF-8");
		let out = self.adb(&["-s", &self.serial, "push", from, to]).await?;
		eyre::ensure!(out.status.success(), "adb push {from}: {}", String::from_utf8_lossy(&out.stderr).trim());
		Ok(())
	}

	pub(crate) async fn install(&self, apk: &Path) -> eyre::Result<()> {
		self.login().await?;
		let apk = apk.to_str().expect("our store paths are UTF-8");
		let out = self.adb(&["-s", &self.serial, "install", "-r", apk]).await?;
		eyre::ensure!(out.status.success(), "adb install {apk}: {}", String::from_utf8_lossy(&out.stdout).trim());
		Ok(())
	}

	/// The screen as a JPEG, base64, half its size, `after` this.
	async fn frame(&self, after: Duration) -> eyre::Result<String> {
		tokio::time::sleep(after).await;
		self.login().await?;
		let png = self.adb(&["-s", &self.serial, "exec-out", "screencap", "-p"]).await?;
		eyre::ensure!(png.status.success() && !png.stdout.is_empty(), "screencap: {}", String::from_utf8_lossy(&png.stderr).trim());
		let mut ffmpeg = Command::new(&self.ffmpeg)
			.args([
				"-hide_banner", "-loglevel", "error", "-f", "png_pipe", "-i", "-", "-vf", "scale=iw/2:-2", "-q:v", "6", "-f", "mjpeg", "-",
			])
			.stdin(std::process::Stdio::piped())
			.stdout(std::process::Stdio::piped())
			.stderr(std::process::Stdio::piped())
			.kill_on_drop(true)
			.spawn()
			.wrap_err("ffmpeg, for a frame")?;
		let mut stdin = ffmpeg.stdin.take().expect("piped");
		let feed = async move {
			let fed = tokio::io::AsyncWriteExt::write_all(&mut stdin, &png.stdout).await;
			drop(stdin); // its end of file
			fed
		};
		let (fed, jpeg) = tokio::join!(feed, ffmpeg.wait_with_output());
		let jpeg = jpeg.wrap_err("ffmpeg, for a frame")?;
		fed.wrap_err("feeding ffmpeg a frame")?;
		eyre::ensure!(jpeg.status.success(), "ffmpeg refused a frame: {}", String::from_utf8_lossy(&jpeg.stderr).trim());
		Ok(base64::engine::general_purpose::STANDARD.encode(jpeg.stdout))
	}

	/// Text past ASCII as the keyboard's broadcast, the IME it was put back after.
	async fn text(&self, text: &str) -> eyre::Result<()> {
		let before = self.sh(&["settings", "get", "secure", "default_input_method"]).await?;
		self.sh(&["ime", "enable", ADB_KEYBOARD]).await?;
		self.sh(&["ime", "set", ADB_KEYBOARD]).await?;
		let b64 = base64::engine::general_purpose::STANDARD.encode(text);
		let sent = self.sh(&["am", "broadcast", "-a", "ADB_INPUT_B64", "--es", "msg", &b64]).await;
		let back = match before.as_str() {
			"" | "null" => Ok(String::new()),
			ime => self.sh(&["ime", "set", ime]).await,
		};
		sent.and(back).map(drop)
	}
}

/// One argument for the phone's `sh`.
fn quote(arg: &str) -> String {
	format!("'{}'", arg.replace('\'', r"'\''"))
}

/// Android's keycode for a key the viewer may press.
fn keycode(key: &str) -> Option<&'static str> {
	Some(match key {
		"Enter" => "66",
		"Backspace" => "67",
		"Tab" => "61",
		"Escape" => "111",
		"Delete" => "112",
		"Home" => "122",
		"End" => "123",
		"ArrowLeft" => "21",
		"ArrowUp" => "19",
		"ArrowRight" => "22",
		"ArrowDown" => "20",
		_ => return None,
	})
}

/// The viewer of a phone, until either ends.
pub(crate) async fn view(mut socket: WebSocket, phone: &Phone) {
	let (width, height) = (f64::from(phone.screen.0), f64::from(phone.screen.1));
	let mut finger: Option<Finger> = None;
	if say(&mut socket, &Out::Tabs { tabs: Vec::new(), active: None }).await.is_err() {
		return;
	}
	let mut frame = Box::pin(phone.frame(Duration::ZERO));
	//LOOP: frames out, the viewer's input in, until either side ends
	loop {
		let heard = tokio::select! {
			f = &mut frame => {
				let out = match f {
					Ok(data) => Out::Frame { data, width, height },
					Err(e) => {
						tracing::warn!(error = format!("{e:#}"), "phone: a frame failed");
						Out::Error { message: format!("the screen: {e:#}") }
					}
				};
				if say(&mut socket, &out).await.is_err() {
					return;
				}
				frame = Box::pin(phone.frame(FRAME_EVERY));
				continue;
			}
			m = socket.recv() => m,
		};
		let text = match heard {
			Some(Ok(ws::Message::Text(t))) => t,
			Some(Ok(ws::Message::Ping(_) | ws::Message::Pong(_))) => continue,
			Some(Ok(ws::Message::Binary(_))) => return,
			Some(Ok(ws::Message::Close(_)) | Err(_)) | None => return,
		};
		let Ok(input) = serde_json::from_str::<In>(&text) else {
			return; // the conductor checked it: a frame not of the viewer's is a broken socket
		};
		let done = match input {
			In::Touch { phase: TouchPhase::Start, x, y } => {
				finger = Some(((x, y), (x, y), Instant::now()));
				Ok(())
			}
			In::Touch { phase: TouchPhase::Move, x, y } => {
				if let Some((_, last, _)) = &mut finger {
					*last = (x, y);
				}
				Ok(())
			}
			In::Touch { phase: TouchPhase::End, .. } => match finger.take() {
				Some((from, to, _)) if (to.0 - from.0).hypot(to.1 - from.1) < TAP_SLOP => phone.sh(&["input", "tap", &px(from.0), &px(from.1)]).await.map(drop),
				Some((from, to, at)) => {
					let ms = at.elapsed().as_millis().clamp(50, 3000).to_string();
					phone.sh(&["input", "swipe", &px(from.0), &px(from.1), &px(to.0), &px(to.1), &ms]).await.map(drop)
				}
				None => Ok(()),
			},
			// a phone scrolls under a finger: the wheel's turn as a swipe the other way
			In::Wheel { x, y, dx, dy } => phone.sh(&["input", "swipe", &px(x), &px(y), &px(x - dx), &px(y - dy), "150"]).await.map(drop),
			In::Key { key, ctrl: true } if key == "a" => phone.sh(&["input", "keycombination", "113", "29"]).await.map(drop),
			In::Key { key, .. } => match keycode(&key) {
				Some(code) => phone.sh(&["input", "keyevent", code]).await.map(drop),
				None => Err(eyre::eyre!("the key {key:?} is not passed")),
			},
			In::Text { text } => phone.text(&text).await,
			In::Navigate { url } => match web_url(&url) {
				Some(url) => phone.sh(&["am", "start", "-a", "android.intent.action.VIEW", "-d", &url]).await.map(drop),
				None => Err(eyre::eyre!("{url:?} is not a web address")),
			},
			In::History { delta: -1 } => phone.sh(&["input", "keyevent", "4"]).await.map(drop),
			In::History { .. } => Err(eyre::eyre!("a phone goes back only")),
			In::Activate { .. } | In::NewTab | In::CloseTab { .. } | In::Dialog { .. } => Err(eyre::eyre!("a phone has no tabs nor page dialogs")),
		};
		if let Err(e) = done
			&& say(&mut socket, &Out::Error { message: format!("{e:#}") }).await.is_err()
		{
			return;
		}
	}
}

/// A point, in whole screen px.
fn px(v: f64) -> String {
	format!("{}", v.round().max(0.0) as i64)
}

async fn say(socket: &mut WebSocket, out: &Out) -> Result<(), ()> {
	let text = serde_json::to_string(out).expect("a derived Serialize of strings and numbers");
	socket.send(ws::Message::Text(text.into())).await.map_err(drop)
}
