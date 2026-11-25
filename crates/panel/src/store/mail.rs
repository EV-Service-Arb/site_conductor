//! Google's mail as kept: the cases, their messages (each text sealed), the drafts of replies to them.

use std::collections::{BTreeSet, HashMap};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	gbp,
	mail::{DraftState, Holds, Mark, Reading, Sender, Topic},
};
use sqlx::{SqliteConnection, types::Json};
use uuid::Uuid;

use super::{connections::Sealed, from_db, to_db};

const MAIL: &str = "SELECT m.id, m.case_id, m.gmail_id, m.message, m.thread, m.at, m.sent, m.from_address, m.reply_to, m.recipients, m.subject, m.message_id, m.refs, \
	m.authentic, m.repliable, m.gmb_id, m.listing, m.reading, m.read_by, m.attempts, m.read_error, m.spam FROM mails m";

/// The cases that are one with case $1.
const WITH_IT: &str = "SELECT o.id FROM mail_cases o JOIN mail_cases c ON coalesce(o.joined, o.id) = coalesce(c.joined, c.id) WHERE c.id = $1";

const DRAFT: &str = "SELECT d.id, d.case_id, d.answers, d.body_sealed, d.data_key_fp, d.needs, d.state, d.send_at, d.created_at, d.sent_at, d.error, d.login FROM mail_drafts d";

/// Who read a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema, serde::Serialize)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "MailReader")]
pub enum ReadBy {
	Rule,
	Model,
	/// A person corrected it.
	Person,
}

/// A message to keep.
pub(crate) struct NewMail<'a> {
	pub case: i64,
	pub gmail: i64,
	pub message: &'a str,
	pub thread: &'a str,
	pub at: Timestamp,
	pub sent: bool,
	pub from: Option<&'a str>,
	pub reply_to: Option<&'a str>,
	pub to: &'a [String],
	pub subject: Option<&'a str>,
	pub message_id: Option<&'a str>,
	pub refs: &'a [String],
	pub authentic: bool,
	pub repliable: bool,
	pub gmb: Option<i64>,
	pub listing: Option<&'a str>,
	pub spam: bool,
	pub text: &'a Sealed,
}

#[derive(sqlx::FromRow)]
pub(crate) struct MailRow {
	pub id: i64,
	pub case_id: i64,
	pub gmail_id: i64,
	pub message: String,
	pub thread: String,
	at: i64,
	pub sent: bool,
	pub from_address: Option<String>,
	pub reply_to: Option<String>,
	recipients: Json<Vec<String>>,
	pub subject: Option<String>,
	pub message_id: Option<String>,
	refs: Json<Vec<String>>,
	pub authentic: bool,
	pub repliable: bool,
	pub gmb_id: Option<i64>,
	pub listing: Option<String>,
	reading: Option<String>,
	read_by: Option<String>,
	pub attempts: i64,
	pub read_error: Option<String>,
	pub spam: bool,
}
impl MailRow {
	pub(crate) fn at(&self) -> eyre::Result<Timestamp> {
		from_db(self.at)
	}

	pub(crate) fn to(&self) -> &[String] {
		&self.recipients.0
	}

	pub(crate) fn refs(&self) -> &[String] {
		&self.refs.0
	}

	/// `Err`: another build's, of a kind this one does not know.
	pub(crate) fn reading(&self) -> Result<Option<Reading>, serde_json::Error> {
		self.reading.as_deref().map(serde_json::from_str).transpose()
	}

	pub(crate) fn read_by(&self) -> eyre::Result<Option<ReadBy>> {
		self.read_by.as_deref().map(|r| r.parse().wrap_err("a stored reader")).transpose()
	}
}

#[derive(sqlx::FromRow)]
pub(crate) struct CaseRow {
	pub id: i64,
	pub gmail_id: i64,
	numbers: Json<BTreeSet<String>>,
	pub gmb_id: Option<i64>,
	topic: Option<String>,
	mark_mail: Option<i64>,
	mark_open: bool,
	/// The case it is part of.
	pub joined: Option<i64>,
}
impl CaseRow {
	pub(crate) fn numbers(&self) -> &BTreeSet<String> {
		&self.numbers.0
	}

	pub(crate) fn mark(&self) -> Option<(Mark, i64)> {
		Some((if self.mark_open { Mark::Open } else { Mark::Done }, self.mark_mail?))
	}

	pub(crate) fn topic(&self) -> eyre::Result<Option<Topic>> {
		self.topic.as_deref().map(|t| t.parse().wrap_err("a stored topic")).transpose()
	}
}

/// A draft as stored; its body stays sealed.
pub(crate) struct DraftRow {
	pub id: i64,
	pub case: i64,
	pub answers: i64,
	pub body: Sealed,
	pub needs: Vec<String>,
	pub state: DraftState,
	pub send_at: Option<Timestamp>,
	pub created_at: Timestamp,
	pub sent_at: Option<Timestamp>,
	pub error: Option<String>,
	pub login: bool,
}

#[derive(sqlx::FromRow)]
struct RawDraft {
	id: i64,
	case_id: i64,
	answers: i64,
	body_sealed: Vec<u8>,
	data_key_fp: Vec<u8>,
	needs: Json<Vec<String>>,
	state: String,
	send_at: Option<i64>,
	created_at: i64,
	sent_at: Option<i64>,
	error: Option<String>,
	login: bool,
}

fn draft(r: RawDraft) -> eyre::Result<DraftRow> {
	Ok(DraftRow {
		id: r.id,
		case: r.case_id,
		answers: r.answers,
		body: Sealed {
			blob: r.body_sealed,
			data_key_fp: r.data_key_fp,
		},
		needs: r.needs.0,
		state: r.state.parse().wrap_err("a stored draft's state")?,
		send_at: r.send_at.map(from_db).transpose()?,
		created_at: from_db(r.created_at)?,
		sent_at: r.sent_at.map(from_db).transpose()?,
		error: r.error,
		login: r.login,
	})
}

fn json<T: serde::Serialize + ?Sized>(v: &T) -> String {
	serde_json::to_string(v).expect("strings serialize")
}

/// What each case of the gmail's holds, for a message to join; a message not Google's draws none.
pub(crate) async fn holds(conn: &mut SqliteConnection, gmail: i64) -> eyre::Result<Vec<Holds>> {
	type Row = (i64, Json<BTreeSet<String>>, String, Option<String>, bool, Option<String>);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT c.id, c.numbers, m.thread, m.message_id, m.sent, m.from_address FROM mail_cases c \
		 JOIN mails m ON m.case_id = c.id AND (m.authentic OR m.sent) WHERE c.gmail_id = $1 ORDER BY c.id",
	)
	.bind(gmail)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("reading what a gmail's cases hold")?;
	let mut held: Vec<Holds> = Vec::new();
	for (case, Json(numbers), thread, message_id, sent, from) in rows {
		if held.last().is_none_or(|h| h.case != case) {
			held.push(Holds { case, numbers, ..Holds::default() });
		}
		let h = held.last_mut().expect("pushed for its first message");
		if sent || from.is_some_and(|f| Sender::of(&f) == Sender::Support) {
			h.threads.insert(thread);
		}
		h.message_ids.extend(message_id);
	}
	Ok(held)
}

/// Whether it is kept already; where Gmail has it now kept with it.
pub(crate) async fn kept(conn: &mut SqliteConnection, gmail: i64, message: &str, spam: bool) -> eyre::Result<bool> {
	sqlx::query("UPDATE mails SET spam = $3 WHERE gmail_id = $1 AND message = $2 AND spam != $3")
		.bind(gmail)
		.bind(message)
		.bind(spam)
		.execute(&mut *conn)
		.await
		.wrap_err("keeping where a message is")?;
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM mails WHERE gmail_id = $1 AND message = $2)")
		.bind(gmail)
		.bind(message)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a message up")
}

pub(crate) async fn add_case(conn: &mut SqliteConnection, gmail: i64, numbers: &BTreeSet<String>) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO mail_cases (gmail_id, numbers) VALUES ($1, $2) RETURNING id")
		.bind(gmail)
		.bind(json(numbers))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("opening a case")
}

pub(crate) async fn set_numbers(conn: &mut SqliteConnection, case: i64, numbers: &BTreeSet<String>) -> eyre::Result<()> {
	sqlx::query("UPDATE mail_cases SET numbers = $2 WHERE id = $1")
		.bind(case)
		.bind(json(numbers))
		.execute(&mut *conn)
		.await
		.wrap_err("numbering a case")?;
	Ok(())
}

pub(crate) async fn add(conn: &mut SqliteConnection, m: &NewMail<'_>) -> eyre::Result<i64> {
	sqlx::query_scalar(
		"INSERT INTO mails (case_id, gmail_id, message, thread, at, sent, from_address, reply_to, recipients, subject, message_id, refs, authentic, repliable, gmb_id, \
		 listing, text_sealed, data_key_fp, spam) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19) RETURNING id",
	)
	.bind(m.case)
	.bind(m.gmail)
	.bind(m.message)
	.bind(m.thread)
	.bind(to_db(m.at))
	.bind(m.sent)
	.bind(m.from)
	.bind(m.reply_to)
	.bind(json(m.to))
	.bind(m.subject)
	.bind(m.message_id)
	.bind(json(m.refs))
	.bind(m.authentic)
	.bind(m.repliable)
	.bind(m.gmb)
	.bind(m.listing)
	.bind(&m.text.blob)
	.bind(&m.text.data_key_fp)
	.bind(m.spam)
	.fetch_one(&mut *conn)
	.await
	.wrap_err("keeping a message")
}

/// The drafts not handed to SMTP of the case, whatever is joined with it: another message came.
pub(crate) async fn supersede(conn: &mut SqliteConnection, case: i64) -> eyre::Result<()> {
	sqlx::query(sqlx::AssertSqlSafe(format!(
		"UPDATE mail_drafts SET state = 'superseded' WHERE case_id IN ({WITH_IT}) AND state IN ('proposed', 'held', 'failed')"
	)))
	.bind(case)
	.execute(&mut *conn)
	.await
	.wrap_err("superseding a case's drafts")?;
	Ok(())
}

/// The draft going by itself of the case, whatever is joined with it, waits for a person.
pub(crate) async fn unhold(conn: &mut SqliteConnection, case: i64) -> eyre::Result<()> {
	sqlx::query(sqlx::AssertSqlSafe(format!(
		"UPDATE mail_drafts SET state = 'proposed' WHERE case_id IN ({WITH_IT}) AND state = 'held'"
	)))
	.bind(case)
	.execute(&mut *conn)
	.await
	.wrap_err("holding a case's draft back")?;
	Ok(())
}

/// Nothing of the gmail's goes by itself any more, until switched on anew.
pub(crate) async fn let_go(conn: &mut SqliteConnection, gmail: i64) -> eyre::Result<()> {
	for sql in [
		"UPDATE mail_drafts SET state = 'proposed' WHERE state = 'held' AND case_id IN (SELECT id FROM mail_cases WHERE gmail_id = $1)",
		"DELETE FROM gmb_mail_auto WHERE gmb_id IN (SELECT id FROM gmbs WHERE gmail_id = $1)",
	] {
		sqlx::query(sql).bind(gmail).execute(&mut *conn).await.wrap_err("letting a gmail's automatic replies go")?;
	}
	Ok(())
}

/// Whether a reply to message `answers` was handed to SMTP.
pub(crate) async fn handed(conn: &mut SqliteConnection, answers: i64) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM mail_drafts WHERE answers = $1 AND state IN ('sending', 'sent'))")
		.bind(answers)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a reply handed over up")
}

/// The cases of the owner's gmails not removed, oldest first.
pub(crate) async fn cases(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<CaseRow>> {
	sqlx::query_as(
		"SELECT c.id, c.gmail_id, c.numbers, c.gmb_id, c.topic, c.mark_mail, c.mark_open, c.joined FROM mail_cases c JOIN gmails g ON g.id = c.gmail_id \
		 WHERE g.owner = $1 AND g.removed_at IS NULL ORDER BY c.id",
	)
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a person's cases")
}

/// The messages of the owner's gmails not removed, oldest first.
pub(crate) async fn mails(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<MailRow>> {
	sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{MAIL} JOIN gmails g ON g.id = m.gmail_id WHERE g.owner = $1 AND g.removed_at IS NULL ORDER BY m.at, m.id"
	)))
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a person's mail with Google")
}

/// What each of the owner's cases asked of its person and was given, oldest first: the case, the item, the document.
pub(crate) async fn given(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<(i64, String, Option<i64>)>> {
	sqlx::query_as(
		"SELECT v.case_id, v.item, v.document_id FROM mail_given v JOIN mail_cases c ON c.id = v.case_id JOIN gmails g ON g.id = c.gmail_id \
		 WHERE g.owner = $1 AND g.removed_at IS NULL ORDER BY v.given_at",
	)
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing what a person gave")
}

/// Given, by `document` if by one; `false`, not given any more.
pub(crate) async fn give(conn: &mut SqliteConnection, case: i64, item: &str, given: bool, document: Option<i64>, by: Uuid, now: Timestamp) -> eyre::Result<()> {
	let set = match given {
		true => sqlx::query(
			"INSERT INTO mail_given (case_id, item, document_id, given_by, given_at) VALUES ($1, $2, $3, $4, $5) \
			 ON CONFLICT (case_id, item) DO UPDATE SET document_id = excluded.document_id, given_by = excluded.given_by, given_at = excluded.given_at",
		)
		.bind(case)
		.bind(item)
		.bind(document)
		.bind(by)
		.bind(to_db(now)),
		false => sqlx::query("DELETE FROM mail_given WHERE case_id = $1 AND item = $2").bind(case).bind(item),
	};
	set.execute(&mut *conn).await.wrap_err("keeping what a person gave")?;
	Ok(())
}

/// The gmail's messages that may name a GBP, each with the one it names as kept.
pub(crate) async fn naming(conn: &mut SqliteConnection, gmail: i64) -> eyre::Result<Vec<(i64, String, Option<String>, Option<i64>)>> {
	sqlx::query_as("SELECT id, message, subject, gmb_id FROM mails WHERE gmail_id = $1 AND (authentic OR sent) ORDER BY id")
		.bind(gmail)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a gmail's mail to name")
}

pub(crate) async fn set_gmb(conn: &mut SqliteConnection, mail: i64, gmb: i64) -> eyre::Result<()> {
	sqlx::query("UPDATE mails SET gmb_id = $2 WHERE id = $1")
		.bind(mail)
		.bind(gmb)
		.execute(&mut *conn)
		.await
		.wrap_err("naming a message's GBP")?;
	Ok(())
}

/// By Message-ID, who sent each reply the conductor handed to SMTP: a person's name; `None`, it went by itself.
pub(crate) async fn senders(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<HashMap<String, Option<String>>> {
	let rows: Vec<(String, Option<String>)> = sqlx::query_as(
		"SELECT d.message_id, u.name FROM mail_drafts d LEFT JOIN users u ON u.user_id = d.decided_by \
		 JOIN mail_cases c ON c.id = d.case_id JOIN gmails g ON g.id = c.gmail_id WHERE g.owner = $1 AND d.state IN ('sending', 'sent')",
	)
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing who sent a person's replies")?;
	Ok(rows.into_iter().collect())
}

/// Each case's latest draft, of the owner's gmails not removed.
pub(crate) async fn drafts(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<DraftRow>> {
	let rows: Vec<RawDraft> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{DRAFT} JOIN mail_cases c ON c.id = d.case_id JOIN gmails g ON g.id = c.gmail_id WHERE g.owner = $1 AND g.removed_at IS NULL \
		 AND d.id = (SELECT max(o.id) FROM mail_drafts o WHERE o.case_id = d.case_id)"
	)))
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a person's drafts")?;
	rows.into_iter().map(draft).collect()
}

/// A message's text, sealed, by its message's id.
pub(crate) async fn text(conn: &mut SqliteConnection, mail: i64) -> eyre::Result<Sealed> {
	let (blob, data_key_fp) = sqlx::query_as("SELECT text_sealed, data_key_fp FROM mails WHERE id = $1")
		.bind(mail)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("loading a message's text")?;
	Ok(Sealed { blob, data_key_fp })
}

/// Each move of the owner's GBPs, from which state (none, it was added) and to which.
pub(crate) async fn moves(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<(i64, Timestamp, Option<gbp::State>, gbp::State)>> {
	let rows: Vec<(i64, i64, Option<String>, String)> = sqlx::query_as(
		"SELECT e.gmb_id, e.at, e.data ->> 'from', coalesce(e.data ->> 'to', e.data ->> 'state') FROM gmb_events e JOIN gmbs b ON b.id = e.gmb_id \
		 WHERE b.owner = $1 AND e.kind IN ('state', 'added') ORDER BY e.id",
	)
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing the moves of a person's GBPs")?;
	rows.into_iter()
		.map(|(gmb, at, from, to)| Ok((gmb, from_db(at)?, from.map(|f| f.parse()).transpose()?, to.parse()?)))
		.collect()
}

/// The owner's GBPs not removed.
pub(crate) async fn live(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<BTreeSet<i64>> {
	let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM gmbs WHERE owner = $1 AND removed_at IS NULL")
		.bind(owner)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a person's live GBPs")?;
	Ok(ids.into_iter().collect())
}

/// Whom each of the owner's gmails' inboxes is read as, and whether another live gmail's is too.
pub(crate) async fn inboxes(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<HashMap<i64, (String, bool)>> {
	let rows: Vec<(i64, String, bool)> = sqlx::query_as(
		"SELECT i.gmail_id, i.username, EXISTS (SELECT 1 FROM gmail_imap o JOIN gmails og ON og.id = o.gmail_id \
		 WHERE o.gmail_id != i.gmail_id AND lower(o.username) = lower(i.username) AND og.removed_at IS NULL) \
		 FROM gmail_imap i JOIN gmails g ON g.id = i.gmail_id WHERE g.owner = $1 AND g.removed_at IS NULL",
	)
	.bind(owner)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing whom a person's inboxes are read as")?;
	Ok(rows.into_iter().map(|(gmail, username, shared)| (gmail, (username, shared))).collect())
}

/// Whoever has mail kept, in a gmail not removed.
pub(crate) async fn owners(conn: &mut SqliteConnection) -> eyre::Result<Vec<Uuid>> {
	sqlx::query_scalar("SELECT DISTINCT g.owner FROM mail_cases c JOIN gmails g ON g.id = c.gmail_id WHERE g.removed_at IS NULL ORDER BY g.owner")
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing whose mail is kept")
}

/// The model's never goes over a person's.
pub(crate) async fn set_reading(conn: &mut SqliteConnection, mail: i64, reading: &Reading, by: ReadBy, now: Timestamp) -> eyre::Result<()> {
	let by: &'static str = by.into();
	sqlx::query("UPDATE mails SET reading = $2, read_by = $3, read_at = $4, read_error = NULL WHERE id = $1 AND (read_by IS NOT 'person' OR $3 = 'person')")
		.bind(mail)
		.bind(json(reading))
		.bind(by)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("keeping a message's reading")?;
	Ok(())
}

pub(crate) async fn read_failed(conn: &mut SqliteConnection, mail: i64, error: &str) -> eyre::Result<()> {
	sqlx::query("UPDATE mails SET attempts = attempts + 1, read_error = $2 WHERE id = $1")
		.bind(mail)
		.bind(error)
		.execute(&mut *conn)
		.await
		.wrap_err("keeping a failed reading")?;
	Ok(())
}

/// `Some(None)` takes what was set by hand away.
pub(crate) async fn edit_case(conn: &mut SqliteConnection, case: i64, gmb: Option<Option<i64>>, topic: Option<Option<Topic>>) -> eyre::Result<()> {
	sqlx::query("UPDATE mail_cases SET gmb_id = CASE WHEN $2 THEN $3 ELSE gmb_id END, topic = CASE WHEN $4 THEN $5 ELSE topic END WHERE id = $1")
		.bind(case)
		.bind(gmb.is_some())
		.bind(gmb.flatten())
		.bind(topic.is_some())
		.bind(topic.flatten().map(<&'static str>::from))
		.execute(&mut *conn)
		.await
		.wrap_err("editing a case")?;
	Ok(())
}

/// Made part of case `to`, with what is part of it; `None`, one by itself again.
pub(crate) async fn join(conn: &mut SqliteConnection, case: i64, to: Option<i64>) -> eyre::Result<()> {
	sqlx::query("UPDATE mail_cases SET joined = $2 WHERE id = $1 OR ($2 IS NOT NULL AND joined = $1)")
		.bind(case)
		.bind(to)
		.execute(&mut *conn)
		.await
		.wrap_err("joining a case")?;
	Ok(())
}

/// Marked at a message by a person; `None`, no longer.
pub(crate) async fn set_mark(conn: &mut SqliteConnection, case: i64, mark: Option<(Mark, i64, Uuid)>) -> eyre::Result<()> {
	sqlx::query("UPDATE mail_cases SET mark_mail = $2, mark_by = $3, mark_open = $4 WHERE id = $1")
		.bind(case)
		.bind(mark.map(|(_, at, _)| at))
		.bind(mark.map(|(.., by)| by))
		.bind(mark.is_some_and(|(mark, ..)| mark == Mark::Open))
		.execute(&mut *conn)
		.await
		.wrap_err("marking a case")?;
	Ok(())
}

/// What the owner was charged under `keys`.
pub(crate) async fn spent(conn: &mut SqliteConnection, owner: Uuid, keys: &[String]) -> eyre::Result<i64> {
	sqlx::query_scalar("SELECT coalesce(-sum(delta), 0) FROM token_ledger WHERE user_id = $1 AND kind = 'charge' AND key IN (SELECT value FROM json_each($2))")
		.bind(owner)
		.bind(json(keys))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("summing what a case was charged")
}

/// A draft to keep.
pub(crate) struct NewDraft<'a> {
	pub case: i64,
	pub answers: i64,
	pub body: &'a Sealed,
	pub needs: &'a [String],
	pub state: DraftState,
	pub send_at: Option<Timestamp>,
	pub error: Option<&'a str>,
}

pub(crate) async fn add_draft(conn: &mut SqliteConnection, d: &NewDraft<'_>, now: Timestamp) -> eyre::Result<()> {
	let state: &'static str = d.state.into();
	sqlx::query(
		"INSERT INTO mail_drafts (case_id, answers, body_sealed, data_key_fp, needs, state, send_at, created_at, error) \
		 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
	)
	.bind(d.case)
	.bind(d.answers)
	.bind(&d.body.blob)
	.bind(&d.body.data_key_fp)
	.bind(json(d.needs))
	.bind(state)
	.bind(d.send_at.map(to_db))
	.bind(to_db(now))
	.bind(d.error)
	.execute(&mut *conn)
	.await
	.wrap_err("keeping a draft")?;
	Ok(())
}

/// A draft of the owner's.
pub(crate) async fn draft_of(conn: &mut SqliteConnection, owner: Uuid, id: i64) -> eyre::Result<Option<DraftRow>> {
	let row: Option<RawDraft> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
		"{DRAFT} JOIN mail_cases c ON c.id = d.case_id JOIN gmails g ON g.id = c.gmail_id WHERE d.id = $1 AND g.owner = $2 AND g.removed_at IS NULL"
	)))
	.bind(id)
	.bind(owner)
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("loading a draft")?;
	row.map(draft).transpose()
}

/// The held drafts due, with whose they are.
pub(crate) async fn held_due(conn: &mut SqliteConnection, now: Timestamp) -> eyre::Result<Vec<(Uuid, i64)>> {
	sqlx::query_as(
		"SELECT g.owner, d.id FROM mail_drafts d JOIN mail_cases c ON c.id = d.case_id JOIN gmails g ON g.id = c.gmail_id \
		 WHERE d.state = 'held' AND d.send_at <= $1 AND g.removed_at IS NULL ORDER BY d.send_at, d.id",
	)
	.bind(to_db(now))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing the drafts due")
}

/// What becomes of a draft; each absent field stays, but `error` and `login`.
pub(crate) struct Decided<'a> {
	pub state: DraftState,
	pub body: Option<&'a Sealed>,
	pub by: Option<Uuid>,
	pub send_at: Option<Timestamp>,
	pub sent_at: Option<Timestamp>,
	pub message_id: Option<&'a str>,
	pub error: Option<&'a str>,
	pub login: bool,
}
impl Decided<'_> {
	pub(crate) fn new(state: DraftState) -> Self {
		Self {
			state,
			body: None,
			by: None,
			send_at: None,
			sent_at: None,
			message_id: None,
			error: None,
			login: false,
		}
	}
}

pub(crate) async fn decide(conn: &mut SqliteConnection, id: i64, d: &Decided<'_>) -> eyre::Result<()> {
	let state: &'static str = d.state.into();
	sqlx::query(
		"UPDATE mail_drafts SET state = $2, body_sealed = coalesce($3, body_sealed), data_key_fp = coalesce($4, data_key_fp), decided_by = coalesce($5, decided_by), \
		 send_at = coalesce($6, send_at), sent_at = $7, message_id = coalesce($8, message_id), error = $9, login = $10 WHERE id = $1",
	)
	.bind(id)
	.bind(state)
	.bind(d.body.map(|b| &b.blob))
	.bind(d.body.map(|b| &b.data_key_fp))
	.bind(d.by)
	.bind(d.send_at.map(to_db))
	.bind(d.sent_at.map(to_db))
	.bind(d.message_id)
	.bind(d.error)
	.bind(d.login)
	.execute(&mut *conn)
	.await
	.wrap_err("deciding a draft")?;
	Ok(())
}

/// The topics whose replies go by themselves, per GBP of the owner's not removed.
pub(crate) async fn auto(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Vec<(i64, Topic)>> {
	let rows: Vec<(i64, String)> =
		sqlx::query_as("SELECT a.gmb_id, a.topic FROM gmb_mail_auto a JOIN gmbs b ON b.id = a.gmb_id WHERE b.owner = $1 AND b.removed_at IS NULL ORDER BY a.gmb_id, a.topic")
			.bind(owner)
			.fetch_all(&mut *conn)
			.await
			.wrap_err("listing the topics answered automatically")?;
	rows.into_iter().map(|(g, t)| Ok((g, t.parse().wrap_err("a stored topic")?))).collect()
}

pub(crate) async fn set_auto(conn: &mut SqliteConnection, gmb: i64, topic: Topic, on: bool) -> eyre::Result<()> {
	let topic: &'static str = topic.into();
	let sql = match on {
		true => "INSERT INTO gmb_mail_auto (gmb_id, topic) VALUES ($1, $2) ON CONFLICT DO NOTHING",
		false => "DELETE FROM gmb_mail_auto WHERE gmb_id = $1 AND topic = $2",
	};
	sqlx::query(sql)
		.bind(gmb)
		.bind(topic)
		.execute(&mut *conn)
		.await
		.wrap_err("switching a topic's automatic replies")?;
	Ok(())
}

pub(crate) async fn stuck(conn: &mut SqliteConnection, owner: Uuid) -> eyre::Result<Option<String>> {
	sqlx::query_scalar("SELECT why FROM mail_stuck WHERE user_id = $1")
		.bind(owner)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading why a person's mail waits")
}

pub(crate) async fn set_stuck(conn: &mut SqliteConnection, owner: Uuid, why: Option<&str>) -> eyre::Result<()> {
	let set = match why {
		Some(why) => sqlx::query("INSERT INTO mail_stuck (user_id, why) VALUES ($1, $2) ON CONFLICT (user_id) DO UPDATE SET why = excluded.why")
			.bind(owner)
			.bind(why),
		None => sqlx::query("DELETE FROM mail_stuck WHERE user_id = $1").bind(owner),
	};
	set.execute(&mut *conn).await.wrap_err("saying why a person's mail waits")?;
	Ok(())
}

/// A purged gmail's mail, its cases and drafts.
pub(crate) async fn forget(conn: &mut SqliteConnection, gmail: i64) -> eyre::Result<()> {
	for sql in [
		"DELETE FROM mail_given WHERE case_id IN (SELECT id FROM mail_cases WHERE gmail_id = $1)",
		"DELETE FROM mail_drafts WHERE case_id IN (SELECT id FROM mail_cases WHERE gmail_id = $1)",
		"DELETE FROM mails WHERE gmail_id = $1",
		"DELETE FROM mail_cases WHERE gmail_id = $1",
	] {
		sqlx::query(sql).bind(gmail).execute(&mut *conn).await.wrap_err("forgetting a purged gmail's mail")?;
	}
	Ok(())
}
