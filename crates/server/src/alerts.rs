//! Alerts in `serve`: the services' feeds and concierge as the hub reads them, the hub's work, and
//! the `/api/v1/alerts` routes — every signed-in user's own inbox, destinations, tiers and hours.
//! What is told to whom is the engine's (`sa_panel::alerts`), how it goes out [`crate::outlets`]'.

use std::{collections::BTreeMap, time::Duration};

use axum::{
	Extension, Json, Router,
	body::Body as RawBody,
	extract::{FromRef, Query, State},
	http::{HeaderName, HeaderValue, Method, Request, StatusCode, header},
	response::{IntoResponse, NoContent, Response},
	routing::post,
};
use ev_lib::api::{Body, Created, Ids, JsonSchema, Params, Refusal};
use futures::{StreamExt, stream::FuturesUnordered};
use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Call, Family, NotePage, Rule, Tier};
use sa_auth::{Actor, Own, Service, User};
use sa_core::{
	agent::Outward,
	alerts::{Cadence, Cadences, Filter, Method as Outlet},
	feeds,
	notify::{MAX_NAME, one_line},
};
use sa_panel::{
	Panel, Refused,
	alerts::{Address, Alert, Destination, Directory, DirectoryError, Feeds, Hours, Hub, Identity, Pressed, Settings, Sought, Told},
	deliver::Delivered,
	telegram::Kind,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use social_networks_adapters::notify::{discord, slack, webpush};
use tokio::sync::watch;

use crate::{
	api::{ApiResult, Code, Id},
	brands::Png,
	concierge::Concierge,
	forward::{Forward, SITES_TIMEOUT},
	http::{ApiRoute, ApiState, FRESH, Limits, USER},
	kit::Picture,
	outlets::Networks,
	signin::Caller,
	telegram::{self, BotName},
};

/// How often each part of the work runs.
const DELIVER_EVERY: Duration = Duration::from_millis(500);

const FAN_OUT_EVERY: Duration = Duration::from_secs(2);
const RECHECK_EVERY: Duration = Duration::from_secs(60);
/// A feed failing this long is stalled: the admins are told.
const STALLED_AFTER: SignedDuration = SignedDuration::from_mins(10);
/// A service that publishes no feed yet (404) is asked again this often.
pub(crate) const LOOK_AGAIN: Duration = Duration::from_secs(60);

/// The hub, as `serve` runs it with the forward.
#[derive(Clone)]
pub struct Alerting {
	pub hub: Hub<Networks, Concierge, Forward>,
	pub bot: BotName,
}

impl FromRef<ApiState> for Option<Alerting> {
	fn from_ref(s: &ApiState) -> Self {
		s.alerting.clone()
	}
}

// ── what the routes answer ──────────────────────────────────────────────────────────────

/// How the caller's alerts are set, and what every change answers.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Alerts")]
pub struct AlertsDto {
	/// The methods a destination can be added by on this server.
	pub methods: Vec<Outlet>,
	/// The bot's `@username`, once known: what a channel adds as an admin.
	pub bot: Option<String>,
	/// The caller's own address, when it is verified.
	pub email: Option<String>,
	/// What a browser's push subscription is to (its `applicationServerKey`).
	pub push_key: Option<String>,
	/// The rules open to the caller: their permissions', and the GMB rules once they hold a GMB.
	pub rules: Vec<RuleDto>,
	pub destinations: Vec<DestinationDto>,
	/// `null` until set: the daily message then goes out at 9:00 UTC.
	pub hours: Option<Hours>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AlertRule")]
pub struct RuleDto {
	pub rule: Rule,
	pub family: Family,
	/// Its notes' tier, where neither the note nor the caller says another.
	pub default: Tier,
	/// The caller's own.
	pub tier: Option<Tier>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Destination")]
pub struct DestinationDto {
	pub id: i64,
	pub method: Outlet,
	/// Read by the caller alone: the only ones contact details and buttons go to.
	pub private: bool,
	/// A Telegram one's.
	pub kind: Option<Kind>,
	pub topic_id: Option<i64>,
	/// What the caller knows it by.
	pub title: Option<String>,
	pub cadences: Cadences,
	/// The rules at a cadence of their own there.
	pub rules: BTreeMap<Rule, Cadence>,
	pub filter: Filter,
	/// Why nothing reaches it: a test that arrives clears it.
	pub dead: Option<String>,
}

/// A page of the caller's inbox, newest first.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AlertInbox")]
pub struct InboxDto {
	pub alerts: Vec<AlertDto>,
	/// How many are unread, whatever was asked for.
	pub unread: UnreadDto,
}

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AlertsUnread")]
pub struct UnreadDto {
	pub urgent: i64,
	pub important: i64,
	pub info: i64,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Alert")]
pub struct AlertDto {
	pub id: i64,
	pub rule: Rule,
	pub family: Family,
	pub tier: Tier,
	pub at: Timestamp,
	/// Its first line says it, short.
	pub text: String,
	/// What opens it in the panel: a path on this origin.
	pub link: Option<String>,
	/// Elsewhere.
	pub links: Vec<LinkDto>,
	/// The GBPs it is about.
	pub gmbs: Vec<i64>,
	/// `/alerts/inbox/{id}/photo` answers one.
	pub photo: bool,
	/// Those still to press.
	pub buttons: Vec<ButtonDto>,
	pub read_at: Option<Timestamp>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AlertLink")]
pub struct LinkDto {
	pub label: String,
	pub url: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AlertButton")]
pub struct ButtonDto {
	pub id: String,
	pub label: String,
}

/// One note, and where it went.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AlertOpened")]
pub struct OpenedDto {
	pub alert: AlertDto,
	pub told: Vec<Told>,
}

impl From<Alert> for AlertDto {
	fn from(a: Alert) -> Self {
		Self {
			id: a.id,
			rule: a.rule,
			family: a.rule.family(),
			tier: a.tier,
			at: a.at,
			text: a.text.text,
			link: a.link,
			links: a.links.into_iter().map(|(label, url)| LinkDto { label, url }).collect(),
			gmbs: a.gmbs,
			photo: a.photo.is_some(),
			buttons: a.buttons.into_iter().map(|b| ButtonDto { id: b.id, label: b.label }).collect(),
			read_at: a.read_at,
		}
	}
}

impl From<Destination> for DestinationDto {
	fn from(d: Destination) -> Self {
		Self {
			id: d.id,
			method: d.method,
			private: d.private,
			kind: d.chat.map(|c| c.kind),
			topic_id: d.chat.and_then(|c| c.topic_id),
			title: d.title,
			cadences: d.cadences,
			rules: d.rules.into_iter().collect(),
			filter: d.filter,
			dead: d.dead,
		}
	}
}

/// Runs the hub until `shutdown` turns true: the bot's poller (on the replica holding its lease),
/// the conductor's own rules, each service's feed, the delivery and the access checks, each on its
/// own schedule. A failing pass is reported and tried again at the next tick; nothing here ends
/// `serve`.
pub async fn run(a: Alerting, shutdown: watch::Receiver<bool>) {
	let hub = &a.hub;
	let bot = async {
		if let Some(api) = &hub.outlets.telegram {
			telegram::poll(hub, api, &a.bot, shutdown.clone()).await;
		}
	};
	tokio::join!(
		bot,
		crate::every(FAN_OUT_EVERY, shutdown.clone(), "alerts: the conductor's own", || async {
			hub.fan_out_own(Timestamp::now()).await.map(drop)
		}),
		feed(hub, Service::Sites, shutdown.clone()),
		feed(hub, Service::ReviewArchive, shutdown.clone()),
		deliver(hub, shutdown.clone()),
		crate::every(RECHECK_EVERY, shutdown.clone(), "alerts: access check", || async {
			hub.recheck_access(Timestamp::now()).await.map(drop)
		}),
	);
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/alerts", USER, alerts),
		ApiRoute::new(M::GET, "/alerts/inbox", USER, alert_inbox),
		ApiRoute::new(M::POST, "/alerts/inbox/read", USER, read_alerts),
		ApiRoute::new(M::GET, "/alerts/inbox/{id}", USER, alert),
		ApiRoute::new(M::GET, "/alerts/inbox/{id}/photo", USER, alert_photo),
		// as a press in Telegram asks concierge anew
		ApiRoute::new(M::POST, "/alerts/inbox/{id}/press", FRESH, press_alert),
		ApiRoute::new(M::PUT, "/alerts/rules/{rule}", USER, set_alert_tier),
		ApiRoute::new(M::PUT, "/alerts/hours", USER, set_alert_hours),
		// What a link token or a chat carries is concierge's answer of this moment.
		ApiRoute::new(M::POST, "/alerts/telegram/link", FRESH.needs(Own::Credentials), telegram_link),
		ApiRoute::new(M::POST, "/alerts/telegram/chats", FRESH.needs(Own::Credentials), add_chat),
		ApiRoute::new(M::POST, "/alerts/email", FRESH.needs(Own::Credentials), add_email),
		ApiRoute::new(M::POST, "/alerts/email/confirm", FRESH.needs(Own::Credentials), confirm_email),
		ApiRoute::new(M::POST, "/alerts/discord", FRESH.needs(Own::Credentials), add_discord),
		ApiRoute::new(M::POST, "/alerts/slack", FRESH.needs(Own::Credentials), add_slack),
		ApiRoute::new(M::POST, "/alerts/push", FRESH.needs(Own::Credentials), add_push),
		ApiRoute::new(M::POST, "/alerts/webhooks", FRESH.needs(Own::Credentials), add_webhook),
		ApiRoute::new(M::PUT, "/alerts/destinations/{id}", USER, set_destination),
		ApiRoute::new(M::POST, "/alerts/destinations/{id}/test", USER, test_destination),
		ApiRoute::new(M::DELETE, "/alerts/destinations/{id}", USER, remove_destination),
	]
}

/// What mail's one-click unsubscribe posts to: open, its token its only say (RFC 8058).
pub(crate) fn open_routes(panel: Panel, limits: &Limits) -> Router {
	let routes = Router::new().route("/api/alerts/unsubscribe", post(unsubscribe).get(unsubscribe_page));
	crate::http::bounded(routes, limits.auth_concurrent, limits.auth_timeout).with_state(panel)
}

async fn answer(panel: &Panel, a: Option<&Alerting>, caller: &Caller) -> ApiResult<AlertsDto> {
	let alerts = panel.alerts(caller.user_id, &caller.permissions).await?;
	Ok(AlertsDto {
		methods: a.map(|a| a.hub.outlets.methods()).unwrap_or_default(), // no forward: no hub, nothing goes out
		bot: a.and_then(|a| a.bot.get()).map(|n| format!("@{n}")),
		email: caller.email_verified.then(|| caller.email.clone()),
		push_key: a.and_then(|a| a.hub.outlets.push_key.clone()),
		rules: alerts
			.rules
			.into_iter()
			.map(|(rule, tier)| RuleDto {
				rule,
				family: rule.family(),
				default: rule.tier(),
				tier,
			})
			.collect(),
		destinations: alerts.destinations.into_iter().map(DestinationDto::from).collect(),
		hours: alerts.hours,
	})
}

/// How the caller's alerts are set: their destinations, each rule's tier, their hours.
async fn alerts(State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>) -> ApiResult<Json<AlertsDto>> {
	Ok(Json(answer(&panel, a.as_ref(), &caller).await?))
}

fn display(caller: &Caller) -> &str {
	if caller.preferred_name.trim().is_empty() { &caller.email } else { &caller.preferred_name }
}

fn hub(a: Option<Alerting>) -> ApiResult<Alerting> {
	a.ok_or_else(|| Refusal::new(Code::Unavailable, "alerts are not configured"))
}

// ── the inbox ───────────────────────────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InboxQuery {
	tier: Option<Tier>,
	#[serde(default)]
	unread: bool,
	/// About this GBP.
	gmb: Option<i64>,
	/// Older than this one.
	before: Option<i64>,
}

/// The caller's alerts, newest first, 50 at a time.
async fn alert_inbox(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Params(q): Params<InboxQuery>) -> ApiResult<Json<InboxDto>> {
	let sought = Sought {
		id: None,
		tier: q.tier,
		unread: q.unread,
		gmb: q.gmb,
		before: q.before,
	};
	let alerts = panel.alerts_inbox(caller.user_id, &caller.permissions, &sought).await?;
	let unread = panel.alerts_unread(caller.user_id, &caller.permissions).await?;
	Ok(Json(InboxDto {
		alerts: alerts.into_iter().map(AlertDto::from).collect(),
		unread: UnreadDto {
			urgent: unread[&Tier::Urgent],
			important: unread[&Tier::Important],
			info: unread[&Tier::Info],
		},
	}))
}

async fn one(panel: &Panel, caller: &Caller, id: i64) -> ApiResult<Alert> {
	let sought = Sought {
		id: Some(id),
		..Sought::default()
	};
	let found = panel.alerts_inbox(caller.user_id, &caller.permissions, &sought).await?;
	Ok(found.into_iter().next().ok_or(Refused::NotFound)?)
}

/// One of the caller's alerts, and where it went.
async fn alert(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<OpenedDto>> {
	let alert = one(&panel, &caller, id).await?;
	let told = panel.alerts_told(caller.user_id, id).await?;
	Ok(Json(OpenedDto { alert: alert.into(), told }))
}

/// An alert's photo.
async fn alert_photo(State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Picture<Png>> {
	let alert = one(&panel, &caller, id).await?;
	let (Some(path), Some(service)) = (alert.photo, alert.rule.feed()) else {
		return Err(Refused::NotFound.into());
	};
	let bytes = hub(a)?.hub.feeds.photo(service, &path).await?;
	Ok(Picture(bytes, Png))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AlertsRead")]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadBody {
	/// `null`: all of them.
	ids: Option<Vec<i64>>,
	/// Nothing newer than this one: what came since was not on the caller's screen.
	up_to: i64,
}

/// Marks alerts read: what was still waiting to go out of them is given up.
async fn read_alerts(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<ReadBody>) -> ApiResult<NoContent> {
	panel.alerts_read(caller.user_id, b.ids.as_deref(), b.up_to, Timestamp::now()).await?;
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AlertPress")]
#[serde(deny_unknown_fields)]
pub(crate) struct PressBody {
	button: String,
}

/// Presses an alert's button, as under its message: the buttons left.
///
/// 409 recorded before, 404 what it acts on is gone.
async fn press_alert(
	State(panel): State<Panel>,
	State(a): State<Option<Alerting>>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id }): Ids<Id<i64>>,
	Body(b): Body<PressBody>,
) -> ApiResult<Json<AlertDto>> {
	one(&panel, &caller, id).await?;
	let who = Identity {
		user_id: caller.user_id,
		email: caller.email.clone(),
		email_verified: caller.email_verified,
		preferred_name: caller.preferred_name.clone(),
		permissions: caller.permissions.clone(),
	};
	match hub(a)?.hub.press(&who, caller.actor.as_ref(), id, &b.button, Timestamp::now()).await? {
		Pressed::Done { .. } => Ok(Json(one(&panel, &caller, id).await?.into())),
		Pressed::AlreadyDone => Err(Refused::Conflict("already recorded".into()).into()),
		Pressed::Gone | Pressed::Unknown => Err(Refused::NotFound.into()),
		Pressed::NoAccess => Err(Refusal::new(Code::Forbidden, "no access to what it does")),
		Pressed::TryLater => Err(Refusal::new(Code::Unavailable, "not now, try again")),
	}
}

// ── what the caller sets ────────────────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub(crate) struct RulePath {
	rule: Rule,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AlertTier")]
#[serde(deny_unknown_fields)]
pub(crate) struct TierBody {
	/// `null`: the notes' own.
	tier: Option<Tier>,
}

/// Sets how much a rule's notes matter to the caller. 400 a rule not open to them.
async fn set_alert_tier(
	State(panel): State<Panel>,
	State(a): State<Option<Alerting>>,
	Extension(caller): Extension<Caller>,
	Ids(RulePath { rule }): Ids<RulePath>,
	Body(b): Body<TierBody>,
) -> ApiResult<Json<AlertsDto>> {
	panel.alerts_set_tier(caller.user_id, &caller.permissions, rule, b.tier).await??;
	Ok(Json(answer(&panel, a.as_ref(), &caller).await?))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "SetAlertHours")]
#[serde(deny_unknown_fields)]
pub(crate) struct HoursBody {
	/// An IANA time zone.
	tz: String,
	/// When the daily message goes out, in minutes since midnight there.
	daily: u16,
	/// From and to, the same; past midnight when they end before they begin. Nothing but urgent goes
	/// out in them, the rest as one message after.
	#[schemars(with = "Option<[u16; 2]>")] // a tuple's schema is one ajv refuses
	quiet: Option<(u16, u16)>,
	/// Nothing but urgent goes out before this.
	snoozed_until: Option<Timestamp>,
}

/// Sets the caller's hours.
async fn set_alert_hours(State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<HoursBody>) -> ApiResult<Json<AlertsDto>> {
	let hours = Hours {
		tz: b.tz,
		daily: b.daily,
		quiet: b.quiet,
		snoozed_until: b.snoozed_until,
	};
	panel.alerts_set_hours(caller.user_id, &hours, Timestamp::now()).await??;
	Ok(Json(answer(&panel, a.as_ref(), &caller).await?))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "SetDestination")]
#[serde(deny_unknown_fields)]
pub(crate) struct DestinationBody {
	/// `null`: as it is.
	title: Option<String>,
	cadences: Cadences,
	/// The rules at a cadence of their own there.
	rules: BTreeMap<Rule, Cadence>,
	filter: Filter,
}

/// Sets what a destination is told, when, and of what. 400 a rule not open to the caller.
async fn set_destination(
	State(panel): State<Panel>,
	State(a): State<Option<Alerting>>,
	Extension(caller): Extension<Caller>,
	Ids(Id { id }): Ids<Id<i64>>,
	Body(b): Body<DestinationBody>,
) -> ApiResult<Json<AlertsDto>> {
	let settings = Settings {
		title: b.title,
		cadences: b.cadences,
		filter: b.filter,
		rules: b.rules,
	};
	panel.alerts_set(caller.user_id, &caller.permissions, id, &settings, Timestamp::now()).await??;
	Ok(Json(answer(&panel, a.as_ref(), &caller).await?))
}

/// Sends a test message to a destination; one that arrives revives it. 400 with what its network said.
async fn test_destination(State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	hub(a)?.hub.test(caller.user_id, id).await??;
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

// ── destinations added ──────────────────────────────────────────────────────────────────

/// A link to the bot whose `/start` makes the chat the caller's private destination.
#[derive(JsonSchema, Serialize)]
#[schemars(rename = "TelegramLink")]
pub(crate) struct TelegramLinkDto {
	/// `https://t.me/<bot>?start=<token>`, alive 10 min.
	url: String,
}

/// A link that makes the caller's own Telegram chat with the bot their private destination.
///
/// 503 without a bot.
async fn telegram_link(State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>) -> ApiResult<Created<TelegramLinkDto>> {
	let a = hub(a)?;
	if a.hub.outlets.telegram.is_none() {
		return Err(Refusal::new(Code::Unavailable, "Telegram is not configured"));
	}
	let Some(name) = a.bot.get() else {
		return Err(Refusal::new(Code::Unavailable, "the bot is starting, try again in a minute"));
	};
	let token = panel.telegram_link_token(caller.user_id, &caller.permissions, display(&caller), Timestamp::now()).await?;
	tracing::info!(user_id = %caller.user_id, "telegram: link token issued");
	Ok(Created(TelegramLinkDto {
		url: format!("https://t.me/{name}?start={}", token.as_str()),
	}))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddChat {
	/// `@channel`, `-100<id>`, or `<group>/<topic>` for a forum topic.
	chat: String,
}

/// Adds a channel, group or forum topic the caller administers as a destination, once a test went
/// out there.
///
/// 400 refused (nothing stored), 409 a destination already.
async fn add_chat(_: Outward, State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<AddChat>) -> ApiResult<Created<AlertsDto>> {
	let a = hub(a)?;
	let named = b.chat.trim().parse().map_err(|e| Refused::Invalid(format!("not a channel, group or topic: {e}")))?;
	a.hub.add_chat(caller.user_id, &caller.permissions, display(&caller), named, Timestamp::now()).await??;
	Ok(Created(answer(&panel, Some(&a), &caller).await?))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AddEmail")]
#[serde(deny_unknown_fields)]
pub(crate) struct AddEmail {
	address: String,
}

/// Adds the caller's own verified address as a destination; any other is mailed a link that does.
async fn add_email(_: Outward, State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<AddEmail>) -> ApiResult<Created<AlertsDto>> {
	let a = hub(a)?;
	let address = b.address.trim().to_lowercase();
	if caller.email_verified && address == caller.email.to_lowercase() {
		let to = Address::Email { to: address.clone() };
		a.hub.add(caller.user_id, &caller.permissions, display(&caller), &to, &address, Timestamp::now()).await??;
	} else {
		a.hub.prove_email(caller.user_id, &address, "/user/alerts/settings", Timestamp::now()).await??;
	}
	Ok(Created(answer(&panel, Some(&a), &caller).await?))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "ConfirmEmail")]
#[serde(deny_unknown_fields)]
pub(crate) struct ConfirmEmail {
	/// The mailed link's.
	token: String,
}

/// Adds the address the mailed token proves. 400 another's token, or one out of date.
async fn confirm_email(State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<ConfirmEmail>) -> ApiResult<Created<AlertsDto>> {
	let a = hub(a)?;
	let now = Timestamp::now();
	let address = panel
		.alerts_email_proved(caller.user_id, &b.token, now)
		.ok_or_else(|| Refused::Invalid("this link is another account's, or out of date".into()))?;
	let to = Address::Email { to: address.clone() };
	a.hub.add(caller.user_id, &caller.permissions, display(&caller), &to, &address, now).await??;
	Ok(Created(answer(&panel, Some(&a), &caller).await?))
}

/// Adds `to` as a destination of the caller's, once a test went out there.
async fn add(a: Option<Alerting>, caller: &Caller, to: &Address, title: &str) -> ApiResult<AlertsDto> {
	let a = hub(a)?;
	a.hub.add(caller.user_id, &caller.permissions, display(caller), to, title, Timestamp::now()).await??;
	answer(&a.hub.panel, Some(&a), caller).await
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AddDiscord")]
#[serde(deny_unknown_fields)]
pub(crate) struct AddDiscord {
	/// The channel's webhook: Edit channel → Integrations → Webhooks.
	url: String,
	/// Urgent ones say `@here`.
	ping: bool,
}

/// Adds a Discord channel by its webhook, once a test went out there.
///
/// 400 refused (nothing stored), 409 a destination already.
async fn add_discord(_: Outward, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<AddDiscord>) -> ApiResult<Created<AlertsDto>> {
	let webhook: discord::Webhook = b.url.parse().map_err(invalid)?;
	let Some(channels) = hub(a.clone())?.hub.outlets.discord else {
		return Err(Refusal::new(Code::Unavailable, "Discord is not configured"));
	};
	let title = channels.look(&webhook).await.map_err(invalid)?.name.unwrap_or_else(|| "Discord channel".to_owned()); // Discord names none
	let to = Address::Discord {
		url: webhook.to_string(),
		ping: b.ping,
	};
	Ok(Created(add(a, &caller, &to, &title).await?))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AddSlack")]
#[serde(deny_unknown_fields)]
pub(crate) struct AddSlack {
	/// The channel's incoming webhook, `https://hooks.slack.com/services/…`.
	url: String,
}

/// Adds a Slack channel by its incoming webhook, once a test went out there.
///
/// 400 refused (nothing stored), 409 a destination already.
async fn add_slack(_: Outward, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<AddSlack>) -> ApiResult<Created<AlertsDto>> {
	let webhook: slack::Webhook = b.url.parse().map_err(invalid)?;
	let to = Address::Slack { url: webhook.to_string() };
	Ok(Created(add(a, &caller, &to, "Slack channel").await?))
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AddPush")]
#[serde(deny_unknown_fields)]
pub(crate) struct AddPush {
	/// As the browser's `PushSubscription.toJSON()` gives them.
	endpoint: String,
	p256dh: String,
	auth: String,
	/// The browser and what it runs on.
	title: String,
}

/// Adds the browser's push subscription, once a test went out there; one the caller has already is
/// left as they set it.
async fn add_push(_: Outward, State(panel): State<Panel>, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<AddPush>) -> ApiResult<Created<AlertsDto>> {
	webpush::Subscription::try_new(&b.endpoint, &b.p256dh, &b.auth).map_err(invalid)?;
	let to = Address::Push {
		endpoint: b.endpoint,
		p256dh: b.p256dh,
		auth: b.auth,
	};
	let a = hub(a)?;
	match a.hub.add(caller.user_id, &caller.permissions, display(&caller), &to, one_line(&b.title, MAX_NAME).as_str(), Timestamp::now()).await? {
		Ok(_) | Err(Refused::Conflict(_)) => Ok(Created(answer(&panel, Some(&a), &caller).await?)),
		Err(refused) => Err(refused.into()),
	}
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "AddWebhook")]
#[serde(deny_unknown_fields)]
pub(crate) struct AddWebhook {
	/// An https URL on the internet: each alert is POSTed there as JSON.
	url: String,
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "AddedWebhook")]
pub(crate) struct WebhookDto {
	/// What each delivery is signed with, shown this once:
	/// `x-signature: t=<unix>,v1=<hex HMAC-SHA256 of "<t>.<body>">`.
	secret: String,
	alerts: AlertsDto,
}

/// Adds a webhook, once a test went out there.
///
/// 400 refused (nothing stored), 409 a destination already.
async fn add_webhook(_: Outward, State(a): State<Option<Alerting>>, Extension(caller): Extension<Caller>, Body(b): Body<AddWebhook>) -> ApiResult<Created<WebhookDto>> {
	let url: url::Url = b.url.trim().parse().map_err(|e| Refused::Invalid(format!("not a URL: {e}")))?;
	let mut raw = [0; 32];
	getrandom::fill(&mut raw).map_err(|e| eyre::eyre!("the OS random source failed: {e}"))?;
	let secret = format!("whsec_{}", hex::encode(raw));
	let title = url.host_str().ok_or_else(|| Refused::Invalid("no host".into()))?.to_owned();
	let to = Address::Webhook {
		url: url.into(),
		secret: secret.clone(),
	};
	let alerts = add(a, &caller, &to, &title).await?;
	Ok(Created(WebhookDto { secret, alerts }))
}

fn invalid(f: social_networks_adapters::notify::Failure) -> Refused {
	Refused::Invalid(f.to_string())
}

#[derive(Deserialize)]
struct Unsubscribe {
	token: String,
}

async fn unsubscribe(State(panel): State<Panel>, Query(q): Query<Unsubscribe>) -> Response {
	match panel.alerts_unsubscribe(&q.token).await {
		Ok(true) => "Unsubscribed.".into_response(),
		Ok(false) => (StatusCode::BAD_REQUEST, "Not a link of ours.").into_response(),
		Err(e) => {
			crate::report(&e, "alerts: an unsubscription");
			StatusCode::INTERNAL_SERVER_ERROR.into_response()
		}
	}
}

/// A mail scanner opens every link, so opening one changes nothing: the page's button posts.
async fn unsubscribe_page() -> Response {
	const PAGE: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width\"><title>Unsubscribe</title>\
		<form method=post><p>Stop these alerts to this address?<p><button>Unsubscribe</button></form>";
	([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CONTENT_SECURITY_POLICY, "default-src 'none'; form-action 'self'; frame-ancestors 'none'")], PAGE).into_response()
}

// ── the hub's work in `serve` ───────────────────────────────────────────────────────────

/// A pass begins every [`DELIVER_EVERY`], whatever the ones before still wait for: a network that hangs
/// delays its own messages alone.
async fn deliver(hub: &Hub<Networks, Concierge, Forward>, mut shutdown: watch::Receiver<bool>) {
	let mut passes = FuturesUnordered::new();
	let mut tick = tokio::time::interval(DELIVER_EVERY);
	tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
	let done = |pass: eyre::Result<Delivered>| {
		if let Err(e) = pass {
			crate::report(&e, "alerts: delivery");
		}
	};
	while !*shutdown.borrow() {
		tokio::select! {
			_ = tick.tick() => passes.push(hub.deliver(Timestamp::now())),
			Some(pass) = passes.next() => done(pass),
			_ = shutdown.changed() => {}
		}
	}
	// a send cut short would go out twice
	while let Some(pass) = passes.next().await {
		done(pass);
	}
}

/// Follows `service`'s feed: a page every [`FAN_OUT_EVERY`]; while it publishes none, said once
/// and asked again after [`LOOK_AGAIN`].
async fn feed(hub: &Hub<Networks, Concierge, Forward>, service: Service, mut shutdown: watch::Receiver<bool>) {
	let mut absent = false;
	let mut failing = None;
	while !*shutdown.borrow() {
		let taken = hub.take_feed(service, Timestamp::now()).await;
		if taken.is_ok() {
			failing = None;
		}
		let wait = match taken {
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
				let now = Timestamp::now();
				if now.duration_since(*failing.get_or_insert(now)) >= STALLED_AFTER
					&& let Err(e) = hub.panel.alerts_feed_stalled(service, &format!("{e:#}"), now).await
				{
					crate::report(&e, "alerts: telling of a feed stalled");
				}
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

	async fn press(&self, who: &Identity, actor: Option<&Actor>, service: Service, call: &Call, idempotency_key: &str) -> eyre::Result<u16> {
		let (method, path) = self.catalog().resolve(&format!("{service}.{}", call.operation), &call.path).map_err(|e| eyre::eyre!(e))?;
		let mut req = Request::builder().method(method).uri(path).header(
			HeaderName::from_static("idempotency-key"),
			HeaderValue::from_str(idempotency_key).expect("alert/<note>/<user>/<button id> is visible ASCII"),
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
			actor: actor.cloned(),
		};
		let sent = self.as_user(req.body(body).expect("a request from parts that parsed"), user);
		let res = tokio::time::timeout(SITES_TIMEOUT, sent)
			.await
			.map_err(|_| eyre::eyre!("{service} did not answer a press in time"))?;
		Ok(res.status().as_u16())
	}
}
