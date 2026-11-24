//! Google's mail about a person's GBPs (docs/ARCHITECTURE.md, Google's mail): kept as each inbox is
//! followed, joined into cases, read by rule or by the model the server asks, answered by a draft that
//! a person sends, or that goes by itself where they switched that on.
//!
//! ```text
//! a reply: proposed ─┬─▶ sending ─▶ sent | failed        sending is committed before SMTP; one left so
//!          held ─────┘                                  is never tried again
//!          either, once another message came ─▶ superseded
//! ```

use std::{
	collections::{BTreeSet, HashMap},
	pin::Pin,
};

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Rule, Tier};
use sa_core::{
	mail::{self as rules, Draft as Reply, DraftState, HOLD, Holds, Joining, Kind, MAX_ASKS, MAX_READS, Message, READ_BACK, Reading, Ruled, Sender, State, Topic, Why},
	notify::{Locale, Note, one_line},
};
use sqlx::SqliteConnection;
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::{smtp::Gmail, store::mail::ReadBy};
use crate::{
	Panel, Refused,
	alerts::{Telling, derived_event_id},
	inbox::{Account, Mail},
	seal::{gmail_imap_aad, gmail_mail_aad, mail_draft_aad},
	store::{
		connections,
		gmbs::{self as gmbs_db, Gmb},
		mail::{self as db, Decided, DraftRow, MailRow, NewDraft, NewMail},
	},
};

/// The ports a gmail's mail is read and sent on.
pub const PORTS: [u16; 2] = [crate::imap::PORT, crate::smtp::PORT];
/// Topics a person may have answered automatically.
pub const AUTO: [Topic; 2] = [Topic::Verification, Topic::Reviews];
/// Unread mail the model is handed in one pass, per person.
pub const READS: usize = 20;
/// A reply's files, in all.
pub const FILES_MAX: usize = 18 << 20;
const SUMMARY_MAX: usize = 160;
/// A turn older than this is told of no more: the marks of what was told are kept a little longer.
const TELL_WITHIN: SignedDuration = SignedDuration::from_hours(24 * 6);

pub type Sending<'a> = Pin<Box<dyn Future<Output = Result<(), SendError>> + Send + 'a>>;

/// How a gmail's mail goes out.
pub trait Smtp: std::fmt::Debug + Send + Sync {
	fn send<'a>(&'a self, account: Account<'a>, mail: &'a Outgoing) -> Sending<'a>;
}

/// A message to send.
#[derive(Clone, Debug)]
pub struct Outgoing {
	pub to: String,
	pub subject: String,
	pub body: String,
	/// Without its brackets, as each below.
	pub message_id: String,
	pub in_reply_to: Option<String>,
	pub references: Vec<String>,
	pub files: Vec<File>,
}

#[derive(Clone, Debug)]
pub struct File {
	pub name: String,
	/// Its media type.
	pub kind: String,
	pub bytes: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum SendError {
	/// It did not go out.
	#[error("{0}")]
	NotSent(String),
	/// It may have.
	#[error("{0}")]
	Unknown(String),
}

/// A case, a person's.
#[derive(Clone, Debug)]
pub struct Case {
	pub id: i64,
	pub gmail: i64,
	/// The GBP it is about.
	pub gmb: Option<i64>,
	pub topic: Topic,
	pub numbers: BTreeSet<String>,
	pub state: State,
	/// A reply is due and none is written, whatever it waits on.
	pub due: bool,
	/// Oldest first.
	pub messages: Vec<Kept>,
	/// The latest.
	pub draft: Option<Draft>,
}
impl Case {
	/// What its state is about: its last message somebody reads.
	pub fn last(&self) -> &Kept {
		let heard = self.messages.iter().rev().find(|m| !(m.said.sent && m.said.noreply));
		heard.or(self.messages.last()).expect("a case is opened by its first message")
	}

	/// Google's last word in it.
	pub fn google(&self) -> Option<&Kept> {
		self.messages.iter().rev().find(|m| !m.said.sent)
	}

	fn messages(&self) -> Vec<Message> {
		self.messages.iter().map(|m| m.said.clone()).collect()
	}
}

/// A message kept; its text apart ([`Panel::mail_case`]).
#[derive(Clone, Debug)]
pub struct Kept {
	pub said: Message,
	pub gmail: i64,
	/// Gmail's id of it, and its thread's, hex.
	pub message: String,
	pub thread: String,
	pub from: Option<String>,
	pub reply_to: Option<String>,
	pub to: Vec<String>,
	pub subject: Option<String>,
	pub message_id: Option<String>,
	pub refs: Vec<String>,
	pub authentic: bool,
	pub listing: Option<String>,
	/// The GBP it is about: what a person said, else what it names, what its link is known by, what its reader saw.
	pub gmb: Option<i64>,
	pub read_by: Option<ReadBy>,
}

#[derive(Clone, Debug)]
pub struct Draft {
	pub id: i64,
	pub reply: Reply,
	pub body: String,
	pub created_at: Timestamp,
}

/// A message the model is to read.
pub struct Unread {
	pub owner: Uuid,
	pub mail: i64,
	pub from: String,
	pub subject: Option<String>,
	pub at: Timestamp,
	pub text: Zeroizing<String>,
	/// The gmail's GBPs.
	pub gmbs: Vec<rules::Named>,
	/// What was said before it in its case: when, whether by us, the gist.
	pub before: Vec<(Timestamp, bool, String)>,
}

/// What a case's writer works from.
pub struct Brief {
	pub case: Case,
	/// Each message's text, in the case's order.
	pub texts: Vec<Zeroizing<String>>,
	pub gmb: Option<Gmb>,
	/// Google's other readings of its GBP, oldest first.
	pub record: Vec<(Timestamp, Reading)>,
}

/// What the writer answered.
pub enum Written {
	Reply {
		body: String,
		needs: Vec<String>,
	},
	Nothing(String),
	/// It answered nothing that reads.
	Failed(String),
}

/// What a person changes of a case; each absent is left.
pub struct CaseEdit {
	pub gmb: Option<Option<i64>>,
	pub topic: Option<Option<Topic>>,
	pub done: Option<bool>,
}

/// What a person corrects of a reading; each absent is left.
pub struct ReadingEdit {
	pub kind: Option<Kind>,
	pub topic: Option<Topic>,
	pub gmb: Option<Option<i64>>,
	pub summary: Option<String>,
	pub deadline: Option<Option<Timestamp>>,
}

impl Panel {
	/// Message `m` of `gmail` kept, joined to its case of `cases` (which follow), read when rules alone
	/// read it, its case's drafts superseded; `named` the GBPs its text names. Nothing kept already, nor
	/// what is no Business Profile's.
	pub(crate) async fn keep_mail(&self, tx: &mut SqliteConnection, gmail: i64, m: &Mail, named: &[i64], cases: &mut Vec<Holds>, now: Timestamp) -> eyre::Result<()> {
		let message = m.message.to_string();
		if db::kept(tx, gmail, &message).await? {
			return Ok(());
		}
		let subject = m.subject.as_deref().unwrap_or_default(); // none: nothing a subject says
		let ruled = match m.sent {
			true if m.to.iter().any(|a| Sender::of(a) != Sender::Other) => None,
			true => return Ok(()),
			false => match rules::ruled(m.from.as_deref(), &m.counterparts.join(", "), m.authentic, subject) {
				Ruled::Not => return Ok(()),
				Ruled::Reads(kind) => Some(kind),
				Ruled::Model => None,
			},
		};
		let forged = ruled == Some(Kind::Impersonation);
		let sender = m.from.as_deref().map_or(Sender::Other, Sender::of);
		let thread = format!("{:x}", m.thread);
		let numbers = rules::numbers(subject);
		let alone = !m.sent && sender != Sender::Support;
		let joined = match forged {
			true => None, // one that drew a case could silence it
			false => rules::join(
				&Joining {
					alone,
					thread: &thread,
					refs: &m.refs,
					numbers: &numbers,
				},
				cases,
			),
		};
		let case = match joined {
			Some(case) => case,
			None => db::add_case(tx, gmail, &numbers).await?,
		};
		let text = rules::trimmed(&m.text);
		let sealed = self.seal(&gmail_mail_aad(gmail, &message), &text)?;
		let listing = rules::listing(&text).filter(|_| !forged);
		let new = NewMail {
			case,
			gmail,
			message: &message,
			thread: &thread,
			at: m.date,
			sent: m.sent,
			from: m.from.as_deref(),
			reply_to: m.reply_to.as_deref(),
			to: &m.to,
			subject: m.subject.as_deref(),
			message_id: m.message_id.as_deref(),
			refs: &m.refs,
			authentic: m.authentic,
			repliable: !m.sent && m.authentic && rules::repliable(sender, &text),
			gmb: match (named, forged) {
				([one], false) => Some(*one),
				_ => None,
			},
			listing: listing.as_deref(),
			text: &sealed,
		};
		let mail = db::add(tx, &new).await?;
		if let Some(kind) = ruled {
			let reading = Reading {
				topic: kind.topic().expect("what a rule reads is of one topic"),
				kind,
				gmb: None,
				summary: subject.to_owned(),
				asks: Vec::new(),
				reasons: Vec::new(),
				deadline: None,
				answer_within_hours: None,
			};
			db::set_reading(tx, mail, &reading, ReadBy::Rule, now).await?;
		}
		if forged {
			return Ok(());
		}
		let held = match cases.iter().position(|c| c.case == case) {
			Some(at) => &mut cases[at],
			None => {
				cases.push(Holds { case, ..Holds::default() });
				cases.last_mut().expect("pushed just now")
			}
		};
		if !alone {
			held.threads.insert(thread.clone());
		}
		held.message_ids.extend(m.message_id.clone());
		let known = held.numbers.len();
		held.numbers.extend(numbers);
		if joined.is_some() && held.numbers.len() > known {
			db::set_numbers(tx, case, &held.numbers).await?;
		}
		if joined.is_some() && !unheard(m.sent, &m.to) {
			db::supersede(tx, case).await?;
		}
		Ok(())
	}

	/// The person's cases, those last spoken in first.
	pub async fn mail_cases(&self, owner: Uuid, now: Timestamp) -> eyre::Result<Vec<Case>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's cases")?;
		let mut cases = self.gathered(&mut conn, owner, now).await?;
		cases.sort_by_key(|c| std::cmp::Reverse((c.last().said.at, c.id)));
		Ok(cases)
	}

	/// The person's case `id`, with what its writer works from.
	pub async fn mail_case(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<Brief, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a case")?;
		let cases = self.gathered(&mut conn, owner, now).await?;
		let Some(case) = cases.iter().find(|c| c.id == id).cloned() else {
			return Ok(Err(Refused::NotFound));
		};
		let mut record: Vec<(Timestamp, Reading)> = cases
			.iter()
			.filter(|c| c.id != id)
			.flat_map(|c| c.messages.iter().filter_map(|m| (case.gmb.is_some() && m.gmb.or(c.gmb) == case.gmb).then(|| m.said.reading.clone().map(|r| (m.said.at, r)))?))
			.collect();
		record.sort_by_key(|(at, _)| *at);
		let mut texts = Vec::new();
		for m in &case.messages {
			texts.push(self.mail_text(&mut conn, m.gmail, &m.message, m.said.id).await?);
		}
		let gmb = match case.gmb {
			Some(gmb) => gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await?,
			None => None,
		};
		Ok(Ok(Brief { case, texts, gmb, record }))
	}

	/// The person's case `id` corrected, or marked done at its last message.
	pub async fn edit_mail_case(&self, owner: Uuid, by: Uuid, id: i64, e: CaseEdit, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(case) = self.gathered(&mut tx, owner, now).await?.into_iter().find(|c| c.id == id) else {
			return Ok(Err(Refused::NotFound));
		};
		if let Some(Some(gmb)) = e.gmb
			&& gmbs_db::gmb_of(&mut tx, Some(owner), gmb).await?.is_none()
		{
			return Ok(Err(Refused::NotFound));
		}
		db::edit_case(&mut tx, id, e.gmb, e.topic).await?;
		if let Some(done) = e.done {
			db::set_done(&mut tx, id, done.then(|| (case.messages.last().expect("a case has a message").said.id, by))).await?;
			if done {
				db::supersede(&mut tx, id).await?;
			}
		}
		tx.commit().await.wrap_err("committing a case's edit")?;
		Ok(Ok(()))
	}

	/// The reading of message `mail` of a case of the person's, corrected: their own from then on.
	pub async fn correct_mail(&self, owner: Uuid, mail: i64, e: ReadingEdit, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let cases = self.gathered(&mut tx, owner, now).await?;
		let Some((case, kept)) = cases.iter().find_map(|c| c.messages.iter().find(|m| m.said.id == mail).map(|m| (c.id, m))) else {
			return Ok(Err(Refused::NotFound));
		};
		if kept.said.sent {
			return Ok(Err(Refused::Invalid("only what Google sent is read".to_owned())));
		}
		if let Some(Some(gmb)) = e.gmb
			&& gmbs_db::gmb_of(&mut tx, Some(owner), gmb).await?.is_none()
		{
			return Ok(Err(Refused::NotFound));
		}
		let mut reading = match (&kept.said.reading, e.kind) {
			(Some(r), _) => r.clone(),
			(None, Some(kind)) => Reading {
				topic: Topic::Other,
				kind,
				gmb: kept.gmb,
				summary: kept.subject.clone().unwrap_or_default(), // none: the person may write one
				asks: Vec::new(),
				reasons: Vec::new(),
				deadline: None,
				answer_within_hours: None,
			},
			(None, None) => return Ok(Err(Refused::Invalid("it is not read yet: say its kind".to_owned()))),
		};
		reading.kind = e.kind.unwrap_or(reading.kind);
		reading.gmb = e.gmb.unwrap_or(kept.gmb);
		reading.summary = e.summary.unwrap_or(reading.summary);
		reading.deadline = e.deadline.unwrap_or(reading.deadline);
		reading.topic = match (reading.kind.topic(), e.topic) {
			(Some(of), Some(said)) if of != said => {
				let (kind, of): (&str, &str) = (reading.kind.into(), of.into());
				return Ok(Err(Refused::Invalid(format!("{kind} is of {of}"))));
			}
			(Some(of), _) => of,
			(None, said) => said.unwrap_or(reading.topic),
		};
		db::set_reading(&mut tx, mail, &reading, ReadBy::Person, now).await?;
		tx.commit().await.wrap_err("committing a corrected reading")?;
		Ok(Ok(case))
	}

	/// The gmails' topics whose replies go by themselves, the person's.
	pub async fn mail_auto(&self, owner: Uuid) -> eyre::Result<Vec<(i64, Topic)>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for automatic replies")?;
		db::auto(&mut conn, owner).await
	}

	pub async fn set_mail_auto(&self, owner: Uuid, gmail: i64, topic: Topic, on: bool) -> eyre::Result<Result<(), Refused>> {
		if !AUTO.contains(&topic) {
			let topic: &str = topic.into();
			return Ok(Err(Refused::Invalid(format!("replies on {topic} never go by themselves"))));
		}
		if let Err(r) = self.gmail_of(owner, gmail).await? {
			return Ok(Err(r));
		}
		let mut tx = self.store.begin_write().await?;
		db::set_auto(&mut tx, gmail, topic, on).await?;
		tx.commit().await.wrap_err("committing automatic replies")?;
		Ok(Ok(()))
	}

	/// Whoever has mail kept.
	pub async fn mail_owners(&self) -> eyre::Result<Vec<Uuid>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for whose mail is kept")?;
		db::owners(&mut conn).await
	}

	/// The person's mail the model is to read, the latest first, at most [`READS`].
	pub async fn unread_mail(&self, owner: Uuid, now: Timestamp) -> eyre::Result<Vec<Unread>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for unread mail")?;
		let cases = self.gathered(&mut conn, owner, now).await?;
		let mut due: Vec<(&Case, usize)> = cases
			.iter()
			.flat_map(|c| (0..c.messages.len()).map(move |at| (c, at)))
			.filter(|(c, at)| {
				let m = &c.messages[*at];
				!m.said.sent && m.authentic && m.said.reading.is_none() && m.said.read_error.is_none() && now.duration_since(m.said.at) <= READ_BACK
			})
			.collect();
		due.sort_by_key(|(c, at)| std::cmp::Reverse(c.messages[*at].said.at));
		let mut unread = Vec::new();
		for (c, at) in due.into_iter().take(READS) {
			let m = &c.messages[at];
			unread.push(Unread {
				owner,
				mail: m.said.id,
				from: m.from.clone().ok_or_else(|| eyre::eyre!("message {} is authentic and names no sender", m.said.id))?,
				subject: m.subject.clone(),
				at: m.said.at,
				text: self.mail_text(&mut conn, m.gmail, &m.message, m.said.id).await?,
				gmbs: crate::store::inbox::gmbs(&mut conn, m.gmail).await?,
				before: c.messages[..at]
					.iter()
					.filter_map(|b| match &b.said.reading {
						Some(r) => Some((b.said.at, false, r.summary.clone())),
						None => b.said.sent.then(|| (b.said.at, true, b.subject.clone().unwrap_or_default())), // a message may have no subject
					})
					.collect(),
			});
		}
		Ok(unread)
	}

	/// What the model read of message `mail`: kept unless a person corrected it meanwhile, its GBP one of
	/// `gmbs`.
	pub async fn mail_read(&self, mail: i64, mut reading: Reading, gmbs: &[rules::Named], now: Timestamp) -> eyre::Result<()> {
		reading.gmb = reading.gmb.filter(|id| gmbs.iter().any(|g| g.id == *id));
		reading.summary = one_line(&reading.summary, SUMMARY_MAX);
		if let Some(topic) = reading.kind.topic() {
			reading.topic = topic;
		}
		let mut tx = self.store.begin_write().await?;
		db::set_reading(&mut tx, mail, &reading, ReadBy::Model, now).await?;
		tx.commit().await.wrap_err("committing a reading")
	}

	/// Message `mail` was not read; given up on after [`MAX_READS`].
	pub async fn mail_unread(&self, mail: i64, error: &str) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		db::read_failed(&mut tx, mail, error).await?;
		tx.commit().await.wrap_err("committing a failed reading")
	}

	/// Why the person's mail waits: no Claude, no balance; `None`, it does not.
	pub async fn mail_stuck(&self, owner: Uuid, why: Option<&str>) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		if db::stuck(&mut tx, owner).await?.as_deref() == why {
			return Ok(());
		}
		db::set_stuck(&mut tx, owner, why).await?;
		tx.commit().await.wrap_err("committing why mail waits")
	}

	/// A reply to the person's case `id`, written of its messages up to `answers`; it goes by itself only when
	/// none asked for it, its topic is switched on there and it lacks nothing. `None`: another message came
	/// meanwhile.
	pub async fn draft_mail(&self, owner: Uuid, id: i64, answers: i64, written: Written, asked_for: bool, now: Timestamp) -> eyre::Result<Option<()>> {
		let mut tx = self.store.begin_write().await?;
		let Some(case) = self.gathered(&mut tx, owner, now).await?.into_iter().find(|c| c.id == id) else {
			return Ok(None);
		};
		if case.last().said.id != answers || (!asked_for && !case.due) {
			return Ok(None);
		}
		let auto = !asked_for && db::auto(&mut tx, owner).await?.contains(&(case.gmail, case.topic));
		let (body, needs, state, error) = match written {
			Written::Reply { body, needs } if auto && needs.is_empty() => (body, needs, DraftState::Held, None),
			Written::Reply { body, needs } => (body, needs, DraftState::Proposed, None),
			Written::Nothing(why) => (String::new(), Vec::new(), DraftState::Dismissed, Some(why)),
			Written::Failed(error) => (String::new(), Vec::new(), DraftState::Failed, Some(error)),
		};
		db::supersede(&mut tx, id).await?;
		let new = NewDraft {
			case: id,
			answers,
			body: &self.seal(&mail_draft_aad(id, answers), &body)?,
			needs: &needs,
			state,
			send_at: (state == DraftState::Held).then(|| now + HOLD),
			error: error.as_deref(),
		};
		db::add_draft(&mut tx, &new, now).await?;
		tx.commit().await.wrap_err("committing a draft")?;
		Ok(Some(()))
	}

	/// The person's edit of a draft not sent; one going by itself waits for them now. Answers its case.
	pub async fn edit_mail_draft(&self, owner: Uuid, by: Uuid, id: i64, body: &str) -> eyre::Result<Result<i64, Refused>> {
		if body.trim().is_empty() {
			return Ok(Err(Refused::Invalid("a reply says something".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		let d = match db::draft_of(&mut tx, owner, id).await? {
			None => return Ok(Err(Refused::NotFound)),
			Some(d) if !open(&d) => return Ok(Err(decided(d.state))),
			Some(d) => d,
		};
		let sealed = self.seal(&mail_draft_aad(d.case, d.answers), body)?;
		let edited = Decided {
			body: Some(&sealed),
			by: Some(by),
			..Decided::new(DraftState::Proposed)
		};
		db::decide(&mut tx, id, &edited).await?;
		tx.commit().await.wrap_err("committing a draft's edit")?;
		Ok(Ok(d.case))
	}

	/// Answers its case.
	pub async fn dismiss_mail_draft(&self, owner: Uuid, by: Uuid, id: i64) -> eyre::Result<Result<i64, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let d = match db::draft_of(&mut tx, owner, id).await? {
			None => return Ok(Err(Refused::NotFound)),
			Some(d) if !open(&d) => return Ok(Err(decided(d.state))),
			Some(d) => d,
		};
		let dismissed = Decided {
			by: Some(by),
			..Decided::new(DraftState::Dismissed)
		};
		db::decide(&mut tx, id, &dismissed).await?;
		tx.commit().await.wrap_err("committing a dismissed draft")?;
		Ok(Ok(d.case))
	}

	/// Sends the person's draft `id` as `body`, `by` them, with `files`. Answers its case.
	pub async fn send_mail_draft(&self, owner: Uuid, by: Uuid, id: i64, body: &str, files: Vec<File>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		if body.trim().is_empty() {
			return Ok(Err(Refused::Invalid("a reply says something".to_owned())));
		}
		if files.iter().map(|f| f.bytes.len()).sum::<usize>() > FILES_MAX {
			return Ok(Err(Refused::TooLarge(format!("a reply's files are at most {} MiB in all", FILES_MAX >> 20))));
		}
		self.dispatch(owner, id, Some((by, body)), files, now).await
	}

	/// Sends the drafts held until `now`; how many went.
	pub async fn send_held_mail(&self, now: Timestamp) -> eyre::Result<usize> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the drafts due")?;
		let due = db::held_due(&mut conn, now).await?;
		drop(conn);
		let mut sent = 0;
		for (owner, id) in due {
			match self.dispatch(owner, id, None, Vec::new(), now).await? {
				Ok(_) => sent += 1,
				Err(why) => tracing::info!(draft = id, %why, "a held reply did not go"),
			}
		}
		Ok(sent)
	}

	/// Draft `id` handed to SMTP: `sending` is committed first, so whatever happens it is handed once.
	/// `person`: whose confirmation this is, and the body they sent; none, it goes by itself.
	async fn dispatch(&self, owner: Uuid, id: i64, person: Option<(Uuid, &str)>, files: Vec<File>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let d = match db::draft_of(&mut tx, owner, id).await? {
			None => return Ok(Err(Refused::NotFound)),
			Some(d) if !open(&d) => return Ok(Err(decided(d.state))),
			Some(d) => d,
		};
		let case = self
			.gathered(&mut tx, owner, now)
			.await?
			.into_iter()
			.find(|c| c.id == d.case)
			.ok_or_else(|| eyre::eyre!("draft {id} is of case {}, which is not its owner's", d.case))?;
		// what stops it is kept, whoever asked
		let stopped = |state, why: &str| (state, why.to_owned());
		let messages = case.messages();
		let stop = if case.last().said.id != d.answers {
			Some(stopped(DraftState::Superseded, "another message came since it was written"))
		} else if person.is_some() {
			None
		} else if !db::auto(&mut tx, owner).await?.contains(&(case.gmail, case.topic)) {
			Some(stopped(DraftState::Proposed, "replies on its topic go by themselves no more"))
		} else if !d.needs.is_empty() || !matches!(case.state, State::Sending { .. }) {
			Some(stopped(DraftState::Proposed, "its case is not what it was"))
		} else if rules::asked(messages.iter()) >= MAX_ASKS {
			Some(stopped(DraftState::Proposed, "Google was asked often enough"))
		} else {
			None
		};
		if let Some((state, why)) = stop {
			db::decide(&mut tx, id, &Decided::new(state)).await?;
			tx.commit().await.wrap_err("committing a draft not sent")?;
			return Ok(Err(Refused::Conflict(why)));
		}
		let counterpart = rules::counterpart(messages.iter()).and_then(|m| case.messages.iter().find(|k| k.said.id == m.id));
		let to = match counterpart {
			None => Err("nothing of Google's in it takes a reply".to_owned()),
			Some(m) => rules::recipient(m.authentic, m.from.as_deref(), m.reply_to.as_deref()).map(str::to_owned),
		};
		let to = match to {
			Ok(to) => to,
			Err(why) => {
				let failed = Decided {
					error: Some(&why),
					..Decided::new(DraftState::Failed)
				};
				db::decide(&mut tx, id, &failed).await?;
				tx.commit().await.wrap_err("committing a draft not sent")?;
				return Ok(Err(Refused::Invalid(why)));
			}
		};
		let Some((imap, sealed)) = connections::imap(&mut tx, case.gmail).await? else {
			return Ok(Err(Refused::Unavailable("its gmail has no inbox set to send from".to_owned())));
		};
		let password = self.unseal(&gmail_imap_aad(case.gmail), &sealed)?;
		let last = case.last();
		let subject = counterpart.and_then(|m| m.subject.clone()).unwrap_or_default(); // answered as Google titled it, untitled too
		let mail = Outgoing {
			to,
			subject: if subject.to_lowercase().starts_with("re:") { subject } else { format!("Re: {subject}") },
			body: match person {
				Some((_, body)) => body.to_owned(),
				None => self.unseal(&mail_draft_aad(d.case, d.answers), &d.body)?.to_string(),
			},
			// as Gmail's own: one host of ours in every gmail's would link them
			message_id: format!("{}@mail.gmail.com", Uuid::now_v7().simple()),
			in_reply_to: last.message_id.clone(),
			references: last.refs.iter().chain(&last.message_id).fold(Vec::new(), |mut ids, i| {
				if !ids.contains(i) {
					ids.push(i.clone());
				}
				ids
			}),
			files,
		};
		let body = self.seal(&mail_draft_aad(d.case, d.answers), &mail.body)?;
		let sending = Decided {
			body: Some(&body),
			by: person.map(|(by, _)| by),
			send_at: Some(now),
			message_id: Some(&mail.message_id),
			..Decided::new(DraftState::Sending)
		};
		db::decide(&mut tx, id, &sending).await?;
		tx.commit().await.wrap_err("committing a draft about to be sent")?;

		let account = Account {
			host: &imap.host,
			username: &imap.username,
			password: &password,
		};
		let sent = self.smtp.send(account, &mail).await;
		let mut tx = self.store.begin_write().await?;
		let (after, answer) = match &sent {
			Ok(()) => (
				Decided {
					sent_at: Some(now),
					..Decided::new(DraftState::Sent)
				},
				Ok(d.case),
			),
			Err(SendError::NotSent(why)) => (
				Decided {
					error: Some(why),
					..Decided::new(DraftState::Failed)
				},
				Err(Refused::Unavailable(format!("not sent: {why}"))),
			),
			Err(SendError::Unknown(why)) => (
				Decided {
					error: Some(why),
					..Decided::new(DraftState::Sending)
				},
				Err(Refused::Unavailable(format!("it may have gone out, check the thread: {why}"))),
			),
		};
		db::decide(&mut tx, id, &after).await?;
		tx.commit().await.wrap_err("committing a draft's send")?;
		tracing::info!(draft = id, case = d.case, gmail_id = case.gmail, person = person.is_some(), sent = sent.is_ok(), "a reply to Google");
		Ok(answer)
	}

	/// Tells what turned to a person in anyone's cases, once per message and turn. How many messages
	/// were queued.
	pub async fn tell_mail(&self, locale: Locale, now: Timestamp) -> eyre::Result<usize> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection to tell of mail")?;
		let mut queued = 0;
		for owner in db::owners(&mut conn).await? {
			let all = gmbs_db::gmbs(&mut conn, owner).await?;
			for c in self.gathered(&mut conn, owner, now).await? {
				let last = c.last();
				let at = match &c.draft {
					Some(d) if d.reply.answers == last.said.id => d.created_at,
					_ => last.said.at,
				};
				let turn = match &c.state {
					// each again once mended, under what it then is
					State::NeedsYou {
						why: Why::Unread { error: None } | Why::Stuck { .. },
					} => continue,
					State::NeedsYou { .. } => "needs_you",
					State::ReplyReady => "reply_ready",
					State::Sending { .. } if c.draft.as_ref().is_some_and(|d| d.reply.state == DraftState::Held) => "held",
					State::Sending { .. } | State::ReplyDue | State::Waiting { .. } | State::Done | State::Quiet => continue,
				};
				if now.duration_since(at) > TELL_WITHIN {
					continue;
				}
				let read = c.google().and_then(|m| m.said.reading.as_ref());
				let note = Note::GoogleMail {
					state: c.state.clone(),
					kind: read.map(|r| r.kind),
					gmb: c.gmb.and_then(|id| all.iter().find(|g| g.id == id)).map(|g| g.name.clone()),
					summary: read.map(|r| r.summary.clone()).or_else(|| last.subject.clone()),
					deadline: read.and_then(|r| r.deadline).filter(|d| *d > now),
				};
				let gmbs: Vec<i64> = match c.gmb {
					Some(gmb) => vec![gmb],
					None => all.iter().filter(|g| g.gmail_id == Some(c.gmail)).map(|g| g.id).collect(),
				};
				let event = derived_event_id(&["google_mail", &last.said.id.to_string(), turn]);
				let telling = Telling {
					person: Some(owner),
					gmail: gmbs.is_empty().then_some(c.gmail),
					gmbs,
					tier: match read.map(|r| r.kind) {
						Some(Kind::Suspended | Kind::AccountRestricted | Kind::AccessLost) => Some(Tier::Urgent),
						_ if turn == "held" => Some(Tier::Info),
						_ => None,
					},
					link: Some(format!("/email?case={}", c.id)),
					..Telling::new(Rule::GoogleMail, event, at, note.render(locale))
				};
				let mut tx = crate::store::begin_write(&mut conn).await?;
				queued += self.tell(&mut tx, &telling, now).await?;
				tx.commit().await.wrap_err("committing a note")?;
			}
		}
		Ok(queued)
	}

	async fn mail_text(&self, conn: &mut SqliteConnection, gmail: i64, message: &str, mail: i64) -> eyre::Result<Zeroizing<String>> {
		let sealed = db::text(conn, mail).await?;
		self.unseal(&gmail_mail_aad(gmail, message), &sealed)
	}

	/// The person's cases, each message's GBP and each case's state known of them all.
	async fn gathered(&self, conn: &mut SqliteConnection, owner: Uuid, now: Timestamp) -> eyre::Result<Vec<Case>> {
		let rows = db::cases(conn, owner).await?;
		let mails = db::mails(conn, owner).await?;
		let mut drafts: HashMap<i64, Draft> = HashMap::new();
		for d in db::drafts(conn, owner).await? {
			drafts.insert(d.case, self.draft(d)?);
		}
		let stuck = db::stuck(conn, owner).await?;
		// the latest mail to name a GBP beside a link says whose the link is
		let known: HashMap<String, i64> = mails.iter().filter_map(|m| Some((m.listing.clone()?, m.gmb_id?))).collect();
		let mut held: HashMap<i64, Vec<Kept>> = HashMap::new();
		for m in mails {
			held.entry(m.case_id).or_default().push(kept(m, &known)?);
		}
		let mut cases = Vec::new();
		for r in rows {
			let messages = held.remove(&r.id).ok_or_else(|| eyre::eyre!("case {} holds no message", r.id))?;
			let gmb = r.gmb_id.or_else(|| messages.iter().rev().find_map(|m| m.gmb));
			let read = messages.iter().rev().filter_map(|m| m.said.reading.as_ref()).map(|r| r.topic).find(|t| *t != Topic::Other);
			let topic = r.topic()?.or(read).unwrap_or(Topic::Other); // nothing read yet, or only of none
			cases.push((
				r.done_mail,
				Case {
					id: r.id,
					gmail: r.gmail_id,
					gmb,
					topic,
					numbers: r.numbers().clone(),
					state: State::Quiet,
					due: false,
					messages,
					draft: drafts.remove(&r.id),
				},
			));
		}
		let mut elsewhere: HashMap<i64, Vec<(i64, Timestamp, Topic, Kind)>> = HashMap::new();
		for (_, c) in &cases {
			for m in &c.messages {
				if let (Some(gmb), Some(r)) = (m.gmb.or(c.gmb), &m.said.reading) {
					elsewhere.entry(gmb).or_default().push((c.id, m.said.at, r.topic, r.kind));
				}
			}
		}
		let state = |done: Option<i64>, c: &Case, asked_elsewhere: bool, stuck: Option<&str>| {
			let messages = c.messages();
			let others: Vec<(Timestamp, Topic, Kind)> = c
				.gmb
				.and_then(|g| elsewhere.get(&g))
				.into_iter()
				.flatten()
				.filter(|(case, ..)| *case != c.id)
				.map(|(_, at, topic, kind)| (*at, *topic, *kind))
				.collect();
			rules::state(
				&rules::Case {
					topic: c.topic,
					messages: &messages,
					draft: c.draft.as_ref().map(|d| &d.reply),
					done,
					elsewhere: &others,
					asked_elsewhere,
					stuck,
				},
				now,
			)
		};
		// a notice's state stands on whether a conversation asks for its GBP's verification
		let asking: HashMap<i64, i64> = cases
			.iter()
			.filter(|(_, c)| c.topic == Topic::Verification && c.messages.iter().any(|m| !m.said.noreply))
			.filter(|(done, c)| !matches!(state(*done, c, false, None), State::Done | State::Quiet))
			.filter_map(|(_, c)| Some((c.gmb?, c.id)))
			.collect();
		Ok(cases
			.iter()
			.map(|(done, c)| {
				let asked = c.gmb.and_then(|g| asking.get(&g)).is_some_and(|id| *id != c.id);
				let open = c.draft.as_ref().is_some_and(|d| d.reply.answers == c.last().said.id);
				Case {
					state: state(*done, c, asked, stuck.as_deref()),
					due: !open && state(*done, c, asked, None) == State::ReplyDue,
					..c.clone()
				}
			})
			.collect())
	}

	fn draft(&self, d: DraftRow) -> eyre::Result<Draft> {
		Ok(Draft {
			id: d.id,
			body: self.unseal(&mail_draft_aad(d.case, d.answers), &d.body)?.to_string(),
			created_at: d.created_at,
			reply: Reply {
				state: d.state,
				answers: d.answers,
				needs: d.needs,
				send_at: d.send_at,
				sent_at: d.sent_at,
				error: d.error,
			},
		})
	}
}

fn kept(m: MailRow, known: &HashMap<String, i64>) -> eyre::Result<Kept> {
	let read_by = m.read_by()?;
	let reading = m.reading().cloned();
	let seen = reading.as_ref().and_then(|r| r.gmb);
	let linked = m.listing.as_ref().and_then(|l| known.get(l).copied());
	Ok(Kept {
		gmb: match read_by {
			Some(ReadBy::Person) => seen,
			_ => m.gmb_id.or(linked).or(seen),
		},
		said: Message {
			id: m.id,
			at: m.at()?,
			sent: m.sent,
			noreply: match m.sent {
				true => unheard(true, m.to()),
				false => m.from_address.as_deref().is_none_or(|f| Sender::of(f) != Sender::Support),
			},
			repliable: m.repliable,
			reading,
			read_error: m.read_error.clone().filter(|_| m.attempts >= MAX_READS),
		},
		gmail: m.gmail_id,
		read_by,
		to: m.to().to_vec(),
		refs: m.refs().to_vec(),
		message: m.message,
		thread: m.thread,
		from: m.from_address,
		reply_to: m.reply_to,
		subject: m.subject,
		message_id: m.message_id,
		authentic: m.authentic,
		listing: m.listing,
	})
}

/// Whether what was sent went only where nobody reads it.
fn unheard(sent: bool, to: &[String]) -> bool {
	sent && to.iter().filter(|a| Sender::of(a) != Sender::Other).all(|a| Sender::of(a) != Sender::Support)
}

/// Whether a draft may still be edited, dismissed or sent.
fn open(d: &DraftRow) -> bool {
	matches!(d.state, DraftState::Proposed | DraftState::Held | DraftState::Failed)
}

fn decided(state: DraftState) -> Refused {
	let state: &str = state.into();
	Refused::Conflict(format!("the draft is {state} already"))
}
