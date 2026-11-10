//! One gmail's browser, from its launch to its stop: a future of `serve`'s, its process group
//! killed however the future ends.
//!
//! ```text
//! Starting ── devtools answer ──▶ Up ── its window's process exits | stop | idle | the runner stops ──▶ gone
//!     └── exits, or no devtools within 60 s ─────────────────────────────────────────────────────────▶ gone
//! ```

use std::{path::Path, sync::Arc, time::Duration};

use eyre::WrapErr;
use sa_browsers::{Browser, Launch};
use serde_json::Value;
use tokio::sync::{Notify, oneshot, watch};

use crate::Runner;

/// How long after its last devtools client leaves (or its launch, if none came) a browser runs on.
const IDLE: Duration = Duration::from_secs(10 * 60);
const UP_WITHIN: Duration = Duration::from_secs(60);
const STOP_WITHIN: Duration = Duration::from_secs(15);

/// A browser in the runner's table, from its launch until it is gone.
pub(crate) struct Slot {
	pub(crate) run: String,
	state: State,
	stop: Arc<Notify>,
	ended: watch::Receiver<bool>,
	clients: Arc<watch::Sender<usize>>,
}

enum State {
	Starting(Vec<oneshot::Sender<Result<(), String>>>),
	Up { devtools: String },
}

/// What `serve` runs: the launch, and the slot's other ends.
pub(crate) struct Start {
	gmail: i64,
	launch: Launch,
	stop: Arc<Notify>,
	ended: watch::Sender<bool>,
	clients: watch::Receiver<usize>,
}

/// A devtools client of a browser's, counted while it lives.
pub(crate) struct Client(Arc<watch::Sender<usize>>);

impl Drop for Client {
	fn drop(&mut self) {
		self.0.send_modify(|n| *n -= 1);
	}
}

impl Slot {
	pub(crate) fn starting(gmail: i64, launch: Launch) -> (Self, Start) {
		let stop = Arc::new(Notify::new());
		let (ended_tx, ended) = watch::channel(false);
		let (clients, clients_rx) = watch::channel(0);
		let slot = Self {
			run: launch.run.clone(),
			state: State::Starting(Vec::new()),
			stop: stop.clone(),
			ended,
			clients: Arc::new(clients),
		};
		let start = Start {
			gmail,
			launch,
			stop,
			ended: ended_tx,
			clients: clients_rx,
		};
		(slot, start)
	}

	/// Answered once its devtools answer, or with why it never will.
	pub(crate) fn wait_up(&mut self) -> oneshot::Receiver<Result<(), String>> {
		let (tx, rx) = oneshot::channel();
		match &mut self.state {
			State::Starting(waiters) => waiters.push(tx),
			State::Up { .. } => tx.send(Ok(())).expect("its receiver is right here"),
		}
		rx
	}

	/// Its devtools socket, once up.
	pub(crate) fn client(&self) -> Option<(String, Client)> {
		let State::Up { devtools } = &self.state else { return None };
		self.clients.send_modify(|n| *n += 1);
		Some((devtools.clone(), Client(self.clients.clone())))
	}

	/// Told to stop; the receiver turns `true` once it is gone.
	pub(crate) fn stop(&self) -> watch::Receiver<bool> {
		self.stop.notify_one();
		self.ended.clone()
	}
}

/// The browser's life; its slot leaves the table when it ends.
pub(crate) async fn run(runner: Runner, start: Start, mut stopping: watch::Receiver<bool>) {
	let Start {
		gmail,
		launch,
		stop,
		ended,
		mut clients,
	} = start;
	let dir = runner.0.dir.join(gmail.to_string());
	let lived = life(&runner, gmail, &dir, launch.browser, &stop, &mut clients, &mut stopping).await;
	let scrubbed = scrub(&dir).await;
	let why = match lived.and(scrubbed) {
		Ok(()) => "it stopped while starting".to_owned(),
		Err(e) => {
			tracing::error!(gmail, error = format!("{e:#}"), "browser");
			format!("{e:#}")
		}
	};
	let slot = runner.0.profiles.lock().expect("never held across a panic").remove(&gmail).expect("a slot leaves only here");
	if let State::Starting(waiters) = slot.state {
		for w in waiters {
			// the asker gave up waiting: nobody to tell
			let _told = w.send(Err(why.clone()));
		}
	}
	tracing::info!(gmail, "browser stopped");
	ended.send_replace(true);
}

async fn life(runner: &Runner, gmail: i64, dir: &Path, browser: Browser, stop: &Notify, clients: &mut watch::Receiver<usize>, stopping: &mut watch::Receiver<bool>) -> eyre::Result<()> {
	let Browser::Orbita {
		preferences,
		config,
		proxy,
		zone,
		locale,
		screen: (width, height),
	} = browser;
	let prefs = dir.join("Default/Preferences");
	tokio::fs::create_dir_all(dir.join("Default")).await.wrap_err_with(|| format!("creating {}", dir.display()))?;
	let mut merged = match tokio::fs::read(&prefs).await {
		Ok(b) => serde_json::from_slice(&b).wrap_err_with(|| format!("{} is not JSON", prefs.display()))?,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Object(Default::default()),
		Err(e) => return Err(eyre::Report::new(e).wrap_err(format!("reading {}", prefs.display()))),
	};
	merge(&mut merged, preferences);
	tokio::fs::write(&prefs, serde_json::to_vec(&merged)?).await.wrap_err("writing the preferences")?;
	tokio::fs::write(dir.join("orbita.config"), serde_json::to_vec(&config)?)
		.await
		.wrap_err("writing orbita.config")?;
	let port_file = dir.join("DevToolsActivePort");
	match tokio::fs::remove_file(&port_file).await {
		Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(eyre::Report::new(e).wrap_err("removing a stale DevToolsActivePort")),
		_ => {}
	}
	let log = std::fs::File::create(dir.join("orbita.log")).wrap_err("creating orbita.log")?;
	let mut child = tokio::process::Command::new(&runner.0.orbita)
		.current_dir(dir) // its crash reporter writes into its cwd
		.args([
			"--headless=new".to_owned(),
			"--remote-debugging-port=0".into(),
			"--password-store=basic".into(),
			"--no-first-run".into(),
			format!("--proxy-server={proxy}"),
			format!("--tz={zone}"),
			format!("--lang={locale}"),
			format!("--window-size={width},{height}"),
			format!("--user-data-dir={}", dir.display()),
			// what GoLogin's own launcher passes for android profiles
			"--font-masking-mode=1".into(),
		])
		.stdin(std::process::Stdio::null())
		.stdout(log.try_clone()?)
		.stderr(log)
		.process_group(0)
		.spawn()
		.wrap_err_with(|| format!("starting {}", runner.0.orbita.display()))?;
	let group = Group(rustix::process::Pid::from_raw(child.id().expect("not waited on yet").try_into().expect("a pid fits an i32")).expect("a child's pid is positive"));
	let up = async {
		//LOOP: polled until the file names a port; UP_WITHIN bounds it
		loop {
			if let Ok(s) = tokio::fs::read_to_string(&port_file).await
				&& let [port, path, ..] = s.lines().collect::<Vec<_>>()[..]
			{
				return format!("ws://127.0.0.1:{port}{path}");
			}
			tokio::time::sleep(Duration::from_millis(200)).await;
		}
	};
	let devtools = tokio::select! {
		d = tokio::time::timeout(UP_WITHIN, up) => d.map_err(|_| eyre::eyre!("no devtools within {}s; see {}/orbita.log", UP_WITHIN.as_secs(), dir.display()))?,
		status = child.wait() => eyre::bail!("it exited ({}) before it was up; see {}/orbita.log", status?, dir.display()),
	};
	{
		let mut profiles = runner.0.profiles.lock().expect("never held across a panic");
		let slot = profiles.get_mut(&gmail).expect("a slot leaves only at its future's end");
		let State::Starting(waiters) = std::mem::replace(&mut slot.state, State::Up { devtools }) else {
			unreachable!("only this future moves it up");
		};
		for w in waiters {
			// the asker gave up waiting: it is up all the same
			let _told = w.send(Ok(()));
		}
	}
	tracing::info!(gmail, "browser up");
	let idle = async {
		//LOOP: until nobody has held its devtools for IDLE
		loop {
			if *clients.borrow_and_update() == 0 {
				tokio::select! {
					() = tokio::time::sleep(IDLE) => return,
					r = clients.changed() => r.expect("the slot holds the sender"),
				}
			} else {
				clients.changed().await.expect("the slot holds the sender");
			}
		}
	};
	tokio::select! {
		status = child.wait() => {
			tracing::info!(gmail, status = %status?, "browser exited");
			return Ok(());
		}
		() = stop.notified() => tracing::info!(gmail, "browser told to stop"),
		() = idle => tracing::info!(gmail, "browser idle: stopped"),
		_ = stopping.wait_for(|s| *s) => {}
	}
	// the browser itself shuts down cleanly on TERM, its last writes to the session kept
	group.signal(rustix::process::Signal::TERM);
	if tokio::time::timeout(STOP_WITHIN, child.wait()).await.is_err() {
		tracing::warn!(gmail, "browser took over {}s to stop: killed", STOP_WITHIN.as_secs());
	}
	Ok(())
}

/// What a launch brought leaves the disk with the run: the profile token and the proxy's
/// credentials.
async fn scrub(dir: &Path) -> eyre::Result<()> {
	match tokio::fs::remove_file(dir.join("orbita.config")).await {
		Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(eyre::Report::new(e).wrap_err("removing orbita.config")),
		_ => {}
	}
	let prefs = dir.join("Default/Preferences");
	let mut v: Value = match tokio::fs::read(&prefs).await {
		Ok(b) => serde_json::from_slice(&b).wrap_err_with(|| format!("{} is not JSON", prefs.display()))?,
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
		Err(e) => return Err(eyre::Report::new(e).wrap_err(format!("reading {}", prefs.display()))),
	};
	if let Some(g) = v.get_mut("gologin").and_then(Value::as_object_mut) {
		g.remove("proxy");
		g.remove("profile_token");
	}
	tokio::fs::write(&prefs, serde_json::to_vec(&v)?).await.wrap_err("writing the preferences")
}

/// RFC 7386: `patch` onto `target`, a `null` removing.
fn merge(target: &mut Value, patch: Value) {
	let Value::Object(patch) = patch else {
		*target = patch;
		return;
	};
	if !target.is_object() {
		*target = Value::Object(Default::default());
	}
	let t = target.as_object_mut().expect("made one above");
	for (k, v) in patch {
		if v.is_null() {
			t.remove(&k);
		} else {
			merge(t.entry(k).or_insert(Value::Null), v);
		}
	}
}

/// The browser's process group: its renderers and helpers too. Killed when dropped, however
/// [`life`] ends.
struct Group(rustix::process::Pid);

impl Group {
	fn signal(&self, signal: rustix::process::Signal) {
		match rustix::process::kill_process_group(self.0, signal) {
			Ok(()) | Err(rustix::io::Errno::SRCH) => {} // SRCH: every one of them has exited
			Err(e) => panic!("signalling our own child's group {:?}: {e}", self.0),
		}
	}
}

impl Drop for Group {
	fn drop(&mut self) {
		self.signal(rustix::process::Signal::KILL);
	}
}
