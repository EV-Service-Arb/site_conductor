//! A gmail's connections (`sa_panel::connections`), its owner's only: its inbox and its own Maps
//! key, checked before they are stored and never answered back; its browser is [`crate::browser`]'s.

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::JsonSchema;
use jiff::Timestamp;
use sa_panel::{
	Panel,
	browsers::Provider,
	connections::{Checked, Connection, Connections, Imap, MapsKey, NewImap},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
	api::{ApiResult, Body, Id, Ids},
	browser::{BrowserDto, ExitDto},
	http::{ApiRoute, ApiState, Gate},
	signin::Caller,
	tokens::PriceDto,
};

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Checked")]
pub struct CheckedDto {
	pub at: String,
	/// `null`: it went through.
	pub error: Option<String>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Imap")]
pub struct ImapDto {
	pub host: String,
	pub username: String,
	pub set_at: String,
	pub checked: CheckedDto,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MapsKey")]
pub struct MapsKeyDto {
	pub last4: String,
	pub set_at: String,
	pub checked: CheckedDto,
	/// Why the key is passed over now, until Google takes it again.
	pub out: Option<String>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Connections")]
pub struct ConnectionsDto {
	pub imap: Option<ImapDto>,
	pub maps_key: Option<MapsKeyDto>,
	/// What a Places request on the shared keys costs.
	pub prices: Vec<PriceDto>,
	pub browser: Option<crate::browser::BrowserDto>,
}
impl ConnectionsDto {
	fn of(c: Connections, running: Option<crate::browser::RunningDto>, film: bool) -> Self {
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
			prices: c.prices.into_iter().map(PriceDto::from).collect(),
			browser: c.browser.map(|b| BrowserDto {
				provider: b.provider,
				device: b.device.to_string(),
				endpoint: b.endpoint,
				exit: ExitDto {
					ip: b.exit.ip,
					country: b.exit.country,
					city: b.exit.city,
					zone: b.exit.zone,
					mobile: b.exit.mobile,
				},
				set_at: b.set_at.to_string(),
				checked: b.checked.into(),
				running,
				film,
			}),
		}
	}
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetImap {
	host: String,
	username: String,
	password: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetMapsKey {
	key: String,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let user = Gate::USER;
	vec![
		ApiRoute::new(M::GET, "/gmails/{id}/connections", user, gmail_connections),
		ApiRoute::new(M::PUT, "/gmails/{id}/imap", user, set_imap),
		ApiRoute::new(M::PUT, "/gmails/{id}/maps_key", user, set_maps_key),
		ApiRoute::new(M::POST, "/gmails/{id}/imap/test", user, test_imap),
		ApiRoute::new(M::POST, "/gmails/{id}/maps_key/test", user, test_maps_key),
		ApiRoute::new(M::DELETE, "/gmails/{id}/imap", user, remove_imap),
		ApiRoute::new(M::DELETE, "/gmails/{id}/maps_key", user, remove_maps_key),
	]
}

/// A gmail's inbox, Maps key and browser, and what Places requests cost on the shared keys: no
/// secret, ever.
async fn gmail_connections(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<ConnectionsDto>> {
	answer(&state, caller.user_id, id).await.map(Json)
}

/// The gmail's connections, its browser's running asked of the runner.
pub(crate) async fn answer(state: &ApiState, owner: uuid::Uuid, gmail: i64) -> ApiResult<ConnectionsDto> {
	let c = state.panel.connections(owner, gmail).await??;
	let (running, film) = match (&c.browser, &state.browsers) {
		(Some(browser), Some(b)) => {
			let (running, on_runner) = b.state(gmail).await?;
			(
				running,
				if browser.provider == Provider::Multilogin {
					browser.film_material.is_some()
				} else {
					on_runner
				},
			)
		}
		_ => (None, false),
	};
	Ok(ConnectionsDto::of(c, running, film))
}

/// Sets the gmail's inbox, kept only once it logs in (IMAPS :993, a public address only).
///
/// 400 refused, nothing stored.
async fn set_imap(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>, Body(b): Body<SetImap>) -> ApiResult<Json<ConnectionsDto>> {
	let new = NewImap {
		host: b.host,
		username: b.username,
		password: Zeroizing::new(b.password),
	};
	state.panel.set_imap(caller.user_id, gmail, new, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's IMAP set");
	answer(&state, caller.user_id, gmail).await.map(Json)
}

/// Sets the gmail's own Maps key, kept only once Google answers for it (an unbilled id-only
/// details); the caller's Places requests are paid with it first.
///
/// 400 refused, nothing stored.
async fn set_maps_key(
	State(state): State<ApiState>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id: gmail }): Ids<Id<i64>>,
	Body(b): Body<SetMapsKey>,
) -> ApiResult<Json<ConnectionsDto>> {
	state.panel.set_maps_key(caller.user_id, gmail, Zeroizing::new(b.key), Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's Maps key set");
	answer(&state, caller.user_id, gmail).await.map(Json)
}

/// Checks the gmail's inbox again, the check recorded. 404 none set.
async fn test_imap(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<ConnectionsDto>> {
	check(&state, &caller, id, Connection::Imap).await
}

/// Checks the gmail's Maps key again, the check recorded. 404 none set.
async fn test_maps_key(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<ConnectionsDto>> {
	check(&state, &caller, id, Connection::MapsKey).await
}

/// Forgets the gmail's inbox.
async fn remove_imap(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	disconnect(&panel, &caller, id, Connection::Imap).await
}

/// Forgets the gmail's Maps key.
async fn remove_maps_key(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	disconnect(&panel, &caller, id, Connection::MapsKey).await
}

async fn check(state: &ApiState, caller: &Caller, gmail: i64, which: Connection) -> ApiResult<Json<ConnectionsDto>> {
	state.panel.check(caller.user_id, gmail, which, Timestamp::now()).await??;
	answer(state, caller.user_id, gmail).await.map(Json)
}

async fn disconnect(panel: &Panel, caller: &Caller, gmail: i64, which: Connection) -> ApiResult<NoContent> {
	panel.disconnect(caller.user_id, gmail, which).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, %which, "gmail's connection removed");
	Ok(NoContent)
}

impl From<Checked> for CheckedDto {
	fn from(c: Checked) -> Self {
		Self {
			at: c.at.to_string(),
			error: c.error,
		}
	}
}
