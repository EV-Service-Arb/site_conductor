//! `/mcp`: the operations to an MCP client (Streamable HTTP), as tools over [`Tools`]: `search`,
//! `call`, `execute`, `poll`, and each operation that is its own (`x-tool`). The bearer is a grant's
//! session ([`crate::oauth`]); a call runs through the `/api/v1` gate with it, as a click would.
//!
//! `initialize` opens an MCP session (`Mcp-Session-Id`), which every later request names: it keys the
//! sandbox session `execute` runs in, and holds what the client can do. What the grant's level holds
//! is asked of the user through the client (`elicitation/create`, on the answer's SSE stream) when it
//! declared `elicitation`; else it is not run until the call comes with `confirmed: true`, and a cell's
//! held call fails at once, its proposal in the error.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
	time::Instant,
};

use axum::{
	Json, Router,
	body::Body,
	extract::State,
	http::{HeaderMap, HeaderValue, StatusCode, header},
	response::{IntoResponse, Response},
	routing::post,
};
use futures::{SinkExt, StreamExt, channel::mpsc};
use sa_core::agent::Approval;
use sa_sandbox::{Call, Ended, IDLE, Until};
use serde_json::{Value, json};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::{
	api::Code,
	catalog::Catalog,
	execute::{CellState, Key, PAUSE, Told},
	oauth,
	signin::{self, Caller, Freshness, SignIn},
	tools::{self, Def, Gate, Tools},
};

/// Newest first; the first is answered to a client asking for one not here.
const VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];
const SESSION: &str = "mcp-session-id";

#[derive(Clone)]
pub(crate) struct Mcp {
	pub(crate) sign_in: SignIn,
	pub(crate) tools: Tools,
	pub(crate) catalog: Catalog,
	pub(crate) sessions: Sessions,
}

/// The MCP sessions open, by id.
#[derive(Clone, Default)]
pub(crate) struct Sessions(Arc<Mutex<HashMap<String, Arc<Session>>>>);
impl Sessions {
	/// The session the request names, the caller's; else the answer refusing it.
	fn of(&self, headers: &HeaderMap, caller: &Caller) -> Result<Arc<Session>, Box<Response>> {
		let id = headers
			.get(SESSION)
			.and_then(|v| v.to_str().ok())
			.ok_or_else(|| Box::new((StatusCode::BAD_REQUEST, "Mcp-Session-Id: initialize first").into_response()))?;
		let s = self.0.lock().expect("never held across a panic").get(id).cloned();
		match s {
			Some(s) if s.user == caller.user_id => {
				*s.used.lock().expect("never held across a panic") = Instant::now();
				Ok(s)
			}
			// unknown, ended, or another's: the client initializes anew (Streamable HTTP's 404)
			_ => Err(Box::new((StatusCode::NOT_FOUND, "no such MCP session: initialize anew").into_response())),
		}
	}

	/// A session for `caller`'s client; those idle past [`IDLE`] go first, their sandbox sessions with them.
	async fn open(&self, tools: &Tools, caller: &Caller, version: &'static str, elicit: bool) -> String {
		let idle: Vec<String> = {
			let mut sessions = self.0.lock().expect("never held across a panic");
			let idle = sessions
				.values()
				.filter(|s| s.used.lock().expect("never held across a panic").elapsed() > IDLE)
				.map(|s| s.id.clone())
				.collect::<Vec<_>>();
			for id in &idle {
				sessions.remove(id);
			}
			idle
		};
		for id in idle {
			tools.sandbox().close(&Key::Mcp(id));
		}
		let mut bytes = [0; 32];
		getrandom::fill(&mut bytes).expect("the OS has randomness");
		let id = hex::encode(bytes);
		let s = Session {
			id: id.clone(),
			user: caller.user_id,
			version,
			elicit,
			used: Mutex::new(Instant::now()),
			asked: Mutex::default(),
		};
		self.0.lock().expect("never held across a panic").insert(id.clone(), Arc::new(s));
		id
	}
}

struct Session {
	id: String,
	/// The grant's user: no other bearer names this session.
	user: Uuid,
	version: &'static str,
	/// The client declared `elicitation`: it asks its user what the level holds.
	elicit: bool,
	used: Mutex<Instant>,
	/// What was asked of the client, by the request's id, until it answers.
	asked: Mutex<HashMap<String, oneshot::Sender<bool>>>,
}

pub(crate) fn routes(mcp: Mcp) -> Router {
	let no_stream = || async { (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST, DELETE")]) };
	Router::new().route(oauth::PATH, post(endpoint).get(no_stream).delete(end)).with_state(mcp)
}

/// The caller, else the answer refusing them.
async fn caller(m: &Mcp, headers: &HeaderMap) -> Result<Caller, Box<Response>> {
	match signin::bearer(&m.sign_in, headers, Freshness::Cached).await {
		Ok(c) => Ok(c),
		Err(r) if r.code() == Code::Unauthenticated => {
			let challenge = HeaderValue::from_str(&oauth::challenge(&m.sign_in)).expect("the origin is visible ASCII");
			Err(Box::new((StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, challenge)]).into_response()))
		}
		Err(r) => Err(Box::new(signin::refused(&m.sign_in, r))),
	}
}

/// `DELETE /mcp`: the client ends its session, and the sandbox's with it.
async fn end(State(m): State<Mcp>, headers: HeaderMap) -> Response {
	let caller = match caller(&m, &headers).await {
		Ok(c) => c,
		Err(res) => return *res,
	};
	let s = match m.sessions.of(&headers, &caller) {
		Ok(s) => s,
		Err(res) => return *res,
	};
	m.sessions.0.lock().expect("never held across a panic").remove(&s.id);
	m.tools.sandbox().close(&Key::Mcp(s.id.clone()));
	StatusCode::NO_CONTENT.into_response()
}

async fn endpoint(State(m): State<Mcp>, headers: HeaderMap, body: Result<Json<Value>, axum::extract::rejection::JsonRejection>) -> Response {
	let caller = match caller(&m, &headers).await {
		Ok(c) => c,
		Err(res) => return *res,
	};
	let Ok(Json(msg)) = body else {
		return Json(failure(Value::Null, -32700, "not JSON")).into_response();
	};
	let id = msg.get("id").cloned();
	let Some(method) = msg.get("method").and_then(Value::as_str) else {
		if let (Some(id), true) = (&id, msg.get("result").is_some() || msg.get("error").is_some()) {
			// the client's answer to what it was asked
			let s = match m.sessions.of(&headers, &caller) {
				Ok(s) => s,
				Err(res) => return *res,
			};
			let asked = id.as_str().and_then(|id| s.asked.lock().expect("never held across a panic").remove(id));
			let agreed = msg["result"]["action"] == "accept" && msg["result"]["content"]["approve"] == true;
			if let Some(asked) = asked {
				let _ = asked.send(agreed); //IGNORED_ERROR: the call stopped waiting (its wait passed, or its stream closed): there is no one left to tell
			}
			return StatusCode::ACCEPTED.into_response();
		}
		return Json(failure(id.unwrap_or_default(), -32600, "one JSON-RPC request, notification or response")).into_response();
	};
	if method == "initialize" {
		let Some(id) = id else {
			return Json(failure(Value::Null, -32600, "initialize is a request")).into_response();
		};
		let params = msg.get("params").cloned().unwrap_or_default();
		let asked = params.get("protocolVersion").and_then(Value::as_str);
		let version = VERSIONS.into_iter().find(|v| Some(*v) == asked).unwrap_or(VERSIONS[0]);
		let elicit = params["capabilities"]["elicitation"].is_object();
		let session = m.sessions.open(&m.tools, &caller, version, elicit).await;
		let result = json!({
			"protocolVersion": version,
			"capabilities": { "tools": { "listChanged": false } },
			"serverInfo": { "name": "service-arb", "version": env!("CARGO_PKG_VERSION") },
			"instructions": m.catalog.instructions(),
		});
		let mut res = Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response();
		res.headers_mut().insert(SESSION, HeaderValue::from_str(&session).expect("hex"));
		return res;
	}
	let s = match m.sessions.of(&headers, &caller) {
		Ok(s) => s,
		Err(res) => return *res,
	};
	let Some(id) = id else {
		return StatusCode::ACCEPTED.into_response();
	};
	let params = msg.get("params").cloned().unwrap_or_default();
	let streams = headers
		.get(header::ACCEPT)
		.and_then(|v| v.to_str().ok())
		.is_some_and(|a| a.split(',').any(|t| t.trim().starts_with("text/event-stream")));
	let answer = match method {
		"ping" => Ok(json!({})),
		"tools/list" => Ok(json!({ "tools": list(m.tools.definitions(|p| may(&caller, p), false, approval(&caller)), approval(&caller), s.elicit) })),
		"tools/call" => match call(&m, &caller, &s, &headers, params, streams).await {
			Ok(Reply::Now(result)) => Ok(result),
			Ok(Reply::Asking(work)) => return stream(m, s, headers, work, id),
			Err(e) => Err(e),
		},
		_ => Err((-32601, format!("no method {method}"))),
	};
	Json(match answer {
		Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
		Err((code, message)) => failure(id, code, &message),
	})
	.into_response()
}

/// What a held call needs before it runs when the client cannot ask: the user agreed.
fn confirmed_property() -> Value {
	json!({
		"type": "boolean",
		"description": "The user agreed to this call. Needed when it would be proposed, as the tool's description says; ask them first, saying what it does.",
	})
}

fn list(defs: Vec<Def>, approval: Approval, elicit: bool) -> Value {
	let tools = defs
		.into_iter()
		.map(|t| {
			let mut input = t.input;
			if !elicit && (t.hints.held_by(approval) || t.name == "call") {
				input["properties"]["confirmed"] = confirmed_property();
			}
			let h = &t.hints;
			json!({
				"name": t.name,
				"description": t.description,
				"inputSchema": input,
				"annotations": { "readOnlyHint": h.read_only, "destructiveHint": h.destructive, "idempotentHint": h.idempotent, "openWorldHint": !h.read_only },
			})
		})
		.collect();
	Value::Array(tools)
}

/// A tool's answer: now, or on a stream that asks the user first.
enum Reply {
	Now(Value),
	Asking(Work),
}

/// What runs on a stream, asking the user what the level holds as it goes.
enum Work {
	/// A cell, from its first answer.
	Cell(Told),
	/// A held call, once the user agrees.
	Call { call: Call, own: bool },
}

async fn call(m: &Mcp, caller: &Caller, s: &Session, headers: &HeaderMap, params: Value, streams: bool) -> Result<Reply, (i64, String)> {
	let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "a tool name".to_owned()))?;
	let mut args = match params.get("arguments").cloned().unwrap_or_else(|| json!({})) {
		Value::Object(a) => a,
		_ => return Err((-32602, "arguments are an object".to_owned())),
	};
	let invalid = |e: serde_json::Error| (-32602, format!("{name}: {e}"));
	let asks = s.elicit && streams;
	let key = Key::Mcp(s.id.clone());
	match name {
		"search" => {
			let q: tools::Search = serde_json::from_value(Value::Object(args)).map_err(invalid)?;
			return Ok(Reply::Now(text(&m.tools.search(&q.query, |p| may(caller, p)), false)));
		}
		"execute" | "poll" => {
			let told = match name {
				"execute" => {
					let e: tools::Execute = serde_json::from_value(Value::Object(args)).map_err(invalid)?;
					m.tools.execute(key, &bearer(headers), e.code, approval(caller), asks).await
				}
				_ => {
					let p: tools::PollCell = serde_json::from_value(Value::Object(args)).map_err(invalid)?;
					m.tools.poll(key, &bearer(headers), p.cell, Until::More).await
				}
			};
			return Ok(match told {
				Ok(t) if asks && matches!(t.state, CellState::Paused(_)) => Reply::Asking(Work::Cell(t)),
				Ok(t) => Reply::Now(cell(&m.tools, &t)),
				Err(e) => Reply::Now(text(&json!({ "error": e }), true)),
			});
		}
		_ => {}
	}
	let confirmed = match args.remove("confirmed") {
		None | Some(Value::Bool(false)) => false,
		Some(Value::Bool(true)) if !s.elicit => true,
		Some(Value::Bool(true)) => return Err((-32602, "confirmed: this client asks its user itself".to_owned())),
		Some(_) => return Err((-32602, "confirmed is a boolean".to_owned())),
	};
	let (c, own) = match name {
		"call" => (serde_json::from_value::<Call>(Value::Object(args)).map_err(invalid)?, false),
		_ => match m.tools.own(|p| may(caller, p)).into_iter().find(|t| t.name == name) {
			Some(t) => (t.call(args), true),
			None => return Err((-32602, format!("no tool {name}: search, call, or one tools/list names"))),
		},
	};
	let gate = match m.tools.gate(&c, approval(caller)) {
		Ok(g) => g,
		Err(e) => return Ok(Reply::Now(text(&json!({ "error": e }), true))),
	};
	if matches!(gate, Gate::Held) && !confirmed {
		if asks {
			return Ok(Reply::Asking(Work::Call { call: c, own }));
		}
		let Some(p) = m.tools.proposal(&c) else {
			return Ok(Reply::Now(text(&json!({ "error": format!("no operation {:?}: search for one", c.operation) }), true)));
		};
		let not_run = if s.elicit {
			"Not run: this client asks its user only on a request that accepts an event stream."
		} else {
			"Tell the user what this does and ask them; once they agree, call it again with \"confirmed\": true."
		};
		return Ok(Reply::Now(text(&json!({ "proposed": p, "not_run": not_run }), false)));
	}
	Ok(Reply::Now(run(m, headers, c, own).await))
}

async fn run(m: &Mcp, headers: &HeaderMap, c: Call, own: bool) -> Value {
	match m.tools.call(&bearer(headers), c).await {
		Ok(a) => {
			let ok = StatusCode::from_u16(a.status).is_ok_and(|s| s.is_success());
			match a.body {
				// an operation's own tool answers the model as it answered
				Value::String(t) if own && ok => json!({ "content": [{ "type": "text", "text": t }], "isError": false }),
				body => text(&json!({ "status": a.status, "body": body }), !ok),
			}
		}
		Err(e) => text(&json!({ "error": e }), true),
	}
}

/// A cell's answer: an error when it ended other than done.
fn cell(tools: &Tools, t: &Told) -> Value {
	let failed = matches!(t.state, CellState::Ended(e) if e != Ended::Done);
	text(&tools.told(t), failed)
}

/// The answer to request `id` as an SSE stream: what the level holds asked of the user on it, the
/// work going on as they decide, and the result last. The work is the stream's: it ends with it.
fn stream(m: Mcp, s: Arc<Session>, headers: HeaderMap, work: Work, id: Value) -> Response {
	let (out, events) = mpsc::channel::<Value>(4);
	let doing = async move {
		let result = converse(&m, &s, &headers, work, out.clone()).await;
		let mut out = out;
		let _ = out.send(json!({ "jsonrpc": "2.0", "id": id, "result": result })).await; //IGNORED_ERROR: the client closed the stream: no one reads the result
	};
	let doing = futures::stream::once(doing).filter_map(|()| std::future::ready(None::<Value>));
	let body = futures::stream::select(events, doing).map(|v| Ok::<_, std::convert::Infallible>(format!("event: message\ndata: {v}\n\n")));
	Response::builder()
		.header(header::CONTENT_TYPE, "text/event-stream")
		.header(header::CACHE_CONTROL, "no-cache")
		.body(Body::from_stream(body))
		.expect("a response from parts that parse")
}

/// Does `work`, asking the user through `out` what the level holds: the tool's result.
async fn converse(m: &Mcp, s: &Session, headers: &HeaderMap, work: Work, mut out: mpsc::Sender<Value>) -> Value {
	match work {
		Work::Call { call, own } =>
			if ask(m, s, &call, &mut out).await {
				run(m, headers, call, own).await
			} else {
				text(&json!({ "declined": format!("the user did not agree to {}: it did not run", call.operation) }), false)
			},
		Work::Cell(mut so_far) => {
			let key = Key::Mcp(s.id.clone());
			let sandbox = m.tools.sandbox();
			while let CellState::Paused(call) = &so_far.state {
				let agreed = ask(m, s, &call.clone(), &mut out).await;
				sandbox.decide(&key, None, agreed);
				let next = match m.tools.poll(key.clone(), &bearer(headers), so_far.cell, Until::Ended).await {
					Ok(t) => t,
					Err(e) => return text(&json!({ "error": e }), true),
				};
				so_far = so_far.then(next);
			}
			cell(&m.tools, &so_far)
		}
	}
}

/// Asks the user, through the client, to agree to `call`: whether they did within [`PAUSE`].
async fn ask(m: &Mcp, s: &Session, call: &Call, out: &mut mpsc::Sender<Value>) -> bool {
	let mut bytes = [0; 16];
	getrandom::fill(&mut bytes).expect("the OS has randomness");
	let id = format!("ask-{}", hex::encode(bytes));
	let (answer, answered) = oneshot::channel();
	s.asked.lock().expect("never held across a panic").insert(id.clone(), answer);
	let proposal = m.tools.proposal(call).unwrap_or_else(|| json!({ "operation": call.operation }));
	let what = format!(
		"{} — {} {}\n{}",
		proposal["summary"].as_str().unwrap_or(&call.operation),
		proposal["method"].as_str().unwrap_or_default(),
		proposal["path"].as_str().unwrap_or_default(),
		proposal["with"]
	);
	let mut params = json!({
		"message": format!("Your agent wants to run this, which removes or spends:\n{what}"),
		"requestedSchema": {
			"type": "object",
			"properties": { "approve": { "type": "boolean", "title": "Run it", "description": "Let it run this once." } },
			"required": ["approve"],
		},
	});
	if s.version == VERSIONS[0] {
		params["mode"] = json!("form");
	}
	if out.send(json!({ "jsonrpc": "2.0", "id": id, "method": "elicitation/create", "params": params })).await.is_err() {
		s.asked.lock().expect("never held across a panic").remove(&id);
		return false;
	}
	let agreed = matches!(tokio::time::timeout(PAUSE, answered).await, Ok(Ok(true)));
	s.asked.lock().expect("never held across a panic").remove(&id);
	agreed
}

/// The grant's bearer, all a call made for an MCP client carries.
fn bearer(headers: &HeaderMap) -> HeaderMap {
	let mut credentials = HeaderMap::new();
	credentials.insert(header::AUTHORIZATION, headers[header::AUTHORIZATION].clone());
	credentials
}

/// What the grant lets the client do without asking.
fn approval(caller: &Caller) -> Approval {
	caller.granted.expect("a bearer's caller is a grant's")
}

/// Whether `caller` holds `permission`, an operation's; none is everyone's.
fn may(caller: &Caller, permission: Option<&str>) -> bool {
	permission.is_none_or(|p| caller.permissions.iter().any(|h| h == p))
}

fn text(v: &Value, is_error: bool) -> Value {
	json!({ "content": [{ "type": "text", "text": v.to_string() }], "isError": is_error })
}

fn failure(id: Value, code: i64, message: &str) -> Value {
	json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}
