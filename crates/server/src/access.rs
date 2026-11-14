//! A signed-in user asking the admins for access (`sa_panel::access`). Open to every signed-in
//! user: whoever asks usually holds nothing yet.
//!
//! A need is a permission or an alias of the `sa` catalog; held is holding every permission it
//! stands for. A request whose need the caller holds is dropped whenever they call either.

use std::{borrow::Cow, sync::LazyLock};

use axum::{Extension, Json, extract::State, http::Method};
use ev_lib::api::{
	JsonSchema, Upserted,
	schemars::{Schema, SchemaGenerator, json_schema},
};
use jiff::Timestamp;
use sa_auth::{Catalog, PermissionSet};
use sa_panel::{Panel, access::AccessRequest};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiError, ApiResult, Body},
	http::{ApiRoute, Gate},
	signin::Caller,
};

/// An access request of the caller's, standing.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AccessRequest")]
pub struct AccessRequestDto {
	#[schemars(with = "Need")]
	pub need: String,
	/// RFC 3339.
	pub requested_at: String,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| Catalog::collect("sa", 0));

/// A permission of the `sa` catalog, any service's.
pub(crate) struct PermissionName;

impl JsonSchema for PermissionName {
	fn schema_name() -> Cow<'static, str> {
		"Permission".into()
	}

	fn json_schema(_: &mut SchemaGenerator) -> Schema {
		json_schema!({ "type": "string", "enum": CATALOG.permissions.iter().collect::<Vec<_>>() })
	}
}

/// A permission or an alias of the `sa` catalog.
struct Need;

impl JsonSchema for Need {
	fn schema_name() -> Cow<'static, str> {
		"Need".into()
	}

	fn json_schema(_: &mut SchemaGenerator) -> Schema {
		json_schema!({ "type": "string", "enum": CATALOG.permissions.iter().chain(CATALOG.aliases.keys()).collect::<Vec<_>>() })
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	vec![
		ApiRoute::new(Method::POST, "/access/requests", Gate::USER, request_access),
		ApiRoute::new(Method::GET, "/access/requests/mine", Gate::USER, my_access_requests),
	]
}

impl From<AccessRequest> for AccessRequestDto {
	fn from(r: AccessRequest) -> Self {
		Self {
			need: r.need,
			requested_at: r.requested_at.to_string(),
		}
	}
}

/// Whether `permissions` hold everything `need` stands for; `None` for a need outside the catalog.
fn holds(permissions: &PermissionSet, need: &str) -> Option<bool> {
	let has = |p: &str| permissions.iter().any(|q| q == p);
	if CATALOG.permissions.contains(need) {
		return Some(has(need));
	}
	CATALOG.aliases.get(need).map(|members| members.iter().all(|m| has(m)))
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct Ask {
	#[schemars(with = "Need")]
	need: String,
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "AccessRequests")]
pub(crate) struct AccessRequestsDto {
	requests: Vec<AccessRequestDto>,
}

/// Asks the admins for a need, made now (201); the one standing (200) if asked within 24 h, nobody
/// told again.
///
/// 400 a need neither a permission nor an alias of the sa catalog; 409 one the caller holds already.
async fn request_access(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(Ask { need }): Body<Ask>) -> ApiResult<Upserted<AccessRequestDto>> {
	let held = holds(&caller.permissions, &need).ok_or_else(|| ApiError::BadRequest(format!("{need:?} is no permission or alias of the sa catalog")))?;
	panel.pending_access(caller.user_id, |n| holds(&caller.permissions, n) == Some(true)).await?;
	if held {
		return Err(ApiError::Conflict(format!("you hold {need} already")));
	}
	let (r, made) = panel.request_access(caller.user_id, &caller.email, &caller.preferred_name, &need, Timestamp::now()).await?;
	let r = AccessRequestDto::from(r);
	Ok(if made { Upserted::Created(r) } else { Upserted::Existed(r) })
}

/// The caller's standing access requests; those whose need they now hold are dropped.
async fn my_access_requests(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<AccessRequestsDto>> {
	let pending = panel.pending_access(caller.user_id, |n| holds(&caller.permissions, n) == Some(true)).await?;
	Ok(Json(AccessRequestsDto {
		requests: pending.into_iter().map(AccessRequestDto::from).collect(),
	}))
}
