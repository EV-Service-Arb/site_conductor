//! The cells' type check, for every session of the runner: `deno lsp` (Deno's own compiler, kept warm)
//! and `names.ts`, each one long-lived process. Each session is a config scope of its own (its dir with
//! js/deno.json's bytes, a workspace folder), so its program never holds another session's files; a cell's
//! program is the cell and what it imports, the earlier cells it does not import evicted.

use std::{
	collections::{BTreeMap, HashMap, HashSet, VecDeque},
	convert::Infallible,
	fmt::Write as _,
	path::{Path, PathBuf},
	sync::Arc,
};

use eyre::WrapErr;
use futures::{Stream, StreamExt};
use sa_sandbox::CHECK;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
	io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
	process::{ChildStdin, ChildStdout},
	sync::{mpsc, oneshot},
	time::Instant,
};
use tracing::{Instrument, Span};
use url::Url;

const ERROR: u8 = 1;

/// Before the initialize request's answer, no other is sent.
const INITIALIZE: u64 = 0;

pub(crate) struct Checker {
	asks: mpsc::UnboundedSender<Ask>,
	prelude: Url,
	deno_json: Arc<[u8]>,
}
impl Checker {
	/// The checker, and the future that drives its processes: it ends only when a warm-up fails.
	pub(crate) fn try_new(deno: PathBuf, js: PathBuf, cache: PathBuf) -> eyre::Result<(Self, impl Future<Output = eyre::Result<Infallible>>)> {
		let (asks, asked) = mpsc::unbounded_channel(); //UNBOUNDED: a scope's drop cannot wait; a session asks one at a time, and SANDBOX_MAX bounds the sessions
		let deno_json: Arc<[u8]> = std::fs::read(js.join("deno.json")).wrap_err("SANDBOX_JS's deno.json")?.into();
		let prelude = Url::from_file_path(js.join("prelude.ts")).expect("canonical paths are absolute");
		let driver = Driver {
			deno,
			js,
			cache,
			deno_json: deno_json.clone(),
			prelude: prelude.clone(),
		};
		Ok((Self { asks, prelude, deno_json }, driver.drive(asked)))
	}

	/// `dir` as a config scope of its own, js/deno.json's bytes in it.
	pub(crate) fn scope(&self, dir: &Path) -> eyre::Result<Scope> {
		let dir = dir.canonicalize().wrap_err("a session's directory")?;
		std::fs::write(dir.join("deno.json"), &self.deno_json).wrap_err("a session's deno.json")?;
		Ok(Scope {
			uri: Url::from_file_path(&dir).expect("canonical paths are absolute"),
			dir,
			asks: self.asks.clone(),
		})
	}

	/// What `code` declares, once it type-checks as cell `n` of `scope` importing `names`, written there;
	/// else what the check said.
	#[tracing::instrument(skip_all, fields(cell = n))]
	pub(crate) async fn check(&self, scope: &Scope, n: usize, code: &str, names: &BTreeMap<String, (usize, bool)>) -> eyre::Result<Result<Vec<Name>, String>> {
		let deadline = Instant::now() + CHECK;
		let mut declares = async {
			let (reply, replied) = oneshot::channel();
			self.ask(Ask::Names {
				code: code.to_owned(),
				deadline,
				reply,
			});
			replied.await.wrap_err("the checker restarted")?
		}
		.instrument(tracing::info_span!("names"))
		.await?;
		declares.dedup_by(|a, b| a.name == b.name);
		let mut from: BTreeMap<usize, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
		for (name, (cell, is_type)) in names {
			if !declares.iter().any(|d| &d.name == name) {
				let (values, types) = from.entry(*cell).or_default();
				if *is_type { types } else { values }.push(name);
			}
		}
		// after the code, so its line numbers stay its own: imports hoist
		let mut module = format!("{code}\n");
		for (cell, (values, types)) in &from {
			for (names, kind) in [(values, ""), (types, "type ")] {
				if !names.is_empty() {
					writeln!(module, "import {kind}{{ {} }} from \"./cell_{cell}.ts\";", names.join(", ")).expect("a String takes writes");
				}
			}
		}
		for (is_type, kind) in [(false, ""), (true, "type ")] {
			let own: Vec<&str> = declares.iter().filter(|d| d.is_type == is_type && !d.exported).map(|d| d.name.as_str()).collect();
			if !own.is_empty() {
				writeln!(module, "export {kind}{{ {} }};", own.join(", ")).expect("a String takes writes");
			}
		}
		writeln!(module, "import {{ api }} from \"{}\";", self.prelude).expect("a String takes writes");
		let at = |k: usize| Url::from_file_path(scope.dir.join(format!("cell_{k}.ts"))).expect("canonical paths are absolute");
		let cell = at(n);
		let said = async {
			let (reply, replied) = oneshot::channel();
			self.ask(Ask::Diagnose(Box::new(Diagnose {
				scope: scope.uri.clone(),
				cell: cell.clone(),
				evict: (0..n).map(at).collect(),
				text: module.clone(),
				deadline,
				span: Span::current(),
				reply: Reply::Cell(reply),
			})));
			replied.await.wrap_err("the checker restarted")?
		}
		.instrument(tracing::info_span!("diagnostics"))
		.await?;
		let errors: Vec<Diagnostic> = said.into_iter().filter(|d| d.severity == ERROR).collect();
		if errors.is_empty() {
			std::fs::write(scope.dir.join(format!("cell_{n}.ts")), module).wrap_err("writing a cell")?;
			return Ok(Ok(declares));
		}
		Ok(Err(render(&errors, &cell, &module, scope)?))
	}

	fn ask(&self, ask: Ask) {
		self.asks.send(ask).expect("the driver runs as long as the server");
	}
}

/// A session's config scope; dropped, the checker lets go of it.
pub(crate) struct Scope {
	dir: PathBuf,
	uri: Url,
	asks: mpsc::UnboundedSender<Ask>,
}

impl Drop for Scope {
	fn drop(&mut self) {
		let _ = self.asks.send(Ask::Forget(self.uri.clone())); //IGNORED_ERROR: the driver is gone only with the runner, and its scopes with it
	}
}

/// A top-level declaration of a cell, as `js/names.ts` reads it.
#[derive(Clone, Deserialize)]
pub(crate) struct Name {
	pub(crate) name: String,
	#[serde(rename = "type")]
	pub(crate) is_type: bool,
	exported: bool,
}

enum Ask {
	Names {
		code: String,
		deadline: Instant,
		reply: oneshot::Sender<eyre::Result<Vec<Name>>>,
	},
	Diagnose(Box<Diagnose>),
	Forget(Url),
}

struct Diagnose {
	scope: Url,
	cell: Url,
	/// The session's earlier cells: on disk, so in its program once imported, until told deleted.
	evict: Vec<Url>,
	text: String,
	deadline: Instant,
	span: Span,
	reply: Reply,
}

enum Reply {
	Cell(oneshot::Sender<eyre::Result<Vec<Diagnostic>>>),
	WarmUp,
}

#[derive(Debug, Deserialize)]
struct Diagnostic {
	range: Range,
	severity: u8,
	code: Option<Value>,
	source: String,
	message: String,
	#[serde(rename = "relatedInformation")]
	related: Option<Vec<Related>>,
}

#[derive(Debug, Deserialize)]
struct Related {
	location: Location,
	message: String,
}

#[derive(Debug, Deserialize)]
struct Location {
	uri: String,
	range: Range,
}

#[derive(Debug, Deserialize)]
struct Range {
	start: Position,
	end: Position,
}

/// LSP's: `character` counts UTF-16 units.
#[derive(Debug, Deserialize)]
struct Position {
	line: usize,
	character: usize,
}

/// The errors in `deno check`'s shape, `cell`'s own lines shown and paths in `scope` relative.
fn render(errors: &[Diagnostic], cell: &Url, text: &str, scope: &Scope) -> eyre::Result<String> {
	let lines: Vec<&str> = text.lines().collect();
	let dir = format!("{}/", scope.uri);
	let shown = |out: &mut String, uri: &str, range: &Range, indent: &str| {
		if uri == cell.as_str()
			&& let Some(line) = lines.get(range.start.line)
		{
			let char_at = |utf16: usize| line.chars().scan(0, |u, c| (*u < utf16).then(|| *u += c.len_utf16())).count();
			let from = char_at(range.start.character);
			let to = if range.end.line == range.start.line {
				char_at(range.end.character)
			} else {
				line.chars().count()
			};
			let pad: String = line.chars().take(from).map(|c| if c == '\t' { '\t' } else { ' ' }).collect();
			let mark = if to <= from + 1 { "^".to_owned() } else { "~".repeat(to - from) };
			writeln!(out, "{indent}{line}\n{indent}{pad}{mark}").expect("a String takes writes");
		}
		let path = uri.strip_prefix(&dir).unwrap_or(uri); // outside the session's dir: the js's, whole
		writeln!(out, "{indent}    at {path}:{}:{}", range.start.line + 1, range.start.character + 1).expect("a String takes writes");
	};
	let mut out = String::new();
	for d in errors {
		match (d.source.as_str(), &d.code) {
			("deno-ts", Some(Value::Number(code))) => writeln!(out, "TS{code} [ERROR]: {}", d.message),
			("deno", Some(Value::String(code))) => writeln!(out, "{code} [ERROR]: {}", d.message),
			_ => eyre::bail!("a diagnostic neither tsc's nor Deno's resolution: {d:?}"),
		}
		.expect("a String takes writes");
		shown(&mut out, cell.as_str(), &d.range, "");
		for r in d.related.iter().flatten() {
			writeln!(out, "\n    {}", r.message).expect("a String takes writes");
			shown(&mut out, &r.location.uri, &r.location.range, "    ");
		}
		out.push('\n');
	}
	let n = errors.len();
	writeln!(out, "Found {n} error{}.", if n == 1 { "" } else { "s" }).expect("a String takes writes");
	Ok(out)
}

struct Driver {
	deno: PathBuf,
	js: PathBuf,
	cache: PathBuf,
	deno_json: Arc<[u8]>,
	prelude: Url,
}
impl Driver {
	async fn drive(self, mut asked: mpsc::UnboundedReceiver<Ask>) -> eyre::Result<Infallible> {
		//LOOP: a generation each, until a warm-up fails
		loop {
			let why = self.generation(&mut asked).await?;
			tracing::error!("the checker restarts: {why:#}");
		}
	}

	/// A `deno lsp` and a `names.ts` until either is not to be trusted: why, then.
	async fn generation(&self, asked: &mut mpsc::UnboundedReceiver<Ask>) -> eyre::Result<eyre::Report> {
		let warm = tracing::info_span!("checker warm-up");
		let scratch = tempfile::tempdir().wrap_err("the warm-up's directory")?;
		let scratch_dir = scratch.path().canonicalize().wrap_err("the warm-up's directory")?;
		std::fs::write(scratch_dir.join("deno.json"), &self.deno_json).wrap_err("the warm-up's deno.json")?;
		let mut lsp = crate::deno(&self.deno, &self.cache, &self.js).arg("lsp").spawn().wrap_err("starting deno lsp")?;
		let mut names = crate::deno(&self.deno, &self.cache, &self.js)
			.args(["run", "--no-prompt", "--no-remote", "--cached-only", "--allow-env", "--config"])
			.arg(self.js.join("deno.json"))
			.arg(self.js.join("names.ts"))
			.spawn()
			.wrap_err("starting names.ts")?;
		let mut lsp_out = std::pin::pin!(framed(lsp.stdout.take().expect("piped")));
		let mut lsp_err = Some(BufReader::new(lsp.stderr.take().expect("piped")).lines());
		let mut names_out = BufReader::new(names.stdout.take().expect("piped")).lines();
		let mut names_err = Some(BufReader::new(names.stderr.take().expect("piped")).lines());
		let js = Url::from_file_path(&self.js).expect("canonical paths are absolute");
		let mut g = Generation {
			lsp: lsp.stdin.take().expect("piped"),
			names: names.stdin.take().expect("piped"),
			js: js.clone(),
			next: 0,
			initialized: false,
			told: HashSet::new(),
			tree: HashSet::new(),
			registering: HashMap::new(),
			waiting: Vec::new(),
			open: HashMap::new(),
			naming: VecDeque::new(),
			warm: Some(warm.clone()),
		};
		let started = async {
			let initialize = g
				.request(
					"initialize",
					json!({
						"processId": std::process::id(),
						"rootUri": js,
						"capabilities": {
							"experimental": { "didRefreshDenoConfigurationTreeNotifications": true },
							"workspace": { "workspaceFolders": true, "configuration": true, "didChangeWatchedFiles": { "dynamicRegistration": true } },
							"textDocument": { "publishDiagnostics": { "relatedInformation": true } },
						},
						"initializationOptions": settings(),
						"workspaceFolders": [{ "uri": js, "name": "js" }],
					}),
				)
				.await?;
			assert_eq!(initialize, INITIALIZE, "the generation's first request");
			g.waiting.push(Diagnose {
				scope: Url::from_file_path(&scratch_dir).expect("canonical paths are absolute"),
				cell: Url::from_file_path(scratch_dir.join("cell_0.ts")).expect("canonical paths are absolute"),
				evict: Vec::new(),
				text: format!("import {{ api }} from \"{}\";\nconsole.log(api);\n", self.prelude),
				deadline: Instant::now() + CHECK,
				span: warm,
				reply: Reply::WarmUp,
			});
			eyre::Ok(())
		};
		let mut stopped = started.await.err().map(Stop::Restart);
		//LOOP: the generation's life; it breaks when it stops
		let stop = loop {
			if let Some(stop) = stopped {
				break stop;
			}
			let deadline = g.deadline();
			let step: Result<(), Stop> = tokio::select! {
				ask = asked.recv() => g.ask(ask.expect("the runner holds its checker as long as it serves")).await,
				heard = lsp_out.next() => match heard {
					Some(Ok(m)) => g.heard(m).await,
					Some(Err(e)) => Err(e.wrap_err("deno lsp's stdout").into()),
					None => Err(eyre::eyre!("deno lsp closed its stdout").into()),
				},
				line = names_out.next_line() => match line.wrap_err("names.ts's stdout") {
					Ok(Some(line)) => g.named(&line),
					Ok(None) => Err(eyre::eyre!("names.ts closed its stdout").into()),
					Err(e) => Err(e.into()),
				},
				line = async { lsp_err.as_mut().expect("guarded").next_line().await }, if lsp_err.is_some() => {
					said("deno lsp", line, &mut lsp_err);
					Ok(())
				},
				line = async { names_err.as_mut().expect("guarded").next_line().await }, if names_err.is_some() => {
					said("names.ts", line, &mut names_err);
					Ok(())
				},
				status = lsp.wait() => Err(eyre::eyre!("deno lsp exited: {status:?}").into()),
				status = names.wait() => Err(eyre::eyre!("names.ts exited: {status:?}").into()),
				() = async { tokio::time::sleep_until(deadline.expect("guarded")).await }, if deadline.is_some() => Err(eyre::eyre!("a check past CHECK").into()),
			};
			stopped = step.and(g.pump().await).err();
		};
		let why = match stop {
			Stop::End(e) => return Err(e),
			Stop::Restart(why) => why,
		};
		if g.warm.is_some() {
			return Err(why.wrap_err("the checker's warm-up"));
		}
		for d in g.waiting.into_iter().chain(g.open.into_values()) {
			if let Reply::Cell(reply) = d.reply {
				let _ = reply.send(Err(eyre::eyre!("the checker restarted: {why:#}"))); //IGNORED_ERROR: its asker went away
			}
		}
		for (_, reply) in g.naming {
			let _ = reply.send(Err(eyre::eyre!("the checker restarted: {why:#}"))); //IGNORED_ERROR: its asker went away
		}
		Ok(why)
	}
}

/// Why a generation stops.
enum Stop {
	/// The checker is no longer to be trusted, its programs or its time: killed and started anew.
	Restart(eyre::Report),
	/// The checker cannot be made to work: the runner ends.
	End(eyre::Report),
}

impl From<eyre::Report> for Stop {
	fn from(e: eyre::Report) -> Self {
		Self::Restart(e)
	}
}

/// One process's stderr line logged; at its end, no longer read.
fn said(who: &str, line: std::io::Result<Option<String>>, lines: &mut Option<tokio::io::Lines<BufReader<tokio::process::ChildStderr>>>) {
	match line {
		Ok(Some(line)) => tracing::debug!("{who}: {line}"),
		Ok(None) => *lines = None,
		Err(e) => {
			tracing::error!("{who}'s stderr: {e}");
			*lines = None;
		}
	}
}

/// What `workspace/configuration`'s `deno` section and `initializationOptions` answer.
fn settings() -> Value {
	// no file of a dir enters a program but through an import; nothing fetched
	json!({ "enable": true, "lint": false, "unstable": [], "documentPreloadLimit": 0, "cacheOnSave": false })
}

/// `deno lsp`'s messages, as its stdout frames them.
fn framed(out: ChildStdout) -> impl Stream<Item = eyre::Result<Value>> {
	futures::stream::unfold(BufReader::new(out), |mut out| async move {
		let message = async {
			let mut length = None;
			//LOOP: a header a line, up to the blank one
			loop {
				let mut line = String::new();
				if out.read_line(&mut line).await? == 0 {
					return Ok(None);
				}
				let line = line.trim_end();
				if line.is_empty() {
					break;
				}
				if let Some(n) = line.strip_prefix("Content-Length: ") {
					length = Some(n.parse::<usize>()?);
				}
			}
			let mut body = vec![0; length.ok_or_else(|| eyre::eyre!("a message with no Content-Length"))?];
			out.read_exact(&mut body).await?;
			eyre::Ok(Some(serde_json::from_slice(&body)?))
		}
		.await;
		message.transpose().map(|m| (m, out))
	})
}

struct Generation {
	lsp: ChildStdin,
	names: ChildStdin,
	js: Url,
	next: u64,
	initialized: bool,
	/// Scopes added as workspace folders.
	told: HashSet<Url>,
	/// The scopes of the latest config tree.
	tree: HashSet<Url>,
	registering: HashMap<Url, Span>,
	/// Until its scope is in the tree, its cell is not open, and the warm-up is done.
	waiting: Vec<Diagnose>,
	/// Asked, by request id; its cell open.
	open: HashMap<u64, Diagnose>,
	naming: VecDeque<(Instant, oneshot::Sender<eyre::Result<Vec<Name>>>)>,
	/// Until the warm-up's cell is answered clean.
	warm: Option<Span>,
}
impl Generation {
	async fn send(&mut self, message: Value) -> eyre::Result<()> {
		let body = message.to_string();
		self.lsp
			.write_all(format!("Content-Length: {}\r\n\r\n{body}", body.len()).as_bytes())
			.await
			.wrap_err("deno lsp's stdin")
	}

	async fn notify(&mut self, method: &str, params: Value) -> eyre::Result<()> {
		self.send(json!({ "jsonrpc": "2.0", "method": method, "params": params })).await
	}

	async fn request(&mut self, method: &str, params: Value) -> eyre::Result<u64> {
		let id = self.next;
		self.next += 1;
		self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).await?;
		Ok(id)
	}

	fn deadline(&self) -> Option<Instant> {
		let checks = self.waiting.iter().chain(self.open.values()).map(|d| d.deadline);
		checks.chain(self.naming.iter().map(|(deadline, _)| *deadline)).min()
	}

	async fn ask(&mut self, ask: Ask) -> Result<(), Stop> {
		match ask {
			Ask::Names { code, deadline, reply } => {
				let line = format!("{}\n", serde_json::to_string(&code).expect("a string is JSON"));
				self.names.write_all(line.as_bytes()).await.wrap_err("names.ts's stdin")?;
				self.naming.push_back((deadline, reply));
			}
			Ask::Diagnose(d) => self.waiting.push(*d),
			Ask::Forget(scope) => {
				self.registering.remove(&scope);
				if self.told.remove(&scope) {
					let folder = json!({ "uri": scope, "name": scope });
					self.notify("workspace/didChangeWorkspaceFolders", json!({ "event": { "added": [], "removed": [folder] } }))
						.await?;
				}
			}
		}
		Ok(())
	}

	fn named(&mut self, line: &str) -> Result<(), Stop> {
		let (_, reply) = self.naming.pop_front().ok_or_else(|| eyre::eyre!("names.ts answered nothing asked: {line}"))?;
		let names = serde_json::from_str(line).wrap_err_with(|| format!("names.ts's answer: {line}"))?;
		let _ = reply.send(Ok(names)); //IGNORED_ERROR: its asker went away
		Ok(())
	}

	async fn heard(&mut self, m: Value) -> Result<(), Stop> {
		match (m.get("id"), m.get("method").and_then(Value::as_str)) {
			(Some(id), Some(method)) => {
				let id = id.clone();
				let mut answer = match method {
					"workspace/configuration" => {
						let items = m["params"]["items"].as_array().ok_or_else(|| eyre::eyre!("workspace/configuration without items: {m}"))?;
						// another section's settings this client has none of
						let each = items.iter().map(|i| if i["section"] == "deno" { settings() } else { Value::Null });
						json!({ "result": each.collect::<Vec<_>>() })
					}
					"client/registerCapability" | "client/unregisterCapability" => json!({ "result": null }),
					_ => {
						tracing::warn!("deno lsp asked {method}, unanswerable: {m}");
						json!({ "error": { "code": -32601, "message": format!("{method}: not a method of this client") } })
					}
				};
				answer["jsonrpc"] = "2.0".into();
				answer["id"] = id;
				self.send(answer).await?;
			}
			(None, Some("deno/didRefreshDenoConfigurationTree")) => {
				let data = m["params"]["data"].as_array().ok_or_else(|| eyre::eyre!("a config tree without data: {m}"))?;
				self.tree = data
					.iter()
					.map(|s| Url::parse(s["scopeUri"].as_str().ok_or_else(|| eyre::eyre!("a scope without its uri: {s}"))?).wrap_err_with(|| format!("a scope's uri: {s}")))
					.collect::<eyre::Result<_>>()?;
				let tree = &self.tree;
				self.registering.retain(|scope, _| !tree.contains(scope));
			}
			(None, Some(method)) => tracing::trace!("deno lsp: {method}"),
			(Some(id), None) => {
				let id = id.as_u64().ok_or_else(|| eyre::eyre!("an answer to no request of ours: {m}"))?;
				if id == INITIALIZE {
					m.get("result").ok_or_else(|| eyre::eyre!("deno lsp refused to initialize: {m}"))?;
					self.notify("initialized", json!({})).await?;
					self.initialized = true;
					return Ok(());
				}
				let d = self.open.remove(&id).ok_or_else(|| eyre::eyre!("an answer to no request of ours: {m}"))?;
				self.notify("textDocument/didClose", json!({ "textDocument": { "uri": d.cell } })).await?;
				let said = match (&m["result"]["kind"], m.get("result")) {
					(Value::String(kind), Some(result)) if kind == "full" =>
						serde_json::from_value::<Vec<Diagnostic>>(result["items"].clone()).wrap_err_with(|| format!("deno lsp's diagnostics: {m}")),
					_ => Err(eyre::eyre!("deno lsp's diagnostics: {m}")),
				};
				match d.reply {
					Reply::Cell(reply) => {
						let _ = reply.send(said); //IGNORED_ERROR: its asker went away
					}
					Reply::WarmUp => {
						let said = said.map_err(Stop::End)?;
						if said.iter().any(|d| d.severity == ERROR) {
							return Err(Stop::End(eyre::eyre!("the warm-up's cell does not check: {said:?}")));
						}
						self.warm = None;
					}
				}
			}
			(None, None) => return Err(eyre::eyre!("deno lsp said neither a request nor an answer: {m}").into()),
		}
		Ok(())
	}

	/// Each waiting check's scope told, and the checks that may go, asked.
	async fn pump(&mut self) -> Result<(), Stop> {
		if !self.initialized {
			return Ok(());
		}
		let untold: Vec<(Url, Span)> = self.waiting.iter().filter(|d| !self.told.contains(&d.scope)).map(|d| (d.scope.clone(), d.span.clone())).collect();
		for (scope, span) in untold {
			if self.told.insert(scope.clone()) {
				self.registering.insert(scope.clone(), tracing::info_span!(parent: &span, "register scope"));
				let folder = json!({ "uri": scope, "name": scope });
				self.notify("workspace/didChangeWorkspaceFolders", json!({ "event": { "added": [folder], "removed": [] } }))
					.await?;
			}
		}
		let ready = |d: &Diagnose, g: &Self| {
			g.tree.contains(&g.js) && g.tree.contains(&d.scope) && !g.open.values().any(|o| o.cell == d.cell) && (g.warm.is_none() || matches!(d.reply, Reply::WarmUp))
		};
		//LOOP: once per check that may go
		while let Some(at) = self.waiting.iter().position(|d| ready(d, self)) {
			let d = self.waiting.swap_remove(at);
			if !d.evict.is_empty() {
				let changes: Vec<Value> = d.evict.iter().map(|uri| json!({ "uri": uri, "type": 3 })).collect(); // Deleted
				self.notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }))
					.instrument(tracing::info_span!(parent: &d.span, "evict"))
					.await?;
			}
			self.notify(
				"textDocument/didOpen",
				json!({ "textDocument": { "uri": d.cell, "languageId": "typescript", "version": 1, "text": d.text } }),
			)
			.await?;
			let id = self.request("textDocument/diagnostic", json!({ "textDocument": { "uri": d.cell } })).await?;
			self.open.insert(id, d);
		}
		Ok(())
	}
}
