//! One gmail's browser, from its launch to its stop: a future of `serve`'s. Orbita's life is here,
//! its process group killed however the future ends; Mimic's in `mimic`, a cloud phone's in `phone`.
//!
//! ```text
//! Starting ── devtools (a phone: adb) answer ──▶ Up ── it ends | stop | idle | the runner stops ──▶ gone
//!     └── it ends, or is not up in time ───────────────────────────────────────────────────────────▶ gone
//! ```

use std::{path::Path, sync::Arc, time::Duration};

use eyre::WrapErr;
use sa_browsers::{Browser, Launch};
use serde_json::Value;
use tokio::sync::{Notify, oneshot, watch};

use crate::{Runner, film, mimic, phone};

/// How long after its last client leaves (or its launch, if none came) a browser runs on.
pub(crate) const IDLE: Duration = Duration::from_secs(10 * 60);
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

	/// What it is driven through, once up.
	pub(crate) fn client(&self) -> Option<(Up, Client)> {
		let State::Up(up) = &self.state else { return None };
		self.clients.send_modify(|n| *n += 1);
		Some((up.clone(), Client(self.clients.clone())))
	}

	/// A cloud phone's own view at Multilogin.
	pub(crate) fn stream(&self) -> Option<String> {
		match &self.state {
			State::Up(Up::Phone { stream, .. }) => stream.clone(),
			_ => None,
		}
	}

	/// Told to stop; the receiver turns `true` once it is gone.
	pub(crate) fn stop(&self) -> watch::Receiver<bool> {
		self.stop.notify_one();
		self.ended.clone()
	}
}

enum State {
	Starting(Vec<oneshot::Sender<Result<(), String>>>),
	Up(Up),
}

/// What a running browser is driven through.
#[derive(Clone)]
pub(crate) enum Up {
	Devtools(String),
	Phone { adb: Arc<crate::adb::Phone>, stream: Option<String> },
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
	let mut ends = Ends {
		stop: &stop,
		clients: &mut clients,
		stopping: &mut stopping,
	};
	let lived = match launch.browser {
		Browser::Orbita {
			preferences,
			config,
			proxy,
			zone,
			locale,
			screen,
		} =>
			orbita(
				&runner,
				gmail,
				&dir,
				Orbita {
					preferences,
					config,
					proxy,
					zone,
					locale,
					screen,
				},
				&mut ends,
			)
			.await,
		Browser::MultiloginBrowser { token, folder, profile } => mimic::life(&runner, gmail, &token, &folder, &profile, &mut ends).await,
		Browser::MultiloginPhone { token, phone, geo, zone, country } => phone::life(&runner, gmail, phone::Run { token, phone, geo, zone, country }, &mut ends).await,
	};
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

/// What ends a running browser but itself.
pub(crate) struct Ends<'a> {
	stop: &'a Notify,
	clients: &'a mut watch::Receiver<usize>,
	stopping: &'a mut watch::Receiver<bool>,
}

impl Ends<'_> {
	/// Why it is to stop: told to, nobody held it for `idle`, or the runner stops.
	pub(crate) async fn any(&mut self, gmail: i64, idle: Duration) {
		let clients = &mut *self.clients;
		let unheld = async {
			//LOOP: until nobody has held it for `idle`
			loop {
				if *clients.borrow_and_update() == 0 {
					tokio::select! {
						() = tokio::time::sleep(idle) => return,
						r = clients.changed() => r.expect("the slot holds the sender"),
					}
				} else {
					clients.changed().await.expect("the slot holds the sender");
				}
			}
		};
		tokio::select! {
			() = self.stop.notified() => tracing::info!(gmail, "browser told to stop"),
			() = unheld => tracing::info!(gmail, "browser idle: stopped"),
			_ = self.stopping.wait_for(|s| *s) => {}
		}
	}
}

/// It is up: its launch's waiters told.
pub(crate) fn up(runner: &Runner, gmail: i64, up: Up) {
	let mut profiles = runner.0.profiles.lock().expect("never held across a panic");
	let slot = profiles.get_mut(&gmail).expect("a slot leaves only at its future's end");
	let State::Starting(waiters) = std::mem::replace(&mut slot.state, State::Up(up)) else {
		unreachable!("only this future moves it up");
	};
	for w in waiters {
		// the asker gave up waiting: it is up all the same
		let _told = w.send(Ok(()));
	}
	tracing::info!(gmail, "browser up");
}

struct Orbita {
	preferences: Value,
	config: Value,
	proxy: String,
	zone: String,
	locale: String,
	screen: (u32, u32),
}

async fn orbita(runner: &Runner, gmail: i64, dir: &Path, o: Orbita, ends: &mut Ends<'_>) -> eyre::Result<()> {
	let Orbita {
		preferences,
		config,
		proxy,
		zone,
		locale,
		screen: (width, height),
	} = o;
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
	let with_film = film::present(dir);
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
		.args(if with_film { film::args().to_vec() } else { vec![] })
		.stdin(std::process::Stdio::null())
		.stdout(log.try_clone()?)
		.stderr(log)
		.process_group(0)
		.spawn()
		.wrap_err_with(|| format!("starting {}", runner.0.orbita.display()))?;
	let group = Group::of(&child);
	let port = async {
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
	let devtools: String = tokio::select! {
		d = tokio::time::timeout(UP_WITHIN, port) => d.map_err(|_| eyre::eyre!("no devtools within {}s; see {}/orbita.log", UP_WITHIN.as_secs(), dir.display()))?,
		status = child.wait() => eyre::bail!("it exited ({}) before it was up; see {}/orbita.log", status?, dir.display()),
	};
	up(runner, gmail, Up::Devtools(devtools.clone()));
	let facing = async {
		match with_film {
			true => film::facing(&devtools).await,
			false => std::future::pending().await,
		}
	};
	tokio::select! {
		r = facing => {
			// a paused page would never run without it
			if let Err(e) = r {
				group.signal(rustix::process::Signal::TERM);
				let _stopped = tokio::time::timeout(STOP_WITHIN, child.wait()).await;
				return Err(e.wrap_err("facing.js's devtools broke off: stopped"));
			}
		}
		status = child.wait() => {
			tracing::info!(gmail, status = %status?, "browser exited");
			return Ok(());
		}
		() = ends.any(gmail, IDLE) => {}
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
/// its owner ends.
pub(crate) struct Group(pub(crate) rustix::process::Pid);

impl Group {
	pub(crate) fn of(child: &tokio::process::Child) -> Self {
		Self(rustix::process::Pid::from_raw(child.id().expect("not waited on yet").try_into().expect("a pid fits an i32")).expect("a child's pid is positive"))
	}

	pub(crate) fn signal(&self, signal: rustix::process::Signal) {
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
