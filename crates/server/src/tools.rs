//! The operations as an agent reaches them: `search` finds them by what they do, `call` runs one as
//! its caller, through the same `/api/v1` gate a browser's request takes — sections, freshness and
//! the live bus apply as to a click. Nothing here makes a [`crate::signin::Caller`].

use std::sync::{Arc, OnceLock};

use axum::{
	Router,
	body::{Body, to_bytes},
	http::{HeaderMap, HeaderValue, Request, header},
};
use ev_lib::api::{
	JsonSchema,
	openapi::{Operation, ReferenceOr},
};
use sa_core::prices::Item;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tower::ServiceExt;

use crate::catalog::{Catalog, scalar};

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
		let (method, mut uri) = self.catalog.resolve(&call.operation, &call.path.unwrap_or_default())?;
		if let Some(query) = call.query.filter(|q| !q.is_empty()) {
			let mut q = url::form_urlencoded::Serializer::new(String::new());
			for (k, v) in &query {
				q.append_pair(k, &scalar(v));
			}
			uri.push('?');
			uri.push_str(&q.finish());
		}
		let mut req = Request::builder().method(method).uri(uri);
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
		let kind = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned(); // none: an empty answer, below
		let bytes = to_bytes(res.into_body(), ANSWER)
			.await
			.map_err(|e| format!("the answer broke off or ran past {ANSWER} bytes: {e}"))?;
		let body = match kind.split(';').next().unwrap_or_default().trim() {
			_ if bytes.is_empty() => Value::Null,
			"application/json" => serde_json::from_slice(&bytes).map_err(|e| format!("{} answered JSON that does not parse: {e}", call.operation))?,
			text if text.starts_with("text/") => Value::String(String::from_utf8(bytes.to_vec()).map_err(|_| format!("{} answered text that is not UTF-8", call.operation))?),
			other => return Err(format!("{} answered {other:?}, which an agent does not read", call.operation)),
		};
		Ok(Answer { status, body })
	}

	/// The operations that are their own tools (`x-tool`) and that `may` opens, as an agent is given them.
	pub fn own(&self, may: impl Fn(Option<&str>) -> bool) -> Vec<Own> {
		let doc = self.catalog.document();
		let components = serde_json::to_value(&doc.components).expect("components serialize")["schemas"].take();
		doc.operations()
			.filter(|(_, _, op)| op.extensions.get("x-tool") == Some(&Value::Bool(true)))
			.filter(|(_, _, op)| may(op.extensions.get("x-permission").and_then(Value::as_str)))
			.map(|(_, method, op)| {
				let id = op.operation_id.clone().expect("every operation has an id");
				let raw = serde_json::to_value(op).expect("an operation serializes");
				let mut properties = Map::new();
				let mut required = Vec::new();
				let mut path = Vec::new();
				for p in raw["parameters"].as_array().into_iter().flatten() {
					let name = p["name"].as_str().expect("a parameter has a name").to_owned();
					if p["in"] == "path" {
						path.push(name.clone());
					}
					if p["required"] == true {
						required.push(Value::String(name.clone()));
					}
					let mut schema = inline(&p["schema"], &components, &mut Vec::new());
					if let Some(d) = p["description"].as_str() {
						schema["description"] = Value::String(d.to_owned());
					}
					properties.insert(name, schema);
				}
				let body = raw["requestBody"]["content"]["application/json"]["schema"].clone();
				if !body.is_null() {
					properties.insert("body".into(), inline(&body, &components, &mut Vec::new()));
					required.push(Value::String("body".into()));
				}
				Own {
					name: tool_name(&id),
					description: [op.summary.as_deref(), op.description.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("\n\n"),
					input: serde_json::json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false }),
					hints: hints(method, op),
					operation: id,
					path,
				}
			})
			.collect()
	}
}

/// What an agent is named an operation by when it is its own tool: `playbook.search` is `playbook_search`.
pub(crate) fn tool_name(operation: &str) -> String {
	operation.replacen('.', "_", 1)
}

/// The tools every agent has, whatever the catalog holds: an operation's own tool may not take their names.
pub(crate) const BUILT_IN: [&str; 4] = ["search", "call", "open", "run"];

/// A tool as every transport gives it.
pub struct Def {
	pub name: String,
	pub description: String,
	/// A JSON Schema with nothing to resolve.
	pub input: Value,
	pub hints: Hints,
}

/// Offers a view as a link: one of the views' table, its params by name.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Open {
	/// A view's name.
	pub view: String,
	/// Its params and query keys, by name.
	pub params: Map<String, Value>,
	pub label: String,
}

/// Offers a binding of the page's keymap as a button.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Run {
	/// A binding's id, as the page names it.
	pub binding: String,
	pub label: String,
}

impl Tools {
	/// Every tool `may`'s caller is given: `search` and `call`, each operation that is its own; with
	/// `page`, the in-page assistant's `open` and `run` too.
	pub fn definitions(&self, may: impl Fn(Option<&str>) -> bool + Copy, page: bool) -> Vec<Def> {
		let schema = |s: ev_lib::api::schemars::Schema| serde_json::to_value(s).expect("a schema serializes");
		let reads = Hints {
			read_only: true,
			idempotent: true,
			destructive: false,
			spends: Vec::new(),
		};
		let mut defs = vec![
			Def {
				name: "search".into(),
				description: "Finds the panel's operations matching what you need: their ids, parameters, body schemas and hints (read_only, idempotent, destructive, spends). An empty query lists every operation you may call.".into(),
				input: schema(ev_lib::api::schemars::schema_for!(Search)),
				hints: reads.clone(),
			},
			Def {
				name: "call".into(),
				description: "Runs one operation by its id from `search`, as the person asking; answers its status and body. One whose hints say destructive or spends is not run: it is proposed, for the person to agree to.".into(),
				input: schema(ev_lib::api::schemars::generate::SchemaSettings::draft07().into_generator().into_root_schema_for::<Call>()),
				hints: Hints {
					read_only: false,
					idempotent: false,
					destructive: true,
					spends: Vec::new(),
				},
			},
		];
		if page {
			let views = serde_json::to_value(&*self.catalog.views()).expect("a table serializes");
			let listed: Vec<String> = views["views"]
				.as_object()
				.expect("a table holds its views")
				.iter()
				.filter(|(_, v)| may(v["needs"].as_str()))
				.map(|(name, v)| {
					let keys = |part: &str| {
						v[part]["properties"]
							.as_object()
							.expect("a view's params and query are objects")
							.keys()
							.cloned()
							.collect::<Vec<_>>()
					};
					let text = |field: &str| v[field].as_str().expect("a view has its path and description").to_owned();
					format!("- {name} ({}): {}; params {:?}, query {:?}", text("path"), text("description"), keys("params"), keys("query"))
				})
				.collect();
			defs.push(Def {
				name: "open".into(),
				description: format!("Offers a view as a link, by its name and params. The views:\n{}", listed.join("\n")),
				input: schema(ev_lib::api::schemars::schema_for!(Open)),
				hints: reads.clone(),
			});
			defs.push(Def {
				name: "run".into(),
				description: "Offers a binding of the page's keymap, by its id, as a button: the <page> block lists those bound now.".into(),
				input: schema(ev_lib::api::schemars::schema_for!(Run)),
				hints: reads,
			});
		}
		defs.extend(self.own(may).into_iter().map(|t| Def {
			name: t.name,
			description: t.description,
			input: t.input,
			hints: t.hints,
		}));
		defs
	}

	/// The href `open` offers: a view of the table `may`'s caller may open, its params of its own.
	pub fn open(&self, open: &Open, may: impl Fn(Option<&str>) -> bool) -> Result<String, String> {
		let views = self.catalog.views();
		let entry = serde_json::to_value(&*views).expect("a table serializes")["views"][&open.view].take();
		if entry.is_null() {
			return Err(format!("no view {:?}: open names one of the views listed", open.view));
		}
		if !may(entry["needs"].as_str()) {
			return Err(format!("{} is not the person's to open", open.view));
		}
		let at = ev_lib::views::At {
			view: open.view.clone(),
			params: open.params.clone(),
		};
		views.href(&at, &Map::new()).map_err(|e| e.to_string())
	}
}

/// An operation that is its own tool.
pub struct Own {
	pub name: String,
	pub description: String,
	/// Its parameters by name, and its JSON body as `body`: a JSON Schema with nothing to resolve.
	pub input: Value,
	pub hints: Hints,
	operation: String,
	/// Which of the input's names are the path's.
	path: Vec<String>,
}

impl Own {
	/// The call its input stands for: the path's parameters, the query's, and the body.
	pub fn call(&self, mut input: Map<String, Value>) -> Call {
		let body = input.remove("body");
		let (path, query) = input.into_iter().partition(|(k, _)| self.path.contains(k));
		Call {
			operation: self.operation.clone(),
			path: Some(path),
			query: Some(query),
			body,
		}
	}
}

/// `schema` with every `$ref` to `components` put in place.
fn inline(schema: &Value, components: &Value, within: &mut Vec<String>) -> Value {
	match schema {
		Value::Object(o) => {
			if let Some(name) = o.get("$ref").and_then(Value::as_str).and_then(|r| r.strip_prefix("#/components/schemas/")) {
				assert!(!within.iter().any(|n| n == name), "{name} holds itself: a tool's input is no recursive schema");
				within.push(name.to_owned());
				let inlined = inline(&components[name], components, within);
				within.pop();
				return inlined;
			}
			Value::Object(o.iter().map(|(k, v)| (k.clone(), inline(v, components, within))).collect())
		}
		Value::Array(a) => Value::Array(a.iter().map(|v| inline(v, components, within)).collect()),
		v => v.clone(),
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
