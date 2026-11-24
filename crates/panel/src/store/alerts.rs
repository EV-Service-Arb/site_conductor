//! Alerts: what concierge last said each user holds, the notes and whose inbox they are in, users'
//! hours and tiers, their destinations, and the services' feed cursors. What is sent where is the
//! outbox's; a Telegram destination's chat is `telegram`'s. Every timestamp is the caller's `now`.

use std::ops::Range;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_alerts::{Button, Rule, Tier};
use sa_auth::{PermissionSet, Service};
use sa_core::alerts::{Cadence, Cadences, Filter, Method, Subject};
use sqlx::{SqliteConnection, types::Json};
use uuid::Uuid;

use super::{from_db, telegram::Chat, to_db};

type DestinationRow = (
	i64,
	Uuid,
	String,
	bool,
	Option<String>,
	String,
	String,
	String,
	Json<Filter>,
	Option<String>,
	Json<Vec<(Rule, Cadence)>>,
	Option<String>,
	Option<i64>,
	Option<i64>,
	Option<Vec<u8>>,
	Option<Vec<u8>>,
);

type NoteRow = (i64, String, Uuid, Option<String>, Vec<u8>, Vec<u8>, Json<Vec<i64>>, Option<String>, Option<String>, Json<Vec<Button>>, i64);

// ── access ──────────────────────────────────────────────────────────────────────────────

/// What concierge said of a user at `at`: their permissions and name. An answer older than the
/// one kept is not taken (a link token's, redeemed after the gate asked again); the same answer
/// within 5 minutes of the last is kept as it is, so the gate's GetMe tells no change.
pub async fn access_seen(conn: &mut SqliteConnection, user: Uuid, permissions: &PermissionSet, display: &str, at: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO alert_access (user_id, permissions, confirmed_at, tried_at, display_name) VALUES ($1, $2, $3, $3, $4) \
		 ON CONFLICT (user_id) DO UPDATE SET permissions = $2, confirmed_at = $3, tried_at = max(coalesce(tried_at, $3), $3), display_name = $4 \
		 WHERE excluded.confirmed_at >= alert_access.confirmed_at \
		 AND (excluded.permissions IS NOT alert_access.permissions OR excluded.display_name IS NOT alert_access.display_name \
		 OR excluded.confirmed_at >= alert_access.confirmed_at + 300000000)",
	)
	.bind(user)
	.bind(Json(permissions))
	.bind(to_db(at))
	.bind(display)
	.execute(&mut *conn)
	.await
	.wrap_err("recording a user's access")?;
	Ok(())
}

/// A try to confirm a user's access that got no answer.
pub async fn access_tried(conn: &mut SqliteConnection, user: Uuid, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_access SET tried_at = $2 WHERE user_id = $1")
		.bind(user)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording an access check")?;
	Ok(())
}

/// A user's access is known to be gone: nothing more is sent to them, queued or not.
pub async fn access_lost(conn: &mut SqliteConnection, user: Uuid, now: Timestamp) -> eyre::Result<()> {
	super::outbox::drop_of_user(conn, user, "access lost").await?;
	sqlx::query("UPDATE alert_access SET permissions = NULL, confirmed_at = $2, tried_at = $2 WHERE user_id = $1")
		.bind(user)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("recording a user's lost access")?;
	Ok(())
}

/// What concierge last confirmed of a user, and when: `None` inside once their access is known
/// lost.
pub async fn access(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Option<(Option<PermissionSet>, Timestamp)>> {
	let row: Option<(Option<Json<PermissionSet>>, i64)> = sqlx::query_as("SELECT permissions, confirmed_at FROM alert_access WHERE user_id = $1")
		.bind(user)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading a user's access")?;
	row.map(|(permissions, at)| Ok((permissions.map(|Json(p)| p), from_db(at)?))).transpose()
}

/// Users with a destination still reached whose access was last confirmed or tried before
/// `before`, oldest first.
pub async fn stale_access(conn: &mut SqliteConnection, before: Timestamp, limit: i64) -> eyre::Result<Vec<Uuid>> {
	sqlx::query_scalar(
		"SELECT a.user_id FROM alert_access a \
		 WHERE a.confirmed_at < $1 AND (a.tried_at IS NULL OR a.tried_at < $1) \
		 AND EXISTS (SELECT 1 FROM alert_destinations d WHERE d.user_id = a.user_id AND d.dead_at IS NULL) \
		 ORDER BY coalesce(a.tried_at, a.confirmed_at) LIMIT $2",
	)
	.bind(to_db(before))
	.bind(limit)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing users to confirm")
}

// ── destinations ────────────────────────────────────────────────────────────────────────

/// Where a person is told, and of what.
#[derive(Clone, Debug)]
pub struct Destination {
	pub id: i64,
	pub user_id: Uuid,
	pub method: Method,
	/// Read by its person alone.
	pub private: bool,
	pub title: Option<String>,
	pub cadences: Cadences,
	pub filter: Filter,
	/// Why nothing reaches it, when nothing does.
	pub dead: Option<String>,
	/// The rules at a cadence of their own there.
	pub rules: Vec<(Rule, Cadence)>,
	/// A Telegram one's.
	pub chat: Option<Chat>,
	/// Any other's address, sealed, and the fingerprint of the key it is sealed under.
	pub(crate) address: Option<(Vec<u8>, Vec<u8>)>,
}

impl Destination {
	pub fn cadence(&self, rule: Rule, tier: Tier) -> Cadence {
		self.rules.iter().find(|(r, _)| *r == rule).map_or(self.cadences.of(tier), |(_, c)| *c)
	}

	/// What is paced together.
	pub fn lane(&self) -> String {
		match &self.chat {
			Some(chat) => format!("chat {}", chat.id),
			None => format!("destination {}", self.id),
		}
	}
}

macro_rules! destination_select {
	() => {
		"SELECT d.id, d.user_id, d.method, d.private, d.title, d.urgent, d.important, d.info, d.filter, d.dead_reason, \
		 (SELECT json_group_array(json_array(r.rule, r.cadence)) FROM alert_rules r WHERE r.destination_id = d.id), \
		 t.kind, t.chat_id, t.topic_id, d.address_sealed, d.data_key_fp \
		 FROM alert_destinations d LEFT JOIN telegram_destinations t ON t.destination_id = d.id "
	};
}

/// The user's destinations, oldest first.
pub async fn destinations(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Vec<Destination>> {
	let rows: Vec<DestinationRow> = sqlx::query_as(concat!(destination_select!(), "WHERE d.user_id = $1 ORDER BY d.id"))
		.bind(user)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a user's destinations")?;
	rows.into_iter().map(destination).collect()
}

/// One of the user's destinations.
pub async fn destination_of(conn: &mut SqliteConnection, user: Uuid, id: i64) -> eyre::Result<Option<Destination>> {
	let row: Option<DestinationRow> = sqlx::query_as(concat!(destination_select!(), "WHERE d.user_id = $1 AND d.id = $2"))
		.bind(user)
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading a destination")?;
	row.map(destination).transpose()
}

/// A destination's address but Telegram's, sealed under `fp`'s key, and the tag that tells it from
/// its person's others of the method.
pub struct Sealed<'a> {
	pub blob: &'a [u8],
	pub fp: [u8; 32],
	pub tag: [u8; 32],
}

/// A new destination of the user's; `None` when the address is one of theirs already.
#[expect(clippy::too_many_arguments, reason = "a row's columns")]
pub async fn add(
	conn: &mut SqliteConnection,
	user: Uuid,
	method: Method,
	private: bool,
	title: Option<&str>,
	address: Option<Sealed<'_>>,
	now: Timestamp,
) -> eyre::Result<Option<i64>> {
	let c = Cadences::starting(method, private);
	sqlx::query_scalar(
		"INSERT INTO alert_destinations (user_id, method, private, title, urgent, important, info, filter, address_sealed, data_key_fp, address_tag, created_at) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) ON CONFLICT DO NOTHING RETURNING id",
	)
	.bind(user)
	.bind(method.as_ref())
	.bind(private)
	.bind(title)
	.bind(c.urgent.as_ref())
	.bind(c.important.as_ref())
	.bind(c.info.as_ref())
	.bind(Json(Filter::default()))
	.bind(address.as_ref().map(|a| a.blob))
	.bind(address.as_ref().map(|a| a.fp.as_slice()))
	.bind(address.as_ref().map(|a| a.tag.as_slice()))
	.bind(to_db(now))
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("adding a destination")
}

/// Removes a push subscription of another user's on the browser one just subscribed.
pub async fn remove_others_push(conn: &mut SqliteConnection, user: Uuid, tag: &[u8; 32]) -> eyre::Result<()> {
	sqlx::query("DELETE FROM alert_destinations WHERE method = 'push' AND address_tag = $2 AND user_id <> $1")
		.bind(user)
		.bind(tag.as_slice())
		.execute(&mut *conn)
		.await
		.wrap_err("removing a browser's other subscriptions")?;
	Ok(())
}

/// Removes one of the user's destinations, its rules and its messages; `false` when it was not
/// theirs.
pub async fn remove(conn: &mut SqliteConnection, user: Uuid, id: i64) -> eyre::Result<bool> {
	let gone = sqlx::query("DELETE FROM alert_destinations WHERE id = $1 AND user_id = $2")
		.bind(id)
		.bind(user)
		.execute(&mut *conn)
		.await
		.wrap_err("removing a destination")?
		.rows_affected();
	Ok(gone == 1)
}

/// What a destination's person may change of it.
pub async fn set(conn: &mut SqliteConnection, id: i64, title: Option<&str>, cadences: Cadences, filter: &Filter, rules: &[(Rule, Cadence)]) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_destinations SET title = $2, urgent = $3, important = $4, info = $5, filter = $6 WHERE id = $1")
		.bind(id)
		.bind(title)
		.bind(cadences.urgent.as_ref())
		.bind(cadences.important.as_ref())
		.bind(cadences.info.as_ref())
		.bind(Json(filter))
		.execute(&mut *conn)
		.await
		.wrap_err("changing a destination")?;
	sqlx::query("DELETE FROM alert_rules WHERE destination_id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping a destination's rules")?;
	for (rule, cadence) in rules {
		sqlx::query("INSERT INTO alert_rules (destination_id, rule, cadence) VALUES ($1, $2, $3)")
			.bind(id)
			.bind(rule.as_ref())
			.bind(cadence.as_ref())
			.execute(&mut *conn)
			.await
			.wrap_err("setting a destination's rule")?;
	}
	Ok(())
}

/// Nothing reaches the destination — a Telegram one: its chat, so none of the chat's: nothing more
/// goes there until a send succeeds again.
pub async fn dead(conn: &mut SqliteConnection, id: i64, why: &str, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"UPDATE alert_destinations SET dead_at = $2, dead_reason = $3 WHERE dead_at IS NULL AND (id = $1 OR id IN ( \
		   SELECT o.destination_id FROM telegram_destinations t JOIN telegram_destinations o ON o.chat_id = t.chat_id WHERE t.destination_id = $1))",
	)
	.bind(id)
	.bind(to_db(now))
	.bind(why)
	.execute(&mut *conn)
	.await
	.wrap_err("marking a destination dead")?;
	super::outbox::drop_of_dead(conn, why).await
}

/// A send to the destination succeeded: it is reached again.
pub async fn revive(conn: &mut SqliteConnection, id: i64) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_destinations SET dead_at = NULL, dead_reason = NULL WHERE id = $1 AND dead_at IS NOT NULL")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("reviving a destination")?;
	Ok(())
}

/// A destination whose owner asked, from a message, to be told no more there.
pub async fn silence(conn: &mut SqliteConnection, id: i64) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_destinations SET urgent = 'off', important = 'off', info = 'off' WHERE id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("silencing a destination")?;
	sqlx::query("DELETE FROM alert_rules WHERE destination_id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("dropping a destination's rules")?;
	Ok(())
}

fn destination(
	(id, user_id, method, private, title, urgent, important, info, Json(filter), dead, Json(rules), kind, chat_id, topic_id, address, fp): DestinationRow,
) -> eyre::Result<Destination> {
	let cadence = |c: String| c.parse::<Cadence>().wrap_err("a stored cadence");
	Ok(Destination {
		id,
		user_id,
		method: method.parse().wrap_err("a stored method")?,
		private,
		title,
		cadences: Cadences {
			urgent: cadence(urgent)?,
			important: cadence(important)?,
			info: cadence(info)?,
		},
		filter,
		dead,
		rules,
		chat: match (kind, chat_id) {
			(Some(kind), Some(id)) => Some(Chat {
				kind: kind.parse().wrap_err("a stored chat's kind")?,
				id,
				topic_id,
			}),
			_ => None,
		},
		address: address.zip(fp),
	})
}

// ── a person's hours and tiers ──────────────────────────────────────────────────────────

/// A person's hours as they set them, in minutes since midnight in `tz`.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, schemars::JsonSchema)]
#[schemars(rename = "AlertHours")]
pub struct Hours {
	/// An IANA time zone.
	pub tz: String,
	/// When the daily message goes out.
	pub daily: u16,
	pub quiet: Option<(u16, u16)>,
	pub snoozed_until: Option<Timestamp>,
}

impl Default for Hours {
	/// Until their panel says where they are.
	fn default() -> Self {
		Self {
			tz: "UTC".to_owned(),
			daily: 9 * 60,
			quiet: None,
			snoozed_until: None,
		}
	}
}

/// The user's hours; `None` until they, or their panel, set them.
pub async fn hours(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Option<Hours>> {
	type Row = (String, u16, Option<u16>, Option<u16>, Option<i64>);
	let row: Option<Row> = sqlx::query_as("SELECT tz, daily, quiet_from, quiet_to, snoozed_until FROM alert_hours WHERE user_id = $1")
		.bind(user)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading a user's hours")?;
	row.map(|(tz, daily, from, to, snoozed)| {
		Ok(Hours {
			tz,
			daily,
			quiet: from.zip(to),
			snoozed_until: snoozed.map(from_db).transpose()?,
		})
	})
	.transpose()
}

pub async fn set_hours(conn: &mut SqliteConnection, user: Uuid, h: &Hours) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO alert_hours (user_id, tz, daily, quiet_from, quiet_to, snoozed_until) VALUES ($1, $2, $3, $4, $5, $6) \
		 ON CONFLICT (user_id) DO UPDATE SET tz = $2, daily = $3, quiet_from = $4, quiet_to = $5, snoozed_until = $6",
	)
	.bind(user)
	.bind(&h.tz)
	.bind(h.daily)
	.bind(h.quiet.map(|(from, _)| from))
	.bind(h.quiet.map(|(_, to)| to))
	.bind(h.snoozed_until.map(to_db))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a user's hours")?;
	Ok(())
}

/// The rules the user gave a tier of their own.
pub async fn tiers(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Vec<(Rule, Tier)>> {
	let rows: Vec<(String, String)> = sqlx::query_as("SELECT rule, tier FROM alert_tiers WHERE user_id = $1")
		.bind(user)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("reading a user's tiers")?;
	rows.into_iter()
		.map(|(rule, tier)| Ok((rule.parse().wrap_err("a stored rule")?, tier.parse().wrap_err("a stored tier")?)))
		.collect()
}

/// `None`: as the note, else the rule, says.
pub async fn set_tier(conn: &mut SqliteConnection, user: Uuid, rule: Rule, tier: Option<Tier>) -> eyre::Result<()> {
	let query = match tier {
		Some(tier) => sqlx::query("INSERT INTO alert_tiers (user_id, rule, tier) VALUES ($1, $2, $3) ON CONFLICT (user_id, rule) DO UPDATE SET tier = $3")
			.bind(user)
			.bind(rule.as_ref())
			.bind(tier.as_ref()),
		None => sqlx::query("DELETE FROM alert_tiers WHERE user_id = $1 AND rule = $2").bind(user).bind(rule.as_ref()),
	};
	query.execute(&mut *conn).await.wrap_err("setting a rule's tier")?;
	Ok(())
}

// ── whom a note is for ──────────────────────────────────────────────────────────────────

/// Whether `gmail` is the user's and not removed.
pub async fn owns_gmail(conn: &mut SqliteConnection, user: Uuid, gmail: i64) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM gmails WHERE id = $1 AND owner = $2 AND removed_at IS NULL)")
		.bind(gmail)
		.bind(user)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a gmail up")
}

/// Whether the user has a GMB not removed: the GMB rules are theirs.
pub async fn has_gmbs(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM gmbs WHERE owner = $1 AND removed_at IS NULL)")
		.bind(user)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a user's GMBs up")
}

/// A person concierge once confirmed, whose access is not known lost.
#[derive(Clone, Debug)]
pub struct Person {
	pub user_id: Uuid,
	pub permissions: PermissionSet,
	pub confirmed_at: Timestamp,
	/// The rule's tier as they set it.
	pub tier: Option<Tier>,
}

/// The people `rule` may reach; `only`: one of them.
pub async fn people(conn: &mut SqliteConnection, rule: Rule, only: Option<Uuid>) -> eyre::Result<Vec<Person>> {
	type Row = (Uuid, Json<PermissionSet>, i64, Option<String>);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT a.user_id, a.permissions, a.confirmed_at, t.tier FROM alert_access a \
		 LEFT JOIN alert_tiers t ON t.user_id = a.user_id AND t.rule = $1 \
		 WHERE a.permissions IS NOT NULL AND ($2 IS NULL OR a.user_id = $2) ORDER BY a.user_id",
	)
	.bind(rule.as_ref())
	.bind(only)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a rule's people")?;
	rows.into_iter()
		.map(|(user_id, Json(permissions), at, tier)| {
			Ok(Person {
				user_id,
				permissions,
				confirmed_at: from_db(at)?,
				tier: tier.map(|t| t.parse()).transpose().wrap_err("a stored tier")?,
			})
		})
		.collect()
}

/// The GMBs of `ids` that are on, they and their gmails not removed: whose, and what a filter asks.
pub async fn subjects(conn: &mut SqliteConnection, ids: &[i64]) -> eyre::Result<Vec<(Uuid, Subject)>> {
	type Row = (i64, Uuid, Option<i64>, Option<String>, Option<i64>);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT b.id, b.owner, b.gmail_id, b.trade, b.brand_id FROM json_each($1) j JOIN gmbs b ON b.id = j.value \
		 LEFT JOIN gmails g ON g.id = b.gmail_id \
		 WHERE b.removed_at IS NULL AND g.removed_at IS NULL AND NOT EXISTS (SELECT 1 FROM gmbs_off o WHERE o.gmb_id = b.id)",
	)
	.bind(Json(ids))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading a note's GMBs")?;
	Ok(rows
		.into_iter()
		.map(|(id, owner, gmail, trade, brand)| {
			(
				owner,
				Subject {
					gmb: Some(id),
					gmail,
					trade,
					brand,
				},
			)
		})
		.collect())
}

// ── notes ───────────────────────────────────────────────────────────────────────────────

/// A note as it is kept.
#[derive(Clone, Debug)]
pub struct Note {
	pub id: i64,
	pub rule: Rule,
	pub event_id: Uuid,
	pub tier: Option<Tier>,
	pub body_sealed: Vec<u8>,
	pub data_key_fp: Vec<u8>,
	pub gmbs: Vec<i64>,
	pub link: Option<String>,
	pub photo: Option<String>,
	pub buttons: Vec<Button>,
	pub noted_at: Timestamp,
}

/// Keeps a note; `None` when `(rule, event)` was told already (by another replica).
#[expect(clippy::too_many_arguments, reason = "a row's columns")]
pub async fn insert_note(
	conn: &mut SqliteConnection,
	rule: Rule,
	event: Uuid,
	tier: Option<Tier>,
	body_sealed: &[u8],
	data_key_fp: [u8; 32],
	gmbs: &[i64],
	gmail: Option<i64>,
	link: Option<&str>,
	photo: Option<&str>,
	buttons: &[Button],
	noted_at: Timestamp,
) -> eyre::Result<Option<i64>> {
	sqlx::query_scalar(
		"INSERT INTO alert_notes (rule, event_id, tier, body_sealed, data_key_fp, gmbs, gmail_id, link, photo, buttons, noted_at) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) ON CONFLICT (rule, event_id) DO NOTHING RETURNING id",
	)
	.bind(rule.as_ref())
	.bind(event)
	.bind(tier.map(|t| t.as_ref().to_owned()))
	.bind(body_sealed)
	.bind(data_key_fp.as_slice())
	.bind(Json(gmbs))
	.bind(gmail)
	.bind(link)
	.bind(photo)
	.bind(Json(buttons))
	.bind(to_db(noted_at))
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("keeping a note")
}

/// Notes noted before `before` go, with what they fanned out to.
pub async fn prune(conn: &mut SqliteConnection, before: Timestamp) -> eyre::Result<()> {
	sqlx::query("DELETE FROM alert_notes WHERE noted_at < $1")
		.bind(to_db(before))
		.execute(&mut *conn)
		.await
		.wrap_err("pruning notes")?;
	Ok(())
}

macro_rules! note_columns {
	() => {
		"n.id, n.rule, n.event_id, n.tier, n.body_sealed, n.data_key_fp, n.gmbs, n.link, n.photo, n.buttons, n.noted_at"
	};
}

pub async fn note(conn: &mut SqliteConnection, id: i64) -> eyre::Result<Note> {
	let row: NoteRow = sqlx::query_as(concat!("SELECT ", note_columns!(), " FROM alert_notes n WHERE n.id = $1"))
		.bind(id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err_with(|| format!("reading note {id}"))?;
	note_of(row)
}

fn note_of((id, rule, event_id, tier, body_sealed, data_key_fp, Json(gmbs), link, photo, Json(buttons), noted_at): NoteRow) -> eyre::Result<Note> {
	Ok(Note {
		id,
		rule: rule.parse().wrap_err_with(|| format!("note {id}'s rule"))?,
		event_id,
		tier: tier.map(|t| t.parse()).transpose().wrap_err_with(|| format!("note {id}'s tier"))?,
		body_sealed,
		data_key_fp,
		gmbs,
		link,
		photo,
		buttons,
		noted_at: from_db(noted_at)?,
	})
}

// ── the inbox ───────────────────────────────────────────────────────────────────────────

/// A note in a person's inbox.
#[derive(Clone, Debug)]
pub struct Received {
	pub note: Note,
	pub tier: Tier,
	pub read_at: Option<Timestamp>,
	/// The last of its buttons they pressed.
	pub pressed: Option<String>,
}

/// Which of an inbox to read; a field unset takes all.
#[derive(Clone, Debug, Default)]
pub struct Sought {
	pub id: Option<i64>,
	pub tier: Option<Tier>,
	pub unread: bool,
	pub gmb: Option<i64>,
	pub before: Option<i64>,
}

pub async fn receive(conn: &mut SqliteConnection, user: Uuid, note: i64, tier: Tier) -> eyre::Result<()> {
	sqlx::query("INSERT INTO alert_inbox (user_id, note_id, tier) VALUES ($1, $2, $3)")
		.bind(user)
		.bind(note)
		.bind(tier.as_ref())
		.execute(&mut *conn)
		.await
		.wrap_err("putting a note in an inbox")?;
	Ok(())
}

/// The user's notes of `rules`, newest first.
pub async fn inbox(conn: &mut SqliteConnection, user: Uuid, rules: &[Rule], s: &Sought, limit: i64) -> eyre::Result<Vec<Received>> {
	type Row = (i64, String, Uuid, Option<String>, Vec<u8>, Vec<u8>, Json<Vec<i64>>, Option<String>, Option<String>, Json<Vec<Button>>, i64, String, Option<i64>, Option<String>);
	let rows: Vec<Row> = sqlx::query_as(concat!(
		"SELECT ",
		note_columns!(),
		", i.tier, i.read_at, i.pressed FROM alert_inbox i JOIN alert_notes n ON n.id = i.note_id \
		 WHERE i.user_id = $1 AND n.rule IN (SELECT value FROM json_each($2)) AND ($3 IS NULL OR i.note_id = $3) AND ($4 IS NULL OR i.tier = $4) \
		 AND (NOT $5 OR i.read_at IS NULL) AND ($6 IS NULL OR EXISTS (SELECT 1 FROM json_each(n.gmbs) j WHERE j.value = $6)) \
		 AND ($7 IS NULL OR i.note_id < $7) ORDER BY i.note_id DESC LIMIT $8"
	))
	.bind(user)
	.bind(Json(rules))
	.bind(s.id)
	.bind(s.tier.map(|t| t.as_ref().to_owned()))
	.bind(s.unread)
	.bind(s.gmb)
	.bind(s.before)
	.bind(limit)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading an inbox")?;
	rows.into_iter()
		.map(|(id, rule, event, own, body, fp, gmbs, link, photo, buttons, noted, tier, read, pressed)| {
			Ok(Received {
				note: note_of((id, rule, event, own, body, fp, gmbs, link, photo, buttons, noted))?,
				tier: tier.parse().wrap_err("a stored tier")?,
				read_at: read.map(from_db).transpose()?,
				pressed,
			})
		})
		.collect()
}

/// How many of the user's notes of `rules` are unread, by tier.
pub async fn unread(conn: &mut SqliteConnection, user: Uuid, rules: &[Rule]) -> eyre::Result<Vec<(Tier, i64)>> {
	let rows: Vec<(String, i64)> = sqlx::query_as(
		"SELECT i.tier, count(*) FROM alert_inbox i JOIN alert_notes n ON n.id = i.note_id \
		 WHERE i.user_id = $1 AND i.read_at IS NULL AND n.rule IN (SELECT value FROM json_each($2)) GROUP BY i.tier",
	)
	.bind(user)
	.bind(Json(rules))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("counting an inbox's unread")?;
	rows.into_iter().map(|(tier, n)| Ok((tier.parse().wrap_err("a stored tier")?, n))).collect()
}

/// Marks the user's notes read — `ids`, or all of them up to `up_to` — and gives up what was still
/// waiting to go out of them.
pub async fn read(conn: &mut SqliteConnection, user: Uuid, ids: Option<&[i64]>, up_to: i64, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"UPDATE alert_inbox SET read_at = $4 WHERE user_id = $1 AND read_at IS NULL AND note_id <= $3 \
		 AND ($2 IS NULL OR note_id IN (SELECT value FROM json_each($2)))",
	)
	.bind(user)
	.bind(ids.map(Json))
	.bind(up_to)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("marking notes read")?;
	super::outbox::drop_read(conn, user, now).await
}

pub async fn pressed(conn: &mut SqliteConnection, user: Uuid, note: i64, button: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE alert_inbox SET pressed = $3 WHERE user_id = $1 AND note_id = $2")
		.bind(user)
		.bind(note)
		.bind(button)
		.execute(&mut *conn)
		.await
		.wrap_err("recording a press")?;
	Ok(())
}

// ── the conductor's own ─────────────────────────────────────────────────────────────────

/// An active source: its key id, kind and when it was added.
pub async fn active_sources(conn: &mut SqliteConnection) -> eyre::Result<Vec<(String, String, Timestamp)>> {
	let rows: Vec<(String, String, i64)> = sqlx::query_as("SELECT key_id, kind, created_at FROM sources WHERE revoked_at IS NULL ORDER BY key_id")
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing the active sources")?;
	rows.into_iter().map(|(key_id, kind, created_at)| Ok((key_id, kind, from_db(created_at)?))).collect()
}

// ── the feeds ───────────────────────────────────────────────────────────────────────────

/// The last of a service's notes already fanned out.
pub async fn feed_after(conn: &mut SqliteConnection, service: Service) -> eyre::Result<i64> {
	sqlx::query_scalar("SELECT after FROM alert_feeds WHERE service = $1")
		.bind(service.as_str())
		.fetch_one(&mut *conn)
		.await
		.wrap_err_with(|| format!("reading {service}'s feed cursor"))
}

/// Moves a service's cursor from `moved.start` to `moved.end`; `false` when it no longer stood at
/// the start (another replica took the page).
pub async fn move_feed(conn: &mut SqliteConnection, service: Service, moved: Range<i64>) -> eyre::Result<bool> {
	let moved = sqlx::query("UPDATE alert_feeds SET after = $3 WHERE service = $1 AND after = $2")
		.bind(service.as_str())
		.bind(moved.start)
		.bind(moved.end)
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("moving {service}'s feed cursor"))?
		.rows_affected();
	Ok(moved == 1)
}
