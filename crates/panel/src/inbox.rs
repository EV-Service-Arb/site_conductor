//! A gmail's inbox followed for its mail with Google (docs/ARCHITECTURE.md, Tokens and GBPs): each
//! message logged once, on each GBP of the gmail it names, else the gmail's; read through a
//! [`Mailbox`], in `serve` [`Gmail`].

use eyre::WrapErr;
use jiff::Timestamp;

pub use crate::imap::Gmail;
use crate::{
	Panel,
	seal::gmail_imap_aad,
	store::{connections, gmbs as gmbs_db, gmbs::Event, inbox as db},
};

/// Messages read at once.
pub const BATCH: usize = 50;
const SNIPPET: usize = 200;
/// A shorter name, place id or address line is in every other message.
const NAMES_FROM: usize = 4;

/// A mailbox the follower reads.
pub trait Mailbox: Sync {
	/// Its UIDVALIDITY and its mail with Google past `after` (another UIDVALIDITY's: from the first),
	/// at most `limit`.
	fn read(&self, account: Account<'_>, after: Option<Cursor>, limit: usize) -> impl Future<Output = eyre::Result<Batch>> + Send;
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
	/// From the account itself.
	pub sent: bool,
	/// Its text, as far as it was read.
	pub text: String,
}

impl Panel {
	/// The gmails with an inbox set, not removed.
	pub async fn inboxes(&self) -> eyre::Result<Vec<i64>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the inboxes")?;
		db::followed(&mut conn).await
	}

	/// A batch of gmail `gmail`'s mail with Google past its cursor, logged with the cursor moved in one
	/// write; `true` a full one, more may follow. Nothing while its inbox is unset or the cursor moved
	/// meanwhile.
	pub async fn follow_inbox(&self, gmail: i64, mailbox: &impl Mailbox, now: Timestamp) -> eyre::Result<bool> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for an inbox")?;
		let Some((imap, sealed)) = connections::imap(&mut conn, gmail).await? else {
			return Ok(false);
		};
		let before = db::cursor(&mut conn, gmail).await?;
		drop(conn);
		let password = self.unseal(&gmail_imap_aad(gmail), &sealed)?;
		// another account set on the gmail starts over
		let after = before.as_ref().filter(|(u, _)| *u == imap.username).map(|(_, c)| *c);
		let account = Account {
			host: &imap.host,
			username: &imap.username,
			password: &password,
		};
		let batch = mailbox.read(account, after, BATCH).await?;
		let mut tx = self.store.begin_write().await?;
		let set = connections::imap(&mut tx, gmail).await?.map(|(i, _)| i.username);
		if set.as_ref() != Some(&imap.username) || db::cursor(&mut tx, gmail).await? != before {
			return Ok(false);
		}
		let gmbs: Vec<(i64, String, Vec<String>)> = db::gmbs(&mut tx, gmail)
			.await?
			.into_iter()
			.map(|(id, name, place, address)| {
				let title = name.clone().expect("gmbs.name is NOT NULL");
				let line = address.as_deref().map(|a| a.split(',').next().expect("split yields one").trim().to_owned());
				let names = [name, place, line].into_iter().flatten().map(|n| n.to_lowercase()).filter(|n| n.chars().count() >= NAMES_FROM);
				(id, title, names.collect())
			})
			.collect();
		for m in &batch.mails {
			let message = m.message.to_string();
			if db::logged(&mut tx, gmail, &message).await? {
				continue;
			}
			let read = m.subject.iter().chain([&m.text]).map(|s| s.to_lowercase()).collect::<Vec<_>>().join("\n");
			let named: Vec<(i64, &str)> = gmbs
				.iter()
				.filter(|(_, _, names)| names.iter().any(|n| read.contains(n.as_str())))
				.map(|(id, title, _)| (*id, title.as_str()))
				.collect();
			self.tell_google_mail(&mut tx, gmail, &named, m, now).await?;
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
			match named.as_slice() {
				[] => gmbs_db::log(&mut tx, None, Some(gmail), None, now, &event).await?,
				named =>
					for &(gmb, _) in named {
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
		db::set_cursor(&mut tx, gmail, &imap.username, Cursor { validity: batch.validity, uid }).await?;
		tx.commit().await.wrap_err("committing an inbox's batch")?;
		Ok(batch.mails.len() == BATCH)
	}
}
