//! A person's own OpenAI key, pasted: `/api/v1/me/openai`.

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, JsonSchema, Refusal};
use jiff::Timestamp;
use sa_auth::Own;
use sa_core::prices::Provider;
use sa_panel::own_models::OpenAi;
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
	api::{ApiResult, Code},
	claude::model,
	connections::CheckedDto,
	http::{ApiRoute, ApiState, USER},
	signin::Caller,
};

/// A person's OpenAI key: its last four alone.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "OpenAi")]
pub struct OpenAiDto {
	pub last4: String,
	/// RFC 3339.
	pub set_at: String,
	/// The last time OpenAI took it or refused it.
	pub checked: CheckedDto,
}
impl From<OpenAi> for OpenAiDto {
	fn from(o: OpenAi) -> Self {
		Self {
			last4: o.last4,
			set_at: o.set_at.to_string(),
			checked: o.checked.into(),
		}
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OpenAiKey")]
pub(crate) struct Key {
	key: String,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/me/openai", USER.needs(Own::Credentials), openai),
		ApiRoute::new(M::PUT, "/me/openai", USER.needs(Own::Credentials), set_openai),
		ApiRoute::new(M::DELETE, "/me/openai", USER.needs(Own::Credentials), remove_openai),
	]
}

/// The caller's own OpenAI key, which their assistant's answers run on, after their Claude; `null`, none.
async fn openai(State(state): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<Json<Option<OpenAiDto>>> {
	Ok(Json(state.sign_in.panel.openai(caller.user_id).await?.map(OpenAiDto::from)))
}

/// Connects the caller's own OpenAI key, kept only once OpenAI answers on it: their account pays it.
///
/// 400 OpenAI refuses it, it is out of credit, or it has no access to the model; 503 it does not answer now.
async fn set_openai(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Body(b): Body<Key>) -> ApiResult<Json<OpenAiDto>> {
	let key = Zeroizing::new(b.key.trim().to_owned());
	if key.len() < 8 || !key.chars().all(|c| c.is_ascii_graphic()) {
		return Err(Refusal::new(Code::BadRequest, "an OpenAI key is one word, sk-…"));
	}
	model(&state)?.probe(Provider::OpenAi, &key).await?;
	let set = state.sign_in.panel.set_openai(caller.user_id, &key, Timestamp::now()).await?;
	tracing::info!(user_id = %caller.user_id, "a person's OpenAI key connected");
	Ok(Json(set.into()))
}

/// Disconnects the caller's own OpenAI key. 404 none.
async fn remove_openai(State(state): State<ApiState>, Extension(caller): Extension<Caller>) -> ApiResult<NoContent> {
	state.sign_in.panel.remove_own_model(caller.user_id, Provider::OpenAi).await??;
	tracing::info!(user_id = %caller.user_id, "a person's OpenAI key disconnected");
	Ok(NoContent)
}
