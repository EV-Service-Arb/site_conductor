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
	collections::{BTreeMap, BTreeSet, HashMap},
	pin::Pin,
};

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Button, Call, Rule, Tier};
use sa_core::{
	gbp::State as Stands,
	mail::{
		self as rules, About, Draft as Reply, DraftState, Goal, HOLD, Halt, Holds, Joining, Kind, MAX_READS, Message, Pursuit, READ_BACK, Reading, Ruled, Said, Sender, Settled, Spoken,
		State, Topic, Why,
	},
	notify::{Locale, Note, Rendered, one_line},
};
use sqlx::SqliteConnection;
use strum::VariantArray;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
	Panel, Refused,
	alerts::{Body, Telling, derived_event_id},
	inbox::{Account, Mail},
	seal::{gmail_imap_aad, gmail_mail_aad, mail_draft_aad},
	store::{
		connections, documents,
		gmbs::{self as gmbs_db, Gmb},
		mail::{self as db, CaseRow, Decided, DraftRow, MailRow, NewDraft, NewMail},
	},
};
pub use crate::{smtp::Gmail, store::mail::ReadBy};

/// The ports a gmail's mail is read and sent on.
pub const PORTS: [u16; 2] = [crate::imap::PORT, crate::smtp::PORT];
/// Topics a person may have answered automatically.
pub const AUTO: [Topic; 2] = [Topic::Verification, Topic::Reviews];
/// A reply's files, in all.
pub const FILES_MAX: usize = 18 << 20;
/// Unread mail the model is handed in one pass, per person.
const READS: usize = 20;
const SUMMARY_MAX: usize = 160;
/// A turn older than this is told of no more: the marks of what was told are kept a little longer.
const TELL_WITHIN: SignedDuration = SignedDuration::from_hours(24 * 6);
/// A date Google set is told of again this long before.
const DEADLINE_NEAR: SignedDuration = SignedDuration::from_hours(24);
/// What a held reply's note does: the reply waits for its person.
const STOP: &str = "stop_mail_draft";
/// What waits on a person for this long is told of again.
const REMIND_AFTER: SignedDuration = SignedDuration::from_hours(48);
/// A held reply that did not go for this long past its moment goes by itself no more.
const LATE: SignedDuration = SignedDuration::from_hours(1);
/// Mail read by nobody for this long is told of.
const UNREAD_FOR: SignedDuration = SignedDuration::from_hours(1);
/// What we wrote, as the reader of what answers it is given it.
const BEFORE_MAX: usize = 500;

pub type Sending<'a> = Pin<Box<dyn Future<Output = Result<(), SendError>> + Send + 'a>>;

/// A message's text, or why it does not open.
pub type Text = Result<Zeroizing<String>, String>;

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
	/// By a person's word, or Google's own words; else as its reader saw it, or its messages tell several.
	pub gmb_certain: bool,
	/// The other GBPs its messages are about.
	pub also: Vec<i64>,
	pub topic: Topic,
	pub by_hand: ByHand,
	pub numbers: BTreeSet<String>,
	/// The cases a person made part of it.
	pub joined: Vec<i64>,
	pub state: State,
	/// Why none of its owner's mail is read or answered now.
	pub stuck: Option<String>,
	/// A reply is due and none is written, whatever it waits on.
	pub due: bool,
	/// From when a reply lacking nothing may go by itself, or why none does.
	pub press: Result<Timestamp, Halt>,
	/// When its inbox was last read, as the account it has now, since the conductor started.
	read: Option<Timestamp>,
	/// Oldest first.
	pub messages: Vec<Kept>,
	/// The latest.
	pub draft: Option<Draft>,
	/// What it asked of its person that they gave: the item, the document.
	given: Vec<(String, Option<i64>)>,
}
impl Case {
	/// What its state is about: its last message somebody reads.
	pub fn last(&self) -> &Kept {
		let heard = self.messages.iter().rev().find(|m| m.said.heard());
		heard.or(self.messages.last()).expect("a case is opened by its first message")
	}

	/// Google's last word in it.
	pub fn google(&self) -> Option<&Kept> {
		let mut said = self.messages.iter().rev().filter(|m| !m.said.sent);
		said.clone().find(|m| m.said.heard()).or_else(|| said.next())
	}

	/// What it wants of its person: what its reply lacks, else what Google asks.
	pub fn wanted(&self) -> Vec<Wanted> {
		let lacks = self.draft.as_ref().filter(|d| d.reply.answers == self.last().said.id).map(|d| d.reply.needs.as_slice());
		let asks = || Some(self.google()?.said.reading.as_ref()?.asks.as_slice());
		let items = lacks.filter(|l| !l.is_empty()).or_else(asks);
		let wanted = |item: &String| {
			let given = self.given.iter().find(|(i, _)| i == item);
			Wanted {
				item: item.clone(),
				given: given.is_some(),
				document: given.and_then(|(_, d)| *d),
			}
		};
		items.into_iter().flatten().map(wanted).collect()
	}

	/// Ours in it since Google last gave something new.
	pub fn asked(&self) -> usize {
		rules::asked(&self.messages())
	}

	/// Whether it, or a message of its, is about GBP `gmb`.
	pub fn of(&self, gmb: i64) -> bool {
		self.gmb == Some(gmb) || self.also.contains(&gmb)
	}

	/// Whether `id` is its, or of what is part of it.
	fn is(&self, id: i64) -> bool {
		self.id == id || self.joined.contains(&id)
	}

	fn messages(&self) -> Vec<Message> {
		self.messages.iter().map(|m| m.said.clone()).collect()
	}
}

/// What of a case a person set over its mail.
#[derive(Clone, Copy, Debug, Eq, PartialEq, schemars::JsonSchema, serde::Serialize)]
#[schemars(rename = "MailByHand")]
pub struct ByHand {
	pub gmb: bool,
	pub topic: bool,
}

/// A message kept; its text apart ([`Panel::mail_case`]).
#[derive(Clone, Debug)]
pub struct Kept {
	pub said: Message,
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
	/// The GBP it is about: what a person said, else what it names, what its link is known by, what its reader saw.
	pub gmb: Option<i64>,
	/// Not as its reader alone saw it.
	gmb_certain: bool,
	pub read_by: Option<ReadBy>,
	/// Gmail has it in Spam.
	pub spam: bool,
	/// Ours: who sent it.
	pub sent_by: Option<SentBy>,
}

/// A goal asked of Google for a GBP.
pub struct Pursued {
	pub gmail: i64,
	/// `None`: not known.
	pub gmb: Option<i64>,
	pub pursuit: Pursuit,
	/// Each ask's.
	pub texts: Vec<Text>,
}

/// What a case wants of its person.
#[derive(Clone, Debug, schemars::JsonSchema, serde::Serialize)]
#[schemars(rename = "MailWanted")]
pub struct Wanted {
	pub item: String,
	pub given: bool,
	/// The GBP's document it was given by.
	pub document: Option<i64>,
}

/// Who sent a message of ours.
#[derive(Clone, Debug, Eq, PartialEq, schemars::JsonSchema, serde::Serialize)]
#[serde(tag = "is", rename_all = "snake_case")]
#[schemars(rename = "MailSentBy")]
pub enum SentBy {
	/// It went by itself.
	Itself,
	/// A person, from here.
	Person { name: String },
	/// In Gmail itself.
	Gmail,
}

#[derive(Clone, Debug)]
pub struct Draft {
	pub id: i64,
	pub reply: Reply,
	/// `None`: it does not open, and its reply's error says why.
	pub body: Option<String>,
}

/// What a person sends of a draft.
pub struct Sent<'a> {
	pub body: &'a str,
	pub files: Vec<File>,
	/// Its GBP's documents, after them.
	pub documents: &'a [i64],
	/// With no file, whatever the draft lacks.
	pub as_is: bool,
}

/// A message the model is to read.
pub struct Unread {
	pub mail: i64,
	pub from: String,
	pub subject: Option<String>,
	pub at: Timestamp,
	pub text: Text,
	/// The gmail's GBPs, then its owner's others.
	pub gmbs: Vec<rules::Named>,
	/// What was said before it in its case: when, whether by us, the gist.
	pub before: Vec<(Timestamp, bool, String)>,
}

/// What a case's writer works from.
pub struct Brief {
	pub case: Case,
	/// Each message's text, in the case's order.
	pub texts: Vec<Text>,
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
	/// The case it is part of.
	pub join: Option<Option<i64>>,
}

/// What goes by itself.
pub struct Auto {
	/// Whether anything does here: production's alone.
	pub by_itself: bool,
	/// The GBPs' topics switched on.
	pub on: Vec<(i64, Topic)>,
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
		if db::kept(tx, gmail, &message, m.spam).await? {
			return Ok(());
		}
		let subject = m.subject.as_deref().unwrap_or_default(); // none: nothing a subject says
		let answers = m.refs.iter().any(|r| cases.iter().any(|c| c.message_ids.contains(r)));
		let ruled = match m.sent {
			true if m.to.iter().any(|a| Sender::of(a) != Sender::Other) => None,
			true => return Ok(()),
			false => match rules::ruled(m.from.as_deref(), &m.counterparts.join(", "), m.authentic, subject, answers) {
				Ruled::Not => return Ok(()),
				Ruled::Reads(kind) => Some(kind),
				Ruled::Model => None,
			},
		};
		let forged = ruled == Some(Kind::Impersonation);
		let sender = m.from.as_deref().map_or(Sender::Other, Sender::of);
		let thread = format!("{:x}", m.thread);
		let numbers = rules::numbers(subject);
		let alone = !m.sent && sender != Sender::Support && !(answers && sender == Sender::Other);
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
			repliable: !m.sent && m.authentic && m.whole && rules::repliable(sender, &text),
			gmb: match (named, forged) {
				([one], false) => Some(*one),
				_ => None,
			},
			listing: listing.as_deref(),
			spam: m.spam,
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

	/// What the person asked of Google, by GBP, newest first; of `gmb` alone, if one.
	pub async fn mail_stats(&self, owner: Uuid, gmb: Option<i64>, now: Timestamp) -> eyre::Result<Vec<Pursued>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's asks")?;
		let cases = self.gathered(&mut conn, owner, now).await?;
		let moves = db::moves(&mut conn, owner).await?;
		// a case of no known GBP is its own
		let of = |c: &Case| c.gmb.map_or(Err(c.id), Ok);
		let mut asked = Vec::new();
		for &goal in Goal::VARIANTS {
			let mut spoken: BTreeMap<(i64, Result<i64, i64>), Vec<Spoken>> = BTreeMap::new();
			for c in cases.iter().filter(|c| c.topic == goal.topic() && gmb.is_none_or(|g| c.gmb == Some(g))) {
				let said = c.messages.iter().filter(|m| m.said.heard()).filter_map(|m| {
					let said = m.said.reading.as_ref().map(|r| r.kind);
					(m.said.sent || said.is_some()).then_some(Spoken {
						at: m.said.at,
						message: Some(m.said.id),
						case: Some(c.id),
						said,
					})
				});
				spoken.entry((c.gmail, of(c))).or_default().extend(said);
			}
			for ((gmail, of), mut spoken) in spoken {
				let verified = moves.iter().filter(|(g, .., to)| of == Ok(*g) && *to == Stands::Verified);
				spoken.extend(verified.map(|(_, at, ..)| Spoken {
					at: *at,
					message: None,
					case: None,
					said: Some(Kind::Verified),
				}));
				spoken.sort_by_key(|s| (s.at, s.message));
				for pursuit in rules::pursuits(goal, &spoken) {
					let mut texts = Vec::new();
					for (id, ..) in &pursuit.asks {
						let message = cases.iter().flat_map(|c| &c.messages).find(|m| m.said.id == *id).expect("one of theirs");
						texts.push(self.mail_text(&mut conn, gmail, &message.message, *id).await?);
					}
					asked.push(Pursued {
						gmail,
						gmb: of.ok(),
						pursuit,
						texts,
					});
				}
			}
		}
		asked.sort_by_key(|a| std::cmp::Reverse(a.pursuit.asks.last().map(|(_, at, _)| *at)));
		Ok(asked)
	}

	/// The person's case `id`, with what its writer works from.
	pub async fn mail_case(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<Brief, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a case")?;
		let cases = self.gathered(&mut conn, owner, now).await?;
		let Some(case) = cases.iter().find(|c| c.is(id)).cloned() else {
			return Ok(Err(Refused::NotFound));
		};
		let mut record: Vec<(Timestamp, Reading)> = cases
			.iter()
			.filter(|c| c.id != case.id)
			.flat_map(|c| {
				c.messages
					.iter()
					.filter(|m| m.said.heard())
					.filter_map(|m| (case.gmb.is_some() && m.gmb.or(c.gmb) == case.gmb).then(|| m.said.reading.clone().map(|r| (m.said.at, r)))?)
			})
			.collect();
		record.sort_by_key(|(at, _)| *at);
		let mut texts = Vec::new();
		for m in &case.messages {
			texts.push(self.mail_text(&mut conn, case.gmail, &m.message, m.said.id).await?);
		}
		let gmb = match case.gmb {
			Some(gmb) => gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await?,
			None => None,
		};
		Ok(Ok(Brief { case, texts, gmb, record }))
	}

	/// The person's case `id` corrected, made part of another, or marked done at its last message.
	pub async fn edit_mail_case(&self, owner: Uuid, by: Uuid, id: i64, e: CaseEdit, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let cases = self.gathered(&mut tx, owner, now).await?;
		let Some(case) = cases.iter().find(|c| c.is(id)) else {
			return Ok(Err(Refused::NotFound));
		};
		if let Some(Some(gmb)) = e.gmb
			&& gmbs_db::gmb_of(&mut tx, Some(owner), gmb).await?.is_none()
		{
			return Ok(Err(Refused::NotFound));
		}
		db::edit_case(&mut tx, case.id, e.gmb, e.topic).await?;
		db::unhold(&mut tx, case.id).await?;
		if let Some(done) = e.done {
			let at = case.messages.last().expect("a case has a message").said.id;
			db::set_done(&mut tx, case.id, done.then_some((at, by))).await?;
		}
		match e.join {
			None => {}
			Some(None) if id == case.id => return Ok(Err(Refused::Invalid("it is part of no other".to_owned()))),
			Some(None) => {
				db::supersede(&mut tx, case.id).await?;
				db::join(&mut tx, id, None).await?;
			}
			Some(Some(to)) => {
				let Some(other) = cases.iter().find(|c| c.is(to)) else {
					return Ok(Err(Refused::NotFound));
				};
				let unproven = |c: &Case| c.messages.iter().any(|m| !m.said.sent && !m.authentic);
				if other.id == case.id || other.gmail != case.gmail || unproven(case) || unproven(other) {
					return Ok(Err(Refused::Invalid("only two of one gmail's, both proved Google's, are one".to_owned())));
				}
				for c in [case, other] {
					db::supersede(&mut tx, c.id).await?;
				}
				db::join(&mut tx, case.id, Some(other.id)).await?;
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
		reading.summary = e.summary.map_or(reading.summary, |s| one_line(&s, SUMMARY_MAX));
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
		db::unhold(&mut tx, case).await?;
		tx.commit().await.wrap_err("committing a corrected reading")?;
		Ok(Ok(case))
	}

	/// The person's replies that go by themselves.
	pub async fn mail_auto(&self, owner: Uuid) -> eyre::Result<Auto> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for automatic replies")?;
		Ok(Auto {
			by_itself: self.mail_by_itself,
			on: db::auto(&mut conn, owner).await?,
		})
	}

	/// Each of the person's GBPs `gmbs` switched, or none.
	pub async fn set_mail_auto(&self, owner: Uuid, gmbs: &[i64], topic: Topic, on: bool) -> eyre::Result<Result<(), Refused>> {
		if !AUTO.contains(&topic) {
			let topic: &str = topic.into();
			return Ok(Err(Refused::Invalid(format!("replies on {topic} never go by themselves"))));
		}
		let mut tx = self.store.begin_write().await?;
		let live = db::live(&mut tx, owner).await?;
		if !gmbs.iter().all(|g| live.contains(g)) {
			return Ok(Err(Refused::NotFound));
		}
		for &gmb in gmbs {
			db::set_auto(&mut tx, gmb, topic, on).await?;
		}
		tx.commit().await.wrap_err("committing automatic replies")?;
		Ok(Ok(()))
	}

	/// Whoever has mail kept.
	pub async fn mail_owners(&self) -> eyre::Result<Vec<Uuid>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for whose mail is kept")?;
		db::owners(&mut conn).await
	}

	/// The person's mail the model is to read, the oldest first, no more than a pass reads.
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
		due.sort_by_key(|(c, at)| c.messages[*at].said.at);
		let mut unread = Vec::new();
		for (c, at) in due.into_iter().take(READS) {
			let m = &c.messages[at];
			let (own, others) = crate::store::inbox::gmbs(&mut conn, c.gmail).await?;
			let mut before = Vec::new();
			for b in &c.messages[..at] {
				match &b.said.reading {
					Some(r) => {
						let topic: &str = r.topic.into();
						before.push((b.said.at, false, format!("{topic}: {}", r.summary)));
					}
					None if b.said.sent => {
						let text = self.mail_text(&mut conn, c.gmail, &b.message, b.said.id).await?;
						let said = text.map_or_else(|why| format!("(not known: {why})"), |t| one_line(&t, BEFORE_MAX));
						before.push((b.said.at, true, said));
					}
					None => {}
				}
			}
			unread.push(Unread {
				mail: m.said.id,
				from: m.from.clone().ok_or_else(|| eyre::eyre!("message {} is authentic and names no sender", m.said.id))?,
				subject: m.subject.clone(),
				at: m.said.at,
				text: self.mail_text(&mut conn, c.gmail, &m.message, m.said.id).await?,
				gmbs: own.into_iter().chain(others).collect(),
				before,
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

	/// A reply to the person's case as it was written of, `of`; it goes by itself only when none asked for it, the
	/// case may be pressed and it lacks nothing. `None`: the case changed
	/// meanwhile, or a reply to its last message was handed to SMTP.
	pub async fn draft_mail(&self, owner: Uuid, of: &Case, written: Written, asked_for: bool, now: Timestamp) -> eyre::Result<Option<()>> {
		let (id, answers) = (of.id, of.last().said.id);
		let mut tx = self.store.begin_write().await?;
		let Some(case) = self.gathered(&mut tx, owner, now).await?.into_iter().find(|c| c.id == id) else {
			return Ok(None);
		};
		let written_of = |c: &Case| (c.last().said.id, c.last().said.reading.clone(), c.gmb, c.topic);
		if written_of(&case) != written_of(of) || (!asked_for && !case.due) || db::handed(&mut tx, answers).await? {
			return Ok(None);
		}
		let held = case.press.ok().filter(|_| !asked_for).map(|from| from.max(now + HOLD));
		let (body, needs, state, error) = match written {
			Written::Reply { body, needs } if held.is_some() && needs.is_empty() => (body, needs, DraftState::Held, None),
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
			send_at: held.filter(|_| state == DraftState::Held),
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

	/// A button of a note of the conductor's own pressed, as a status: of what goes by itself, stopped.
	pub(crate) async fn press_own(&self, owner: Uuid, by: Uuid, call: &Call) -> eyre::Result<u16> {
		eyre::ensure!(call.operation == STOP, "{} is no operation of a note of the conductor's own", call.operation);
		let id = call.path.get("id").and_then(serde_json::Value::as_i64).ok_or_else(|| eyre::eyre!("a button names no draft"))?;
		let mut tx = self.store.begin_write().await?;
		match db::draft_of(&mut tx, owner, id).await? {
			None => return Ok(404),
			Some(d) if d.state != DraftState::Held => return Ok(409),
			Some(_) => {}
		}
		let stopped = Decided {
			by: Some(by),
			..Decided::new(DraftState::Proposed)
		};
		db::decide(&mut tx, id, &stopped).await?;
		tx.commit().await.wrap_err("committing a draft held back")?;
		Ok(201)
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

	/// The person's dismissed draft proposed again. Answers its case.
	pub async fn restore_mail_draft(&self, owner: Uuid, by: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(d) = db::draft_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let case = self.case_of(&mut tx, owner, &d, now).await?;
		let latest = case.draft.as_ref().is_some_and(|l| l.id == id) && case.last().said.id == d.answers;
		if d.state != DraftState::Dismissed || d.error.is_some() || !latest {
			return Ok(Err(Refused::Conflict("another message came, or another reply was written".to_owned())));
		}
		let restored = Decided {
			by: Some(by),
			..Decided::new(DraftState::Proposed)
		};
		db::decide(&mut tx, id, &restored).await?;
		tx.commit().await.wrap_err("committing a draft brought back")?;
		Ok(Ok(d.case))
	}

	/// An item case `id` wants of its person, given (by a document of its GBP's) or not.
	#[expect(clippy::too_many_arguments, reason = "who, whose, which, what")]
	pub async fn give_mail(&self, owner: Uuid, by: Uuid, id: i64, item: &str, given: bool, document: Option<i64>, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let cases = self.gathered(&mut tx, owner, now).await?;
		let Some(case) = cases.iter().find(|c| c.is(id)) else {
			return Ok(Err(Refused::NotFound));
		};
		if given && !case.wanted().iter().any(|w| w.item == item) {
			return Ok(Err(Refused::Conflict("it asks that no more".to_owned())));
		}
		if let Some(document) = document {
			let Some(gmb) = case.gmb else {
				return Ok(Err(Refused::Invalid("a document is of a GBP: which one it is about is not known".to_owned())));
			};
			if !documents::of_gmb(&mut tx, gmb).await?.iter().any(|d| d.id == document) {
				return Ok(Err(Refused::NotFound));
			}
		}
		db::give(&mut tx, case.id, item, given, document, by, now).await?;
		tx.commit().await.wrap_err("committing what a person gave")?;
		Ok(Ok(()))
	}

	/// Each message kept of the person's: the GBPs it is of, by its gmail and Gmail's id of it, and who sent it.
	pub(crate) async fn filed_mail(&self, conn: &mut SqliteConnection, owner: Uuid, now: Timestamp) -> eyre::Result<gmbs_db::Filed> {
		let cases = self.gathered(conn, owner, now).await?;
		let of = |c: Case| {
			let (gmail, gmb) = (c.gmail, c.gmb);
			c.messages.into_iter().map(move |m| ((gmail, m.message), (m.gmb.or(gmb), m.sent_by)))
		};
		Ok(cases.into_iter().flat_map(of).collect())
	}

	/// Sends the person's draft `id` as `sent`, `by` them. Answers its case.
	pub async fn send_mail_draft(&self, owner: Uuid, by: Uuid, id: i64, mut sent: Sent<'_>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		if sent.body.trim().is_empty() {
			return Ok(Err(Refused::Invalid("a reply says something".to_owned())));
		}
		if !sent.documents.is_empty() {
			let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a reply's documents")?;
			let Some(d) = db::draft_of(&mut conn, owner, id).await? else {
				return Ok(Err(Refused::NotFound));
			};
			let Some(gmb) = self.case_of(&mut conn, owner, &d, now).await?.gmb else {
				return Ok(Err(Refused::NotFound));
			};
			drop(conn);
			for &document in sent.documents {
				match self.document(owner, gmb, document).await? {
					Ok(file) => sent.files.push(file),
					Err(r) => return Ok(Err(r)),
				}
			}
		}
		if sent.files.iter().map(|f| f.bytes.len()).sum::<usize>() > FILES_MAX {
			return Ok(Err(Refused::TooLarge(format!("a reply's files are at most {} MiB in all", FILES_MAX >> 20))));
		}
		self.dispatch(owner, id, Some((by, sent)), now).await
	}

	/// Sends the drafts held until `now`, where replies go by themselves; how many went.
	pub async fn send_held_mail(&self, now: Timestamp) -> eyre::Result<usize> {
		if !self.mail_by_itself {
			return Ok(0);
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the drafts due")?;
		let due = db::held_due(&mut conn, now).await?;
		drop(conn);
		let (mut sent, mut failed) = (0, None);
		for (owner, id) in due {
			match self.dispatch(owner, id, None, now).await {
				Ok(Ok(_)) => sent += 1,
				Ok(Err(why)) => tracing::info!(draft = id, %why, "a held reply did not go"),
				Err(e) => failed = failed.or(Some(e.wrap_err(format!("held draft {id}")))),
			}
		}
		failed.map_or(Ok(sent), Err)
	}

	/// Draft `id` handed to SMTP: `sending` is committed first, so whatever happens it is handed once.
	/// `person`: whose confirmation this is, and what they sent; none, it goes by itself.
	async fn dispatch(&self, owner: Uuid, id: i64, person: Option<(Uuid, Sent<'_>)>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let d = match db::draft_of(&mut tx, owner, id).await? {
			None => return Ok(Err(Refused::NotFound)),
			Some(d) if !open(&d) => return Ok(Err(decided(d.state))),
			Some(d) => d,
		};
		let case = self.case_of(&mut tx, owner, &d, now).await?;
		if let Some((_, sent)) = &person
			&& sent.files.is_empty()
			&& !sent.as_is
		{
			let lacks: Vec<&str> = case
				.wanted()
				.iter()
				.filter(|w| !w.given)
				.filter_map(|w| d.needs.iter().find(|n| **n == w.item))
				.map(String::as_str)
				.collect();
			if !lacks.is_empty() {
				return Ok(Err(Refused::Conflict(format!("it lacks {}: attach it, or say it goes as it is", lacks.join("; ")))));
			}
		}
		// what stops it is kept, whoever asked
		let stopped = |state, why: &str| (state, why.to_owned());
		let send_at = d.send_at.filter(|_| d.state == DraftState::Held);
		let stop = if case.last().said.id != d.answers {
			Some(stopped(DraftState::Superseded, "another message came since it was written"))
		} else if db::handed(&mut tx, d.answers).await? {
			Some(stopped(DraftState::Superseded, "another reply to it was handed over already"))
		} else if person.is_some() {
			None
		} else if send_at.is_none_or(|at| now.duration_since(at) > LATE) {
			Some(stopped(DraftState::Proposed, "its moment is past"))
		} else if !matches!(case.state, State::Sending { .. }) || case.press.is_err() {
			Some(stopped(DraftState::Proposed, "its case is not what it was"))
		} else {
			None
		};
		if let Some((state, why)) = stop {
			db::decide(&mut tx, id, &Decided::new(state)).await?;
			tx.commit().await.wrap_err("committing a draft not sent")?;
			return Ok(Err(Refused::Conflict(why)));
		}
		// Google may have answered since the inbox was read
		if person.is_none() && (case.read < send_at || case.press.is_ok_and(|from| from > now)) {
			return Ok(Err(Refused::Unavailable("its inbox is not read past its moment yet".to_owned())));
		}
		let messages = case.messages();
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
		let by = person.as_ref().map(|(by, _)| *by);
		let subject = counterpart.and_then(|m| m.subject.clone()).unwrap_or_default(); // answered as Google titled it, untitled too
		let mail = Outgoing {
			to,
			subject: if subject.to_lowercase().starts_with("re:") { subject } else { format!("Re: {subject}") },
			body: match &person {
				Some((_, sent)) => sent.body.to_owned(),
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
			files: person.map(|(_, sent)| sent.files).into_iter().flatten().collect(),
		};
		let body = self.seal(&mail_draft_aad(d.case, d.answers), &mail.body)?;
		let sending = Decided {
			body: Some(&body),
			by,
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
		tracing::info!(draft = id, case = d.case, gmail_id = case.gmail, person = by.is_some(), sent = sent.is_ok(), "a reply to Google");
		Ok(answer)
	}

	/// Tells what turned to a person in anyone's cases, once per message and turn. How many messages
	/// were queued.
	pub async fn tell_mail(&self, locale: Locale, now: Timestamp) -> eyre::Result<usize> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection to tell of mail")?;
		let (mut queued, mut failed) = (0, None);
		for owner in db::owners(&mut conn).await? {
			match self.tell_mail_of(&mut conn, owner, locale, now).await {
				Ok(n) => queued += n,
				Err(e) => failed = failed.or(Some(e.wrap_err(format!("user {owner}")))),
			}
		}
		failed.map_or(Ok(queued), Err)
	}

	async fn tell_mail_of(&self, conn: &mut SqliteConnection, owner: Uuid, locale: Locale, now: Timestamp) -> eyre::Result<usize> {
		let mut queued = 0;
		let all = gmbs_db::gmbs(conn, owner).await?;
		let cases = self.gathered(conn, owner, now).await?;
		// one cause of them all: told of the first it keeps
		let unread = |c: &&Case| {
			matches!(
				c.state,
				State::NeedsYou {
					why: Why::Unread { error: None } | Why::Stuck { .. }
				}
			)
		};
		let within = |c: &&Case| (UNREAD_FOR..=TELL_WITHIN).contains(&now.duration_since(c.last().said.at));
		let waits = cases.iter().filter(unread).filter(within).min_by_key(|c| c.last().said.at).map(|c| c.id);
		for c in &cases {
			let last = c.last();
			let at = match &c.draft {
				Some(d) if d.reply.answers == last.said.id => d.reply.written_at,
				_ => last.said.at,
			};
			let google = c.google();
			let read = google.and_then(|m| m.said.reading.as_ref());
			let deadline = read.and_then(|r| r.deadline).filter(|d| *d > now);
			let (turn, at) = match &c.state {
				// each again once mended, under what it then is
				State::NeedsYou {
					why: Why::Unread { error: None } | Why::Stuck { .. },
				} => match waits == Some(c.id) {
					true => ("waits", at),
					false => continue,
				},
				State::Done { by: Settled::Here } if read.is_some_and(|r| !matches!(r.kind, Kind::Closed | Kind::DuplicateTicket)) => ("done", at),
				State::NeedsYou { .. } | State::ReplyReady if deadline.is_some_and(|d| d.duration_since(now) <= DEADLINE_NEAR) => ("deadline", now),
				State::NeedsYou { why: Why::Unanswered { since } } => ("needs_you", *since),
				State::NeedsYou { .. } => ("needs_you", at),
				State::ReplyReady => ("reply_ready", at),
				State::Sending { .. } if c.draft.as_ref().is_some_and(|d| d.reply.state == DraftState::Held) => ("held", at),
				State::Waiting { .. } if last.said.reading.as_ref().is_some_and(|r| r.kind == Kind::Routed) => ("routed", at),
				State::Sending { .. } | State::ReplyDue | State::Waiting { .. } | State::Done { .. } | State::Quiet => continue,
			};
			if now.duration_since(at) > TELL_WITHIN {
				continue;
			}
			let again = matches!(turn, "needs_you" | "reply_ready") && now.duration_since(at) >= REMIND_AFTER;
			let (turn, at) = if again { ("again", at + REMIND_AFTER) } else { (turn, at) };
			// what goes by itself unless it is stopped
			let reply = c.draft.as_ref().filter(|_| turn == "held").and_then(|d| d.body.as_deref()).map(|b| one_line(b, BEFORE_MAX));
			let note = |summary, reply| Note::GoogleMail {
				again,
				reply,
				state: c.state.clone(),
				kind: read.map(|r| r.kind),
				gmb: c.gmb.and_then(|id| all.iter().find(|g| g.id == id)).map(|g| g.name.clone()),
				summary,
				deadline,
				spam: last.spam,
			};
			let gmbs: Vec<i64> = match c.gmb {
				Some(gmb) => vec![gmb],
				None => all.iter().filter(|g| g.gmail_id == Some(c.gmail)).map(|g| g.id).collect(),
			};
			let gmbs: Vec<i64> = gmbs.into_iter().chain(c.also.iter().copied().filter(|_| c.gmb.is_some())).collect();
			// a thanks of ours after it makes no news of it
			let of = google.filter(|_| turn == "done").unwrap_or(last).said.id;
			let event = derived_event_id(&["google_mail", &of.to_string(), turn]);
			let telling = Telling {
				person: Some(owner),
				gmail: gmbs.is_empty().then_some(c.gmail),
				gmbs,
				tier: match read.map(|r| r.kind) {
					_ if matches!(turn, "routed" | "done") => Some(Tier::Info),
					Some(Kind::Suspended | Kind::AccountRestricted | Kind::AccessLost | Kind::OthersAccess | Kind::OwnershipRequested | Kind::StillActive) => Some(Tier::Urgent),
					_ => None,
				},
				link: Some(format!("/email?case={}", c.id)),
				buttons: c.draft.iter().filter(|_| turn == "held").map(|d| stop(d.id, locale)).collect(),
				// what the mail says may be a code to sign in with: its owner's alone
				body: Body {
					open: Some(note(None, None).render(locale)),
					private: Some(note(read.map(|r| r.summary.clone()).or_else(|| last.subject.clone()), reply).render(locale)),
					..Body::default()
				},
				..Telling::new(Rule::GoogleMail, event, at, Rendered::default())
			};
			let mut tx = crate::store::begin_write(conn).await?;
			queued += self.tell(&mut tx, &telling, now).await?;
			tx.commit().await.wrap_err("committing a note")?;
		}
		Ok(queued)
	}

	pub(crate) async fn mail_text(&self, conn: &mut SqliteConnection, gmail: i64, message: &str, mail: i64) -> eyre::Result<Text> {
		let sealed = db::text(conn, mail).await?;
		Ok(self.unseal(&gmail_mail_aad(gmail, message), &sealed).map_err(|e| format!("{e:#}")))
	}

	/// The case draft `d` of the owner's is a reply to.
	async fn case_of(&self, conn: &mut SqliteConnection, owner: Uuid, d: &DraftRow, now: Timestamp) -> eyre::Result<Case> {
		let cases = self.gathered(conn, owner, now).await?;
		cases
			.into_iter()
			.find(|c| c.is(d.case))
			.ok_or_else(|| eyre::eyre!("draft {} is of case {}, which is not its owner's", d.id, d.case))
	}

	/// The person's cases, each message's GBP and each case's state known of them all.
	async fn gathered(&self, conn: &mut SqliteConnection, owner: Uuid, now: Timestamp) -> eyre::Result<Vec<Case>> {
		let rows = db::cases(conn, owner).await?;
		let mails = db::mails(conn, owner).await?;
		let part_of: HashMap<i64, i64> = rows.iter().filter_map(|r| Some((r.id, r.joined?))).collect();
		let one = |case: i64| part_of.get(&case).copied().unwrap_or(case); // part of none: itself
		let mut drafts: HashMap<i64, Draft> = HashMap::new();
		for d in db::drafts(conn, owner).await? {
			let case = one(d.case);
			if drafts.get(&case).is_none_or(|o| o.id < d.id) {
				drafts.insert(case, self.draft(d));
			}
		}
		let stuck = db::stuck(conn, owner).await?;
		let auto = db::auto(conn, owner).await?;
		let picked: HashMap<i64, i64> = rows.iter().filter_map(|r| Some((r.id, r.gmb_id?))).collect();
		// whose a link is: a person's word of a message with it or its case, else the latest mail to name a GBP beside
		// it, else what its reader saw
		let mut known: HashMap<String, (u8, i64)> = HashMap::new();
		for m in &mails {
			let Some(listing) = &m.listing else { continue };
			let seen = m.reading().ok().flatten().and_then(|r| r.gmb); // one that does not read teaches nothing: its case says so
			let by_hand = match m.read_by()? {
				Some(ReadBy::Person) => seen,
				_ => picked.get(&m.case_id).or_else(|| picked.get(&one(m.case_id))).copied().filter(|_| m.gmb_id.is_none()),
			};
			let said = [by_hand, m.gmb_id, seen];
			let Some((rank, gmb)) = (0_u8..).zip(said).find_map(|(rank, gmb)| Some((rank, gmb?))) else {
				continue;
			};
			if known.get(listing).is_none_or(|(had, _)| rank <= *had) {
				known.insert(listing.clone(), (rank, gmb));
			}
		}
		let known: HashMap<String, (i64, bool)> = known.into_iter().map(|(l, (rank, gmb))| (l, (gmb, rank < 2))).collect();
		let senders = db::senders(conn, owner).await?;
		let given = db::given(conn, owner).await?;
		let inboxes = db::inboxes(conn, owner).await?;
		let reads = self.inboxes_read.lock().expect("never poisoned: no panic while held").clone();
		let mut held: HashMap<i64, Vec<Kept>> = HashMap::new();
		for m in mails {
			let account = inboxes.get(&m.gmail_id).filter(|(_, shared)| !shared).map(|(u, _)| u.as_str());
			held.entry(one(m.case_id)).or_default().push(kept(m, &known, &senders, account)?);
		}
		let mut parts: HashMap<i64, Vec<&CaseRow>> = HashMap::new();
		for r in &rows {
			parts.entry(one(r.id)).or_default().push(r);
		}
		let mut cases = Vec::new();
		for r in rows.iter().filter(|r| r.joined.is_none()) {
			let messages = held.remove(&r.id).ok_or_else(|| eyre::eyre!("case {} holds no message", r.id))?;
			let gmb = r.gmb_id.or_else(|| messages.iter().rev().find_map(|m| m.gmb));
			let told: BTreeSet<i64> = messages.iter().filter(|m| m.gmb_certain && !m.said.sent).filter_map(|m| m.gmb).collect();
			let about: BTreeSet<i64> = messages.iter().filter_map(|m| m.gmb).collect();
			let by_hand = r.topic()?;
			// a kind of any topic says least of what a conversation is about
			let said = || messages.iter().filter_map(|m| m.said.reading.as_ref()).filter(|r| r.topic != Topic::Other);
			let read = said().find(|r| r.kind.topic().is_some()).or_else(|| said().next());
			let topic = by_hand.or(read.map(|r| r.topic)).unwrap_or(Topic::Other); // nothing read yet, or only of none
			cases.push((
				r.done_mail,
				Case {
					id: r.id,
					gmail: r.gmail_id,
					gmb,
					gmb_certain: r.gmb_id.is_some() || (told.len() == 1 && told.first() == gmb.as_ref()),
					also: about.into_iter().filter(|g| Some(*g) != gmb).collect(),
					topic,
					by_hand: ByHand {
						gmb: r.gmb_id.is_some(),
						topic: by_hand.is_some(),
					},
					numbers: parts[&r.id].iter().flat_map(|p| p.numbers().clone()).collect(),
					joined: parts[&r.id].iter().filter(|p| p.id != r.id).map(|p| p.id).collect(),
					state: State::Quiet,
					stuck: stuck.clone(),
					due: false,
					press: Err(Halt::Unread),
					read: reads
						.get(&r.gmail_id)
						.filter(|(u, _)| inboxes.get(&r.gmail_id).is_some_and(|(now, _)| now == u))
						.map(|(_, at)| *at),
					messages,
					draft: drafts.remove(&r.id),
					given: given.iter().filter(|(c, ..)| one(*c) == r.id).map(|(_, item, document)| (item.clone(), *document)).collect(),
				},
			));
		}
		// a failed call's follow-up names no GBP: the gmail's one whose call stands offered
		let offered: Vec<(i64, i64, Timestamp)> = cases
			.iter()
			.filter_map(|(_, c)| {
				let m = c.google()?;
				(m.said.reading.as_ref()?.kind == Kind::CallOffered).then_some((c.gmail, c.gmb?, m.said.at))
			})
			.collect();
		for (_, c) in cases.iter_mut().filter(|(_, c)| c.gmb.is_none()) {
			let Some(m) = c.google().filter(|m| m.said.reading.as_ref().is_some_and(|r| r.kind == Kind::CallFailed)) else {
				continue;
			};
			let of: BTreeSet<i64> = offered.iter().filter(|(gmail, _, at)| *gmail == c.gmail && *at < m.said.at).map(|(_, gmb, _)| *gmb).collect();
			if let [one] = of.into_iter().collect::<Vec<_>>().as_slice() {
				c.gmb = Some(*one);
			}
		}
		let mut elsewhere: HashMap<i64, Vec<Said>> = HashMap::new();
		for (gmb, at, from, to) in db::moves(conn, owner).await? {
			let as_said: &[Kind] = match (from, to) {
				(Some(Stands::Suspended), Stands::Verified) => &[Kind::AppealApproved, Kind::Verified],
				(Some(Stands::Suspended), Stands::Verifying) => &[Kind::AppealApproved, Kind::ReverificationRequired],
				(Some(Stands::Verified), Stands::Verifying) => &[Kind::ReverificationRequired],
				(_, Stands::Verified) => &[Kind::Verified],
				(_, Stands::Suspended) => &[Kind::Suspended],
				(_, Stands::Draft | Stands::Verifying) => &[],
			};
			elsewhere.entry(gmb).or_default().extend(as_said.iter().map(|&kind| Said { kind, at, case_id: None }));
		}
		for (_, c) in &cases {
			for m in &c.messages {
				if let (Some(gmb), Some(r)) = (m.gmb.or(c.gmb), &m.said.reading) {
					let said = Said {
						kind: r.kind,
						at: m.said.at,
						case_id: Some(c.id),
					};
					elsewhere.entry(gmb).or_default().push(said);
				}
			}
		}
		let live = db::live(conn, owner).await?;
		let state = |done: Option<i64>, c: &Case, asked_elsewhere: bool| {
			let messages = c.messages();
			// what was given since it was written, it lacks no more
			let draft = c.draft.as_ref().map(|d| Reply {
				needs: d.reply.needs.iter().filter(|n| !c.given.iter().any(|(item, _)| item == *n)).cloned().collect(),
				..d.reply.clone()
			});
			let of_gmb = c.gmb.and_then(|g| elsewhere.get(&g)).into_iter().flatten();
			let others: Vec<Said> = of_gmb.filter(|s| s.case_id != Some(c.id)).copied().collect();
			let of = rules::Case {
				topic: c.topic,
				about: match c.gmb {
					None => About::Unknown,
					Some(g) if !live.contains(&g) => About::Removed,
					Some(_) if !c.gmb_certain => About::Likely,
					Some(_) => About::Live,
				},
				messages: &messages,
				draft: draft.as_ref(),
				done,
				elsewhere: &others,
				asked_elsewhere,
				switched: self.mail_by_itself && c.gmb.is_some_and(|g| auto.contains(&(g, c.topic))),
				stuck: c.stuck.as_deref(),
				read: c.read,
			};
			(rules::state(&of, now), rules::due(&of, now), rules::press(&of, now))
		};
		// a notice's state stands on whether a conversation asks the same of Google for its GBP, an older conversation's on
		// a newer's
		let mut open: Vec<&Case> = cases
			.iter()
			.filter(|(_, c)| matches!(c.topic, Topic::Verification | Topic::Suspension | Topic::Reviews) && c.messages.iter().any(|m| !m.said.noreply))
			.filter(|(done, c)| {
				let over = |s: &State| {
					matches!(
						s,
						State::Done { .. }
							| State::Quiet | State::NeedsYou {
							why: Why::Kind {
								kind: Kind::Closed | Kind::DuplicateTicket
							}
						}
					)
				};
				!over(&state(*done, c, false).0)
			})
			.map(|(_, c)| c)
			.collect();
		open.sort_by_key(|c| (c.last().said.at, c.id));
		let asking: HashMap<(i64, Topic), i64> = open.into_iter().filter_map(|c| Some(((c.gmb?, c.topic), c.id))).collect();
		Ok(cases
			.iter()
			.map(|(done, c)| {
				let asked = c.gmb.and_then(|g| asking.get(&(g, c.topic))).is_some_and(|id| *id != c.id);
				let (stands, due, press) = state(*done, c, asked);
				Case {
					state: stands,
					due,
					press,
					..c.clone()
				}
			})
			.collect())
	}

	fn draft(&self, d: DraftRow) -> Draft {
		let opened = self.unseal(&mail_draft_aad(d.case, d.answers), &d.body);
		let (body, unread) = match opened {
			Ok(body) => (Some(body.to_string()), None),
			Err(e) => (None, Some(format!("its text does not open: {e:#}"))),
		};
		Draft {
			id: d.id,
			body,
			reply: Reply {
				state: match d.state {
					DraftState::Proposed | DraftState::Held if unread.is_some() => DraftState::Failed,
					state => state,
				},
				answers: d.answers,
				needs: d.needs,
				written_at: d.created_at,
				send_at: d.send_at,
				sent_at: d.sent_at,
				error: unread.filter(|_| d.state != DraftState::Dismissed).or(d.error), // of one let go, why nothing was written
			},
		}
	}
}

/// `known`: whose each link is, and whether for certain; `senders`: who sent each reply handed to SMTP here;
/// `account`: whom its gmail's mail is read as, that gmail's alone.
fn kept(m: MailRow, known: &HashMap<String, (i64, bool)>, senders: &HashMap<String, Option<String>>, account: Option<&str>) -> eyre::Result<Kept> {
	let read_by = m.read_by()?;
	let (reading, unread) = match m.reading() {
		Ok(reading) => (reading, None),
		Err(e) => (None, Some(format!("what it was read as does not read: {e}"))),
	};
	let seen = reading.as_ref().and_then(|r| r.gmb);
	let ours = |a: &String| account.is_some_and(|account| a.eq_ignore_ascii_case(account));
	let linked = m.listing.as_ref().and_then(|l| known.get(l).copied());
	let of = match read_by {
		Some(ReadBy::Person) => seen.map(|g| (g, true)),
		_ => m.gmb_id.map(|g| (g, true)).or(linked).or(seen.map(|g| (g, false))),
	};
	Ok(Kept {
		gmb: of.map(|(g, _)| g),
		gmb_certain: of.is_some_and(|(_, certain)| certain),
		sent_by: m.sent.then(|| match m.message_id.as_ref().and_then(|id| senders.get(id)) {
			None => SentBy::Gmail,
			Some(None) => SentBy::Itself,
			Some(Some(name)) => SentBy::Person { name: name.clone() },
		}),
		said: Message {
			id: m.id,
			at: m.at()?,
			sent: m.sent,
			noreply: match m.sent {
				true => unheard(true, m.to()),
				false => m.from_address.as_deref().is_none_or(|f| Sender::of(f) != Sender::Support),
			},
			repliable: m.repliable,
			addressed: match m.sent {
				true => m.from_address.as_ref().is_some_and(ours),
				false => m.to().iter().any(ours),
			},
			reading,
			read_error: unread.or(m.read_error.clone().filter(|_| m.attempts >= MAX_READS)),
		},
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
		spam: m.spam,
	})
}

fn stop(draft: i64, locale: Locale) -> Button {
	let (label, pressed) = match locale {
		Locale::Ru => ("Остановить", "Остановлено"),
		Locale::En => ("Stop", "Stopped"),
	};
	Button {
		id: "stop".to_owned(),
		label: label.to_owned(),
		pressed: pressed.to_owned(),
		call: Call {
			operation: STOP.to_owned(),
			path: [("id".to_owned(), draft.into())].into_iter().collect(),
			body: serde_json::Value::Null,
		},
		leaves: Vec::new(),
	}
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
