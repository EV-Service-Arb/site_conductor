//! A gmail's connections (`sa_panel::connections`), its owner's only: its inbox and its own Maps
//! key, checked before they are stored and never answered back.
//!
//! ```text
//! GET    /api/v1/gmails/{id}/connections     Connections
//! PUT    /api/v1/gmails/{id}/imap {host, username, password}  → Connections; 400 refused, nothing stored
//! PUT    /api/v1/gmails/{id}/maps_key {key}  → Connections; 400 Google refuses it, nothing stored
//! POST   /api/v1/gmails/{id}/imap/test       → Connections, the check recorded; 404 none set
//! POST   /api/v1/gmails/{id}/maps_key/test   (the same)
//! DELETE /api/v1/gmails/{id}/imap            → 204
//! DELETE /api/v1/gmails/{id}/maps_key        → 204
//! ```

use axum::{
	Extension, Json,
	extract::{Path, State, rejection::JsonRejection},
	http::{Method, StatusCode},
};
use jiff::Timestamp;
use sa_panel::{
	Panel,
	connections::{Checked, Connection, Connections, Imap, MapsKey, NewImap},
};
use sa_places::Sku;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use zeroize::Zeroizing;

use crate::{
	api::{ApiError, ApiResult, body},
	http::ApiRoute,
	signin::{Caller, Freshness},
};

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Checked")]
pub struct CheckedDto {
	pub at: String,
	/// `null`: it went through.
	pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Imap")]
pub struct ImapDto {
	pub host: String,
	pub username: String,
	pub set_at: String,
	pub checked: CheckedDto,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "MapsKey")]
pub struct MapsKeyDto {
	pub last4: String,
	pub set_at: String,
	pub checked: CheckedDto,
	/// Why the key is passed over now, until Google takes it again.
	pub out: Option<String>,
}

/// Tokens a Places request on the shared keys costs; `null`, not sold yet.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[ts(rename = "Prices")]
#[serde(deny_unknown_fields)]
pub struct PricesDto {
	#[ts(type = "number | null")]
	pub details: Option<i64>,
	#[ts(type = "number | null")]
	pub search: Option<i64>,
}

#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Connections")]
pub struct ConnectionsDto {
	pub imap: Option<ImapDto>,
	pub maps_key: Option<MapsKeyDto>,
	pub prices: PricesDto,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetImap {
	host: String,
	username: String,
	password: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetMapsKey {
	key: String,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/gmails/{id}/connections", None, Cached, connections),
		ApiRoute::new(M::PUT, "/gmails/{id}/imap", None, Cached, set_imap),
		ApiRoute::new(M::PUT, "/gmails/{id}/maps_key", None, Cached, set_maps_key),
		ApiRoute::new(M::POST, "/gmails/{id}/imap/test", None, Cached, |s: State<Panel>, c: Extension<Caller>, p: Path<String>| {
			check(s, c, p, Connection::Imap)
		}),
		ApiRoute::new(M::POST, "/gmails/{id}/maps_key/test", None, Cached, |s: State<Panel>, c: Extension<Caller>, p: Path<String>| {
			check(s, c, p, Connection::MapsKey)
		}),
		ApiRoute::new(M::DELETE, "/gmails/{id}/imap", None, Cached, |s: State<Panel>, c: Extension<Caller>, p: Path<String>| {
			disconnect(s, c, p, Connection::Imap)
		}),
		ApiRoute::new(M::DELETE, "/gmails/{id}/maps_key", None, Cached, |s: State<Panel>, c: Extension<Caller>, p: Path<String>| {
			disconnect(s, c, p, Connection::MapsKey)
		}),
	]
}

fn id(raw: &str) -> ApiResult<i64> {
	raw.parse().map_err(|_| ApiError::NotFound)
}

async fn connections(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<Json<ConnectionsDto>> {
	Ok(Json(panel.connections(caller.user_id, id(&raw)?).await??.into()))
}

async fn set_imap(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetImap>, JsonRejection>) -> ApiResult<Json<ConnectionsDto>> {
	let gmail = id(&raw)?;
	let b = body(b)?;
	let new = NewImap {
		host: b.host,
		username: b.username,
		password: Zeroizing::new(b.password),
	};
	let set = panel.set_imap(caller.user_id, gmail, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's IMAP set");
	Ok(Json(set.into()))
}

async fn set_maps_key(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Path(raw): Path<String>,
	b: Result<Json<SetMapsKey>, JsonRejection>,
) -> ApiResult<Json<ConnectionsDto>> {
	let gmail = id(&raw)?;
	let set = panel.set_maps_key(caller.user_id, gmail, Zeroizing::new(body(b)?.key), Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's Maps key set");
	Ok(Json(set.into()))
}

async fn check(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, which: Connection) -> ApiResult<Json<ConnectionsDto>> {
	Ok(Json(panel.check(caller.user_id, id(&raw)?, which, Timestamp::now()).await??.into()))
}

async fn disconnect(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, which: Connection) -> ApiResult<StatusCode> {
	let gmail = id(&raw)?;
	panel.disconnect(caller.user_id, gmail, which).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, %which, "gmail's connection removed");
	Ok(StatusCode::NO_CONTENT)
}

impl From<Checked> for CheckedDto {
	fn from(c: Checked) -> Self {
		Self {
			at: c.at.to_string(),
			error: c.error,
		}
	}
}

impl From<Vec<(Sku, Option<i64>)>> for PricesDto {
	fn from(prices: Vec<(Sku, Option<i64>)>) -> Self {
		let of = |sku| prices.iter().find(|(s, _)| *s == sku).expect("every SKU is read").1;
		Self {
			details: of(Sku::Details),
			search: of(Sku::Search),
		}
	}
}

impl From<Connections> for ConnectionsDto {
	fn from(c: Connections) -> Self {
		Self {
			imap: c.imap.map(|i: Imap| ImapDto {
				host: i.host,
				username: i.username,
				set_at: i.set_at.to_string(),
				checked: i.checked.into(),
			}),
			maps_key: c.maps_key.map(|(k, out): (MapsKey, _)| MapsKeyDto {
				last4: k.last4,
				set_at: k.set_at.to_string(),
				checked: k.checked.into(),
				out,
			}),
			prices: c.prices.into(),
		}
	}
}
