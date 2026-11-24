//! A gmail's connections (`sa_panel::connections`), its owner's only: its inbox, its own Maps key
//! and its Business Profile client and grant, checked before they are stored and never answered
//! back; its browser is [`crate::browser`]'s.
//! In `serve`, each inbox followed for its mail with Google (`sa_panel::inbox`).

use std::{collections::BTreeMap, time::Duration};

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, Ids, JsonSchema};
use jiff::{SignedDuration, Timestamp};
use sa_panel::{
	Panel,
	browsers::Provider,
	connections::{Business, Checked, Connection, Connections, Imap, MapsKey, NewImap},
	inbox::Gmail,
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use zeroize::Zeroizing;

use crate::{
	api::{ApiResult, Id},
	browser::{BrowserDto, ExitDto},
	http::{ApiRoute, ApiState, USER},
	signin::Caller,
	tokens::PriceDto,
};

/// Each inbox is read this often.
const FOLLOW_EVERY: Duration = Duration::from_secs(60);

/// An inbox failing this long is lost: its owner is told.
const LOST_AFTER: SignedDuration = SignedDuration::from_mins(15);

/// Where Google sends a Business Profile consent back to.
pub(crate) const BUSINESS_CALLBACK: &str = "/auth/google/business";

/// Reads every gmail's inbox past its cursor until `shutdown` turns true. Nothing here ends `serve`.
pub async fn follow(panel: Panel, mut shutdown: watch::Receiver<bool>) {
	// an inbox failing every pass (its app password revoked) is said when it starts and ends
	let mut failing: BTreeMap<i64, Timestamp> = BTreeMap::new();
	while !*shutdown.borrow() {
		let pass = async {
			let gmails = match panel.inboxes().await {
				Ok(g) => g,
				Err(e) => return crate::report(&e, "listing the inboxes followed"),
			};
			for gmail in gmails {
				let read = async {
					while panel.follow_inbox(gmail, &Gmail, Timestamp::now()).await? {}
					eyre::Ok(())
				};
				match read.await {
					Ok(()) =>
						if failing.remove(&gmail).is_some() {
							tracing::info!(gmail_id = gmail, "its inbox reads again");
						},
					Err(e) => {
						let now = Timestamp::now();
						let since = *failing.entry(gmail).or_insert(now);
						if since == now {
							crate::report(&e.wrap_err(format!("gmail {gmail}'s inbox, read again every {}s", FOLLOW_EVERY.as_secs())), "following an inbox");
							continue;
						}
						tracing::debug!(gmail_id = gmail, error = %e, "its inbox still fails");
						if now.duration_since(since) >= LOST_AFTER
							&& let Err(e) = panel.alerts_inbox_lost(gmail, &format!("{e:#}"), now).await
						{
							crate::report(&e, "telling of an inbox lost");
						}
					}
				}
			}
		};
		tokio::select! {
			() = pass => {}
			_ = shutdown.changed() => return,
		}
		tokio::select! {
			() = tokio::time::sleep(FOLLOW_EVERY) => {}
			_ = shutdown.changed() => return,
		}
	}
}

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
#[schemars(rename = "Business")]
pub struct BusinessDto {
	pub client_id: String,
	pub set_at: String,
	/// `null`: the gmail has not granted `business.manage` yet.
	pub granted_at: Option<String>,
	/// `null`: Google not asked since the client was set.
	pub checked: Option<CheckedDto>,
	/// What the client's OAuth settings must list as an authorized redirect URI.
	pub redirect_uri: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Connections")]
pub struct ConnectionsDto {
	pub imap: Option<ImapDto>,
	pub maps_key: Option<MapsKeyDto>,
	/// What a Places request on the shared keys costs.
	pub prices: Vec<PriceDto>,
	pub browser: Option<crate::browser::BrowserDto>,
	pub business: Option<BusinessDto>,
	/// What a Business Profile client's OAuth settings must list as an authorized redirect URI.
	pub business_redirect_uri: String,
}
impl ConnectionsDto {
	fn of(c: Connections, running: Option<crate::browser::RunningDto>, film: bool, redirect_uri: String) -> Self {
		Self {
			business: c.business.map(|b: Business| BusinessDto {
				client_id: b.client_id,
				set_at: b.set_at.to_string(),
				granted_at: b.granted_at.map(|t| t.to_string()),
				checked: b.checked.map(CheckedDto::from),
				redirect_uri: redirect_uri.clone(),
			}),
			business_redirect_uri: redirect_uri,
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

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetBusiness {
	client_id: String,
	client_secret: String,
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "Consent")]
pub(crate) struct ConsentDto {
	/// Google's consent page, for the gmail to grant `business.manage` on.
	url: String,
}

fn redirect_uri(s: &crate::signin::SignIn) -> String {
	format!("{}{BUSINESS_CALLBACK}", s.panel_origin())
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/gmails/{id}/connections", USER, gmail_connections),
		ApiRoute::new(M::PUT, "/gmails/{id}/imap", USER, set_imap),
		ApiRoute::new(M::PUT, "/gmails/{id}/maps_key", USER, set_maps_key),
		ApiRoute::new(M::POST, "/gmails/{id}/imap/test", USER, test_imap),
		ApiRoute::new(M::POST, "/gmails/{id}/maps_key/test", USER, test_maps_key),
		ApiRoute::new(M::DELETE, "/gmails/{id}/imap", USER, remove_imap),
		ApiRoute::new(M::DELETE, "/gmails/{id}/maps_key", USER, remove_maps_key),
		ApiRoute::new(M::PUT, "/gmails/{id}/business", USER, set_business),
		ApiRoute::new(M::POST, "/gmails/{id}/business/consent", USER, business_consent),
		ApiRoute::new(M::POST, "/gmails/{id}/business/test", USER, test_business),
		ApiRoute::new(M::DELETE, "/gmails/{id}/business", USER, remove_business),
	]
}

/// A gmail's inbox, Maps key and browser, and what Places requests cost on the shared keys: no
/// secret, ever.
async fn gmail_connections(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<ConnectionsDto>> {
	answer(&state, caller.user_id, id).await.map(Json)
}

/// The gmail's connections, its browser's running asked of the runner.
pub(crate) async fn answer(state: &ApiState, owner: uuid::Uuid, gmail: i64) -> ApiResult<ConnectionsDto> {
	let c = state.sign_in.panel.connections(owner, gmail).await??;
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
	Ok(ConnectionsDto::of(c, running, film, redirect_uri(&state.sign_in)))
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
	state.sign_in.panel.set_imap(caller.user_id, gmail, new, Timestamp::now()).await??;
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
	state.sign_in.panel.set_maps_key(caller.user_id, gmail, Zeroizing::new(b.key), Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's Maps key set");
	answer(&state, caller.user_id, gmail).await.map(Json)
}

/// Sets the gmail's Business Profile OAuth client, kept only once Google knows it; a grant to
/// the one before is dropped.
///
/// 400 refused, nothing stored.
async fn set_business(
	State(state): State<ApiState>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id: gmail }): Ids<Id<i64>>,
	Body(b): Body<SetBusiness>,
) -> ApiResult<Json<ConnectionsDto>> {
	let uri = redirect_uri(&state.sign_in);
	state
		.sign_in
		.panel
		.set_business(caller.user_id, gmail, &b.client_id, Zeroizing::new(b.client_secret), &uri, Timestamp::now())
		.await??;
	tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's Business Profile client set");
	answer(&state, caller.user_id, gmail).await.map(Json)
}

/// Google's consent page for the gmail's client, back at `/auth/google/business` within 15
/// minutes. 404 no client set.
async fn business_consent(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmail }): Ids<Id<i64>>) -> ApiResult<Json<ConsentDto>> {
	let url = state
		.sign_in
		.panel
		.business_consent(caller.user_id, gmail, &redirect_uri(&state.sign_in), Timestamp::now())
		.await??;
	Ok(Json(ConsentDto { url }))
}

/// Asks the Business Profile APIs on the gmail's grant again, the check recorded. 404 none granted.
async fn test_business(State(state): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<ConnectionsDto>> {
	check(&state, &caller, id, Connection::Business).await
}

/// Forgets the gmail's Business Profile client and grant.
async fn remove_business(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	disconnect(&panel, &caller, id, Connection::Business).await
}

#[derive(Deserialize)]
pub(crate) struct Callback {
	state: String,
	code: Option<String>,
	error: Option<String>,
}

/// Google's redirect after a Business Profile consent: the grant kept, then back to the gmail's
/// Connections; a refusal is a page saying it.
pub(crate) async fn business_callback(
	State(s): State<crate::signin::SignIn>,
	headers: axum::http::HeaderMap,
	axum::extract::Query(q): axum::extract::Query<Callback>,
) -> axum::response::Response {
	use axum::response::{IntoResponse, Redirect};
	let caller = match crate::signin::check(&s, &headers, crate::signin::Freshness::Cached).await {
		Ok(c) => c,
		Err(r) => return crate::signin::refused(&s, r),
	};
	let refusal = |why: String| {
		crate::signin::private(
			crate::signin::Said {
				title: "Business Profile not connected",
				message: why,
			}
			.into_response(),
		)
	};
	let code = match (q.code, q.error) {
		(Some(code), None) => code,
		(_, Some(error)) => return refusal(format!("Google said {error}.")),
		(None, None) => return refusal("Google sent neither a code nor an error.".to_owned()),
	};
	match s.panel.grant_business(caller.user_id, &q.state, &code, &redirect_uri(&s), Timestamp::now()).await {
		Ok(Ok(gmail)) => {
			tracing::info!(user_id = %caller.user_id, gmail_id = gmail, "gmail's Business Profile granted");
			crate::signin::private(Redirect::to(&format!("/?account={gmail}&tab=connections")).into_response())
		}
		Ok(Err(r)) => refusal(r.to_string()),
		Err(e) => {
			crate::report(&e, "granting a gmail's Business Profile");
			refusal("Something failed on our side; it is reported.".to_owned())
		}
	}
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
	state.sign_in.panel.check(caller.user_id, gmail, which, Timestamp::now()).await??;
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
