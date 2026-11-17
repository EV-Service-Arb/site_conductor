//! A dashboard is held to the document it queries: the operation's path is its URL, its bound path,
//! query and header parameters are the operation's (every one it needs given), and a field its
//! JSONata reads is in the operation's answer, or a key it built itself.

use axum::{
	Json,
	http::{Method, request::Parts},
};
use ev_lib::api::{Guard, JsonSchema, Malformed, OpenApi, Params, Refusal, Route, document, op_id};
use sa_core::dashboards::{Board, Column, Query, Select};
use serde::{Deserialize, Serialize};
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

#[derive(Deserialize, JsonSchema)]
struct Window {
	/// RFC 3339.
	from: String,
}

async fn token_flows(Params(w): Params<Window>) -> Json<Flows> {
	Json(Flows {
		flows: vec![Flow { kind: w.from, tokens: 0 }],
	})
}

#[derive(Deserialize, JsonSchema)]
struct GmbId {
	gmb: i64,
}

async fn board_of(ev_lib::api::Ids(GmbId { gmb }): ev_lib::api::Ids<GmbId>) -> Json<Flows> {
	Json(Flows {
		flows: vec![Flow { kind: "gmb".into(), tokens: gmb }],
	})
}

fn doc() -> OpenApi {
	document(
		"t",
		"1",
		"/api/v1",
		&[
			Route::new(Method::GET, "/admin/flows", Open, token_flows),
			Route::new(Method::GET, "/gmbs/{gmb}/board", Open, board_of),
		],
		&[("it::dashboards::token_flows", "Flows."), ("it::dashboards::board_of", "A GMB's board.")],
		None,
	)
}

fn flows<'a>(selector: &'a str, params: &'a [(&'a str, &'a str)]) -> Query<'a> {
	Query {
		operation: op_id(token_flows),
		path: &[],
		params,
		headers: &[],
		select: Some(Select {
			root_selector: selector,
			columns: &[("kind", Column::String), ("tokens", Column::Number)],
		}),
	}
}

#[test]
fn panels_query_their_operations_and_read_one_another() {
	let doc = doc();
	let board = Query {
		operation: op_id(board_of),
		path: &[("gmb", "${gmb}")],
		params: &[],
		headers: &[],
		select: None,
	};
	let built = Board::new(&doc, "conductor")
		.variable("kind", flows(r#"flows.{"__value": kind, "__text": kind}"#, &[("from", "${__from}")]), json!({ "refresh": 1 }))
		.panel(
			10,
			"table",
			"Flows",
			[0, 0, 24, 8],
			flows(
				r#"($r := flows[kind in ["spend", "it's"]].{"kind": kind, "spent": $abs(tokens) * 1e3}; $r.{"kind": kind, "tokens": spent})"#,
				&[("from", "${__from}")],
			),
			json!({ "options": {} }),
		)
		.panel(2, "text", "Board", [0, 8, 12, 4], board, json!({}))
		.of_panel(3, "table", "Flows again", [12, 8, 12, 4], 10, json!({}))
		.build(json!({ "uid": "t-flows", "templating": { "list": [{ "name": "gmb", "type": "textbox" }] } }));
	let panels = built["panels"].as_array().unwrap();
	assert_eq!(panels.iter().map(|p| p["id"].as_u64().unwrap()).collect::<Vec<_>>(), [10, 2, 3]);
	let target = &panels[0]["targets"][0];
	assert_eq!((&target["url"], &target["datasource"]["uid"]), (&json!("/admin/flows"), &json!("conductor")));
	assert_eq!(target["url_options"]["params"], json!([{ "key": "from", "value": "${__from}" }]));
	assert_eq!(panels[1]["targets"][0]["url"], "/gmbs/${gmb}/board");
	assert!(panels[1]["targets"][0].get("root_selector").is_none(), "read whole");
	assert_eq!(
		panels[2]["targets"][0],
		json!({ "refId": "A", "datasource": { "type": "datasource", "uid": "-- Dashboard --" }, "panelId": 10, "withTransforms": true })
	);
	let variables = built["templating"]["list"].as_array().unwrap();
	assert_eq!(variables.iter().map(|v| v["name"].as_str().unwrap()).collect::<Vec<_>>(), ["gmb", "kind"]);
	assert_eq!(variables[1]["query"]["infinityQuery"]["url"], "/admin/flows");
}

#[test]
#[should_panic(expected = "its selector reads \"amount\", which token_flows's answer has not")]
fn a_field_the_answer_lacks_fails_the_build() {
	Board::new(&doc(), "conductor").panel(1, "table", "Flows", [0, 0, 24, 8], flows("flows.amount", &[("from", "x")]), json!({}));
}

#[test]
#[should_panic(expected = "token_flows takes no header parameter \"x-member\"")]
fn a_header_the_operation_does_not_take_fails_the_build() {
	let query = Query {
		headers: &[("x-member", "${member}")],
		..flows("flows", &[("from", "x")])
	};
	Board::new(&doc(), "conductor").panel(1, "table", "Flows", [0, 0, 24, 8], query, json!({}));
}

#[test]
#[should_panic(expected = "token_flows needs its query parameter \"from\"")]
fn a_parameter_the_operation_needs_left_out_fails_the_build() {
	Board::new(&doc(), "conductor").panel(1, "table", "Flows", [0, 0, 24, 8], flows("flows", &[]), json!({}));
}
