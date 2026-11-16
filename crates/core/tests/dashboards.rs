//! A dashboard is held to the document it queries: the operation's path is its URL, and a field its
//! JSONata reads must be in the operation's answer.

use axum::{
	Json,
	http::{Method, request::Parts},
};
use ev_lib::api::{Guard, JsonSchema, Malformed, OpenApi, Refusal, Route, document, op_id};
use sa_core::dashboards::{Board, Column, Query};
use serde::Serialize;
use serde_json::json;

#[derive(Clone, Copy)]
struct Open;

impl Guard<()> for Open {
	type Refusal = Malformed;

	fn permission(&self) -> Option<&'static str> {
		None
	}

	fn refusals(&self) -> Vec<Malformed> {
		Vec::new()
	}

	async fn admit(&self, _: &mut Parts, _: &()) -> Result<(), Refusal<Malformed>> {
		Ok(())
	}
}

#[derive(JsonSchema, Serialize)]
struct Flows {
	flows: Vec<Flow>,
}

#[derive(JsonSchema, Serialize)]
struct Flow {
	kind: String,
	tokens: i64,
}

async fn token_flows() -> Json<Flows> {
	Json(Flows { flows: Vec::new() })
}

fn doc() -> OpenApi {
	document("t", "1", "/api/v1", &[Route::new(Method::GET, "/admin/flows", Open, token_flows)], &[("dashboards::token_flows", "Flows.")], None)
}

fn board(doc: &OpenApi, selector: &str) -> serde_json::Value {
	let query = Query {
		operation: op_id(token_flows),
		params: &[("from", "${__from}")],
		root_selector: selector,
		columns: &[("kind", Column::String), ("tokens", Column::Number)],
	};
	Board::new(doc, "conductor").panel("table", "Flows", [0, 0, 24, 8], query, json!({ "options": {} })).build(json!({ "uid": "t-flows" }))
}

#[test]
fn a_panel_queries_its_operations_path() {
	let built = board(&doc(), r#"($r := flows[kind in ["spend", "it's"]].{"kind": kind, "spent": $abs(tokens) * 1e3}; $r.{"kind": kind, "tokens": spent})"#);
	let target = &built["panels"][0]["targets"][0];
	assert_eq!(target["url"], "/admin/flows");
	assert_eq!(target["datasource"]["uid"], "conductor");
	assert_eq!(target["url_options"]["params"], json!([{ "key": "from", "value": "${__from}" }]));
	assert_eq!(built["panels"][0]["options"], json!({}));
}

#[test]
#[should_panic(expected = "its selector reads \"amount\", which token_flows's answer has not")]
fn a_field_the_answer_lacks_fails_the_build() {
	board(&doc(), "flows.amount");
}
