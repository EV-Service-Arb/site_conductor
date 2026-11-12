//! The caller's trash (`sa_panel::trash`): what they removed, restorable until it is purged.
//! Under dev sign-in a person who has not said keeps everything.
//!
//! ```text
//! GET  /api/v1/trash                        Trash
//! POST /api/v1/trash/gmails/{id}/restore    → 204, its GBPs removed with it back too; 409 blocked
//! POST /api/v1/trash/gmbs/{id}/restore      → 204; 409 blocked
//! PUT  /api/v1/trash/settings {keep_days, never_purge?}  → 204; never_purge needs sa:admin:trash:keep
//! ```
//!
//! `serve` purges everyone's trash hourly ([`run`]).

use std::time::Duration;

use axum::{
	Extension, Json,
	extract::{Path, State, rejection::JsonRejection},
	http::{Method, StatusCode},
};
use jiff::Timestamp;
use sa_auth::Trash as TrashPermission;
use sa_core::gbp::State as GbpState;
use sa_panel::{
	Panel,
	trash::{Settings, Trash, TrashedGmail, TrashedGmb},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{
	api::{ApiError, ApiResult, body},
	gmbs::id,
	http::ApiRoute,
	signin::{Caller, Freshness},
};

#[derive(Clone, Copy, Debug, Serialize, TS)]
#[ts(rename = "TrashSettings")]
pub struct SettingsDto {
	/// 1 to 90.
	#[ts(type = "number")]
	pub keep_days: i64,
	/// Nothing purged, ever.
	pub never_purge: bool,
}

/// A gmail in the trash.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "TrashedGmail")]
pub struct TrashedGmailDto {
	#[ts(type = "number")]
	pub id: i64,
	pub gmail: String,
	/// RFC 3339.
	pub removed_at: String,
	/// RFC 3339; `null`: kept until restored.
	pub purge_at: Option<String>,
	/// Its GBPs removed with it, which come back with it.
	#[ts(type = "number")]
	pub gmbs: usize,
	/// Why it cannot be restored now.
	pub blocked: Option<String>,
}

/// A GBP in the trash on its own.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "TrashedGmb")]
pub struct TrashedGmbDto {
	#[ts(type = "number")]
	pub id: i64,
	pub name: String,
	pub state: GbpState,
	pub gmail: Option<String>,
	/// RFC 3339.
	pub removed_at: String,
	/// RFC 3339; `null`: kept until restored.
	pub purge_at: Option<String>,
	pub blocked: Option<String>,
}

/// `GET /trash`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Trash")]
pub struct TrashDto {
	pub settings: SettingsDto,
	pub gmails: Vec<TrashedGmailDto>,
	pub gmbs: Vec<TrashedGmbDto>,
}

/// `PUT /trash/settings`.
#[derive(Clone, Copy, Debug, Deserialize, TS)]
#[ts(rename = "SetTrashSettings")]
#[serde(deny_unknown_fields)]
pub struct SetSettingsDto {
	#[ts(type = "number")]
	pub keep_days: i64,
	/// Absent: left as it is. Present: `sa:admin:trash:keep` only.
	#[ts(optional)]
	pub never_purge: Option<bool>,
}

impl From<Settings> for SettingsDto {
	fn from(s: Settings) -> Self {
		Self {
			keep_days: s.keep_days,
			never_purge: s.never_purge,
		}
	}
}

impl From<TrashedGmail> for TrashedGmailDto {
	fn from(g: TrashedGmail) -> Self {
		Self {
			id: g.id,
			gmail: g.gmail,
			removed_at: g.removed_at.to_string(),
			purge_at: g.purge_at.map(|t| t.to_string()),
			gmbs: g.gmbs,
			blocked: g.blocked,
		}
	}
}

impl From<TrashedGmb> for TrashedGmbDto {
	fn from(b: TrashedGmb) -> Self {
		Self {
			id: b.id,
			name: b.name,
			state: b.state,
			gmail: b.gmail,
			removed_at: b.removed_at.to_string(),
			purge_at: b.purge_at.map(|t| t.to_string()),
			blocked: b.blocked,
		}
	}
}

impl From<Trash> for TrashDto {
	fn from(t: Trash) -> Self {
		Self {
			settings: t.settings.into(),
			gmails: t.gmails.into_iter().map(Into::into).collect(),
			gmbs: t.gmbs.into_iter().map(Into::into).collect(),
		}
	}
}

/// Purges everyone's trash past their kept days until `shutdown`; `keep_by_default` for those
/// who have not said.
pub async fn run(panel: Panel, keep_by_default: bool, shutdown: tokio::sync::watch::Receiver<bool>) {
	crate::every(Duration::from_secs(3600), shutdown, "purging the trash", || async {
		panel.purge(Timestamp::now(), keep_by_default).await
	})
	.await;
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/trash", None, Cached, trash),
		ApiRoute::new(M::POST, "/trash/gmails/{id}/restore", None, Cached, restore_gmail),
		ApiRoute::new(M::POST, "/trash/gmbs/{id}/restore", None, Cached, restore_gmb),
		ApiRoute::new(M::PUT, "/trash/settings", None, Cached, set_settings),
	]
}

async fn trash(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<TrashDto>> {
	Ok(Json(panel.trash(caller.user_id, caller.dev_sign_in).await?.into()))
}

async fn restore_gmail(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	panel.restore_gmail(caller.user_id, gmail).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail restored");
	Ok(StatusCode::NO_CONTENT)
}

async fn restore_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let gmb = id(&raw)?;
	panel.restore_gmb(caller.user_id, gmb).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GBP restored");
	Ok(StatusCode::NO_CONTENT)
}

async fn set_settings(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<SetSettingsDto>, JsonRejection>) -> ApiResult<StatusCode> {
	let set = body(b)?;
	if set.never_purge.is_some() && !caller.permissions.may(TrashPermission::Keep) {
		return Err(ApiError::Forbidden);
	}
	panel.set_trash_settings(caller.user_id, set.keep_days, set.never_purge).await??;
	Ok(StatusCode::NO_CONTENT)
}
