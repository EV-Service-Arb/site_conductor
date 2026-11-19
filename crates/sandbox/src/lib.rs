//! A session of model-written code, as the conductor drives the runner and is answered. A session is
//! one process whose cells run in turn, each seeing what the ones before it declared; its code reaches
//! the operations only back through the conductor's `/api/run`, with its cell's token, which the
//! conductor alone maps to its caller (docs/ARCHITECTURE.md, Code mode).
//!
//! ```text
//! POST   /sessions                         → 201 Opened
//! POST   /sessions/{session}/cells  Cell   → Ran once it is checked: refused, or running; 409 while one runs
//! GET    /sessions/{session}/cells/{cell}?stdout=&stderr=&until=
//!                                          → Ran past what was seen, once `until` holds or WAIT passed
//! DELETE /sessions/{session}               → 204
//! a session the runner no longer holds     → 410: its state is gone
//! ```

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A cell's run: past it, its session is killed, and its state with it.
pub const LIMIT: Duration = Duration::from_secs(10 * 60);
/// How long the runner holds an answer for a cell to end, or to say more.
pub const WAIT: Duration = Duration::from_secs(20);
/// How long a cell's type check may take; the conductor waits for it on top of [`WAIT`].
pub const CHECK: Duration = Duration::from_secs(60);
/// A session no cell ran in for this long is let go.
pub const IDLE: Duration = Duration::from_secs(30 * 60);
/// What is kept of each of a cell's streams.
pub const OUTPUT: usize = 64 << 10;
/// The operations one cell may call.
pub const CALLS: usize = 100;
/// Marks the conductor's own refusal of a cell's request (not the operation's): it throws in the code.
pub const REFUSED: &str = "x-sa-run-refused";

#[derive(Debug, Deserialize, Serialize)]
pub struct Opened {
	pub session: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Cell {
	/// TypeScript, type-checked against the catalog and the earlier cells' declarations before it runs;
	/// `api` is in scope.
	pub code: String,
	/// The bearer of the cell's requests back to the conductor.
	pub token: String,
}

/// How much of a cell's streams was handed on: a poll answers what follows.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Seen {
	pub stdout: usize,
	pub stderr: usize,
}

/// What a poll waits for, at most [`WAIT`].
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Until {
	/// Its end.
	Ended,
	/// More than was seen, or its end.
	More,
	/// Nothing: what it is now.
	Now,
}

/// A poll's query: what was seen, and what to wait for.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Poll {
	pub stdout: usize,
	pub stderr: usize,
	pub until: Until,
}

/// A cell as it stands: what it printed past what was seen.
#[derive(Debug, Deserialize, Serialize)]
pub struct Ran {
	pub cell: usize,
	/// `None`: it still runs.
	pub ended: Option<Ended>,
	pub stdout: String,
	pub stderr: String,
	/// A stream ran past [`OUTPUT`]: what follows it is not kept.
	pub truncated: bool,
	/// What has been handed on now.
	pub seen: Seen,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ended {
	/// Ran to its end: what it declared, later cells see.
	Done,
	/// The type check refused it: nothing ran, and stderr says why.
	Rejected,
	/// It threw: stderr says what. Nothing it declared is seen after it.
	Threw,
	/// Its session's process exited under it (3: the caller's session ended); the state is gone.
	Exited(i32),
	/// Its session was ended by a signal it did not get from the runner (out of memory, say).
	Signalled(i32),
	/// Still running at [`LIMIT`]: killed, with its session.
	Killed,
}
