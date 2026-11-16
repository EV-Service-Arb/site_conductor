//! A handler taking `agent::Tool` is marked an agent tool in its service's document, and no other is.

use axum::{Json, http::{Method, request::Parts}};
use ev_lib::api::{Guard, Malformed, Refusal, Route, document};
use sa_core::agent::Tool;

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

async fn search(_: Tool) -> Json<String> {
	Json(String::new())
}

async fn list() -> Json<String> {
	Json(String::new())
}

#[test]
fn a_tool_is_marked_and_nothing_else_is() {
	let routes = [Route::new(Method::GET, "/search", Open, search), Route::new(Method::GET, "/list", Open, list)];
	let doc = document("t", "1", "/", &routes, &[("agent::search", "Finds."), ("agent::list", "Lists.")], None);
	let marked: Vec<_> = doc.operations().map(|(path, _, op)| (path.to_owned(), op.extensions.get("x-tool").cloned())).collect();
	assert_eq!(marked, [("/list".to_owned(), None), ("/search".to_owned(), Some(serde_json::Value::Bool(true)))]);
}
