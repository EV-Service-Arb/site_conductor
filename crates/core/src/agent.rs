//! What an agent reaches: an operation of a service's table is its own tool on the conductor's
//! `/mcp` and in its assistant, `<service>_<operation_id>`, when its handler takes [`Tool`].

use std::convert::Infallible;

use axum::{extract::FromRequestParts, http::request::Parts};
use ev_lib::api::{GenContext, OperationInput, openapi::Operation};

/// Marks its handler's operation `x-tool: true`. It takes nothing from the request.
pub struct Tool;

impl<S: Send + Sync> FromRequestParts<S> for Tool {
	type Rejection = Infallible;

	async fn from_request_parts(_: &mut Parts, _: &S) -> Result<Self, Infallible> {
		Ok(Self)
	}
}

impl OperationInput for Tool {
	fn operation_input(_: &mut GenContext, op: &mut Operation) {
		assert!(op.extensions.insert("x-tool".into(), serde_json::Value::Bool(true)).is_none(), "one Tool a handler");
	}
}
