//! Alerts in `serve`: the services' feeds and concierge as the hub reads them, the hub's work, and
//! the `/api/v1/alerts` routes — every signed-in user's own destinations. What goes where is the
//! engine's (`sa_panel::alerts`, `sa_panel::telegram`).

use std::{collections::BTreeMap, time::Duration};

use axum::{
	Extension, Json,
	body::Body as RawBody,
	extract::{FromRef, State},
	http::{HeaderName, HeaderValue, Method, Request, StatusCode, header},
	response::NoContent,
};
use ev_lib::api::{Body, Created, Ids, JsonSchema, Refusal};
use jiff::Timestamp;
use sa_alerts::{Call, NotePage, Rule};
use sa_auth::{Service, User};
use sa_core::feeds;
use sa_panel::{
	Panel, Refused,
	alerts::{Destination, Directory, DirectoryError, Feeds, Hub, Identity},
	telegram::Kind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;

use crate::{
	api::{ApiResult, Code, Id},
	concierge::Concierge,
	forward::{Forward, SITES_TIMEOUT},
	http::{ApiRoute, ApiState, Gate},
	signin::Caller,
	telegram::{self, BotApi, BotName},
};

/// How often each part of the work runs.
const DELIVER_EVERY: Duration = Duration::from_millis(500);

const FAN_OUT_EVERY: Duration = Duration::from_secs(2);
const RECHECK_EVERY: Duration = Duration::from_secs(60);
/// A service that publishes no feed yet (404) is asked again this often.
const LOOK_AGAIN: Duration = Duration::from_secs(60);
/// The hub with its Telegram bot, as `serve` runs it.
#[derive(Clone)]
pub struct Telegram {
	pub hub: Hub<BotApi, Concierge, Forward>,
	pub name: BotName,
}

impl FromRef<ApiState> for Option<Telegram> {
	fn from_ref(s: &ApiState) -> Self {
		s.telegram.clone()
	}
}

// ── /api/v1/alerts ──────────────────────────────────────────────────────────────────────

/// The caller's alerts, and what every change answers.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Alerts")]
pub struct AlertsDto {
	/// A bot is configured: Telegram destinations can be added.
	pub enabled: bool,
	/// The bot's `@username`, once known: what a channel adds as an admin.
	pub bot: Option<String>,
	/// The rules open to the caller: their permissions', and the GMB rules once they hold a GMB.
	pub rules: Vec<Rule>,
	pub destinations: Vec<DestinationDto>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Destination")]
pub struct DestinationDto {
	pub id: i64,
	pub kind: Kind,
	/// A private chat's account, a channel's or group's title.
	pub title: Option<String>,
	pub topic_id: Option<i64>,
	/// The GMB rules only for this gmail's GMBs.
	pub gmail_id: Option<i64>,
	/// Why nothing reaches it: a test that arrives clears it.
	pub dead: Option<String>,
	/// Every rule open to the caller, on or off here.
	pub rules: BTreeMap<Rule, bool>,
}

/// Runs the hub until `shutdown` turns true: the bot's poller (on the replica holding its lease),
/// the conductor's own rules, each service's feed, the delivery and the access checks, each on its
/// own schedule. A failing pass is reported and tried again at the next tick; nothing here ends
/// `serve`.
pub async fn run(t: Telegram, shutdown: watch::Receiver<bool>) {
	let hub = &t.hub;
	tokio::join!(
		telegram::poll(hub, &t.name, shutdown.clone()),
		crate::every(FAN_OUT_EVERY, shutdown.clone(), "alerts: the conductor's own", || async {
			hub.fan_out_own(Timestamp::now()).await.map(drop)
		}),
		feed(hub, Service::Sites, shutdown.clone()),
		feed(hub, Service::ReviewArchive, shutdown.clone()),
		crate::every(DELIVER_EVERY, shutdown.clone(), "alerts: telegram delivery", || async {
			hub.deliver(Timestamp::now()).await.map(drop)
		}),
		crate::every(RECHECK_EVERY, shutdown, "alerts: access check", || async { hub.recheck_access(Timestamp::now()).await.map(drop) }),
	);
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let user = Gate::USER;
	vec![
		ApiRoute::new(M::GET, "/alerts", user, alerts),
		// What a link token or a chat carries is concierge's answer of this moment.
		ApiRoute::new(M::POST, "/alerts/telegram/link", user.fresh(), telegram_link),
		ApiRoute::new(M::POST, "/alerts/telegram/chats", user.fresh(), add_chat),
		ApiRoute::new(M::PUT, "/alerts/destinations/{id}/rules", user, set_destination_rules),
		ApiRoute::new(M::PUT, "/alerts/destinations/{id}/gmail", user, set_destination_gmail),
		ApiRoute::new(M::POST, "/alerts/destinations/{id}/test", user, test_destination),
		ApiRoute::new(M::DELETE, "/alerts/destinations/{id}", user, remove_destination),
	]
}

fn dto(t: Option<&Telegram>, alerts: sa_panel::alerts::Alerts) -> AlertsDto {
	let destination = |d: Destination| DestinationDto {
		id: d.id,
		kind: d.kind,
		title: d.title,
		topic_id: d.topic_id,
		gmail_id: d.gmail_id,
		dead: d.dead,
		rules: alerts
			.rules
			.iter()
			.map(|r| (*r, d.chosen.iter().find(|(c, _)| c == r).map_or(r.on_by_default(), |(_, on)| *on)))
			.collect(),
	};
	AlertsDto {
		enabled: t.is_some(),
		bot: t.and_then(|t| t.name.get()).map(|n| format!("@{n}")),
		rules: alerts.rules.clone(),
		destinations: alerts.destinations.into_iter().map(destination).collect(),
	}
}

/// The caller's alert destinations, each with its rules, and the rules open to them.
async fn alerts(State(panel): State<Panel>, State(t): State<Option<Telegram>>, Extension(caller): Extension<Caller>) -> ApiResult<Json<AlertsDto>> {
	Ok(Json(dto(t.as_ref(), panel.alerts(caller.user_id, &caller.permissions).await?)))
}

fn display(caller: &Caller) -> &str {
	if caller.preferred_name.trim().is_empty() { &caller.email } else { &caller.preferred_name }
}

fn bot(t: Option<Telegram>) -> ApiResult<Telegram> {
	t.ok_or_else(|| Refusal::new(Code::Unavailable, "Telegram is not configured"))
}

/// A link to the bot whose `/start` makes the chat the caller's private destination.
#[derive(JsonSchema, Serialize)]
#[schemars(rename = "TelegramLink")]
pub(crate) struct LinkDto {
	/// `https://t.me/<bot>?start=<token>`, alive 10 min.
	url: String,
}

/// A link that makes the caller's own Telegram chat with the bot their private destination.
///
/// 503 without a bot.
async fn telegram_link(State(panel): State<Panel>, State(t): State<Option<Telegram>>, Extension(caller): Extension<Caller>) -> ApiResult<Created<LinkDto>> {
	let t = bot(t)?;
	let Some(name) = t.name.get() else {
		return Err(Refusal::new(Code::Unavailable, "the bot is starting, try again in a minute"));
	};
	let token = panel.telegram_link_token(caller.user_id, &caller.permissions, display(&caller), Timestamp::now()).await?;
	tracing::info!(user_id = %caller.user_id, "telegram: link token issued");
	Ok(Created(LinkDto {
		url: format!("https://t.me/{name}?start={}", token.as_str()),
	}))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddChat {
	/// `@channel`, `-100<id>`, or `<group>/<topic>` for a forum topic.
	chat: String,
	/// The GMB rules only for this gmail's GMBs.
	gmail_id: Option<i64>,
}

/// Adds a channel, group or forum topic the caller administers as a destination, once a test went
/// out there.
///
/// 400 refused (nothing stored), 409 a destination already.
async fn add_chat(State(t): State<Option<Telegram>>, Extension(caller): Extension<Caller>, Body(b): Body<AddChat>) -> ApiResult<Created<AlertsDto>> {
	let t = bot(t)?;
	t.hub
		.add_chat(caller.user_id, &caller.permissions, display(&caller), &b.chat, b.gmail_id, Timestamp::now())
		.await??;
	let alerts = t.hub.panel.alerts(caller.user_id, &caller.permissions).await?;
	Ok(Created(dto(Some(&t), alerts)))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RulesBody {
	/// `{"new_lead": true, "review_gone": false, …}`: the rules to change.
	rules: BTreeMap<Rule, bool>,
}

/// Turns rules on or off at a destination. 400 a rule not open to the caller.
async fn set_destination_rules(
	State(panel): State<Panel>,
	State(t): State<Option<Telegram>>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id }): Ids<Id<i64>>,
	Body(b): Body<RulesBody>,
) -> ApiResult<Json<AlertsDto>> {
	let rules = b.rules.into_iter().collect::<Vec<_>>();
	panel.alerts_set_rules(caller.user_id, &caller.permissions, id, &rules).await??;
	Ok(Json(dto(t.as_ref(), panel.alerts(caller.user_id, &caller.permissions).await?)))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GmailBody {
	/// `null`: every GMB of the caller's.
	gmail_id: Option<i64>,
}

/// Scopes a destination's GMB rules to one of the caller's gmails. 404 not theirs.
async fn set_destination_gmail(
	State(panel): State<Panel>,
	State(t): State<Option<Telegram>>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id }): Ids<Id<i64>>,
	Body(b): Body<GmailBody>,
) -> ApiResult<Json<AlertsDto>> {
	panel.alerts_set_gmail(caller.user_id, id, b.gmail_id).await??;
	Ok(Json(dto(t.as_ref(), panel.alerts(caller.user_id, &caller.permissions).await?)))
}

/// Sends a test message to a destination; one that arrives revives it. 400 with what Telegram said.
async fn test_destination(State(t): State<Option<Telegram>>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	bot(t)?.hub.test(caller.user_id, id).await??;
	Ok(NoContent)
}

/// Removes a destination.
async fn remove_destination(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	if !panel.alerts_remove(caller.user_id, id).await? {
		return Err(Refused::NotFound.into());
	}
	tracing::info!(user_id = %caller.user_id, destination = id, "alerts: a destination removed");
	Ok(NoContent)
}

// ── the hub's work in `serve` ───────────────────────────────────────────────────────────

/// Follows `service`'s feed: a page every [`FAN_OUT_EVERY`]; while it publishes none, said once
/// and asked again after [`LOOK_AGAIN`].
async fn feed(hub: &Hub<BotApi, Concierge, Forward>, service: Service, mut shutdown: watch::Receiver<bool>) {
	let mut absent = false;
	while !*shutdown.borrow() {
		let wait = match hub.take_feed(service, Timestamp::now()).await {
			Ok(Some(_)) => {
				absent = false;
				FAN_OUT_EVERY
			}
			Ok(None) => {
				if !absent {
					tracing::info!(%service, "alerts: no feed there yet");
					absent = true;
				}
				LOOK_AGAIN
			}
			Err(e) => {
				crate::report(&e, "alerts: a feed");
				FAN_OUT_EVERY
			}
		};
		tokio::select! {
			() = tokio::time::sleep(wait) => {}
			_ = shutdown.changed() => return,
		}
	}
}

impl Directory for Concierge {
	async fn me(&self, access: &str) -> Result<Identity, DirectoryError> {
		use crate::concierge::ConciergeError;
		match Concierge::me(self, access).await {
			Ok(me) => Ok(Identity {
				user_id: me.user_id,
				email: me.email,
				email_verified: me.email_verified,
				preferred_name: me.preferred_name,
				permissions: me.permissions,
			}),
			Err(ConciergeError::Refused(_)) => Err(DirectoryError::Refused),
			Err(ConciergeError::Unavailable(why)) => Err(DirectoryError::Unavailable(why)),
			Err(ConciergeError::Failed(why)) => Err(DirectoryError::Failed(why)),
		}
	}
}

impl Feeds for Forward {
	async fn notes(&self, service: Service, after: i64, limit: usize) -> eyre::Result<Option<NotePage>> {
		let path = format!("{}?after={after}&limit={limit}", feeds::NOTIFICATIONS);
		let (status, body) = self.services().conductor_fetch(service, &path).await?;
		match status {
			StatusCode::NOT_FOUND => Ok(None),
			StatusCode::OK => serde_json::from_slice(&body).map(Some).map_err(|e| eyre::eyre!("{service}'s notes: {e}")),
			other => eyre::bail!("{service} answered its notes with {other}: {}", String::from_utf8_lossy(&body)),
		}
	}

	async fn received(&self) -> eyre::Result<feeds::Received> {
		self.services().conductor_get(feeds::SOURCES_RECEIVED).await
	}

	async fn photo(&self, service: Service, path: &str) -> eyre::Result<Vec<u8>> {
		let (status, body) = self.services().conductor_fetch(service, path).await?;
		eyre::ensure!(status == StatusCode::OK, "{service} answered {path} with {status}");
		Ok(body.into())
	}

	fn callable(&self, service: Service, call: &Call) -> eyre::Result<()> {
		let (method, _) = self.catalog().resolve(&format!("{service}.{}", call.operation), &call.path).map_err(|e| eyre::eyre!(e))?;
		eyre::ensure!(!method.is_safe(), "{} is a read, nothing a press does", call.operation);
		Ok(())
	}

	async fn press(&self, who: &Identity, service: Service, call: &Call, idempotency_key: &str) -> eyre::Result<u16> {
		let (method, path) = self.catalog().resolve(&format!("{service}.{}", call.operation), &call.path).map_err(|e| eyre::eyre!(e))?;
		let mut req = Request::builder().method(method).uri(path).header(
			HeaderName::from_static("idempotency-key"),
			HeaderValue::from_str(idempotency_key).expect("tg/<outbox>/<button id> is visible ASCII"),
		);
		let body = match &call.body {
			Value::Null => RawBody::empty(),
			body => {
				req = req.header(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
				RawBody::from(serde_json::to_vec(body).expect("a JSON value serializes"))
			}
		};
		let user = |service: Service| User {
			sub: who.user_id.to_string(),
			email: who.email.clone(),
			email_verified: who.email_verified,
			name: who.preferred_name.clone(),
			permissions: who.permissions.iter().filter(|p| p.starts_with(service.prefix())).collect(),
		};
		let sent = self.as_user(req.body(body).expect("a request from parts that parsed"), user);
		let res = tokio::time::timeout(SITES_TIMEOUT, sent)
			.await
			.map_err(|_| eyre::eyre!("{service} did not answer a press in time"))?;
		Ok(res.status().as_u16())
	}
}
