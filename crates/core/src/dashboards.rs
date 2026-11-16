//! A service's Grafana dashboards, built over its own document: a panel's query names an operation,
//! so its URL is that operation's path, and every field its JSONata reads must be in the
//! operation's answer. A dashboard that drifted from its API fails where it is built.

use std::collections::BTreeSet;

use ev_lib::api::OpenApi;
use serde::Serialize;
use serde_json::{Value, json};

/// The Grafana plugin every service's datasource is.
pub const INFINITY: &str = "yesoreyeram-infinity-datasource";
/// JSONata's words that are no field.
const KEYWORDS: [&str; 7] = ["and", "or", "in", "true", "false", "null", "function"];

/// What one panel asks: an operation of the board's document, read with JSONata into columns.
pub struct Query<'a> {
	/// `op_id(handler)`.
	pub operation: &'a str,
	/// Its query string; values may be Grafana's (`${__from}`).
	pub params: &'a [(&'a str, &'a str)],
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
}

impl<'a> Board<'a> {
	pub fn new(doc: &'a OpenApi, datasource: &'a str) -> Self {
		Self { doc, datasource, panels: Vec::new() }
	}

	/// A panel of `kind` at `grid` (`x, y, w, h`); `display` (its `options`, `fieldConfig`,
	/// `transformations`) is Grafana's, taken as it is.
	///
	/// Panics on an operation the document lacks, or a field the selector reads that its answer has not.
	pub fn panel(mut self, kind: &str, title: &str, grid: [u32; 4], query: Query, display: Value) -> Self {
		let (path, _, op) = self
			.doc
			.operations()
			.find(|(_, _, op)| op.operation_id.as_deref() == Some(query.operation))
			.unwrap_or_else(|| panic!("{title}: no operation {} in the document", query.operation));
		let answer = serde_json::to_value(op).expect("an operation serializes")["responses"]["200"]["content"]["application/json"]["schema"].take();
		let components = serde_json::to_value(&self.doc.components).expect("components serialize")["schemas"].take();
		let mut fields = BTreeSet::new();
		properties(&answer, &components, &mut BTreeSet::new(), &mut fields);
		let (read, built) = names(query.root_selector);
		for name in read.difference(&built) {
			assert!(fields.contains(*name), "{title}: its selector reads {name:?}, which {}'s answer has not", query.operation);
		}
		let datasource = json!({ "type": INFINITY, "uid": self.datasource });
		let target = json!({
			"refId": "A",
			"datasource": datasource,
			"type": "json",
			"source": "url",
			"format": "table",
			"parser": "backend",
			"url": path,
			"url_options": { "method": "GET", "params": query.params.iter().map(|(k, v)| json!({ "key": k, "value": v })).collect::<Vec<_>>() },
			"root_selector": query.root_selector,
			"columns": query.columns.iter().map(|(name, kind)| json!({ "selector": name, "text": name, "type": kind })).collect::<Vec<_>>(),
		});
		let Value::Object(mut panel) = display else { panic!("{title}: its display is an object") };
		let [x, y, w, h] = grid;
		let id = self.panels.len() + 1;
		for (k, v) in [
			("id", json!(id)),
			("type", json!(kind)),
			("title", json!(title)),
			("gridPos", json!({ "x": x, "y": y, "w": w, "h": h })),
			("datasource", datasource),
			("targets", json!([target])),
		] {
			assert!(panel.insert(k.to_owned(), v).is_none(), "{title}: {k} is the board's to write");
		}
		self.panels.push(Value::Object(panel));
		self
	}

	/// A row's heading across the board at `grid`, above the panels after it.
	pub fn row(mut self, title: &str, grid: [u32; 4]) -> Self {
		let [x, y, w, h] = grid;
		let id = self.panels.len() + 1;
		self.panels.push(json!({ "id": id, "type": "row", "title": title, "collapsed": false, "gridPos": { "x": x, "y": y, "w": w, "h": h }, "panels": [] }));
		self
	}

	/// The dashboard: `head` (its `uid`, `title`, `time`, `templating`, …) and the panels in order.
	pub fn build(self, head: Value) -> Value {
		let Value::Object(mut board) = head else { panic!("a dashboard's head is an object") };
		assert!(board.contains_key("uid"), "a dashboard has its uid");
		assert!(board.insert("panels".into(), Value::Array(self.panels)).is_none(), "the panels are the board's");
		Value::Object(board)
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
			b'0'..=b'9' => {
				while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'.') {
					i += 1;
				}
			}
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
