//! The outbox delivered: what is due claimed at each network's pace (`store::outbox::claim_due`), each
//! message sent on its own or several as one, and what became of it. Adding a destination and
//! testing one send through the same.

use std::collections::BTreeMap;

use eyre::WrapErr;
use futures::{StreamExt, TryStreamExt};
use jiff::{SignedDuration, Timestamp};
use sa_alerts::Tier;
use sa_auth::PermissionSet;
use sa_core::{
	alerts::{Method, digest},
	notify::{self, ChatId, Failure, Next, OutboxId, Rendered},
};
use url::Url;
use uuid::Uuid;

use crate::{
	Refused,
	alerts::{ACCESS_TTL, Address, Destination, Directory, Feeds, Hub, Message, Outlets},
	session::Refresher,
	store::{
		alerts as db,
		outbox::{self, Due},
		telegram::Kind,
	},
};

/// Past this many of one rule's messages waiting for one destination, they go out as one.
pub const BACKLOG: i64 = 20;

/// How long a replica sending a message holds it: past the networks' timeouts, so it lapses
/// only when the replica is gone.
pub const SEND_LEASE: SignedDuration = SignedDuration::from_secs(60);

/// A lead message noted longer ago than this is news nobody needs.
const STALE_AFTER: SignedDuration = SignedDuration::from_hours(1);

/// Messages sent at once in a pass.
const PARALLEL_SENDS: usize = 8;

/// What one delivery pass did.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Delivered {
	pub sent: usize,
	pub retrying: usize,
	pub dead: usize,
}

impl Delivered {
	fn add(mut self, o: Self) -> Self {
		self.sent += o.sent;
		self.retrying += o.retrying;
		self.dead += o.dead;
		self
	}
}

impl<O: Outlets, C: Refresher + Directory, F: Feeds> Hub<O, C, F> {
	/// Sends what is due at `now`, within the pace. First, lead messages noted over an hour ago are
	/// given up, and past [`BACKLOG`] of one rule's messages for one destination they go as one.
	pub async fn deliver(&self, now: Timestamp) -> eyre::Result<Delivered> {
		let due = {
			let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to claim messages")?;
			outbox::drop_stale(&mut conn, now - STALE_AFTER).await?;
			outbox::fold_crowded(&mut conn, BACKLOG, now).await?;
			outbox::claim_due(&mut conn, now, now + SEND_LEASE).await?
		};
		let (together, alone): (Vec<Due>, Vec<Due>) = due.into_iter().partition(|d| d.together);
		let mut by_destination: BTreeMap<i64, Vec<Due>> = BTreeMap::new();
		for d in together {
			by_destination.entry(d.destination_id).or_default().push(d);
		}
		let sends = alone.into_iter().map(|d| vec![d]).chain(by_destination.into_values());
		futures::stream::iter(sends)
			.map(|rows| self.send(rows, now))
			.buffer_unordered(PARALLEL_SENDS)
			.try_fold(Delivered::default(), |sum, one| async move { Ok(sum.add(one)) })
			.await
	}

	/// One message of `rows`, claimed for one destination. Whether each may still go is asked again
	/// here, not only when it was queued: its destination reached, its user's access confirmed within
	/// the TTL and opening its rule — PII waits in the outbox.
	async fn send(&self, rows: Vec<Due>, now: Timestamp) -> eyre::Result<Delivered> {
		let mut out = Delivered::default();
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to deliver")?;
		let (user, id) = (rows[0].user_id, rows[0].destination_id);
		let destination = db::destination_of(&mut conn, user, id).await?.filter(|d| d.dead.is_none());
		let permissions = db::access(&mut conn, user)
			.await?
			.and_then(|(permissions, confirmed)| permissions.filter(|_| confirmed >= now - ACCESS_TTL));
		let (Some(destination), Some(permissions)) = (destination, permissions) else {
			for d in &rows {
				outbox::dead(&mut conn, d.id, "access not confirmed").await?;
			}
			return Ok(Delivered { dead: rows.len(), ..out });
		};
		let mut told = Vec::new();
		for d in rows {
			match self.told(&mut conn, &d, &destination, &permissions).await? {
				Ok(t) => told.push(t),
				Err(why) => {
					outbox::dead(&mut conn, d.id, &why).await?;
					out.dead += 1;
				}
			}
		}
		let Some(first) = told.first() else {
			return Ok(out);
		};
		let address = match self.panel.alerts_address(&destination) {
			Ok(address) => address,
			Err(e) => {
				let why = format!("unreadable: {e:#}");
				tracing::warn!(destination = destination.id, why, "alerts: a destination's address is unreadable");
				for t in &told {
					outbox::dead(&mut conn, t.due.id, &why).await?;
				}
				out.dead += told.len();
				return Ok(out);
			}
		};
		let mut message = match told.as_slice() {
			[one] => self.alone(one, &destination),
			several => Message {
				tier: first.due.tier, // the claim's most urgent
				rule: None,
				text: digest(&several.iter().map(|t| (t.due.tier, t.text.clone())).collect::<Vec<_>>(), self.locale),
				links: vec![("Open alerts".to_owned(), self.at("/user/alerts"))],
				buttons: Vec::new(),
				photo: None,
				unsubscribe: None,
				notes: several.iter().map(|t| t.due.note_id).collect(),
			},
		};
		if destination.method == Method::Email {
			message.unsubscribe = Some(self.unsubscribe(destination.id)?);
		}
		let together = told.len() > 1;
		let photo = first.note.photo.clone().filter(|_| !together).map(|p| (first.due.rule, p));
		// No pool connection is held while a service or a network is asked.
		drop(conn);
		let fetched = match photo {
			Some((rule, path)) => match self.feeds.photo(rule.feed().expect("only a service's note carries a photo"), &path).await {
				Ok(photo) => {
					message.photo = Some(photo);
					Ok(())
				}
				Err(e) => Err(Failure::Transient(format!("the photo {path}: {e:#}"))),
			},
			None => Ok(()),
		};
		let answer = match fetched {
			Ok(()) => self.outlets.send_to(&address, &message).await,
			Err(f) => Err(f),
		};
		drop(message);
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to deliver")?;
		let failure = match answer {
			Ok(remote) => {
				for t in &told {
					outbox::sent(&mut conn, t.due.id, &remote, together, now).await?;
				}
				out.sent += told.len();
				return Ok(out);
			}
			Err(f) => f,
		};
		let error = failure_text(&failure);
		tracing::warn!(destination = destination.id, method = destination.method.as_ref(), messages = told.len(), error, "alerts: a send failed");
		for t in &told {
			out = out.add(self.failed(&mut conn, &t.due, &destination, &failure, &error, now).await?);
		}
		Ok(out)
	}

	async fn failed(&self, conn: &mut sqlx::SqliteConnection, d: &Due, to: &Destination, failure: &Failure, error: &str, now: Timestamp) -> eyre::Result<Delivered> {
		// The bot's and the mailbox's limits are ours, whatever the address: the method waits, and the
		// message keeps its tries. A webhook's are its own.
		if let Failure::RetryAfter(wait) = failure
			&& matches!(to.method, Method::Telegram | Method::Email | Method::Discord)
		{
			let until = now.saturating_add((*wait).max(notify::PER_CHAT_GAP)).unwrap_or(Timestamp::MAX);
			outbox::pause(conn, d.id, to.method, until, error).await?;
			return Ok(Delivered {
				retrying: 1,
				..Delivered::default()
			});
		}
		Ok(match notify::after_failure(d.attempts, failure, now) {
			Next::Retry { at } => {
				outbox::retry(conn, d.id, at, error).await?;
				Delivered {
					retrying: 1,
					..Delivered::default()
				}
			}
			next @ (Next::Dead | Next::AddressDead) => {
				outbox::dead(conn, d.id, error).await?;
				if next == Next::AddressDead {
					db::dead(conn, to.id, error, now).await?;
					tracing::info!(user_id = %d.user_id, destination = to.id, "alerts: nothing reaches a destination; marked dead");
				}
				Delivered {
					dead: 1,
					..Delivered::default()
				}
			}
		})
	}

	/// What a claimed message says; why it can never be sent, when it cannot.
	async fn told(&self, conn: &mut sqlx::SqliteConnection, d: &Due, to: &Destination, permissions: &PermissionSet) -> eyre::Result<Result<Told, String>> {
		if !d.rule.open_to(permissions) {
			return Ok(Err("access not confirmed".to_owned()));
		}
		let note = db::note(conn, d.note_id).await?;
		let body = match self.panel.open_note(&note) {
			Ok(body) => body,
			Err(e) => {
				tracing::warn!(message = d.id, error = format!("{e:#}"), "alerts: a queued message is unreadable");
				return Ok(Err(format!("unreadable: {e:#}")));
			}
		};
		let Some(text) = body.read_by(to.private, permissions) else {
			return Ok(Err("nothing in it for a channel".to_owned()));
		};
		Ok(Ok(Told {
			text: text.clone(),
			links: if to.private { body.links } else { Vec::new() },
			note,
			due: d.clone(),
		}))
	}

	fn alone(&self, t: &Told, to: &Destination) -> Message {
		let mut links = t.links.clone();
		if t.note.link.is_some() {
			links.push(("Open".to_owned(), self.at(&format!("/user/alerts?open={}", t.note.id))));
		}
		let buttons = match to.chat.filter(|_| to.private) {
			Some(chat) => self.buttons(ChatId { v: chat.id }, OutboxId { v: t.due.id }, &t.note.buttons),
			None => Vec::new(),
		};
		Message {
			tier: t.due.tier,
			rule: Some(t.due.rule),
			text: t.text.clone(),
			links,
			buttons,
			photo: None,
			unsubscribe: None,
			notes: vec![t.note.id],
		}
	}

	fn at(&self, path: &str) -> String {
		self.origin.join(path).expect("a path on an origin").into()
	}

	fn unsubscribe(&self, destination: i64) -> eyre::Result<Url> {
		let mut url = self.origin.join("/api/alerts/unsubscribe").expect("a path on an origin");
		url.query_pairs_mut().append_pair("token", &self.panel.alerts_unsubscribe_token(destination)?);
		Ok(url)
	}

	/// A destination of the user's at `address`, once a test went out there: a refusal stores nothing.
	pub async fn add(&self, user: Uuid, permissions: &PermissionSet, display: &str, address: &Address, title: &str, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		if let Err(f) = self.outlets.send_to(address, &self.test_message()).await {
			return Ok(Err(Refused::Invalid(format!("the test did not go out: {}", failure_text(&f)))));
		}
		self.panel.alerts_add(user, permissions, display, address, title, now).await
	}

	/// Mails `address` the link that makes it a destination of the user's, on the settings page at
	/// `page`, a path.
	pub async fn prove_email(&self, user: Uuid, address: &str, page: &str, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut url = self.origin.join(page).expect("a path on an origin");
		url.query_pairs_mut().append_pair("confirm", &self.panel.alerts_email_token(user, address, now)?);
		let mut text = Rendered::default();
		text.push("Confirm this address for Service-Arb alerts\nOpen the link signed in as yourself. Not you? Ignore this mail.");
		let message = Message {
			text,
			links: vec![("Confirm".to_owned(), url.into())],
			..self.test_message()
		};
		let to = Address::Email { to: address.to_owned() };
		Ok(match self.outlets.send_to(&to, &message).await {
			Ok(_) => Ok(()),
			Err(f) => Err(Refused::Invalid(format!("the mail did not go out: {}", failure_text(&f)))),
		})
	}

	pub(crate) fn test_message(&self) -> Message {
		Message {
			tier: Tier::Important,
			rule: None,
			text: Rendered {
				text: notify::test_text(self.locale).to_owned(),
				entities: Vec::new(),
			},
			links: Vec::new(),
			buttons: Vec::new(),
			photo: None,
			unsubscribe: None,
			notes: Vec::new(),
		}
	}

	/// Sends a test to one of the user's destinations; one that arrives makes it reached again.
	pub async fn test(&self, user: Uuid, destination: i64) -> eyre::Result<Result<(), Refused>> {
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to test a destination")?;
		let Some(d) = db::destination_of(&mut conn, user, destination).await? else {
			return Ok(Err(Refused::NotFound));
		};
		drop(conn);
		if let Err(f) = self.outlets.send_to(&self.panel.alerts_address(&d)?, &self.test_message()).await {
			return Ok(Err(Refused::Invalid(format!("the test did not go out: {}", failure_text(&f)))));
		}
		// A chat's title is what the bot sees once it is in: an imported one's is learned here; one
		// Telegram does not describe keeps what it had, the test having reached it all the same.
		let seen = match d.chat {
			Some(chat) if chat.kind != Kind::Private => self.outlets.chat(&chat.id.to_string()).await.ok(),
			_ => None,
		};
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to test a destination")?;
		if let Some(info) = seen {
			let kind = if info.kind == "channel" { Kind::Channel } else { Kind::Group };
			crate::store::telegram::describe(&mut conn, destination, kind, info.title.as_deref()).await?;
		}
		db::revive(&mut conn, destination).await?;
		Ok(Ok(()))
	}
}

pub(crate) fn failure_text(f: &Failure) -> String {
	match f {
		Failure::RetryAfter(wait) => format!("429, retry after {}s", wait.as_secs()),
		Failure::Transient(why) | Failure::Refused(why) | Failure::Gone(why) => why.clone(),
	}
}

/// A claimed message, as its destination may read it.
struct Told {
	due: Due,
	note: db::Note,
	text: Rendered,
	links: Vec<(String, String)>,
}
