//! A gmail's inbox followed for its mail with Google (docs/ARCHITECTURE.md, Tokens and GBPs): each
//! message logged once, on each GBP it names, else the gmail's, and what is about a
//! Business Profile kept ([`crate::mail`]); read through a [`Mailbox`], in `serve` [`Gmail`].

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::mail::Named;

pub use crate::imap::Gmail;
use crate::{
	Panel,
	connections::{Checked, Connection},
	seal::gmail_imap_aad,
	store::{connections, gmbs as gmbs_db, gmbs::Event, inbox as db, mail as mail_db},
};

/// Messages read at once.
pub const BATCH: usize = 50;
const SNIPPET: usize = 200;

/// As which account each gmail's inbox was last read to its end, and when, since the start.
pub(crate) type Reads = Arc<Mutex<HashMap<i64, (String, Timestamp)>>>;

/// The GBPs each gmail's mail was last matched with: its own, its owner's others.
pub(crate) type NamedAs = Arc<Mutex<HashMap<i64, (Vec<Named>, Vec<Named>)>>>;

/// A mailbox the follower reads.
pub trait Mailbox: Sync {
	/// `folder`'s UIDVALIDITY and its mail with Google past `after` (another UIDVALIDITY's: from the first),
	/// at most `limit`.
	fn read(&self, account: Account<'_>, folder: Folder, after: Option<Cursor>, limit: usize) -> impl Future<Output = eyre::Result<Batch>> + Send;
}

/// A mailbox of Gmail's; All Mail holds no spam.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Folder {
	All,
	Spam,
}

/// Where a follower stands in a mailbox: its UIDVALIDITY, the last UID taken.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
	pub validity: u32,
	pub uid: u32,
}

#[derive(Clone, Copy)]
pub struct Account<'a> {
	pub host: &'a str,
	pub username: &'a str,
	pub password: &'a str,
}

pub struct Batch {
	pub validity: u32,
	/// By UID.
	pub mails: Vec<Mail>,
}

/// A message with Google, as read.
#[derive(Clone, Debug)]
pub struct Mail {
	pub uid: u32,
	/// Gmail's X-GM-MSGID.
	pub message: u64,
	/// Gmail's X-GM-THRID.
	pub thread: u64,
	/// When Gmail took it in.
	pub date: Timestamp,
	pub subject: Option<String>,
	/// Its sender; one sent, its recipients.
	pub counterparts: Vec<String>,
	/// Its sender's address.
	pub from: Option<String>,
	pub reply_to: Option<String>,
	/// Its recipients' addresses, its copies' too.
	pub to: Vec<String>,
	/// Without its brackets, as each of `refs`.
	pub message_id: Option<String>,
	/// The Message-IDs it answers.
	pub refs: Vec<String>,
	/// Proved Google's ([`sa_core::mail::authentic`]).
	pub authentic: bool,
	/// The account sent it: Gmail's word, not its From's.
	pub sent: bool,
	/// Its text, as far as it was read.
	pub text: String,
	/// It was read to its end.
	pub whole: bool,
	/// Gmail has it in Spam.
	pub spam: bool,
}

/// The GBPs a message names, its subject with it.
fn named(subject: Option<&str>, text: &str, gbps: &[Named]) -> Vec<i64> {
	let read = subject.into_iter().chain([text]).collect::<Vec<_>>().join("\n\n");
	sa_core::mail::named(&read, gbps).into_iter().collect()
}

impl Panel {
	/// The gmails with an inbox set, not removed.
	pub async fn inboxes(&self) -> eyre::Result<Vec<i64>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the inboxes")?;
		db::followed(&mut conn).await
	}

	/// A batch of gmail `gmail`'s mail with Google past its cursors, in each of its mailboxes; `true` a full
	/// one, more may follow. Nothing while its inbox is unset.
	pub async fn follow_inbox(&self, gmail: i64, mailbox: &impl Mailbox, now: Timestamp) -> eyre::Result<bool> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an inbox")?;
		let Some((imap, sealed)) = connections::imap(&mut conn, gmail).await? else {
			return Ok(false);
		};
		drop(conn);
		let password = self.unseal(&gmail_imap_aad(gmail), &sealed)?;
		let account = Account {
			host: &imap.host,
			username: &imap.username,
			password: &password,
		};
		let mut more = false;
		for folder in [Folder::All, Folder::Spam] {
			match self.follow_folder(gmail, mailbox, account, folder, now).await? {
				Some(full) => more |= full,
				None => return Ok(false),
			}
		}
		self.rename_mail(gmail).await?;
		if !more {
			if imap.checked.error.is_some() {
				let mut tx = self.store.begin_write().await?;
				connections::checked(&mut tx, gmail, Connection::Imap, &Checked { at: now, error: None }).await?;
				tx.commit().await.wrap_err("committing an inbox's check")?;
			}
			let was = (imap.username.clone(), now);
			self.inboxes_read.lock().expect("never poisoned: no panic while held").insert(gmail, was);
		}
		Ok(more)
	}

	/// The gmail's mail matched again once its owner's GBPs are named otherwise than when it last was, by each
	/// message's own words; one they do not name stays as it was.
	async fn rename_mail(&self, gmail: i64) -> eyre::Result<()> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for whom mail names")?;
		let gbps = db::gmbs(&mut conn, gmail).await?;
		if self.mail_named.lock().expect("never poisoned: no panic while held").get(&gmail) == Some(&gbps) {
			return Ok(());
		}
		let mut found = Vec::new();
		for (mail, message, subject, was) in mail_db::naming(&mut conn, gmail).await? {
			// a text that does not open names nothing: its case says so
			let Ok(text) = self.mail_text(&mut conn, gmail, &message, mail).await? else { continue };
			let names = |gbps: &[Named]| named(subject.as_deref(), &text, gbps);
			let own = names(&gbps.0);
			let about = if own.is_empty() { names(&gbps.1) } else { own };
			if let [one] = about.as_slice()
				&& was != Some(*one)
			{
				found.push((mail, *one));
			}
		}
		drop(conn);
		if !found.is_empty() {
			let mut tx = self.store.begin_write().await?;
			for (mail, gmb) in found {
				mail_db::set_gmb(&mut tx, mail, gmb).await?;
			}
			tx.commit().await.wrap_err("committing whom mail names")?;
		}
		self.mail_named.lock().expect("never poisoned: no panic while held").insert(gmail, gbps);
		Ok(())
	}

	/// A batch of one mailbox logged, with its cursor moved in one write; `None`, another took it or the inbox was
	/// set anew meanwhile.
	async fn follow_folder(&self, gmail: i64, mailbox: &impl Mailbox, account: Account<'_>, folder: Folder, now: Timestamp) -> eyre::Result<Option<bool>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a mailbox")?;
		let before = db::cursor(&mut conn, gmail, folder).await?;
		drop(conn);
		// another account set on the gmail starts over
		let after = before.as_ref().filter(|(u, _)| u == account.username).map(|(_, c)| *c);
		let batch = mailbox.read(account, folder, after, BATCH).await?;
		if batch.mails.is_empty() && after.is_some_and(|c| c.validity == batch.validity) {
			return Ok(Some(false));
		}
		let mut tx = self.store.begin_write().await?;
		let set = connections::imap(&mut tx, gmail).await?.map(|(i, _)| i.username);
		if set.as_deref() != Some(account.username) || db::cursor(&mut tx, gmail, folder).await? != before {
			return Ok(None);
		}
		let (own, others) = db::gmbs(&mut tx, gmail).await?;
		let mut cases = mail_db::holds(&mut tx, gmail).await?;
		// in Spam, only what is proved Google's is mail with it
		for m in batch.mails.iter().filter(|m| !m.spam || m.authentic) {
			let message = m.message.to_string();
			let words = sa_core::mail::trimmed(&m.text);
			let of = |gbps: &[Named]| {
				let own = named(m.subject.as_deref(), &words, gbps);
				if own.is_empty() { named(None, &m.text, gbps) } else { own }
			};
			let (by_gmails, by_others) = (of(&own), of(&others));
			let about = if by_gmails.is_empty() { &by_others } else { &by_gmails };
			self.keep_mail(&mut tx, gmail, m, about, &mut cases, now).await?;
			if db::logged(&mut tx, gmail, &message).await? {
				continue;
			}
			let words = m.text.split_whitespace().collect::<Vec<_>>().join(" ");
			let snippet = match words.char_indices().nth(SNIPPET) {
				Some((cut, _)) => format!("{}…", &words[..cut]),
				None => words,
			};
			let event = Event::Email {
				message,
				thread: format!("{:x}", m.thread),
				date: m.date,
				subject: m.subject.clone(),
				counterparts: m.counterparts.clone(),
				snippet,
				sent: m.sent,
			};
			match about.as_slice() {
				[] => gmbs_db::log(&mut tx, None, Some(gmail), None, now, &event).await?,
				named =>
					for &gmb in named {
						gmbs_db::log(&mut tx, Some(gmb), Some(gmail), None, now, &event).await?;
					},
			}
		}
		let uid = match (batch.mails.last(), after) {
			(Some(m), _) => m.uid,
			(None, Some(c)) if c.validity == batch.validity => c.uid,
			// a mailbox new to the follower, nothing in it yet: UIDs start at 1
			(None, _) => 0,
		};
		let at = Cursor { validity: batch.validity, uid };
		db::set_cursor(&mut tx, gmail, folder, account.username, at).await?;
		tx.commit().await.wrap_err("committing a mailbox's batch")?;
		Ok(Some(batch.mails.len() == BATCH))
	}
}
