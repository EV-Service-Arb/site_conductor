//! The conductor's own `/api/v1`: `/me`, the source keys and the connected sites
//! (`connected`), and what every module's operations take and answer. JSON in and out; every
//! request has passed the sign-in gate (`signin::gate`), so a [`Caller`] is in its extensions.

use std::{borrow::Cow, collections::BTreeSet};

use axum::{
	Extension, Json,
	extract::{FromRequest, FromRequestParts, Path, Query, Request, State, rejection::JsonRejection},
	http::{Method, StatusCode, request::Parts},
	response::{IntoResponse, NoContent, Response},
};
use ev_lib::api::{
	Created, GenContext, JsonSchema, OpenApi, OperationInput, OperationOutput,
	openapi::{Operation, Response as Answer, StatusCode as Status},
	schemars::{Schema, SchemaGenerator, json_schema},
};
use sa_auth::SourceKind;
use sa_core::{
	Invalid,
	ids::{BrandId, SiteOrigin},
};
use sa_panel::{Panel, Refused};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{
	connected,
	http::{ApiRoute, ApiState, Gate, Section},
	signin::Caller,
};

pub(crate) type ApiResult<T> = Result<T, ApiError>;

/// Why a request failed, as the front end is told: every JSON refusal the conductor writes itself.
#[derive(Debug)]
pub enum ApiError {
	BadRequest(String),
	/// No session, or one closed.
	Unauthenticated,
	/// A cookie's write without the page's CSRF header, or a socket from another origin.
	Csrf,
	Forbidden,
	NotFound,
	Conflict(String),
	TooLarge(String),
	TooMany(String),
	/// Not now: the message says until when.
	Unavailable(String),
	/// No browser provider is configured: no browser route answers anything else.
	BrowsersNotConnected,
	/// What this answer stands on did not answer.
	BadGateway(String),
	/// A study placement cannot build, and why.
	Unstudied(Unstudied, String),
	/// A pull needs more than the balance affords.
	Unaffordable(String),
	Internal(eyre::Report),
}

/// Why a study cannot be built.
#[derive(Clone, Copy, Debug)]
pub enum Unstudied {
	/// The category names no placement trade to study it by.
	NoStudy,
	/// The category has no primary Google category.
	NoGoogleCategory,
	/// The category has no keyword.
	NoKeywords,
	/// The category is set up for another country than the area's.
	AnotherCountry,
	/// Nothing placement serves studies it: no such trade file, no base for the country, or the
	/// trade refuses it.
	Unstudied,
}

/// Which failure, for a client to act on; the status follows from it.
#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
	BadRequest,
	Unauthenticated,
	Csrf,
	Forbidden,
	NotFound,
	Conflict,
	TooLarge,
	TooMany,
	Unavailable,
	BrowsersNotConnected,
	BadGateway,
	NoStudy,
	NoGoogleCategory,
	NoKeywords,
	AnotherCountry,
	Unstudied,
	Unaffordable,
	Internal,
}

/// What every failed operation answers: `error` is for a person.
#[derive(JsonSchema, Serialize)]
#[schemars(rename = "Error")]
pub struct ErrorDto {
	code: ErrorCode,
	error: String,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let admin = Gate::section(Section::Admin);
	vec![
		ApiRoute::new(M::GET, "/me", Gate::USER, me),
		ApiRoute::new(M::GET, "/openapi.json", Gate::USER, openapi),
		ApiRoute::new(M::GET, "/sources", admin, list_sources),
		ApiRoute::new(M::GET, "/sources/sites", admin, connected::sites),
		// A key must not be minted or revoked on a permission revoked a moment ago.
		ApiRoute::new(M::POST, "/sources", admin.fresh(), add_source),
		ApiRoute::new(M::DELETE, "/sources/{key_id}", admin.fresh(), revoke_source),
	]
}

impl IntoResponse for ApiError {
	fn into_response(self) -> Response {
		use ErrorCode as C;
		let (status, code, msg) = match self {
			Self::BadRequest(m) => (StatusCode::BAD_REQUEST, C::BadRequest, m),
			Self::Unauthenticated => (StatusCode::UNAUTHORIZED, C::Unauthenticated, "sign in".to_owned()),
			Self::Csrf => (StatusCode::FORBIDDEN, C::Csrf, "the page is out of date: reload it".to_owned()),
			Self::Forbidden => (StatusCode::FORBIDDEN, C::Forbidden, "your permissions do not allow this".to_owned()),
			Self::NotFound => (StatusCode::NOT_FOUND, C::NotFound, "not found".to_owned()),
			Self::Conflict(m) => (StatusCode::CONFLICT, C::Conflict, m),
			Self::TooLarge(m) => (StatusCode::PAYLOAD_TOO_LARGE, C::TooLarge, m),
			Self::TooMany(m) => (StatusCode::TOO_MANY_REQUESTS, C::TooMany, m),
			Self::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, C::Unavailable, m),
			Self::BrowsersNotConnected => (StatusCode::SERVICE_UNAVAILABLE, C::BrowsersNotConnected, "browsers not connected".to_owned()),
			Self::BadGateway(m) => (StatusCode::BAD_GATEWAY, C::BadGateway, m),
			Self::Unstudied(why, m) => (
				StatusCode::UNPROCESSABLE_ENTITY,
				match why {
					Unstudied::NoStudy => C::NoStudy,
					Unstudied::NoGoogleCategory => C::NoGoogleCategory,
					Unstudied::NoKeywords => C::NoKeywords,
					Unstudied::AnotherCountry => C::AnotherCountry,
					Unstudied::Unstudied => C::Unstudied,
				},
				m,
			),
			Self::Unaffordable(m) => (StatusCode::PAYMENT_REQUIRED, C::Unaffordable, m),
			Self::Internal(e) => {
				crate::report(&e, "the conductor's API");
				(StatusCode::INTERNAL_SERVER_ERROR, C::Internal, "internal error".to_owned())
			}
		};
		(status, Json(ErrorDto { code, error: msg })).into_response()
	}
}

impl OperationOutput for ApiError {
	type Inner = ErrorDto;

	fn operation_response(ctx: &mut GenContext, op: &mut Operation) -> Option<Answer> {
		Json::<ErrorDto>::operation_response(ctx, op)
	}

	fn inferred_responses(ctx: &mut GenContext, op: &mut Operation) -> Vec<(Option<Status>, Answer)> {
		Self::operation_response(ctx, op).map(|r| (None, r)).into_iter().collect()
	}
}

impl From<eyre::Report> for ApiError {
	fn from(e: eyre::Report) -> Self {
		Self::Internal(e)
	}
}

impl From<Refused> for ApiError {
	fn from(r: Refused) -> Self {
		match r {
			Refused::Invalid(m) => Self::BadRequest(m),
			Refused::NotFound => Self::NotFound,
			Refused::Forbidden => Self::Forbidden,
			Refused::Conflict(m) => Self::Conflict(m),
			Refused::TooLarge(m) => Self::TooLarge(m),
			Refused::Unavailable(m) => Self::Unavailable(m),
			Refused::Closed => Self::Unavailable(r.to_string()),
		}
	}
}

impl From<Invalid> for ApiError {
	fn from(e: Invalid) -> Self {
		Self::BadRequest(e.0)
	}
}

/// A JSON body, refused in the shape every failure has.
pub(crate) struct Body<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequest<S> for Body<T> {
	type Rejection = ApiError;

	async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
		Json::<T>::from_request(req, state).await.map(|Json(v)| Self(v)).map_err(|e: JsonRejection| match e.status() {
			StatusCode::PAYLOAD_TOO_LARGE => ApiError::TooLarge(e.body_text()),
			_ => ApiError::BadRequest(e.body_text()),
		})
	}
}

impl<T: JsonSchema> OperationInput for Body<T> {
	fn operation_input(ctx: &mut GenContext, op: &mut Operation) {
		Json::<T>::operation_input(ctx, op);
	}
}

/// A query string, refused in the shape every failure has.
pub(crate) struct Params<T>(pub T);

impl<T: DeserializeOwned, S: Send + Sync> FromRequestParts<S> for Params<T> {
	type Rejection = ApiError;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
		Query::<T>::from_request_parts(parts, state)
			.await
			.map(|Query(v)| Self(v))
			.map_err(|e| ApiError::BadRequest(e.body_text()))
	}
}

impl<T: JsonSchema> OperationInput for Params<T> {
	fn operation_input(ctx: &mut GenContext, op: &mut Operation) {
		Query::<T>::operation_input(ctx, op);
	}
}

/// The ids in a path, by their names in it; one not the caller's, or not an id at all, is a 404.
pub(crate) struct Ids<T>(pub T);

impl<T: DeserializeOwned + Send, S: Send + Sync> FromRequestParts<S> for Ids<T> {
	type Rejection = ApiError;

	async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, ApiError> {
		Path::<T>::from_request_parts(parts, state).await.map(|Path(v)| Self(v)).map_err(|_| ApiError::NotFound)
	}
}

impl<T: JsonSchema> OperationInput for Ids<T> {
	fn operation_input(ctx: &mut GenContext, op: &mut Operation) {
		Path::<T>::operation_input(ctx, op);
	}
}

/// A path's one `{id}`.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Id<T> {
	pub id: T,
}

/// The caller: who they are and every `sa` permission they hold, every service's.
async fn me(Extension(caller): Extension<Caller>) -> Json<Caller> {
	Json(caller)
}

/// The operations the caller's permissions open, as an OpenAPI document.
///
/// Context, not authority: each operation is judged again when it is called.
async fn openapi(State(state): State<ApiState>, Extension(caller): Extension<Caller>) -> Json<Document> {
	Json(Document(state.catalog.opened(&caller.permissions)))
}

/// An OpenAPI 3.1 document.
#[derive(Serialize)]
#[serde(transparent)]
pub(crate) struct Document(OpenApi);

impl JsonSchema for Document {
	fn schema_name() -> Cow<'static, str> {
		"OpenApi".into()
	}

	fn json_schema(_: &mut SchemaGenerator) -> Schema {
		json_schema!({ "type": "object", "description": "An OpenAPI 3.1 document." })
	}
}

// ── sources (admin) ─────────────────────────────────────────────────────────────────────

/// A signing key, never its secret.
#[derive(JsonSchema, Serialize)]
#[schemars(rename = "SourceKey")]
pub(crate) struct SourceDto {
	key_id: String,
	kind: SourceKind,
	brands: Vec<String>,
	/// RFC 3339.
	created_at: String,
	revoked_at: Option<String>,
	/// A site key's landing: `https://<domain>[:port]`.
	origin: Option<String>,
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "SourceKeys")]
pub(crate) struct SourcesDto {
	sources: Vec<SourceDto>,
}

/// Every source key, revoked ones too.
async fn list_sources(State(panel): State<Panel>) -> ApiResult<Json<SourcesDto>> {
	let sources: Vec<SourceDto> = panel
		.store()
		.sources()
		.await?
		.into_iter()
		.map(|s| SourceDto {
			key_id: s.grant.key_id,
			kind: s.grant.kind,
			brands: s.grant.brands.iter().map(|b| b.as_str().to_owned()).collect(),
			created_at: s.created_at.to_string(),
			revoked_at: s.revoked_at.map(|t| t.to_string()),
			origin: s.origin.map(|o| o.to_string()),
		})
		.collect();
	Ok(Json(SourcesDto { sources }))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddSource {
	/// A lowercase slug of 1–64 of `[a-z0-9_-]`, never reused.
	key_id: String,
	#[schemars(with = "KeyedSourceKind")]
	kind: SourceKind,
	/// At least one.
	brands: Vec<String>,
	/// A `site` key's landing, `https://<domain>[:port]`; no other kind has one.
	origin: Option<String>,
}

/// A kind a key is issued for: not `panel` nor `booking`, which sites writes itself.
struct KeyedSourceKind;

impl JsonSchema for KeyedSourceKind {
	fn schema_name() -> Cow<'static, str> {
		"KeyedSourceKind".into()
	}

	fn json_schema(_: &mut SchemaGenerator) -> Schema {
		json_schema!({ "type": "string", "enum": SourceKind::ALL.into_iter().filter(|k| k.keyed()).map(SourceKind::as_str).collect::<Vec<_>>() })
	}
}

/// The key and its secret, shown this once.
#[derive(JsonSchema, Serialize)]
#[schemars(rename = "NewSourceKey")]
pub(crate) struct NewSourceDto {
	key_id: String,
	secret: String,
}

/// Issues a signing key; its secret is in this answer only.
///
/// 409 if the key id is taken; 400 for kind `panel` or `booking`, a `site` without an origin or
/// another kind with one.
async fn add_source(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<AddSource>) -> ApiResult<Created<NewSourceDto>> {
	let kind = b.kind;
	// A key of kind panel would only let something outside pass its writes off as typed in by hand.
	if !kind.keyed() {
		return Err(ApiError::BadRequest(format!("a key of kind {kind} is not issued: it is written without one")));
	}
	let brands = b.brands.iter().map(|b| BrandId::parse(b)).collect::<Result<BTreeSet<_>, _>>()?;
	if !sa_core::ids::is_slug(&b.key_id) {
		return Err(ApiError::BadRequest("key_id is a lowercase slug of 1–64 of [a-z0-9_-]".into()));
	}
	if brands.is_empty() {
		return Err(ApiError::BadRequest("a source writes for at least one brand".into()));
	}
	let origin = b.origin.as_deref().map(SiteOrigin::parse).transpose()?;
	if origin.is_some() != (kind == SourceKind::Site) {
		return Err(ApiError::BadRequest("a site key has an origin, and no other kind has one".into()));
	}
	let Some(added) = panel.add_source(&b.key_id, kind, brands, origin).await? else {
		return Err(ApiError::Conflict(format!("a source {} exists already", b.key_id)));
	};
	tracing::info!(user_id = %caller.user_id, key_id = added.key_id, kind = %kind, "source added from the panel");
	Ok(Created(NewSourceDto {
		secret: added.secret.as_str().to_owned(),
		key_id: added.key_id,
	}))
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct KeyId {
	key_id: String,
}

/// Revokes a source key for good: its id is never issued again.
async fn revoke_source(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(KeyId { key_id }): Ids<KeyId>) -> ApiResult<NoContent> {
	if !panel.revoke_source(&key_id).await? {
		return Err(ApiError::NotFound);
	}
	tracing::info!(user_id = %caller.user_id, key_id, "source revoked from the panel");
	Ok(NoContent)
}
