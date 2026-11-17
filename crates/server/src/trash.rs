//! The caller's trash (`sa_panel::trash`): what they removed, restorable until it is purged.
//! Under dev sign-in a person who has not said keeps everything.
//!
//! `serve` purges everyone's trash hourly ([`run`]).

use std::time::Duration;

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, Ids, JsonSchema, Refusal};
use jiff::Timestamp;
use sa_auth::Trash as TrashPermission;
use sa_core::gbp::State as GbpState;
use sa_panel::{
	Panel,
	trash::{Settings, Trash, TrashedGmail, TrashedGmb},
};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiResult, Code, Id},
	http::{ApiRoute, USER},
	signin::Caller,
};

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[schemars(rename = "TrashSettings")]
pub struct SettingsDto {
	/// 1, 7, 30 or 90.
	pub keep_days: i64,
	/// Nothing purged, ever.
	pub never_purge: bool,
}

/// A gmail in the trash.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "TrashedGmail")]
pub struct TrashedGmailDto {
	pub id: i64,
	pub gmail: String,
	/// RFC 3339.
	pub removed_at: String,
	/// RFC 3339; `null`: kept until restored.
	pub purge_at: Option<String>,
	/// Its GBPs removed with it, which come back with it.
	pub gmbs: usize,
	/// Why it cannot be restored now.
	pub blocked: Option<String>,
}

/// A GBP in the trash on its own.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "TrashedGmb")]
pub struct TrashedGmbDto {
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
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Trash")]
pub struct TrashDto {
	pub settings: SettingsDto,
	pub gmails: Vec<TrashedGmailDto>,
	pub gmbs: Vec<TrashedGmbDto>,
}

/// `PUT /trash/settings`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema)]
#[schemars(rename = "SetTrashSettings")]
#[serde(deny_unknown_fields)]
pub struct SetSettingsDto {
	/// 1, 7, 30 or 90.
	pub keep_days: i64,
	/// Absent: left as it is. Present: `sa:admin:trash:keep` only.
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
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/trash", USER, trash),
		ApiRoute::new(M::POST, "/trash/gmails/{id}/restore", USER, restore_gmail),
		ApiRoute::new(M::POST, "/trash/gmbs/{id}/restore", USER, restore_gmb),
		ApiRoute::new(M::PUT, "/trash/settings", USER, set_trash_settings),
	]
}

/// The caller's trash: what they removed, restorable until purged `keep_days` after its removal.
///
/// `gmbs` are those removed on their own; `blocked` says why a restore is refused.
async fn trash(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<TrashDto>> {
	Ok(Json(panel.trash(caller.user_id, caller.dev_sign_in).await?.into()))
}

/// Restores a gmail, with the GBPs removed with it.
///
/// 409 blocked: a live gmail's address, a live GBP's name, place or phone.
async fn restore_gmail(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.restore_gmail(caller.user_id, gmail).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail restored");
	Ok(NoContent)
}

/// Restores a GBP.
///
/// 409 blocked: its gmail in the trash, a live GBP's name, place or phone.
async fn restore_gmb(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.restore_gmb(caller.user_id, gmb).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, "GBP restored");
	Ok(NoContent)
}

/// Sets how long the caller's trash keeps what they removed. `never_purge` needs
/// `sa:admin:trash:keep` (403).
async fn set_trash_settings(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(set): Body<SetSettingsDto>) -> ApiResult<NoContent> {
	if set.never_purge.is_some() && !caller.permissions.may(TrashPermission::Keep) {
		return Err(Refusal::new(Code::Forbidden, "your permissions do not allow this"));
	}
	panel.set_trash_settings(caller.user_id, set.keep_days, set.never_purge).await??;
	Ok(NoContent)
}
