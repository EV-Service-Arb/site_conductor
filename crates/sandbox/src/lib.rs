//! The sandbox worker: model-written Starlark, one session's cells in turn, in a child of the conductor's
//! own binary sealed before it reads one (docs/ARCHITECTURE.md, Code mode). Its one way out is `call`, a
//! line to the conductor, which runs the operation as the session's caller and answers on the worker's
//! stdin.
//!
//! ```text
//! conductor ─ Down::Cell(code), Down::Answer ─ a JSON line each ─▶ worker's stdin
//! conductor ◀─ Up::Print, Up::Call, Up::{Done, Threw, Died} ──────── worker's stdout
//! ```

use std::{cell::Cell, io::BufRead, rc::Rc, time::Duration};

use schemars::JsonSchema;
use seccompiler::{BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompFilter, SeccompRule};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};
use starlark::{
	PrintHandler,
	codemap::FileSpanRef,
	environment::{FrozenModule, GlobalsBuilder, LibraryExtension, Module},
	eval::{BeforeStmtFuncDyn, Evaluator},
	starlark_module,
	syntax::{AstModule, Dialect},
	values::{Heap, Value},
};

/// A cell's run: past it, its session is killed, and its state with it.
pub const LIMIT: Duration = Duration::from_secs(10 * 60);
/// How long an answer waits for a cell to end, or to say more.
pub const WAIT: Duration = Duration::from_secs(20);
/// A session no cell ran in for this long is let go.
pub const IDLE: Duration = Duration::from_secs(30 * 60);
/// What is kept of each of a cell's streams.
pub const OUTPUT: usize = 64 << 10;
/// The operations one cell may call.
pub const CALLS: usize = 100;
/// Sessions held at once, each a worker: `MAX` × the worker's memory is the sandbox's share of the pod.
pub const MAX: usize = 4;
/// What a session's Starlark heaps may hold: past it, the session dies.
pub const HEAP: usize = 24 << 20;
/// The conductor's binary, started with this as its one argument, is a worker.
pub const WORKER: &str = "__sa_sandbox_worker";
/// Starlark frames deep.
const STACK: usize = 100;

/// The conductor's word to a worker.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Down {
	/// Code to run, once the cell before it ended.
	Cell(String),
	/// What the call the worker waits on answered.
	Answer(Answer),
}

/// A worker's word to the conductor.
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Up {
	Print(String),
	/// The cell waits for its `Down::Answer`.
	Call(Call),
	Done,
	/// What the cell failed on.
	Threw(String),
	/// Past [`HEAP`]: the worker exits after it.
	Died(String),
}

/// One operation of the catalog, as `search` names it, the caller's to run.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
	/// An operation's id, as `search` names it.
	pub operation: String,
	/// The path's parameters by name: `{"id": 7}`.
	pub path: Option<Map<String, Json>>,
	/// The query's parameters by name.
	pub query: Option<Map<String, Json>>,
	/// The JSON body, for an operation that takes one.
	pub body: Option<Json>,
}

/// What an operation answered.
#[derive(Debug, Deserialize, Serialize)]
pub struct Answer {
	pub status: u16,
	pub body: Json,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ended {
	/// Ran to its end: what it bound at its top level, later cells see.
	Done,
	/// It failed: stderr says what. Nothing it bound is seen after it.
	Threw,
	/// A call answered that the caller's session is over: the session ended with it.
	SignedOut,
	/// Still running at [`LIMIT`]: killed, with its session.
	Killed,
	/// Its worker died under it: a signal, or past [`HEAP`]. The session is gone.
	Died,
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

/// The worker's whole life, on its stdin and stdout: sealed first, it runs nothing if it cannot be.
pub fn worker() -> ! {
	if let Err(e) = seal() {
		eprintln!("the worker is not sealed, so it runs nothing: {e}");
		std::process::exit(2);
	}
	let globals = GlobalsBuilder::extended_by(&[LibraryExtension::Json, LibraryExtension::Print]).with(host).build();
	let dialect = Dialect {
		enable_load: false,
		enable_top_level_stmt: true,
		enable_f_strings: true,
		..Dialect::Standard
	};
	// each cell that ended Done, in order: a later one's name hides an earlier one's
	let mut kept: Vec<FrozenModule> = Vec::new();
	//LOOP: a cell a turn, until the conductor closes the worker's stdin
	loop {
		let Down::Cell(code) = receive() else {
			panic!("an answer comes only to a call");
		};
		let retained: usize = kept.iter().map(|m| m.frozen_heap().allocated_bytes()).sum();
		let ast = match AstModule::parse("cell", code, &dialect) {
			Ok(ast) => ast,
			Err(e) => {
				send(&Up::Threw(e.to_string()));
				continue;
			}
		};
		let module = Module::new();
		for done in &kept {
			module.import_public_symbols(done);
		}
		let past = Rc::new(Cell::new(false));
		let mut eval = Evaluator::new(&module);
		eval.set_print_handler(&Printer);
		eval.set_max_callstack_size(STACK).expect("set once");
		let cap: Box<dyn BeforeStmtFuncDyn> = Box::new(Cap {
			room: HEAP.saturating_sub(retained),
			past: past.clone(),
		});
		eval.before_stmt_for_dap(cap.into());
		let result = eval.eval_module(ast, &globals);
		drop(eval);
		let ran = match result {
			Ok(_) => Ok(module.freeze().map_err(|e| format!("{e:?}"))),
			Err(e) if past.get() => Err(format!("its memory passed {} MiB: {e}", HEAP >> 20)),
			Err(e) => Ok(Err(e.to_string())),
		};
		match ran {
			Ok(Ok(frozen)) => {
				kept.push(frozen);
				send(&Up::Done);
			}
			Ok(Err(e)) => send(&Up::Threw(e)),
			Err(e) => {
				send(&Up::Died(e));
				std::process::exit(3);
			}
		}
	}
}

/// Caps the process (its data, its CPU, its files), then lets it make no syscall but reading its stdin,
/// writing its stdout and stderr, its own memory and time, and exiting: anything else kills it (SIGSYS).
pub fn seal() -> Result<(), String> {
	use rustix::process::{Resource, Rlimit, setrlimit};

	let limit = |resource, at: u64| {
		setrlimit(
			resource,
			Rlimit {
				current: Some(at),
				maximum: Some(at),
			},
		)
		.map_err(|e| format!("{resource:?}: {e}"))
	};
	if let Some((k, _)) = std::env::vars_os().next() {
		return Err(format!("its environment is not empty: {}", k.display()));
	}
	let status = std::fs::read_to_string("/proc/self/status").map_err(|e| format!("/proc/self/status: {e}"))?;
	let data: u64 = status
		.lines()
		.find_map(|l| l.strip_prefix("VmData:"))
		.and_then(|v| v.trim().strip_suffix("kB")?.trim().parse().ok())
		.ok_or("/proc/self/status names no VmData")?;
	// what the binary holds already, and the heap twice over: the allocator's slack, an answer parsed beside the value made of it
	limit(Resource::Data, data * 1024 + 2 * HEAP as u64)?;
	limit(Resource::Cpu, LIMIT.as_secs())?;
	limit(Resource::Nofile, 3)?;
	let filter: BpfProgram = filter()?;
	seccompiler::apply_filter(&filter).map_err(|e| format!("the seccomp filter: {e}"))
}

fn filter() -> Result<BpfProgram, String> {
	let fd = |n: u64| SeccompRule::new(vec![SeccompCondition::new(0, SeccompCmpArgLen::Dword, SeccompCmpOp::Eq, n).expect("a valid condition")]).expect("a valid rule");
	let any = Vec::new;
	let rules = [
		(libc::SYS_read, vec![fd(0)]),
		(libc::SYS_write, vec![fd(1), fd(2)]),
		(libc::SYS_mmap, any()),
		(libc::SYS_munmap, any()),
		(libc::SYS_mremap, any()),
		(libc::SYS_mprotect, any()),
		(libc::SYS_madvise, any()),
		(libc::SYS_brk, any()),
		(libc::SYS_futex, any()),
		(libc::SYS_clock_gettime, any()),
		(libc::SYS_getrandom, any()),
		(libc::SYS_rt_sigreturn, any()),
		(libc::SYS_sigaltstack, any()),
		(libc::SYS_rt_sigprocmask, any()), // glibc's vDSO getrandom masks signals around its state's first use
		(libc::SYS_exit, any()),
		(libc::SYS_exit_group, any()),
	];
	let arch = std::env::consts::ARCH.try_into().map_err(|e| format!("seccomp on {}: {e}", std::env::consts::ARCH))?;
	SeccompFilter::new(rules.into_iter().collect(), SeccompAction::KillProcess, SeccompAction::Allow, arch)
		.and_then(TryInto::try_into)
		.map_err(|e| format!("the seccomp filter: {e}"))
}

fn send(up: &Up) {
	use std::io::Write;

	let mut out = std::io::stdout().lock();
	serde_json::to_writer(&mut out, up).expect("the wire serializes");
	writeln!(out).and_then(|()| out.flush()).unwrap_or_else(|_| std::process::exit(0)); // the conductor let go of the session
}

fn receive() -> Down {
	let mut line = String::new();
	match std::io::stdin().lock().read_line(&mut line) {
		Ok(0) => std::process::exit(0), // the conductor let go of the session
		Ok(_) => serde_json::from_str(&line).expect("the conductor speaks the wire"),
		Err(e) => panic!("the worker's stdin: {e}"),
	}
}

/// Ends the cell before a statement once its heap holds more than `room`.
struct Cap {
	room: usize,
	past: Rc<Cell<bool>>,
}

impl<'a, 'e: 'a> BeforeStmtFuncDyn<'a, 'e> for Cap {
	fn call<'v>(&mut self, _: FileSpanRef, eval: &mut Evaluator<'v, 'a, 'e>) -> starlark::Result<()> {
		if eval.heap().allocated_bytes() + eval.frozen_heap().allocated_bytes() <= self.room {
			return Ok(());
		}
		self.past.set(true);
		Err(anyhow::anyhow!("its heap holds more than {} MiB", HEAP >> 20).into())
	}
}

struct Printer;

impl PrintHandler for Printer {
	fn println(&self, text: &str) -> starlark::Result<()> {
		let mut cut = OUTPUT.min(text.len());
		while !text.is_char_boundary(cut) {
			cut -= 1;
		}
		send(&Up::Print(format!("{}\n", &text[..cut]))); // no more of a cell's output is kept
		Ok(())
	}
}

#[starlark_module]
fn host(builder: &mut GlobalsBuilder) {
	/// Runs an operation of the catalog as the session's caller: `{status, body}`, the conductor's own
	/// refusals among them (403, `body.code == "refused"`).
	fn call<'v>(
		#[starlark(require = pos)] operation: &str,
		#[starlark(require = named)] path: Option<Value<'v>>,
		#[starlark(require = named)] query: Option<Value<'v>>,
		#[starlark(require = named)] body: Option<Value<'v>>,
		heap: &'v Heap,
	) -> anyhow::Result<Value<'v>> {
		let given = |v: Option<Value<'v>>| v.filter(|v| !v.is_none()).map(Value::to_json_value).transpose();
		let map = |what: &str, v: Option<Value<'v>>| -> anyhow::Result<Option<Map<String, Json>>> {
			match given(v)? {
				None => Ok(None),
				Some(Json::Object(m)) => Ok(Some(m)),
				Some(other) => anyhow::bail!("{what} is a dict of names, not {other}"),
			}
		};
		send(&Up::Call(Call {
			operation: operation.to_owned(),
			path: map("path", path)?,
			query: map("query", query)?,
			body: given(body)?,
		}));
		let Down::Answer(a) = receive() else {
			panic!("a call is answered before anything else is said");
		};
		Ok(heap.alloc(serde_json::json!({ "status": a.status, "body": a.body })))
	}
}
