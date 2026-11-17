//! The conductor's own `/api/v1`: `/me`, the source keys and the connected sites
//! (`connected`), and what every module's operations take and answer. JSON in and out; every
//! request has passed its row's gate (`signin`'s `Vouch`), so a [`Caller`] is in its extensions.

use std::{borrow::Cow, collections::BTreeSet};

use axum::{
	Extension, Json,
	extract::State,
	http::{Method, StatusCode},
	response::NoContent,
};
use ev_lib::api::{
	Body, Created, ErrorCode, Ids, JsonSchema, OpenApi, Refusal, Refuses,
	schemars::{Schema, SchemaGenerator, json_schema},
};
use sa_auth::{SourceKind, Sources};
use sa_core::{
	Invalid,
	ids::{BrandId, SiteOrigin},
};
use sa_panel::{Panel, Refused};
use serde::{Deserialize, Serialize};

use crate::{
	connected,
	http::{ApiRoute, ApiState, FRESH, USER},
	signin::Caller,
};

pub(crate) type ApiResult<T> = Result<T, Refusal<Code>>;

/// Why a request was refused, as a client is told: every refusal the conductor writes itself.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
pub enum Code {
	BadRequest,
	/// No session, or one closed.
	Unauthenticated,
	/// A cookie's write without the page's CSRF header, or a socket from another origin.
	Csrf,
	Forbidden,
	NotFound,
	Conflict,
	TooLarge,
	TooMany,
	/// A pull needs more than the balance affords.
	Unaffordable,
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
	/// Not now: the message says until when.
	Unavailable,
	/// No browser provider is configured: no browser route answers anything else.
	BrowsersNotConnected,
	/// What this answer stands on did not answer.
	BadGateway,
	Internal,
}

impl ErrorCode for Code {
	fn status(self) -> StatusCode {
		match self {
			Self::BadRequest => StatusCode::BAD_REQUEST,
			Self::Unauthenticated => StatusCode::UNAUTHORIZED,
			Self::Csrf | Self::Forbidden => StatusCode::FORBIDDEN,
			Self::NotFound => StatusCode::NOT_FOUND,
			Self::Conflict => StatusCode::CONFLICT,
			Self::TooLarge => StatusCode::PAYLOAD_TOO_LARGE,
			Self::TooMany => StatusCode::TOO_MANY_REQUESTS,
			Self::Unaffordable => StatusCode::PAYMENT_REQUIRED,
			Self::NoStudy | Self::NoGoogleCategory | Self::NoKeywords | Self::AnotherCountry | Self::Unstudied => StatusCode::UNPROCESSABLE_ENTITY,
			Self::Unavailable | Self::BrowsersNotConnected => StatusCode::SERVICE_UNAVAILABLE,
			Self::BadGateway => StatusCode::BAD_GATEWAY,
			Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
		}
	}
}

impl Refuses<eyre::Report> for Code {
	fn refusal(e: eyre::Report) -> Refusal<Self> {
		crate::report(&e, "the conductor's API");
		Refusal::new(Self::Internal, "internal error")
	}
}

impl Refuses<Refused> for Code {
	fn refusal(r: Refused) -> Refusal<Self> {
		let code = match r {
			Refused::Invalid(_) => Self::BadRequest,
			Refused::NotFound => Self::NotFound,
			Refused::Forbidden => Self::Forbidden,
			Refused::Conflict(_) => Self::Conflict,
			Refused::TooLarge(_) => Self::TooLarge,
			Refused::Unavailable(_) | Refused::Closed => Self::Unavailable,
		};
		Refusal::new(code, r.to_string())
	}
}

impl Refuses<Invalid> for Code {
	fn refusal(e: Invalid) -> Refusal<Self> {
		Refusal::new(Self::BadRequest, e.0)
	}
}

impl From<sa_auth::Hop> for Code {
	fn from(h: sa_auth::Hop) -> Self {
		match h {
			sa_auth::Hop::BadRequest => Self::BadRequest,
			sa_auth::Hop::Unauthenticated => Self::Unauthenticated,
			sa_auth::Hop::Csrf => Self::Csrf,
			sa_auth::Hop::Unavailable => Self::Unavailable,
			sa_auth::Hop::BadGateway => Self::BadGateway,
			sa_auth::Hop::Internal => Self::Internal,
		}
	}
}

impl From<sa_auth::Denied> for Code {
	fn from(d: sa_auth::Denied) -> Self {
		match d {
			sa_auth::Denied::Unauthenticated => Self::Unauthenticated,
			sa_auth::Denied::Forbidden => Self::Forbidden,
		}
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let admin = USER.needs(Sources::Manage);
	vec![
		ApiRoute::new(M::GET, "/me", USER, me),
		ApiRoute::new(M::GET, "/openapi.json", USER, openapi),
		ApiRoute::new(M::GET, "/sources", admin, list_sources),
		ApiRoute::new(M::GET, "/sources/sites", admin, connected::sites),
		// A key must not be minted or revoked on a permission revoked a moment ago.
		ApiRoute::new(M::POST, "/sources", FRESH.needs(Sources::Manage), add_source),
		ApiRoute::new(M::DELETE, "/sources/{key_id}", FRESH.needs(Sources::Manage), revoke_source),
	]
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
	#[schemars(pattern(sa_core::ids::SLUG_PATTERN))]
	key_id: String,
	#[schemars(with = "KeyedSourceKind")]
	kind: SourceKind,
	/// At least one.
	brands: Vec<String>,
	/// A `site` key's landing, `https://<domain>[:port]`; no other kind has one.
	#[schemars(pattern(sa_core::ids::SITE_ORIGIN_PATTERN))]
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
		return Err(Refusal::new(Code::BadRequest, format!("a key of kind {kind} is not issued: it is written without one")));
	}
	let brands = b.brands.iter().map(|b| BrandId::parse(b)).collect::<Result<BTreeSet<_>, _>>()?;
	if !sa_core::ids::is_slug(&b.key_id) {
		return Err(Refusal::new(Code::BadRequest, "key_id is a lowercase slug of 1–64 of [a-z0-9_-]"));
	}
	if brands.is_empty() {
		return Err(Refusal::new(Code::BadRequest, "a source writes for at least one brand"));
	}
	let origin = b.origin.as_deref().map(SiteOrigin::parse).transpose()?;
	if origin.is_some() != (kind == SourceKind::Site) {
		return Err(Refusal::new(Code::BadRequest, "a site key has an origin, and no other kind has one"));
	}
	let Some(added) = panel.add_source(&b.key_id, kind, brands, origin).await? else {
		return Err(Refusal::new(Code::Conflict, format!("a source {} exists already", b.key_id)));
	};
	tracing::info!(user_id = %caller.user_id, key_id = added.key_id, kind = %kind, "source added from the panel");
	Ok(Created(NewSourceDto {
		secret: added.secret.as_str().to_owned(),
		key_id: added.key_id,
	}))
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct KeyId {
	#[schemars(pattern(sa_core::ids::SLUG_PATTERN))]
	pub(crate) key_id: String,
}

/// Revokes a source key for good: its id is never issued again.
async fn revoke_source(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(KeyId { key_id }): Ids<KeyId>) -> ApiResult<NoContent> {
	if !panel.revoke_source(&key_id).await? {
		return Err(Refusal::new(Code::NotFound, "not found"));
	}
	tracing::info!(user_id = %caller.user_id, key_id, "source revoked from the panel");
	Ok(NoContent)
}
