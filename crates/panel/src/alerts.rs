//! Alerts: the rules' notes — the services' (read from their feeds) and the conductor's own — each
//! put in the inboxes of the people it is for, and queued for the destinations of theirs it is for;
//! and what a person sets of it: their destinations, each rule's tier, their hours. What is queued
//! goes out in [`crate::deliver`], through [`Outlets`].
//!
//! ```text
//! sites, review_archive   GET /api/conductor/notifications?after=<cursor> → a page: each note
//!                         checked against its feed and told, the cursor moved — one
//!                         transaction, taken only from where the cursor stands
//! the conductor's own     told in the write that made it so ([`Panel::tell`]), or found due
//!
//! a note ─ kept once (rule, event) ─▶ people: the rule's audience, open to their permissions
//!            a person ─▶ the inbox, at the tier: theirs for the rule, else the note's, else the rule's
//!            each destination of theirs reached ─ filter ─ cadence ─ hours ─▶ outbox
//!              (their access confirmed within ACCESS_TTL)
//! ```
//!
//! **Who has access.** The conductor learns a user's permissions only from concierge's `GetMe`,
//! which takes the user's own access token. The hub keeps what concierge last confirmed and when:
//! the `/api/v1` gate records every answer it gets, and [`Hub::recheck_access`] asks again through
//! the user's newest session for a user with a destination reached, not confirmed in
//! [`RECHECK_AFTER`] — through a session the user used within [`SESSION_IDLE`], so the hub never
//! keeps an abandoned one alive. A message is queued, and again sent, only to a user confirmed
//! within [`ACCESS_TTL`]. So: a grant revoked at concierge, or a session concierge refuses to
//! rotate, stops messages at the next check, and what was queued for the user is dropped; a user
//! with no session used within [`SESSION_IDLE`], or concierge unreachable, stops them after
//! [`ACCESS_TTL`] — PII does not flow on a stale answer.

use std::{collections::BTreeMap, future::Future};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Audience, Button, Call, NotePage, Rule, Tier};
use sa_auth::{PermissionSet, Pii, Service};
use sa_core::{
	alerts::{Cadence, Cadences, Filter, Method, Subject},
	notify::{self, Failure, Locale, Note, Rendered},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use strum::VariantArray;
use url::Url;
use uuid::Uuid;

pub use crate::store::{
	alerts::{Destination, Hours, Sought},
	outbox::{State, Told},
};
use crate::{
	Panel, Refused,
	seal::{EMAIL_TOKEN_AAD, UNSUBSCRIBE_AAD, alert_address_aad, alert_note_aad},
	session::{Refresher, SessionError},
	store::{
		alerts::{self as db, Sealed},
		outbox,
	},
	telegram::{Bot, Target},
};

/// Nothing is sent to a user whose access concierge has not confirmed for this long.
pub const ACCESS_TTL: SignedDuration = SignedDuration::from_hours(1);

/// A user not confirmed for this long is asked about again.
pub const RECHECK_AFTER: SignedDuration = SignedDuration::from_mins(15);

/// The hub asks concierge only with a session its user used within this long.
pub const SESSION_IDLE: SignedDuration = SignedDuration::from_hours(24 * 7);

/// Notes asked of a service at once.
pub const NOTES_PAGE: usize = 500;

/// How long the link mailed to an address to prove it stays good.
pub const EMAIL_TOKEN_TTL: SignedDuration = SignedDuration::from_hours(24);

/// A person's destinations, at most.
const DESTINATIONS: usize = 50;

/// A note, and what it fanned out to, is kept this long.
const KEEP: SignedDuration = SignedDuration::from_hours(24 * 30);

/// Access requests taken per pass.
const BATCH: i64 = 50;

/// Users re-confirmed per pass.
const RECHECKS: i64 = 20;

/// Notes of an inbox read at once.
const INBOX_PAGE: i64 = 50;

/// concierge's `GetMe`: the port the hub confirms a user's access through.
pub trait Directory: Sync {
	fn me(&self, access: &str) -> impl Future<Output = Result<Identity, DirectoryError>> + Send;
}

/// The services, as the hub needs them; each call under the conductor's assertion.
pub trait Feeds: Sync {
	/// Up to `limit` of `service`'s notes after `after`; `None` while it publishes none (404).
	fn notes(&self, service: Service, after: i64, limit: usize) -> impl Future<Output = eyre::Result<Option<NotePage>>> + Send;
	/// When each source key's last event arrived at sites.
	fn received(&self) -> impl Future<Output = eyre::Result<BTreeMap<String, Timestamp>>> + Send;
	/// Whether `call` is a write of `service`'s that a press can make: refused, the note fails its page.
	fn callable(&self, service: Service, call: &Call) -> eyre::Result<()>;
	/// `call` made to `service` as `who`, under `idempotency_key`: the status it answered.
	fn press(&self, who: &Identity, service: Service, call: &Call, idempotency_key: &str) -> impl Future<Output = eyre::Result<u16>> + Send;
	/// The photo at `path` of `service`, as a note named it.
	fn photo(&self, service: Service, path: &str) -> impl Future<Output = eyre::Result<Vec<u8>>> + Send;
}

/// The networks a message goes out through; Telegram's own ways beside.
pub trait Outlets: Bot {
	/// Sends `message` to `to`: what its network calls it there. A method without its
	/// configuration refuses.
	fn send_to(&self, to: &Address, message: &Message) -> impl Future<Output = Result<String, Failure>> + Send;
}

/// Where a destination is reached. Serialized, what is sealed of it; Telegram's is its chat.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum Address {
	#[serde(skip)]
	Telegram(Target),
	Email {
		to: String,
	},
	Discord {
		user: String,
	},
	DiscordChannel {
		url: String,
		/// Urgent ones say `@here`.
		ping: bool,
	},
	Slack {
		url: String,
	},
	Push {
		endpoint: String,
		p256dh: String,
		auth: String,
	},
	Webhook {
		url: String,
		/// What a delivery is signed with.
		secret: String,
	},
}

impl Address {
	pub fn method(&self) -> Method {
		match self {
			Self::Telegram(_) => Method::Telegram,
			Self::Email { .. } => Method::Email,
			Self::Discord { .. } => Method::Discord,
			Self::DiscordChannel { .. } => Method::DiscordChannel,
			Self::Slack { .. } => Method::Slack,
			Self::Push { .. } => Method::Push,
			Self::Webhook { .. } => Method::Webhook,
		}
	}

	/// Read by its person alone.
	fn private(&self) -> bool {
		matches!(self, Self::Email { .. } | Self::Discord { .. } | Self::Push { .. })
	}

	/// What tells it from a person's others of its method.
	fn identity(&self) -> &str {
		match self {
			Self::Telegram(_) => unreachable!("a chat is its own"),
			Self::Email { to } => to,
			Self::Discord { user } => user,
			Self::DiscordChannel { url, .. } | Self::Slack { url } | Self::Webhook { url, .. } => url,
			Self::Push { endpoint, .. } => endpoint,
		}
	}
}

/// One message, as a network renders it.
#[derive(Clone, Debug)]
pub struct Message {
	pub tier: Tier,
	/// Its rule; `None` for several notes' and a test's.
	pub rule: Option<Rule>,
	/// Its first line is what it says, short.
	pub text: Rendered,
	/// `(label, url)`.
	pub links: Vec<(String, String)>,
	/// `(label, signed data)`: Telegram's, in a private chat.
	pub buttons: Vec<(String, String)>,
	pub photo: Option<Vec<u8>>,
	/// Mail's way out (RFC 8058).
	pub unsubscribe: Option<Url>,
	/// The notes it tells, for what reads them; empty for a test.
	pub notes: Vec<i64>,
}

/// Who concierge says the holder of an access token is.
#[derive(Clone, Debug)]
pub struct Identity {
	pub user_id: Uuid,
	pub email: String,
	pub email_verified: bool,
	pub preferred_name: String,
	pub permissions: PermissionSet,
}

impl Identity {
	/// preferred_name, or the email when there is none.
	pub fn display_name(&self) -> &str {
		if self.preferred_name.trim().is_empty() { &self.email } else { &self.preferred_name }
	}
}

#[derive(Debug, thiserror::Error)]
pub enum DirectoryError {
	/// The token is refused: revoked, expired, the user held or disabled.
	#[error("concierge refused the token")]
	Refused,
	#[error("concierge is unavailable: {0}")]
	Unavailable(String),
	#[error("concierge failed: {0}")]
	Failed(String),
}

/// What concierge said of a user just now.
#[derive(Clone, Debug)]
pub enum Access {
	Granted(Identity),
	/// No live session to ask with, or concierge refused its token.
	NoSession,
	Unavailable,
}

/// A user's alerts, as they set them.
#[derive(Clone, Debug)]
pub struct Alerts {
	/// The rules open to the user — their permissions', and the GMB rules once they hold a GMB —
	/// each with the tier they set for it, if any.
	pub rules: Vec<(Rule, Option<Tier>)>,
	pub destinations: Vec<Destination>,
	/// `None` until set.
	pub hours: Option<Hours>,
}

/// What a person sets of a destination.
#[derive(Clone, Debug)]
pub struct Settings {
	pub title: Option<String>,
	pub cadences: Cadences,
	pub filter: Filter,
	pub rules: BTreeMap<Rule, Cadence>,
}

/// A note in a person's inbox, as they may read it.
#[derive(Clone, Debug)]
pub struct Alert {
	pub id: i64,
	pub rule: Rule,
	pub tier: Tier,
	pub at: Timestamp,
	pub text: Rendered,
	pub links: Vec<(String, String)>,
	pub link: Option<String>,
	pub gmbs: Vec<i64>,
	pub photo: Option<String>,
	/// Those still to press.
	pub buttons: Vec<Button>,
	pub read_at: Option<Timestamp>,
}

/// The hub at work: the feeds read, the conductor's own rules, the access checks; its delivery is
/// [`crate::deliver`]'s, and what the bot hears [`crate::telegram`]'s.
#[derive(Clone, Debug)]
pub struct Hub<O, C, F> {
	pub panel: Panel,
	pub outlets: O,
	/// concierge: to rotate a session's tokens and to ask `GetMe` with them.
	pub concierge: C,
	/// Where the services' notes come from, and what a button calls.
	pub feeds: F,
	pub locale: Locale,
	/// concierge's origin: where the admins grant the access a user asks for.
	pub cabinet: Url,
	/// The conductor's own: what a message's links are on.
	pub origin: Url,
}
impl<O, C: Refresher + Directory, F: Feeds> Hub<O, C, F> {
	/// Tells what the conductor's own rules say is due.
	pub async fn fan_out_own(&self, now: Timestamp) -> eyre::Result<usize> {
		let received = self.feeds.received().await?;
		self.panel.alerts_fan_out_own(now, self.locale, &self.cabinet, &received).await
	}

	/// Tells a page of `service`'s notes: how many messages were queued, `None` while it publishes
	/// none.
	pub async fn take_feed(&self, service: Service, now: Timestamp) -> eyre::Result<Option<usize>> {
		let after = self.panel.alerts_feed_after(service).await?;
		let Some(page) = self.feeds.notes(service, after, NOTES_PAGE).await? else {
			return Ok(None);
		};
		for note in &page.notes {
			for button in &note.buttons {
				self.feeds
					.callable(service, &button.call)
					.wrap_err_with(|| format!("{service}'s note {}: button {}", note.seq, button.id))?;
			}
		}
		self.panel.alerts_take_notes(service, page, after, now).await.map(Some)
	}

	/// Confirms the access of users not confirmed for [`RECHECK_AFTER`], and prunes what is past
	/// keeping. How many were asked about.
	pub async fn recheck_access(&self, now: Timestamp) -> eyre::Result<usize> {
		let users = {
			let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection for access checks")?;
			db::prune(&mut conn, now - KEEP).await?;
			db::stale_access(&mut conn, now - RECHECK_AFTER, RECHECKS).await?
		};
		for user in &users {
			let access = self.confirm_access(*user, now).await?;
			tracing::debug!(user_id = %user, ?access, "alerts: access rechecked");
		}
		Ok(users.len())
	}

	/// `who` presses a button of a note of theirs: what it came to, and the line and buttons it leaves.
	/// The same button of the same note is one action of theirs, wherever and however often it is
	/// pressed or its update delivered: the service derives its event ids from the key.
	pub async fn press(&self, who: &Identity, note: i64, button: &str, now: Timestamp) -> eyre::Result<Pressed> {
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection for a button")?;
		let sought = Sought {
			id: Some(note),
			..Sought::default()
		};
		let Some(received) = db::inbox(&mut conn, who.user_id, Rule::VARIANTS, &sought, 1).await?.pop() else {
			return Ok(Pressed::Unknown);
		};
		drop(conn);
		let Some(pressed) = received.note.buttons.iter().find(|b| b.id == button) else {
			return Ok(Pressed::Unknown);
		};
		let service = received.note.rule.feed().expect("only a service's note carries buttons");
		let key = format!("alert/{note}/{}/{button}", who.user_id);
		let status = match self.feeds.press(who, service, &pressed.call, &key).await {
			Ok(status) => status,
			Err(e) => {
				tracing::warn!(error = format!("{e:#}"), "alerts: a service did not take a press");
				return Ok(Pressed::TryLater);
			}
		};
		match status {
			201 => {
				tracing::info!(user_id = %who.user_id, button, "alerts: a button recorded");
				let mut tx = self.panel.store.begin_write().await?;
				db::pressed(&mut tx, who.user_id, note, button).await?;
				db::read(&mut tx, who.user_id, Some(&[note]), note, now).await?;
				tx.commit().await.wrap_err("committing a press")?;
				Ok(Pressed::Done {
					line: format!("{}: {}", pressed.pressed, notify::one_line(who.display_name(), notify::MAX_NAME)),
					left: left(&received.note.buttons, Some(button)),
				})
			}
			200 | 400 | 409 => Ok(Pressed::AlreadyDone),
			404 => Ok(Pressed::Gone),
			403 => Ok(Pressed::NoAccess),
			other => {
				tracing::warn!(status = other, button, "alerts: a service refused a press");
				Ok(Pressed::TryLater)
			}
		}
	}

	/// Asks concierge, now, what the user may do — with their newest panel session used within
	/// [`SESSION_IDLE`], rotated when due — and records the answer. A session concierge refuses (to
	/// rotate, or `GetMe` twice running) ends the user's alerts and drops what was queued for them.
	pub async fn confirm_access(&self, user: Uuid, now: Timestamp) -> eyre::Result<Access> {
		let panel = &self.panel;
		let conn = || async { panel.store.pool().acquire().await.wrap_err("a connection to record access") };
		let session = match panel.session_of_user(user, now, now - SESSION_IDLE, &self.concierge).await {
			Ok(s) => s,
			Err(SessionError::Missing) => {
				db::access_tried(&mut *conn().await?, user, now).await?;
				return Ok(Access::NoSession);
			}
			Err(SessionError::Rejected) => {
				db::access_lost(&mut *conn().await?, user, now).await?;
				return Ok(Access::NoSession);
			}
			Err(SessionError::Unavailable) => {
				db::access_tried(&mut *conn().await?, user, now).await?;
				return Ok(Access::Unavailable);
			}
			Err(SessionError::Internal(e)) => return Err(e),
		};
		let answer = match self.concierge.me(&session.access).await {
			// One refusal may be a hiccup on concierge's side; the session is closed only on a
			// second.
			Err(DirectoryError::Refused) => self.concierge.me(&session.access).await,
			other => other,
		};
		match answer {
			Ok(id) if id.user_id != user => Err(eyre::eyre!("GetMe answered another user than the session's")),
			Ok(identity) => {
				db::access_seen(&mut *conn().await?, user, &identity.permissions, identity.display_name(), now).await?;
				Ok(Access::Granted(identity))
			}
			Err(DirectoryError::Refused) => {
				// The token is dead at concierge: so is the session, and whatever it vouched for.
				panel.close_session(&session.key).await?;
				db::access_lost(&mut *conn().await?, user, now).await?;
				Ok(Access::NoSession)
			}
			Err(DirectoryError::Unavailable(why)) => {
				tracing::warn!(why, "alerts: concierge unavailable for an access check");
				db::access_tried(&mut *conn().await?, user, now).await?;
				Ok(Access::Unavailable)
			}
			Err(e @ DirectoryError::Failed(_)) => Err(eyre::eyre!(e)),
		}
	}
}


/// What a press came to.
#[derive(Clone, Debug)]
pub enum Pressed {
	Done { line: String, left: Vec<Button> },
	AlreadyDone,
	/// What it acts on is gone.
	Gone,
	NoAccess,
	TryLater,
	/// No such button of a note of theirs.
	Unknown,
}

/// The buttons left to press once `pressed` was.
fn left(buttons: &[Button], pressed: Option<&str>) -> Vec<Button> {
	match pressed.and_then(|id| buttons.iter().find(|b| b.id == id)) {
		None => buttons.to_vec(),
		Some(p) => buttons.iter().filter(|b| p.leaves.contains(&b.id)).cloned().collect(),
	}
}

/// What a note says, to whom.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Body {
	/// To anyone; `None`: nothing for a channel to read.
	pub open: Option<Rendered>,
	/// To its person alone, when that is more.
	pub private: Option<Rendered>,
	/// The same, for who may see PII.
	pub pii: Option<Rendered>,
	/// `(label, url)`, to its person alone.
	pub links: Vec<(String, String)>,
}

impl Body {
	/// What a reader reads of it; `None`: nothing.
	pub(crate) fn read_by(&self, private: bool, permissions: &PermissionSet) -> Option<&Rendered> {
		match private {
			false => self.open.as_ref(),
			true => self.pii.as_ref().filter(|_| permissions.may(Pii::See)).or(self.private.as_ref()).or(self.open.as_ref()),
		}
	}
}

/// A note to tell.
pub(crate) struct Telling {
	pub rule: Rule,
	/// What it is told once for; [`derived_event_id`] names what has no id of its own.
	pub event: Uuid,
	pub noted_at: Timestamp,
	pub body: Body,
	/// Whom it is for, when its rule's audience is a person.
	pub person: Option<Uuid>,
	/// Who did what it tells: not told.
	pub skip: Option<Uuid>,
	pub gmbs: Vec<i64>,
	/// The gmail it is about, when it is about no GMB of it.
	pub gmail: Option<i64>,
	pub tier: Option<Tier>,
	/// A path on the conductor's origin.
	pub link: Option<String>,
	pub photo: Option<String>,
	pub buttons: Vec<Button>,
}

impl Telling {
	/// A note of the conductor's own, told to `person`.
	pub(crate) fn new(rule: Rule, event: Uuid, noted_at: Timestamp, text: Rendered) -> Self {
		Self {
			rule,
			event,
			noted_at,
			body: Body {
				open: Some(text),
				..Body::default()
			},
			person: None,
			skip: None,
			gmbs: Vec::new(),
			gmail: None,
			tier: None,
			link: None,
			photo: None,
			buttons: Vec::new(),
		}
	}
}

/// The id a rule fans out under for what the conductor tells of itself: a UUID (version 8)
/// named by its parts.
pub(crate) fn derived_event_id(parts: &[&str]) -> Uuid {
	let mut h = Sha256::new();
	h.update(b"sa-panel/tg-fanout/v1");
	for p in parts {
		h.update([0]);
		h.update(p.as_bytes());
	}
	let digest = h.finalize();
	let mut bytes = [0u8; 16];
	bytes.copy_from_slice(&digest[..16]);
	uuid::Builder::from_custom_bytes(bytes).into_uuid()
}

impl Panel {
	pub async fn alerts(&self, user: Uuid, permissions: &PermissionSet) -> eyre::Result<Alerts> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for alerts")?;
		let tiers = db::tiers(&mut conn, user).await?;
		Ok(Alerts {
			rules: open_rules(db::has_gmbs(&mut conn, user).await?, permissions)
				.map(|r| (r, tiers.iter().find(|(t, _)| *t == r).map(|(_, tier)| *tier)))
				.collect(),
			destinations: db::destinations(&mut conn, user).await?,
			hours: db::hours(&mut conn, user).await?,
		})
	}

	/// Changes one of the user's destinations. A rule not open to them is refused, not stored: it
	/// would only mislead the page.
	pub async fn alerts_set(&self, user: Uuid, permissions: &PermissionSet, destination: i64, s: &Settings) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(now) = db::destination_of(&mut tx, user, destination).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let title = s.title.as_deref().map(str::trim).filter(|t| !t.is_empty());
		if title.is_some_and(|t| t.chars().count() > notify::MAX_NAME) {
			return Ok(Err(Refused::Invalid(format!("a title is at most {} characters", notify::MAX_NAME))));
		}
		let gmbs = db::has_gmbs(&mut tx, user).await?;
		if let Some(r) = s.rules.keys().find(|r| !open(**r, permissions, gmbs)) {
			return Ok(Err(Refused::Invalid(format!("{r} is not a rule open to you"))));
		}
		let facets = [s.filter.gmails.len(), s.filter.trades.len(), s.filter.brands.len(), s.filter.gmbs.len()];
		if facets.iter().any(|n| *n > 1_000) {
			return Ok(Err(Refused::Invalid("a facet takes at most 1000".into())));
		}
		let rules: Vec<(Rule, Cadence)> = s.rules.iter().map(|(r, c)| (*r, *c)).collect();
		db::set(&mut tx, destination, title.or(now.title.as_deref()), s.cadences, &s.filter, &rules).await?;
		tx.commit().await.wrap_err("committing a destination's change")?;
		Ok(Ok(()))
	}

	/// A rule's notes at a tier of the user's own; `None`: at theirs.
	pub async fn alerts_set_tier(&self, user: Uuid, permissions: &PermissionSet, rule: Rule, tier: Option<Tier>) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !open(rule, permissions, db::has_gmbs(&mut tx, user).await?) {
			return Ok(Err(Refused::Invalid(format!("{rule} is not a rule open to you"))));
		}
		db::set_tier(&mut tx, user, rule, tier).await?;
		tx.commit().await.wrap_err("committing a rule's tier")?;
		Ok(Ok(()))
	}

	pub async fn alerts_set_hours(&self, user: Uuid, h: &Hours) -> eyre::Result<Result<(), Refused>> {
		if let Err(e) = schedule(h) {
			return Ok(Err(Refused::Invalid(e.to_string())));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a user's hours")?;
		db::set_hours(&mut conn, user, h).await?;
		Ok(Ok(()))
	}

	/// Removes one of the user's destinations, with what was queued for it; `false` when it was not
	/// theirs.
	pub async fn alerts_remove(&self, user: Uuid, destination: i64) -> eyre::Result<bool> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection to remove a destination")?;
		db::remove(&mut conn, user, destination).await
	}

	/// A destination of the user's at `address`, already proved theirs; a browser's was whoever's
	/// subscribed there before.
	pub(crate) async fn alerts_add(&self, user: Uuid, permissions: &PermissionSet, display: &str, address: &Address, title: &str, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let tag = self.key.alert_address_tag(address.method().as_ref(), address.identity());
		let plain = serde_json::to_vec(address).wrap_err("serializing an address")?;
		let sealed = self.key.seal(&alert_address_aad(user, address.method().as_ref()), &plain)?;
		let mut tx = self.store.begin_write().await?;
		if address.method() == Method::Push {
			db::remove_others_push(&mut tx, user, &tag).await?;
		}
		if db::destinations(&mut tx, user).await?.len() >= DESTINATIONS {
			return Ok(Err(Refused::Conflict(format!("at most {DESTINATIONS} destinations"))));
		}
		let address_row = Sealed {
			blob: &sealed,
			fp: self.key.fingerprint(),
			tag,
		};
		let Some(id) = db::add(&mut tx, user, address.method(), address.private(), Some(title), Some(address_row), now).await? else {
			return Ok(Err(Refused::Conflict(format!("{title} is a destination already"))));
		};
		db::access_seen(&mut tx, user, permissions, display, now).await?;
		tx.commit().await.wrap_err("committing a destination")?;
		tracing::info!(user_id = %user, destination = id, method = address.method().as_ref(), "alerts: a destination added");
		Ok(Ok(id))
	}

	/// Where a destination is reached.
	pub(crate) fn alerts_address(&self, d: &Destination) -> eyre::Result<Address> {
		if let Some(chat) = d.chat {
			return Ok(Address::Telegram(Target {
				chat_id: chat.id,
				topic_id: chat.topic_id,
			}));
		}
		let (sealed, fp) = d.address.as_ref().expect("a destination is a chat or has an address sealed");
		eyre::ensure!(fp.as_slice() == self.key.fingerprint(), "destination {} is sealed under another PANEL_DATA_KEY", d.id);
		let plain = self.key.open(&alert_address_aad(d.user_id, d.method.as_ref()), sealed).wrap_err_with(|| format!("destination {}'s address", d.id))?;
		serde_json::from_slice(&plain).wrap_err_with(|| format!("destination {}'s address", d.id))
	}

	/// A link's token that makes `address` a destination of the user's, once they open it signed in.
	pub fn alerts_email_token(&self, user: Uuid, address: &str, now: Timestamp) -> eyre::Result<String> {
		let token = EmailToken {
			user,
			address: address.to_owned(),
			until: now + EMAIL_TOKEN_TTL,
		};
		let plain = serde_json::to_vec(&token).wrap_err("serializing an email token")?;
		Ok(URL_SAFE_NO_PAD.encode(self.key.seal(EMAIL_TOKEN_AAD, &plain)?))
	}

	/// The address `token` proves the user's; `None` for any other's token, or one out of date.
	pub fn alerts_email_proved(&self, user: Uuid, token: &str, now: Timestamp) -> Option<String> {
		let token: EmailToken = self.opened(EMAIL_TOKEN_AAD, token)?;
		(token.user == user && token.until > now).then_some(token.address)
	}

	/// What a message's one-click unsubscribe carries.
	pub(crate) fn alerts_unsubscribe_token(&self, destination: i64) -> eyre::Result<String> {
		let plain = serde_json::to_vec(&destination).wrap_err("serializing a destination's id")?;
		Ok(URL_SAFE_NO_PAD.encode(self.key.seal(UNSUBSCRIBE_AAD, &plain)?))
	}

	/// Tells the destination a message's `token` names nothing more; `false` for no such token.
	pub async fn alerts_unsubscribe(&self, token: &str) -> eyre::Result<bool> {
		let Some(destination) = self.opened::<i64>(UNSUBSCRIBE_AAD, token) else {
			return Ok(false);
		};
		let mut tx = self.store.begin_write().await?;
		db::silence(&mut tx, destination).await?;
		outbox::drop_of_destination(&mut tx, destination, "unsubscribed").await?;
		tx.commit().await.wrap_err("committing an unsubscription")?;
		tracing::info!(destination, "alerts: a destination unsubscribed");
		Ok(true)
	}

	/// What a token sealed under `aad` carries, if it is ours.
	fn opened<T: serde::de::DeserializeOwned>(&self, aad: &[u8], token: &str) -> Option<T> {
		let sealed = URL_SAFE_NO_PAD.decode(token).ok()?; // not base64: not ours
		let plain = self.key.open(aad, &sealed).ok()?; // another key's, or tampered with
		Some(serde_json::from_slice(&plain).expect("sealed here, for this purpose alone"))
	}

	/// The user's notes, newest first, as they may read them now.
	pub async fn alerts_inbox(&self, user: Uuid, permissions: &PermissionSet, sought: &Sought) -> eyre::Result<Vec<Alert>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an inbox")?;
		let rules: Vec<Rule> = Rule::VARIANTS.iter().copied().filter(|r| r.open_to(permissions)).collect();
		let received = db::inbox(&mut conn, user, &rules, sought, INBOX_PAGE).await?;
		drop(conn);
		received
			.into_iter()
			.filter_map(|r| {
				let body = match self.open_note(&r.note) {
					Ok(body) => body,
					Err(e) => return Some(Err(e)),
				};
				let text = body.read_by(true, permissions)?.clone();
				Some(Ok(Alert {
					id: r.note.id,
					rule: r.note.rule,
					tier: r.tier,
					at: r.note.noted_at,
					text,
					links: body.links,
					link: r.note.link,
					gmbs: r.note.gmbs,
					photo: r.note.photo,
					buttons: left(&r.note.buttons, r.pressed.as_deref()),
					read_at: r.read_at,
				}))
			})
			.collect()
	}

	/// How many of the user's notes are unread, by tier.
	pub async fn alerts_unread(&self, user: Uuid, permissions: &PermissionSet) -> eyre::Result<BTreeMap<Tier, i64>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an inbox")?;
		let rules: Vec<Rule> = Rule::VARIANTS.iter().copied().filter(|r| r.open_to(permissions)).collect();
		let unread = db::unread(&mut conn, user, &rules).await?;
		Ok(Tier::VARIANTS
			.iter()
			.map(|t| (*t, unread.iter().find(|(u, _)| u == t).map_or(0, |(_, n)| *n)))
			.collect())
	}

	/// Marks the user's notes read: `ids`, or all of them up to `up_to`.
	pub async fn alerts_read(&self, user: Uuid, ids: Option<&[i64]>, up_to: i64, now: Timestamp) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		db::read(&mut tx, user, ids, up_to, now).await?;
		tx.commit().await.wrap_err("committing notes read")
	}

	/// Where a note of the user's went, or will go.
	pub async fn alerts_told(&self, user: Uuid, note: i64) -> eyre::Result<Vec<Told>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for where a note went")?;
		outbox::told(&mut conn, user, note).await
	}

	/// What concierge just told the `/api/v1` gate of a user: their permissions and name.
	pub async fn alerts_access_seen(&self, user: Uuid, permissions: &PermissionSet, display: &str, now: Timestamp) -> eyre::Result<()> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection to record access")?;
		db::access_seen(&mut conn, user, permissions, display, now).await
	}

	/// Tells what the conductor's own rules say is due at `now`: silent sources (by `received`,
	/// sites' last receipt per key) and access requests. Safe on several replicas at once: each is
	/// told once. `cabinet` is concierge's origin, where the admins grant access. How many messages
	/// were queued.
	pub async fn alerts_fan_out_own(&self, now: Timestamp, locale: Locale, cabinet: &Url, received: &BTreeMap<String, Timestamp>) -> eyre::Result<usize> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the fan-out")?;
		let mut own = Vec::new();
		for (key_id, kind, created_at) in db::active_sources(&mut conn).await? {
			let last = received.get(&key_id).copied();
			let since = last.unwrap_or(created_at);
			if since >= now - notify::SOURCE_SILENCE {
				continue;
			}
			// One message per full day of the same silence: at 24 h, at 48 h, …
			let days = now.duration_since(since).as_hours().div_euclid(24);
			let event = derived_event_id(&["source_silent", &key_id, &since.to_string(), &days.to_string()]);
			let note = Note::SourceSilent { key_id, kind, last };
			own.push(Telling {
				link: Some("/sources".to_owned()),
				..Telling::new(Rule::SourceSilent, event, now, note.render(locale))
			});
		}
		for r in crate::store::access::to_tell(&mut conn, now - crate::access::ASK_AGAIN_AFTER, BATCH).await? {
			let note = Note::AccessRequested {
				email: r.email,
				name: r.name,
				need: r.need,
			};
			let mut telling = Telling::new(Rule::AccessRequested, r.event_id, now, Rendered::default());
			// It names who asked: for a person's own eyes only.
			telling.body = Body {
				private: Some(note.render(locale)),
				links: note.link(locale, cabinet).into_iter().map(|(label, url)| (label.to_owned(), url)).collect(),
				..Body::default()
			};
			own.push(telling);
		}
		let mut queued = 0;
		for t in own {
			let mut tx = crate::store::begin_write(&mut conn).await?;
			queued += self.tell(&mut tx, &t, now).await?;
			tx.commit().await.wrap_err("committing a note")?;
		}
		Ok(queued)
	}

	/// The last of a service's notes already told: where its next page starts.
	pub async fn alerts_feed_after(&self, service: Service) -> eyre::Result<i64> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a feed's cursor")?;
		db::feed_after(&mut conn, service).await
	}

	/// Tells a page of `service`'s notes, which started after `after`: each checked against its
	/// feed and told, and the cursor moved to the page's `next`, in one transaction — taken only
	/// while the cursor still stands at `after`. A note the hub cannot tell fails the page whole: the
	/// cursor stays, and the next pass stops on it again. How many messages were queued.
	pub async fn alerts_take_notes(&self, service: Service, page: NotePage, after: i64, now: Timestamp) -> eyre::Result<usize> {
		eyre::ensure!(page.next >= after, "{service} answered next {} for notes after {after}", page.next);
		for note in &page.notes {
			note.check(service).wrap_err_with(|| format!("{service}'s feed"))?;
		}
		let mut tx = self.store.begin_write().await?;
		let mut queued = 0;
		for note in page.notes {
			let rendered = |t: sa_alerts::NoteText| Rendered {
				text: t.text,
				entities: t.entities,
			};
			let telling = Telling {
				rule: note.rule,
				event: note.event_id,
				noted_at: note.created_at,
				body: Body {
					open: Some(rendered(note.text)),
					private: None,
					pii: note.text_pii.map(rendered),
					links: Vec::new(),
				},
				person: None,
				skip: note.skip_user,
				gmbs: note.gmbs,
				gmail: None,
				tier: note.tier,
				link: note.link,
				photo: note.photo,
				buttons: note.buttons,
			};
			queued += self.tell(&mut tx, &telling, now).await?;
		}
		if !db::move_feed(&mut tx, service, after..page.next).await? {
			tracing::info!(%service, after, "alerts: the page was taken meanwhile");
			return Ok(0);
		}
		tx.commit().await.wrap_err("committing a page of notes")?;
		Ok(queued)
	}

	/// Tells a note, inside the caller's write transaction: kept, in the inbox of each person it is
	/// for, and queued for the destinations of theirs it is for — once per `(rule, event)`. How many
	/// messages were queued.
	pub(crate) async fn tell(&self, tx: &mut sqlx::SqliteConnection, t: &Telling, now: Timestamp) -> eyre::Result<usize> {
		assert_eq!(t.person.is_some(), t.rule.audience() == Audience::Person, "{}: whom it is for", t.rule);
		let plain = zeroize::Zeroizing::new(serde_json::to_vec(&t.body).wrap_err("serializing a note")?);
		let sealed = self.key.seal(&alert_note_aad(t.rule.as_ref(), t.event), &plain)?;
		let kept = db::insert_note(
			tx,
			t.rule,
			t.event,
			t.tier,
			&sealed,
			self.key.fingerprint(),
			&t.gmbs,
			t.gmail,
			t.link.as_deref(),
			t.photo.as_deref(),
			&t.buttons,
			t.noted_at,
		)
		.await?;
		let Some(note) = kept else {
			return Ok(0);
		};
		let about = db::subjects(tx, &t.gmbs).await?;
		let mut queued = 0;
		for p in db::people(tx, t.rule, t.person).await? {
			if !t.rule.open_to(&p.permissions) || t.skip == Some(p.user_id) {
				continue;
			}
			let owners = t.rule.audience() == Audience::GmbOwners;
			let mut subjects: Vec<Subject> = about.iter().filter(|(owner, _)| !owners || *owner == p.user_id).map(|(_, s)| s.clone()).collect();
			if owners && subjects.is_empty() {
				continue;
			}
			if let Some(gmail) = t.gmail {
				subjects.push(Subject {
					gmail: Some(gmail),
					..Subject::default()
				});
			}
			let tier = p.tier.or(t.tier).unwrap_or(t.rule.tier()); // the person's, else the note's, else the rule's
			db::receive(tx, p.user_id, note, tier).await?;
			if p.confirmed_at < now - ACCESS_TTL {
				continue;
			}
			let hours = schedule(&db::hours(tx, p.user_id).await?.unwrap_or_default()).wrap_err_with(|| format!("{}'s hours", p.user_id))?;
			for d in db::destinations(tx, p.user_id).await? {
				let reached = d.dead.is_none() && t.body.read_by(d.private, &p.permissions).is_some() && d.filter.passes(&subjects);
				let Some(when) = reached.then(|| hours.due(d.cadence(t.rule, tier), tier, now)).flatten() else {
					continue;
				};
				let message = outbox::New {
					destination_id: d.id,
					user_id: p.user_id,
					note_id: note,
					rule: t.rule,
					tier,
					lane: &d.lane(),
					when,
					noted_at: t.noted_at,
				};
				outbox::enqueue(tx, &message).await?;
				queued += 1;
			}
		}
		tracing::info!(rule = %t.rule, event = %t.event, note, queued, "alerts: told");
		Ok(queued)
	}

	/// What a note says.
	pub(crate) fn open_note(&self, n: &db::Note) -> eyre::Result<Body> {
		eyre::ensure!(n.data_key_fp.as_slice() == self.key.fingerprint(), "note {} is sealed under another PANEL_DATA_KEY", n.id);
		let plain = self
			.key
			.open(&alert_note_aad(n.rule.as_ref(), n.event_id), &n.body_sealed)
			.wrap_err_with(|| format!("note {} does not open", n.id))?;
		serde_json::from_slice(&plain).wrap_err_with(|| format!("note {} is not one", n.id))
	}
}

#[derive(Deserialize, Serialize)]
struct EmailToken {
	user: Uuid,
	address: String,
	until: Timestamp,
}

fn schedule(h: &Hours) -> Result<sa_core::alerts::Hours, sa_core::Invalid> {
	sa_core::alerts::Hours::try_new(&h.tz, h.daily, h.quiet, h.snoozed_until)
}

fn open_rules(gmbs: bool, permissions: &PermissionSet) -> impl Iterator<Item = Rule> {
	Rule::VARIANTS.iter().copied().filter(move |r| open(*r, permissions, gmbs))
}

/// Whether a rule is open to a user holding `permissions`, and a GMB if `gmbs`.
fn open(rule: Rule, permissions: &PermissionSet, gmbs: bool) -> bool {
	rule.open_to(permissions) && (rule.audience() != Audience::GmbOwners || gmbs)
}
