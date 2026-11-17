//! A run of model-written code, as the conductor hands it to the runner and is answered. The code
//! reaches the operations only back through the conductor's `/api/run`, with the run's token, which
//! the conductor alone maps to its caller (docs/ARCHITECTURE.md, Code mode).

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A run's type check and run together: past it, it is killed and its token is dead.
pub const LIMIT: Duration = Duration::from_secs(60);
/// What is kept of each of a run's streams.
pub const OUTPUT: usize = 64 << 10;
/// The operations one run may call.
pub const CALLS: usize = 100;
/// Marks the conductor's own refusal of a run's request (not the operation's): the run stops there.
pub const REFUSED: &str = "x-sa-run-refused";

#[derive(Debug, Deserialize, Serialize)]
pub struct Run {
	/// TypeScript, type-checked against the catalog before it runs; `api` is in scope.
	pub code: String,
	/// The bearer of the run's requests back to the conductor.
	pub token: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Ran {
	pub ended: Ended,
	pub stdout: String,
	pub stderr: String,
	/// A stream ran past [`OUTPUT`]: what follows it is not kept.
	pub truncated: bool,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ended {
	/// Its exit code: 1 for a type error or an uncaught throw.
	Exited(i32),
	/// Ended by a signal it did not get from the runner (out of memory, say).
	Signalled(i32),
	/// Still running at [`LIMIT`].
	Killed,
}
