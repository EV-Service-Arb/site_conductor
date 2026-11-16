//! A service's Grafana dashboards, built over its own document: a panel's or a variable's query
//! names an operation, so its URL is that operation's path, the parameters and headers it binds are
//! the operation's, and every field its JSONata reads is in the operation's answer. A dashboard
//! that drifted from its API fails where it is built.

use std::collections::BTreeSet;

use ev_lib::api::OpenApi;
use serde::Serialize;
use serde_json::{Value, json};

/// The Grafana plugin every service's datasource is.
pub const INFINITY: &str = "yesoreyeram-infinity-datasource";
/// JSONata's words that are no field.
const KEYWORDS: [&str; 7] = ["and", "or", "in", "true", "false", "null", "function"];

/// What one panel or variable asks: an operation of the board's document, its parameters bound
/// (to Grafana's variables, `${gmb}`), read whole or with JSONata into columns.
pub struct Query<'a> {
	/// `op_id(handler)`.
	pub operation: &'a str,
	/// Every parameter of its path, by name.
	pub path: &'a [(&'a str, &'a str)],
	/// Its query string: each a query parameter the operation takes.
	pub params: &'a [(&'a str, &'a str)],
	/// Each a header parameter the operation takes (`x-member`).
	pub headers: &'a [(&'a str, &'a str)],
	/// `None`: the panel takes the answer whole.
	pub select: Option<Select<'a>>,
}

pub struct Select<'a> {
	pub root_selector: &'a str,
	/// Each column the selector's rows hold, by name.
	pub columns: &'a [(&'a str, Column)],
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
	String,
	Number,
	Timestamp,
}

/// The panels of one dashboard, over `doc` as the datasource `datasource` (the service's name).
pub struct Board<'a> {
	doc: &'a OpenApi,
	datasource: &'a str,
	panels: Vec<Value>,
	variables: Vec<Value>,
}

impl<'a> Board<'a> {
	pub fn new(doc: &'a OpenApi, datasource: &'a str) -> Self {
		Self {
			doc,
			datasource,
			panels: Vec::new(),
			variables: Vec::new(),
		}
	}

	/// Panel `id` (layouts outside the board name it) of `kind` at `grid` (`x, y, w, h`); `display`
	/// (its `options`, `fieldConfig`, `transformations`) is Grafana's, taken as it is.
	///
	/// Panics on an operation the document lacks, a parameter it does not take or one it needs
	/// left out, or a field the selector reads that its answer has not.
	pub fn panel(self, id: u32, kind: &str, title: &str, grid: [u32; 4], query: Query, display: Value) -> Self {
		let target = self.target(title, "A", &query);
		let datasource = self.datasource();
		self.place(id, kind, title, grid, datasource, target, display)
	}

	/// Panel `id` over panel `from`'s results, as that panel transformed them.
	pub fn of_panel(self, id: u32, kind: &str, title: &str, grid: [u32; 4], from: u32, display: Value) -> Self {
		assert!(self.panels.iter().any(|p| p["id"] == from), "{title}: no panel {from} before it");
		let datasource = json!({ "type": "datasource", "uid": "-- Dashboard --" });
		let target = json!({ "refId": "A", "datasource": datasource, "panelId": from, "withTransforms": true });
		self.place(id, kind, title, grid, datasource, target, display)
	}

	/// Row `id`'s heading across the board at `grid`, above the panels after it.
	pub fn row(self, id: u32, title: &str, grid: [u32; 4]) -> Self {
		self.place(id, "row", title, grid, Value::Null, Value::Null, json!({ "collapsed": false, "panels": [] }))
	}

	/// A variable `name` whose options an operation answers (its rows' `__value` and `__text`);
	/// `rest` (`label`, `refresh`, `multi`, `includeAll`, …) is Grafana's.
	pub fn variable(mut self, name: &str, query: Query, rest: Value) -> Self {
		let mut infinity = self.target(name, "variable", &query);
		let target = infinity.as_object_mut().expect("a target is an object");
		target.remove("datasource");
		let ref_id = target.remove("refId").expect("a target has its refId");
		let Value::Object(mut variable) = rest else { panic!("{name}: its rest is an object") };
		for (k, v) in [
			("name", json!(name)),
			("type", json!("query")),
			("datasource", self.datasource()),
			("query", json!({ "refId": ref_id, "queryType": "infinity", "infinityQuery": infinity })),
		] {
			assert!(variable.insert(k.to_owned(), v).is_none(), "{name}: {k} is the board's to write");
		}
		self.variables.push(Value::Object(variable));
		self
	}

	/// The dashboard: `head` (its `uid`, `title`, `time`, `templating` with the variables nothing
	/// answers, …), then the variables queried, and the panels in order.
	pub fn build(self, head: Value) -> Value {
		let Value::Object(mut board) = head else { panic!("a dashboard's head is an object") };
		assert!(board.contains_key("uid"), "a dashboard has its uid");
		if !self.variables.is_empty() {
			let list = board
				.entry("templating")
				.or_insert_with(|| json!({ "list": [] }))
				.pointer_mut("/list")
				.and_then(Value::as_array_mut)
				.expect("templating holds a list");
			list.extend(self.variables);
		}
		assert!(board.insert("panels".into(), Value::Array(self.panels)).is_none(), "the panels are the board's");
		Value::Object(board)
	}

	fn datasource(&self) -> Value {
		json!({ "type": INFINITY, "uid": self.datasource })
	}

	#[expect(clippy::too_many_arguments, reason = "each is one of a panel's own parts")]
	fn place(mut self, id: u32, kind: &str, title: &str, grid: [u32; 4], datasource: Value, target: Value, display: Value) -> Self {
		assert!(!self.panels.iter().any(|p| p["id"] == id), "{title}: panel {id} is another's");
		let Value::Object(mut panel) = display else { panic!("{title}: its display is an object") };
		let [x, y, w, h] = grid;
		let own = [
			("id", json!(id)),
			("type", json!(kind)),
			("title", json!(title)),
			("gridPos", json!({ "x": x, "y": y, "w": w, "h": h })),
		];
		let queried = [("datasource", datasource), ("targets", json!([target]))];
		for (k, v) in own.into_iter().chain(queried.into_iter().filter(|(_, v)| !v.is_null())) {
			assert!(panel.insert(k.to_owned(), v).is_none(), "{title}: {k} is the board's to write");
		}
		self.panels.push(Value::Object(panel));
		self
	}

	/// The Infinity query of `query`, held to its operation in the document.
	fn target(&self, title: &str, ref_id: &str, query: &Query) -> Value {
		let (path, _, op) = self
			.doc
			.operations()
			.find(|(_, _, op)| op.operation_id.as_deref() == Some(query.operation))
			.unwrap_or_else(|| panic!("{title}: no operation {} in the document", query.operation));
		let op = serde_json::to_value(op).expect("an operation serializes");
		let components = serde_json::to_value(&self.doc.components).expect("components serialize")["schemas"].take();
		for (place, given) in [("path", query.path), ("query", query.params), ("header", query.headers)] {
			let taken: Vec<&Value> = op["parameters"].as_array().into_iter().flatten().filter(|p| p["in"] == place).collect();
			for (name, _) in given {
				assert!(
					taken.iter().any(|p| p["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(name))),
					"{title}: {} takes no {place} parameter {name:?}",
					query.operation
				);
			}
			for p in taken.iter().filter(|p| p["required"] == true) {
				let name = p["name"].as_str().expect("a parameter has its name");
				assert!(
					given.iter().any(|(n, _)| n.eq_ignore_ascii_case(name)),
					"{title}: {} needs its {place} parameter {name:?}",
					query.operation
				);
			}
		}
		let url = query.path.iter().fold(path.to_owned(), |url, (name, value)| url.replace(&format!("{{{name}}}"), value));
		let pairs = |given: &[(&str, &str)]| given.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>();
		let mut options = json!({ "method": "GET", "params": pairs(query.params) });
		if !query.headers.is_empty() {
			options["headers"] = Value::Array(pairs(query.headers));
		}
		let mut target = json!({
			"refId": ref_id,
			"datasource": self.datasource(),
			"type": "json",
			"source": "url",
			"format": "table",
			"parser": "backend",
			"url": url,
			"url_options": options,
		});
		if let Some(select) = &query.select {
			let answer = &op["responses"]["200"]["content"]["application/json"]["schema"];
			let mut fields = BTreeSet::new();
			properties(answer, &components, &mut BTreeSet::new(), &mut fields);
			let (read, built) = names(select.root_selector);
			for name in read.difference(&built) {
				assert!(fields.contains(*name), "{title}: its selector reads {name:?}, which {}'s answer has not", query.operation);
			}
			target["root_selector"] = json!(select.root_selector);
			target["columns"] = select.columns.iter().map(|(name, kind)| json!({ "selector": name, "text": name, "type": kind })).collect();
		}
		target
	}
}

/// Every property name `schema` can hold, through its `$ref`s.
fn properties(schema: &Value, components: &Value, seen: &mut BTreeSet<String>, out: &mut BTreeSet<String>) {
	match schema {
		Value::Object(o) => {
			if let Some(Value::String(r)) = o.get("$ref") {
				let name = r.strip_prefix("#/components/schemas/").unwrap_or_else(|| panic!("a $ref outside the components: {r}"));
				if seen.insert(name.to_owned()) {
					properties(&components[name], components, seen, out);
				}
			}
			if let Some(Value::Object(props)) = o.get("properties") {
				out.extend(props.keys().cloned());
			}
			o.values().for_each(|v| properties(v, components, seen, out));
		}
		Value::Array(a) => a.iter().for_each(|v| properties(v, components, seen, out)),
		_ => {}
	}
}

/// The field names a JSONata expression reads, and the keys it builds objects with (a later step may
/// read those): its bare words outside strings but its keywords, and each string followed by `:`.
/// `$` variables and functions are its own.
fn names(selector: &str) -> (BTreeSet<&str>, BTreeSet<&str>) {
	let (mut read, mut built) = (BTreeSet::new(), BTreeSet::new());
	let bytes = selector.as_bytes();
	let mut i = 0;
	while i < bytes.len() {
		match bytes[i] {
			q @ (b'"' | b'\'') => {
				let start = i + 1;
				i += 1;
				while i < bytes.len() && bytes[i] != q {
					i += if bytes[i] == b'\\' { 2 } else { 1 };
				}
				let literal = &selector[start..i.min(bytes.len())];
				i += 1;
				if selector[i.min(bytes.len())..].trim_start().starts_with(':') {
					built.insert(literal);
				}
			}
			b'0'..=b'9' =>
				while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
					i += 1;
				},
			b'$' => {
				i += 1;
				while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
					i += 1;
				}
			}
			b if b.is_ascii_alphabetic() || b == b'_' => {
				let start = i;
				while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
					i += 1;
				}
				let word = &selector[start..i];
				if !KEYWORDS.contains(&word) {
					read.insert(word);
				}
			}
			_ => i += 1,
		}
	}
	(read, built)
}
