//! The operations as an agent reaches them: `search` finds them by what they do, `call` runs one as
//! its caller, through the same `/api/v1` gate a browser's request takes — sections, freshness and
//! the live bus apply as to a click. Nothing here makes a [`crate::signin::Caller`].

use std::sync::{Arc, OnceLock};

use axum::{
	Router,
	body::{Body, to_bytes},
	http::{HeaderMap, HeaderValue, Method, Request, header},
};
use ev_lib::api::{
	JsonSchema,
	openapi::{Operation, ReferenceOr},
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use sa_core::prices::Item;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tower::ServiceExt;

use crate::catalog::Catalog;

/// The most operations one search answers whole.
const FOUND: usize = 8;
/// The most of an answer an agent is handed back.
const ANSWER: usize = 1 << 20;

#[derive(Clone)]
pub struct Tools {
	catalog: Catalog,
	/// The whole app, set once it is built: a call goes through it as a request would.
	app: Arc<OnceLock<Router>>,
}
impl Tools {
	pub(crate) fn new(catalog: Catalog) -> Self {
		Self {
			catalog,
			app: Arc::new(OnceLock::new()),
		}
	}

	pub(crate) fn serve(&self, app: Router) {
		assert!(self.app.set(app).is_ok(), "the app is built once");
	}

	/// Each operation's path, method and itself.
	fn operations(&self) -> Vec<(String, String, Operation)> {
		self.catalog.document().operations().map(|(p, m, op)| (p.to_owned(), m.to_owned(), op.clone())).collect()
	}

	pub fn hints(&self, id: &str) -> Option<Hints> {
		self.operations()
			.into_iter()
			.find(|(_, _, op)| op.operation_id.as_deref() == Some(id))
			.map(|(_, method, op)| hints(&method, &op))
	}

	/// The operations matching `query` best, and the schemas they name; `may` says which the
	/// caller's permissions open. An empty query lists them all, briefly.
	pub fn search(&self, query: &str, may: impl Fn(Option<&str>) -> bool) -> Value {
		let operations = self.operations();
		let open = operations
			.iter()
			.filter(|(_, _, op)| may(op.extensions.get("x-permission").and_then(Value::as_str)))
			.map(|(p, m, op)| (p.as_str(), m.as_str(), op));
		let words: Vec<String> = query.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(str::to_lowercase).collect();
		if words.is_empty() {
			let all: Vec<Value> = open
				.map(|(path, method, op)| serde_json::json!({ "id": op.operation_id, "method": method.to_uppercase(), "path": path, "summary": op.summary }))
				.collect();
			return Value::Array(all);
		}
		let mut scored: Vec<(usize, Found)> = open
			.filter_map(|(path, method, op)| {
				let hay = format!(
					"{} {} {} {path} {}",
					op.operation_id.as_deref().unwrap_or_default().replace('_', " "),
					op.summary.as_deref().unwrap_or_default(),
					op.description.as_deref().unwrap_or_default(),
					op.tags.join(" ")
				)
				.to_lowercase();
				let score = words.iter().filter(|w| hay.contains(w.as_str())).count();
				(score > 0).then(|| (score, found(path, method, op)))
			})
			.collect();
		scored.sort_by(|a, b| b.0.cmp(&a.0));
		let found: Vec<Found> = scored.into_iter().take(FOUND).map(|(_, f)| f).collect();
		let schemas = self.schemas(&serde_json::to_value(&found).expect("found serializes"));
		serde_json::json!({ "operations": found, "schemas": schemas })
	}

	/// The component schemas `v` names, and those they name.
	fn schemas(&self, v: &Value) -> Map<String, Value> {
		let doc = self.catalog.document();
		let components = &doc.components.as_ref().expect("the document has components").schemas;
		let mut todo = Vec::new();
		refs(v, &mut todo);
		let mut out = Map::new();
		while let Some(name) = todo.pop() {
			if out.contains_key(&name) {
				continue;
			}
			let schema = serde_json::to_value(&components[&name].json_schema).expect("a schema serializes");
			refs(&schema, &mut todo);
			out.insert(name, schema);
		}
		out
	}

	/// Runs `call` with `credentials` (the caller's session cookie and CSRF header, or their bearer)
	/// through the app. Refused before the app sees it: an unknown operation, a path parameter
	/// missing.
	pub async fn call(&self, credentials: &HeaderMap, call: Call) -> Result<Answer, String> {
		let (path, method, _) = self
			.operations()
			.into_iter()
			.find(|(_, _, op)| op.operation_id.as_deref() == Some(call.operation.as_str()))
			.ok_or_else(|| format!("no operation {:?}: search for one", call.operation))?;
		let given = call.path.unwrap_or_default();
		let mut uri = String::new();
		for segment in path.split('/').filter(|s| !s.is_empty()) {
			uri.push('/');
			match segment.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
				Some(name) => {
					let value = given.get(name).ok_or_else(|| format!("{} needs the path parameter {name:?}", call.operation))?;
					uri.push_str(&utf8_percent_encode(&scalar(value), NON_ALPHANUMERIC).to_string());
				}
				None => uri.push_str(segment),
			}
		}
		if let Some(query) = call.query.filter(|q| !q.is_empty()) {
			let mut q = url::form_urlencoded::Serializer::new(String::new());
			for (k, v) in &query {
				q.append_pair(k, &scalar(v));
			}
			uri.push('?');
			uri.push_str(&q.finish());
		}
		let mut req = Request::builder()
			.method(Method::from_bytes(method.to_uppercase().as_bytes()).expect("the document's methods are standard"))
			.uri(uri);
		for name in [header::COOKIE.as_str(), header::AUTHORIZATION.as_str(), crate::cookies::CSRF_HEADER] {
			if let Some(v) = credentials.get(name) {
				req = req.header(name, v);
			}
		}
		let body = match call.body {
			Some(b) => {
				req = req.header(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
				Body::from(b.to_string())
			}
			None => Body::empty(),
		};
		let app = self.app.get().expect("calls come after the app is built").clone();
		let res = app.oneshot(req.body(body).expect("a request from parts that parsed")).await.expect("the router is infallible");
		let status = res.status().as_u16();
		let bytes = to_bytes(res.into_body(), ANSWER)
			.await
			.map_err(|e| format!("the answer broke off or ran past {ANSWER} bytes: {e}"))?;
		let body = if bytes.is_empty() {
			Value::Null
		} else {
			serde_json::from_slice(&bytes).unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
		};
		Ok(Answer { status, body })
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Search {
	/// What the operation should do, in words: "move a GBP to verified". Empty lists every
	/// operation by id and summary.
	pub query: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
	/// An operation's id, as `search` names it.
	pub operation: String,
	/// The path's parameters by name: `{"id": 7}`.
	pub path: Option<Map<String, Value>>,
	/// The query's parameters by name.
	pub query: Option<Map<String, Value>>,
	/// The JSON body, for an operation that takes one.
	pub body: Option<Value>,
}

/// An operation, as an agent needs it to call it.
#[derive(Serialize)]
pub struct Found {
	pub id: String,
	pub method: String,
	pub path: String,
	pub summary: Option<String>,
	pub description: Option<String>,
	#[serde(flatten)]
	pub hints: Hints,
	pub parameters: Value,
	pub body: Option<Value>,
}

/// What an operation does to the world, from its method and what its author said of it.
#[derive(Clone, Debug, Serialize)]
pub struct Hints {
	pub read_only: bool,
	pub idempotent: bool,
	pub destructive: bool,
	/// The priced items it may buy: a Places request, a cloud phone's minutes.
	pub spends: Vec<Item>,
}

impl Hints {
	/// What the user confirms before it runs.
	pub fn proposed(&self) -> bool {
		self.destructive || !self.spends.is_empty()
	}
}

/// What an operation answered.
#[derive(Debug, Serialize)]
pub struct Answer {
	pub status: u16,
	pub body: Value,
}

fn hints(method: &str, op: &Operation) -> Hints {
	Hints {
		read_only: method == "get",
		idempotent: matches!(method, "get" | "put" | "delete"),
		destructive: method == "delete",
		spends: op
			.extensions
			.get("x-spends")
			.map_or_else(Vec::new, |v| serde_json::from_value(v.clone()).expect("x-spends names priced items")),
	}
}

fn found(path: &str, method: &str, op: &Operation) -> Found {
	let body = op.request_body.as_ref().map(|b| match b {
		ReferenceOr::Item(b) => b.content.get("application/json").and_then(|m| m.schema.as_ref()).map_or_else(
			|| Value::String(b.content.keys().cloned().collect::<Vec<_>>().join(", ")),
			|s| serde_json::to_value(&s.json_schema).expect("a schema serializes"),
		),
		ReferenceOr::Reference { .. } => unreachable!("the document inlines request bodies"),
	});
	Found {
		id: op.operation_id.clone().expect("every operation has an id"),
		method: method.to_uppercase(),
		path: path.to_owned(),
		summary: op.summary.clone(),
		description: op.description.clone(),
		hints: hints(method, op),
		parameters: serde_json::to_value(&op.parameters).expect("parameters serialize"),
		body,
	}
}

/// A string as it is, anything else as JSON writes it.
fn scalar(v: &Value) -> String {
	v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

fn refs(v: &Value, out: &mut Vec<String>) {
	match v {
		Value::Object(o) => {
			if let Some(name) = o.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/components/schemas/")) {
				out.push(name.to_owned());
			}
			o.values().for_each(|v| refs(v, out));
		}
		Value::Array(a) => a.iter().for_each(|v| refs(v, out)),
		_ => {}
	}
}
