//! `execute` and `poll`: an agent's Starlark, in a session of its own, each session a worker (sa_sandbox):
//! this binary started again, sealed before it reads a cell (docs/ARCHITECTURE.md, Code mode). A session is
//! its chat's and asker's, or its MCP session's, and never another's: what its cells bound holds what
//! that caller's permissions fetched. A cell's calls come up its worker's stdout and run here as the
//! caller; one the approval level holds waits for the person, and the cell with it.

use std::{
	collections::HashMap,
	path::PathBuf,
	process::Stdio,
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use axum::http::HeaderMap;
use futures::{FutureExt, StreamExt, future::BoxFuture, stream::FuturesUnordered};
use sa_core::agent::Approval;
use sa_sandbox::{Answer, CALLS, Call, Down, Ended, IDLE, LIMIT, MAX, OUTPUT, Until, Up, WAIT, WORKER};
use serde::Serialize;
use tokio::{
	io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
	process::{Child, ChildStdin},
	sync::{Notify, mpsc, oneshot, watch},
};
use uuid::Uuid;

use crate::tools::{Gate, Tools};

/// How long a held call waits for the person before it is refused in the code.
pub const PAUSE: Duration = Duration::from_secs(10 * 60);

/// The longest line a worker says: a call's body, or what it printed, which it cuts at [`OUTPUT`].
const LINE: usize = 2 * crate::tools::ANSWER;

const GONE: &str =
	"the sandbox no longer holds this session (idle too long, made room for another, or its worker ended): what earlier cells bound is gone. The next execute starts a new one.";

/// The sessions and their workers.
#[derive(Clone)]
pub struct Sandbox(Arc<Inner>);
impl Sandbox {
	/// Sessions whose workers are `exe` (this binary: `current_exe`, or a test's `CARGO_BIN_EXE_panel`).
	pub fn new(exe: PathBuf) -> Self {
		let (started, to_drive) = mpsc::unbounded_channel();
		Self(Arc::new(Inner {
			exe,
			sessions: Mutex::default(),
			started,
			to_drive: Mutex::new(Some(to_drive)),
		}))
	}

	/// Drives every session's worker until `stopped`: its cells' output, their calls and their ends.
	/// The workers are killed with it.
	pub async fn run(&self, mut stopped: watch::Receiver<bool>) {
		let mut started = self.0.to_drive.lock().expect("never held across a panic").take().expect("the sandbox is run once");
		let mut workers = FuturesUnordered::new();
		//LOOP: until the server stops
		loop {
			tokio::select! {
				Some(worker) = started.recv() => workers.push(worker),
				Some(()) = workers.next(), if !workers.is_empty() => {}
				_ = stopped.changed() => return,
			}
		}
	}

	/// Runs `code` as a new cell of `key`'s session, opened if there is none, as the caller
	/// `credentials` names; its calls held as `approval` holds them, waiting for the person when
	/// `pause`. Answers once it ended, paused, or [`WAIT`] passed.
	pub(crate) async fn execute(&self, tools: &Tools, key: Key, credentials: HeaderMap, code: String, approval: Approval, pause: bool) -> Result<Told, String> {
		let held = self.0.sessions.lock().expect("never held across a panic").get(&key).cloned();
		let s = match held {
			Some(s) => s,
			None => self.open(key.clone(), tools)?,
		};
		{
			let mut st = s.state.lock().expect("never held across a panic");
			// the driver sets `gone` under the state's lock, so it marks the cell made here ended if it ends past here
			let orders = s.orders.lock().expect("never held across a panic");
			let Some(orders) = orders.as_ref().filter(|_| !st.gone) else {
				drop((orders, st));
				self.forget(&key, &s);
				return Err(GONE.into());
			};
			st.credentials = credentials;
			st.used = Instant::now();
			match &st.cell {
				Some(c) if c.held.is_some() => return Err(format!("cell {} waits for the person on a call: it goes on once they decide", c.n)),
				Some(c) if c.ended.is_none() => return Err(format!("cell {} still runs: poll it to its end first", c.n)),
				_ => {}
			}
			let n = st.cells;
			st.cells += 1;
			st.cell = Some(Cell::new(n, approval, pause));
			assert!(orders.send(code).is_ok(), "the driver holds its orders until it is gone");
		}
		Ok(wait(&s, Until::Ended).await)
	}

	/// Cell `n` of `key`'s session past what was told, once `until` holds, it paused, or [`WAIT`] passed.
	pub(crate) async fn poll(&self, key: Key, credentials: HeaderMap, n: usize, until: Until) -> Result<Told, String> {
		let s = self
			.0
			.sessions
			.lock()
			.expect("never held across a panic")
			.get(&key)
			.cloned()
			.ok_or("no session here holds a cell: execute starts one")?;
		{
			let mut st = s.state.lock().expect("never held across a panic");
			st.credentials = credentials;
			st.used = Instant::now();
			match &st.cell {
				Some(c) if c.n == n => {}
				Some(c) => return Err(format!("cell {n} is not the session's latest: cell {} is", c.n)),
				None => return Err("the session holds no cell: execute starts one".into()),
			}
		}
		Ok(wait(&s, until).await)
	}

	/// Names the in-page action that proposes the call `key`'s cell waits on.
	pub(crate) fn proposed_as(&self, key: &Key, action: String) {
		let s = self.0.sessions.lock().expect("never held across a panic").get(key).cloned();
		let Some(s) = s else { return };
		if let Some(h) = s.state.lock().expect("never held across a panic").cell.as_mut().and_then(|c| c.held.as_mut()) {
			h.action = Some(action);
		}
	}

	/// The person's word on the call `key`'s cell waits on (the one `action` proposes, when named):
	/// the cell that waited on it, and its approval level.
	pub(crate) fn decide(&self, key: &Key, action: Option<&str>, approve: bool) -> Option<(usize, Approval)> {
		let s = self.0.sessions.lock().expect("never held across a panic").get(key).cloned()?;
		let mut st = s.state.lock().expect("never held across a panic");
		let c = st.cell.as_mut()?;
		if action.is_some() && c.held.as_ref()?.action.as_deref() != action {
			return None;
		}
		let held = c.held.take()?;
		if held.decide.send(approve).is_err() {
			return None; // its wait passed as the person decided
		}
		Some((c.n, c.approval))
	}

	/// Lets go of `key`'s session: its worker is killed.
	pub(crate) fn close(&self, key: &Key) {
		let s = self.0.sessions.lock().expect("never held across a panic").remove(key);
		if let Some(s) = s {
			s.let_go();
		}
	}

	fn forget(&self, key: &Key, s: &Arc<Session>) {
		let mut sessions = self.0.sessions.lock().expect("never held across a panic");
		if sessions.get(key).is_some_and(|held| Arc::ptr_eq(held, s)) {
			sessions.remove(key);
		}
	}

	/// A session for `key`, its worker started, once there is room for it.
	fn open(&self, key: Key, tools: &Tools) -> Result<Arc<Session>, String> {
		let mut sessions = self.0.sessions.lock().expect("never held across a panic");
		sessions.retain(|_, s| {
			let st = s.state.lock().expect("never held across a panic");
			!st.gone || st.used.elapsed() <= IDLE
		});
		let mut live: Vec<(Instant, bool, Arc<Session>)> = sessions
			.values()
			.filter_map(|s| {
				let st = s.state.lock().expect("never held across a panic");
				(!st.gone).then(|| (st.used, st.cell.as_ref().is_some_and(|c| c.ended.is_none()), s.clone()))
			})
			.collect();
		live.sort_by_key(|(used, ..)| *used);
		for (used, running, s) in &live {
			if !running && used.elapsed() > IDLE {
				s.let_go();
			}
		}
		let idle: Vec<&Arc<Session>> = live.iter().filter(|(used, running, _)| !running && used.elapsed() <= IDLE).map(|(.., s)| s).collect();
		let held = live.iter().filter(|(used, running, _)| *running || used.elapsed() <= IDLE).count();
		if held >= MAX {
			let idlest = idle
				.first()
				.ok_or_else(|| format!("the sandbox is busy: each of its {MAX} sessions runs a cell. Ask again shortly."))?;
			idlest.let_go();
		}
		let mut child = tokio::process::Command::new(&self.0.exe)
			.arg(WORKER)
			.env_clear()
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.kill_on_drop(true)
			.spawn()
			.map_err(|e| format!("the sandbox's worker did not start: {e}"))?;
		let (orders, received) = mpsc::unbounded_channel();
		let s = Arc::new(Session {
			state: Mutex::new(State {
				credentials: HeaderMap::new(),
				used: Instant::now(),
				cells: 0,
				cell: None,
				gone: false,
			}),
			changed: Notify::new(),
			orders: Mutex::new(Some(orders)),
		});
		sessions.insert(key, s.clone());
		let stdin = child.stdin.take().expect("piped");
		let driven = drive(s.clone(), tools.clone(), child, stdin, received).boxed();
		assert!(
			self.0.started.send(driven).is_ok(),
			"the sandbox holds its receiver until run takes it, and run lasts as long as the server"
		);
		Ok(s)
	}
}

/// A cell as an agent is told it: what it printed since it was last told.
#[derive(Serialize)]
pub struct Told {
	pub cell: usize,
	pub state: CellState,
	pub stdout: String,
	pub stderr: String,
	pub truncated: bool,
	/// What destroys or spends and ran on its own since it was last told.
	pub ran_on_its_own: Vec<Call>,
}
impl Told {
	/// What was told, then `next`: one telling of both.
	pub(crate) fn then(self, next: Told) -> Told {
		Told {
			stdout: self.stdout + &next.stdout,
			stderr: self.stderr + &next.stderr,
			truncated: self.truncated || next.truncated,
			ran_on_its_own: [self.ran_on_its_own, next.ran_on_its_own].concat(),
			..next
		}
	}
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CellState {
	Running,
	/// Waiting for the person on this call.
	Paused(Call),
	Ended(Ended),
}

struct Inner {
	/// This binary: started with [`WORKER`], it is a session's worker.
	exe: PathBuf,
	sessions: Mutex<HashMap<Key, Arc<Session>>>,
	/// Each new session's worker, for [`Sandbox::run`] to drive.
	started: mpsc::UnboundedSender<BoxFuture<'static, ()>>,
	to_drive: Mutex<Option<mpsc::UnboundedReceiver<BoxFuture<'static, ()>>>>,
}

/// Whose a session is.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Key {
	/// The in-page assistant's: a chat, as one person asks in it (acting as another, as that pair).
	Chat { chat: Uuid, asker: Uuid, actor: Option<String> },
	/// An MCP session's.
	Mcp(String),
}

struct Session {
	state: Mutex<State>,
	/// Its cell said more, ended, or a call of it was held.
	changed: Notify,
	/// Cells' code, to its driver; dropped, the worker is killed.
	orders: Mutex<Option<mpsc::UnboundedSender<String>>>,
}
impl Session {
	fn let_go(&self) {
		self.orders.lock().expect("never held across a panic").take();
	}
}

struct State {
	/// The caller's, as of their latest request about this session.
	credentials: HeaderMap,
	used: Instant,
	/// Cells run so far: the next one's number.
	cells: usize,
	cell: Option<Cell>,
	/// Its worker ended: its last cell is told, and the next execute is told it is gone.
	gone: bool,
}

struct Cell {
	n: usize,
	approval: Approval,
	/// Whether a held call may wait for the person; else it is refused at once, its proposal in the error.
	pause: bool,
	calls: usize,
	stdout: Kept,
	stderr: Kept,
	/// How much of each stream was told.
	seen: (usize, usize),
	ended: Option<Ended>,
	held: Option<Held>,
	/// What destroys or spends and ran on its own, since it was last told.
	done: Vec<Call>,
}
impl Cell {
	fn new(n: usize, approval: Approval, pause: bool) -> Self {
		Self {
			n,
			approval,
			pause,
			calls: 0,
			stdout: Kept::default(),
			stderr: Kept::default(),
			seen: (0, 0),
			ended: None,
			held: None,
			done: Vec::new(),
		}
	}

	/// What it says past what was told; then that is told.
	fn told(&mut self) -> Told {
		let state = match (self.ended, &self.held) {
			(Some(e), _) => CellState::Ended(e),
			(None, Some(h)) => CellState::Paused(h.call.clone()),
			(None, None) => CellState::Running,
		};
		let told = Told {
			cell: self.n,
			state,
			stdout: self.stdout.text[self.seen.0..].to_owned(),
			stderr: self.stderr.text[self.seen.1..].to_owned(),
			truncated: self.stdout.cut || self.stderr.cut,
			ran_on_its_own: std::mem::take(&mut self.done),
		};
		self.seen = (self.stdout.text.len(), self.stderr.text.len());
		told
	}
}

/// A call waiting for the person.
struct Held {
	call: Call,
	/// The in-page answer's action proposing it.
	action: Option<String>,
	decide: oneshot::Sender<bool>,
}

/// A stream's first [`OUTPUT`] bytes; the rest let go.
#[derive(Default)]
struct Kept {
	text: String,
	cut: bool,
}
impl Kept {
	fn add(&mut self, more: &str) {
		let mut room = OUTPUT.saturating_sub(self.text.len()).min(more.len());
		while !more.is_char_boundary(room) {
			room -= 1;
		}
		self.text.push_str(&more[..room]);
		self.cut |= room < more.len();
	}
}

/// `s`'s cell past what was told, once `until` holds, it paused, or [`WAIT`] passed.
async fn wait(s: &Session, until: Until) -> Told {
	let deadline = tokio::time::Instant::now() + WAIT;
	//LOOP: until its cell says what `until` waits for, or WAIT passed
	loop {
		let changed = s.changed.notified();
		tokio::pin!(changed);
		changed.as_mut().enable();
		{
			let mut st = s.state.lock().expect("never held across a panic");
			let c = st.cell.as_mut().expect("a cell is waited on only once it is the session's");
			let said = c.stdout.text.len() > c.seen.0 || c.stderr.text.len() > c.seen.1;
			if c.ended.is_some() || c.held.is_some() || until == Until::Now || (until == Until::More && said) || tokio::time::Instant::now() >= deadline {
				return c.told();
			}
		}
		tokio::select! {
			() = changed => {}
			() = tokio::time::sleep_until(deadline) => {}
		}
	}
}

/// A session's worker, until it ends or is let go: its cells' code down, their words up, their calls run.
async fn drive(s: Arc<Session>, tools: Tools, mut child: Child, mut stdin: ChildStdin, mut orders: mpsc::UnboundedReceiver<String>) {
	// a line's bytes count against LINE across the reads a select cut short; reset once it is whole
	let mut stdout = BufReader::new(child.stdout.take().expect("piped")).take(LINE as u64);
	let mut line = Vec::new();
	let mut stderr = child.stderr.take().expect("piped");
	let mut buf = [0u8; 4096];
	let mut deadline: Option<tokio::time::Instant> = None;
	let mut errs = true;
	let to_cell = |f: &mut dyn FnMut(&mut Cell)| {
		if let Some(c) = s.state.lock().expect("never held across a panic").cell.as_mut() {
			f(c);
		}
		s.changed.notify_waiters();
	};
	// None: let go
	//LOOP: the worker's whole life
	let ended: Option<Ended> = loop {
		tokio::select! {
			order = orders.recv() => match order {
				Some(code) => {
					deadline = Some(tokio::time::Instant::now() + LIMIT);
					if say(&mut stdin, &Down::Cell(code)).await.is_err() {
						break Some(Ended::Died); // it exited under the cell
					}
				}
				None => break None,
			},
			read = stdout.read_until(b'\n', &mut line) => {
				let up = match read {
					Ok(0) => break Some(Ended::Died),
					Ok(_) if line.last() != Some(&b'\n') => Err(format!("the worker said a line past {LINE} bytes")),
					Ok(_) => serde_json::from_slice::<Up>(&line).map_err(|e| format!("the worker broke the wire: {e}")),
					Err(e) => Err(format!("the worker's stdout: {e}")),
				};
				line.clear();
				stdout.set_limit(LINE as u64);
				match up {
					Ok(Up::Print(text)) => to_cell(&mut |c| c.stdout.add(&text)),
					Ok(Up::Call(call)) => {
						let relayed = tokio::select! {
							r = relay(&s, &tools, call) => r,
							() = async { tokio::time::sleep_until(deadline.expect("a call comes from a running cell")).await } => break Some(Ended::Killed),
							order = orders.recv() => {
								assert!(order.is_none(), "no cell is ordered while one runs");
								break None;
							}
						};
						let Some(answer) = relayed else { break Some(Ended::SignedOut) };
						if say(&mut stdin, &Down::Answer(answer)).await.is_err() {
							break Some(Ended::Died);
						}
					}
					Ok(Up::Done) => {
						deadline = None;
						to_cell(&mut |c| c.ended = Some(Ended::Done));
					}
					Ok(Up::Threw(why)) => {
						deadline = None;
						to_cell(&mut |c| {
							c.stderr.add(&why);
							c.ended = Some(Ended::Threw);
						});
					}
					Ok(Up::Died(why)) => {
						to_cell(&mut |c| c.stderr.add(&why));
						break Some(Ended::Died);
					}
					Err(why) => {
						to_cell(&mut |c| c.stderr.add(&why));
						break Some(Ended::Died);
					}
				}
			}
			n = stderr.read(&mut buf), if errs => match n {
				Ok(0) | Err(_) => errs = false, // it exited: its stdout's end says so
				Ok(n) => to_cell(&mut |c| c.stderr.add(&String::from_utf8_lossy(&buf[..n]))),
			},
			() = async { tokio::time::sleep_until(deadline.expect("guarded")).await }, if deadline.is_some() => break Some(Ended::Killed),
		}
	};
	if let Err(e) = child.start_kill() {
		tracing::warn!("killing a sandbox worker: {e}");
	}
	let status = child.wait().await;
	let mut rest = String::new();
	if ended == Some(Ended::Died) {
		let read = (&mut stderr).take(OUTPUT as u64).read_to_string(&mut rest).await;
		if let Err(e) = read {
			rest.push_str(&format!("(its stderr: {e})"));
		}
		match status {
			Ok(status) => rest.push_str(&format!("\nthe worker ended: {status}\n")),
			Err(e) => rest.push_str(&format!("\nthe worker ended, its status unread: {e}\n")),
		}
	}
	let mut st = s.state.lock().expect("never held across a panic");
	st.gone = true;
	let killed = ended.unwrap_or(Ended::Killed);
	if let Some(c) = st.cell.as_mut().filter(|c| c.ended.is_none()) {
		c.stderr.add(&rest);
		c.held = None;
		c.ended = Some(killed);
	}
	drop(st);
	s.changed.notify_waiters();
}

async fn say(stdin: &mut ChildStdin, down: &Down) -> std::io::Result<()> {
	let mut line = serde_json::to_vec(down).expect("the wire serializes");
	line.push(b'\n');
	stdin.write_all(&line).await
}

/// `call` from `s`'s running cell, as its caller and as its level holds it: the answer, `None` when
/// the caller's session is over.
async fn relay(s: &Session, tools: &Tools, call: Call) -> Option<Answer> {
	let refused = |why: String| {
		Some(Answer {
			status: 403,
			body: serde_json::json!({ "code": "refused", "error": why }),
		})
	};
	let (credentials, approval, pause) = {
		let mut st = s.state.lock().expect("never held across a panic");
		let credentials = st.credentials.clone();
		let c = st.cell.as_mut().expect("a call comes from the running cell");
		c.calls += 1;
		if c.calls > CALLS {
			return refused(format!("a cell calls at most {CALLS} operations"));
		}
		(credentials, c.approval, c.pause)
	};
	match tools.gate(&call, approval) {
		Err(e) => return refused(e),
		Ok(Gate::Runs) => {}
		Ok(Gate::OnItsOwn) => s.state.lock().expect("never held across a panic").cell.as_mut().expect("still running").done.push(call.clone()),
		Ok(Gate::Held) => {
			if !pause {
				let proposal = tools.proposal(&call).map_or_else(|| call.operation.clone(), |p| p.to_string());
				return refused(format!(
					"not run: the person agrees to it first, and this client cannot ask them from within the code. `call` it with \"confirmed\": true once they agree: {proposal}"
				));
			}
			let (decide, decided) = oneshot::channel();
			s.state.lock().expect("never held across a panic").cell.as_mut().expect("still running").held = Some(Held {
				call: call.clone(),
				action: None,
				decide,
			});
			s.changed.notify_waiters();
			let decided = tokio::time::timeout(PAUSE, decided).await;
			s.state.lock().expect("never held across a panic").cell.as_mut().expect("still running").held = None;
			match decided {
				Ok(Ok(true)) => {}
				Ok(Ok(false)) => return refused(format!("the person declined {}: it did not run", call.operation)),
				Ok(Err(_)) => unreachable!("the held call is taken only to be decided"),
				Err(_) => return refused(format!("no one agreed to {} within {} s: it did not run", call.operation, PAUSE.as_secs())),
			}
		}
	}
	let operation = call.operation.clone();
	match tools.call(&credentials, call).await {
		Ok(a) if a.status == 401 => None,
		Ok(a) => Some(a),
		Err(e) => refused(format!("{operation}: {e}")),
	}
}
