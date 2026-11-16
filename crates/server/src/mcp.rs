//! `POST /mcp`: the operations to an MCP client (Streamable HTTP, JSON answers, no session),
//! as tools over [`Tools`]: `search`, `call`, and each operation that is its own (`x-tool`). The bearer is a grant's session
//! ([`crate::oauth`]); a `call` runs through the `/api/v1` gate with it, as a click would.
//! What removes or spends is not run until the client says the user confirmed it.

use axum::{
	Json, Router,
	extract::State,
	http::{HeaderMap, HeaderValue, StatusCode, header},
	response::{IntoResponse, Response},
	routing::post,
};
use ev_lib::api::schemars::schema_for;
use serde_json::{Value, json};

use crate::{
	catalog::Catalog,
	oauth,
	signin::{self, Caller, Denied, Freshness, SignIn},
	tools::{self, Own, Tools},
};

/// Newest first; the first is answered to a client asking for one not here.
const VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

#[derive(Clone)]
pub(crate) struct Mcp {
	pub(crate) sign_in: SignIn,
	pub(crate) tools: Tools,
	pub(crate) catalog: Catalog,
}

pub(crate) fn routes(mcp: Mcp) -> Router {
	let no_stream = || async { (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")]) };
	Router::new().route(oauth::PATH, post(endpoint).get(no_stream).delete(no_stream)).with_state(mcp)
}

async fn endpoint(State(m): State<Mcp>, headers: HeaderMap, body: Result<Json<Value>, axum::extract::rejection::JsonRejection>) -> Response {
	let caller = match signin::bearer(&m.sign_in, &headers, Freshness::Cached).await {
		Ok(c) => c,
		Err(Denied::Unauthenticated) => {
			let challenge = HeaderValue::from_str(&oauth::challenge(&m.sign_in)).expect("the origin is visible ASCII");
			return (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, challenge)]).into_response();
		}
		Err(denied) => return denied.into_response(&m.sign_in),
	};
	let Ok(Json(msg)) = body else {
		return Json(failure(Value::Null, -32700, "not JSON")).into_response();
	};
	let id = msg.get("id").cloned();
	let Some(method) = msg.get("method").and_then(Value::as_str) else {
		return Json(failure(id.unwrap_or_default(), -32600, "one JSON-RPC request or notification")).into_response();
	};
	let Some(id) = id else {
		return StatusCode::ACCEPTED.into_response();
	};
	let params = msg.get("params").cloned().unwrap_or_default();
	let answer = match method {
		"initialize" => Ok(initialize(&params, m.catalog.instructions())),
		"ping" => Ok(json!({})),
		"tools/list" => Ok(json!({ "tools": list(&m.tools.own(|p| may(&caller, p))) })),
		"tools/call" => call(&m, &caller, &headers, params).await,
		_ => Err((-32601, format!("no method {method}"))),
	};
	Json(match answer {
		Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
		Err((code, message)) => failure(id, code, &message),
	})
	.into_response()
}

fn initialize(params: &Value, instructions: String) -> Value {
	let asked = params.get("protocolVersion").and_then(Value::as_str);
	let version = VERSIONS.into_iter().find(|v| Some(*v) == asked).unwrap_or(VERSIONS[0]);
	json!({
		"protocolVersion": version,
		"capabilities": { "tools": { "listChanged": false } },
		"serverInfo": { "name": "service-arb", "version": env!("CARGO_PKG_VERSION") },
		"instructions": instructions,
	})
}

/// What a proposed call needs before it runs: the user agreed.
fn confirmed_property() -> Value {
	json!({
		"type": "boolean",
		"description": "The user agreed to this call. Needed when the operation's hints say destructive or spends; ask them first, saying what it does.",
	})
}

fn list(own: &[Own]) -> Value {
	let mut call = serde_json::to_value(schema_for!(tools::Call)).expect("a schema serializes");
	call["properties"]["confirmed"] = confirmed_property();
	let mut tools = vec![
		json!({
			"name": "search",
			"title": "Find operations",
			"description": "Finds the panel's operations matching what you need: their ids, parameters, body schemas and hints (read_only, idempotent, destructive, spends). An empty query lists every operation you may call.",
			"inputSchema": schema_for!(tools::Search),
			"annotations": { "readOnlyHint": true, "openWorldHint": false },
		}),
		json!({
			"name": "call",
			"title": "Run an operation",
			"description": "Runs one operation by its id from `search`, as the signed-in user; answers its status and body. One whose hints say destructive or spends is not run without `confirmed: true`: it answers what it would do, for you to ask the user.",
			"inputSchema": call,
			"annotations": { "readOnlyHint": false, "destructiveHint": true, "idempotentHint": false, "openWorldHint": true },
		}),
	];
	for t in own {
		let mut input = t.input.clone();
		if t.hints.proposed() {
			input["properties"]["confirmed"] = confirmed_property();
		}
		tools.push(json!({
			"name": t.name,
			"description": t.description,
			"inputSchema": input,
			"annotations": { "readOnlyHint": t.hints.read_only, "destructiveHint": t.hints.destructive, "idempotentHint": t.hints.idempotent, "openWorldHint": true },
		}));
	}
	Value::Array(tools)
}

async fn call(m: &Mcp, caller: &Caller, headers: &HeaderMap, params: Value) -> Result<Value, (i64, String)> {
	let name = params.get("name").and_then(Value::as_str).ok_or((-32602, "a tool name".to_owned()))?;
	let mut args = match params.get("arguments").cloned().unwrap_or_else(|| json!({})) {
		Value::Object(a) => a,
		_ => return Err((-32602, "arguments are an object".to_owned())),
	};
	let invalid = |e: serde_json::Error| (-32602, format!("{name}: {e}"));
	if name == "search" {
		let s: tools::Search = serde_json::from_value(Value::Object(args)).map_err(invalid)?;
		return Ok(text(&m.tools.search(&s.query, |p| may(caller, p)), false));
	}
	let confirmed = match args.remove("confirmed") {
		None | Some(Value::Bool(false)) => false,
		Some(Value::Bool(true)) => true,
		Some(_) => return Err((-32602, "confirmed is a boolean".to_owned())),
	};
	let (c, own) = match name {
		"call" => (serde_json::from_value::<tools::Call>(Value::Object(args)).map_err(invalid)?, false),
		_ => match m.tools.own(|p| may(caller, p)).into_iter().find(|t| t.name == name) {
			Some(t) => (t.call(args), true),
			None => return Err((-32602, format!("no tool {name}: search, call, or one tools/list names"))),
		},
	};
	let Some(hints) = m.tools.hints(&c.operation) else {
		return Ok(text(&json!({ "error": format!("no operation {:?}: search for one", c.operation) }), true));
	};
	if hints.proposed() && !confirmed {
		let doc = m.catalog.document();
		// The catalog may have been pulled anew since the hints were read.
		let Some((path, method, op)) = doc.operations().find(|(_, _, op)| op.operation_id.as_deref() == Some(c.operation.as_str())) else {
			return Ok(text(&json!({ "error": format!("no operation {:?}: search for one", c.operation) }), true));
		};
		return Ok(text(
			&json!({
				"proposed": {
					"operation": c.operation,
					"method": method.to_uppercase(),
					"path": path,
					"summary": op.summary,
					"hints": hints,
					"with": { "path": c.path, "query": c.query, "body": c.body },
				},
				"not_run": "Tell the user what this does and ask them; once they agree, call it again with \"confirmed\": true.",
			}),
			false,
		));
	}
	let mut credentials = HeaderMap::new();
	credentials.insert(header::AUTHORIZATION, headers[header::AUTHORIZATION].clone());
	match m.tools.call(&credentials, c).await {
		Ok(a) => {
			let ok = StatusCode::from_u16(a.status).is_ok_and(|s| s.is_success());
			match a.body {
				// an operation's own tool answers the model as it answered
				Value::String(t) if own && ok => Ok(json!({ "content": [{ "type": "text", "text": t }], "isError": false })),
				body => Ok(text(&json!({ "status": a.status, "body": body }), !ok)),
			}
		}
		Err(e) => Ok(text(&json!({ "error": e }), true)),
	}
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
