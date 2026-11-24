//! The conductor's own alerts, but those [`crate::alerts`] finds due itself: each told in the write
//! that makes it so, once for what it is about.
//!
//! ```text
//! google_mail        a case turned to a person ([`crate::mail`])
//! connection_lost    an inbox that has failed for 15 min       \
//!                    a grant, key or proxy refused twice        \
//! feed_stalled       a service's notes that failed for 10 min   > said when it starts, again after a day
//! meter_held         a Google account's guard unlinking billing /
//!                    a meter at its stop                       once a period
//! tokens_low         under two days left at the week's pace; none left   once a top-up
//! browser_stopped    a phone's minutes charged down to less than one more
//! kickback           the first of those waiting to be claimed; each grant
//! assistant_waiting  an answer that took a minute or more and proposes something
//! access_granted     a need asked for, now held
//! release_out        a next version of a listing one's category was taken from
//! trash_purge        3 days before what was kept a week or more goes
//! ```

use std::collections::BTreeMap;

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp, tz::TimeZone};
use sa_alerts::{Rule, Tier};
use sa_auth::Service;
use sa_core::notify::{MAX_NAME, Rendered, one_line};
use sqlx::SqliteConnection;
use strum::{AsRefStr, Display, EnumString};
use url::form_urlencoded;
use uuid::Uuid;

use crate::{
	Panel,
	access::AccessRequest,
	alerts::{Body, Telling, derived_event_id},
	library::{Listing, Released},
	meters::{Gauge, Lever},
	store::{
		alerts as alerts_db, connections as connections_db, gmbs as gmbs_db, own_alerts as db,
		tokens::{Kickback, Owed},
	},
};

/// What goes on failing is said again after this.
const AGAIN: SignedDuration = SignedDuration::from_hours(24);

/// A balance is paced by what was charged over this many days.
const PACE_DAYS: i64 = 7;

/// A balance that lasts fewer days than this is low.
const LOW_DAYS: i64 = 2;

/// How long before what was removed goes its person is told, unless they keep it under [`PURGE_KEPT`].
const PURGE_SOON: SignedDuration = SignedDuration::from_hours(3 * 24);

/// Trash kept fewer days than this goes as its person remembers removing it.
const PURGE_KEPT: i64 = 7;

/// The longest an outside text runs.
const QUOTE: usize = 200;

/// A gmail's connection that no longer answers.
#[derive(AsRefStr, Clone, Copy, Debug, Display, EnumString, Eq, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "snake_case")]
pub enum Lost {
	#[strum(serialize = "imap")] // what its events were named by first
	Inbox,
	Business,
	MapsKey,
	Proxy,
}
impl Lost {
	/// How it is said, what it costs.
	const fn said(self) -> (&'static str, &'static str) {
		match self {
			Self::Inbox => ("Inbox not read", "Mail from Google is missed until it connects again."),
			Self::Business => ("Business Profile grant refused", "Nothing is read or written through it until it is granted again."),
			Self::MapsKey => ("Maps key refused", "Places are bought on the shared keys, in tokens, until it is replaced."),
			Self::Proxy => ("Proxy not working", "Its browser does not start until it works again or is replaced."),
		}
	}
}

fn text(s: &str) -> Rendered {
	let mut out = Rendered::default();
	out.push(s);
	out
}

fn chat_link(chat: Uuid) -> String {
	format!("/assistant?chat={chat}")
}

impl Panel {
	/// The event a lasting failure, named by `parts`, is told under now; `None` while it was told within
	/// [`AGAIN`].
	async fn again(&self, conn: &mut SqliteConnection, rule: Rule, parts: &[&str], now: Timestamp) -> eyre::Result<Option<Uuid>> {
		let of = |days_ago: i64| {
			let day = (now - SignedDuration::from_hours(24 * days_ago)).to_zoned(TimeZone::UTC).date().to_string();
			derived_event_id(&[&[rule.as_ref()], parts, &[&day]].concat())
		};
		let mut told = false;
		for days_ago in [0, 1] {
			told |= db::told_since(conn, rule, of(days_ago), now - AGAIN).await?;
		}
		Ok((!told).then(|| of(0)))
	}

	/// Gmail `gmail`'s `lost` has failed for a while, with `why`.
	pub async fn alerts_connection_lost(&self, gmail: i64, lost: Lost, why: &str, now: Timestamp) -> eyre::Result<()> {
		let (title, cost) = lost.said();
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a connection lost")?;
		let Some(event) = self.again(&mut conn, Rule::ConnectionLost, &[lost.as_ref(), &gmail.to_string()], now).await? else {
			return Ok(());
		};
		let mut tx = crate::store::begin_write(&mut conn).await?;
		let Some(owner) = gmbs_db::gmail_owner(&mut tx, gmail).await? else {
			return Ok(()); // removed meanwhile
		};
		let mut said = text(&format!("{title}: "));
		said.code(&one_line(&connections_db::address(&mut tx, gmail).await?, 60));
		said.push(&format!("\n{cost} "));
		said.code(&one_line(why, QUOTE));
		let telling = Telling {
			person: Some(owner),
			gmail: Some(gmail),
			link: Some(format!("/?account={gmail}&tab=connections")),
			..Telling::new(Rule::ConnectionLost, event, now, said)
		};
		self.tell(&mut tx, &telling, now).await?;
		tx.commit().await.wrap_err("committing a connection lost")
	}

	/// `service`'s notes have not been read for a while, with `why`.
	pub async fn alerts_feed_stalled(&self, service: Service, why: &str, now: Timestamp) -> eyre::Result<()> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a feed stalled")?;
		let Some(event) = self.again(&mut conn, Rule::FeedStalled, &[service.as_str()], now).await? else {
			return Ok(());
		};
		let open = text(&format!(
			"No alerts from {service}: its feed fails\nNothing it has to tell reaches anyone until it is read again."
		));
		let mut private = open.clone();
		private.push("\n");
		private.code(&one_line(why, QUOTE));
		let telling = Telling {
			body: Body {
				open: Some(open),
				private: Some(private),
				..Body::default()
			},
			..Telling::new(Rule::FeedStalled, event, now, Rendered::default())
		};
		let mut tx = crate::store::begin_write(&mut conn).await?;
		self.tell(&mut tx, &telling, now).await?;
		tx.commit().await.wrap_err("committing a feed stalled")
	}

	/// Everyone's balance as just renewed: who is running out.
	pub(crate) async fn tell_low(&self, tx: &mut SqliteConnection, balances: &BTreeMap<Uuid, i64>, now: Timestamp) -> eyre::Result<()> {
		for (user, charged, topped_up) in db::paces(tx, now - SignedDuration::from_hours(24 * PACE_DAYS)).await? {
			let Some(&balance) = balances.get(&user) else { continue }; // first seen since the balances were read
			let a_day = charged.div_euclid(PACE_DAYS) - self.renewal.daily;
			let (stage, said) = match balance {
				_ if charged == 0 => continue,
				..=0 => (
					"out",
					"You are out of tokens\nWhat costs tokens is refused, and walks of your GBPs wait, until you hold some.".to_owned(),
				),
				_ if a_day > 0 && balance < a_day * LOW_DAYS => (
					"low",
					format!(
						"{balance} tokens left, under {LOW_DAYS} days at your pace\nYou spent {charged} over the last {PACE_DAYS} days; {} renew a day.",
						self.renewal.daily
					),
				),
				_ => continue,
			};
			let topped_up = topped_up.map_or_else(|| "never".to_owned(), |id| id.to_string());
			let telling = Telling {
				person: Some(user),
				link: Some("/user/tokens".to_owned()),
				..Telling::new(Rule::TokensLow, derived_event_id(&["tokens_low", stage, &user.to_string(), &topped_up]), now, text(&said))
			};
			self.tell(tx, &telling, now).await?;
		}
		Ok(())
	}

	/// `user`'s cloud phone, its use `id` in `service`'s feed, ran until they could not afford a minute more.
	pub(crate) async fn tell_phone_stopped(&self, tx: &mut SqliteConnection, user: Uuid, service: &str, id: i64, balance: i64, now: Timestamp) -> eyre::Result<()> {
		let said = format!("Your cloud phone stopped: {balance} tokens left\nIt runs no longer than your balance affords.");
		let telling = Telling {
			person: Some(user),
			link: Some("/user/tokens".to_owned()),
			..Telling::new(Rule::BrowserStopped, derived_event_id(&["browser_stopped", service, &id.to_string()]), now, text(&said))
		};
		self.tell(tx, &telling, now).await?;
		Ok(())
	}

	/// `user`'s kick-backs not claimed yet, newest first, one of them just owed.
	pub(crate) async fn tell_kickback(&self, tx: &mut SqliteConnection, user: Uuid, pending: &[Kickback], now: Timestamp) -> eyre::Result<()> {
		let (Some(newest), Some(oldest)) = (pending.first(), pending.last()) else {
			return Ok(());
		};
		let total: i64 = pending.iter().map(|k| k.amount).sum();
		let (event, said) = match &newest.why {
			Owed::Grant { note, .. } => {
				let mut said = text(&format!("{} tokens granted to you\n", newest.amount));
				said.code(&one_line(note, QUOTE));
				(["kickback", "grant", &user.to_string(), &newest.id.to_string()].map(str::to_owned), said)
			}
			Owed::Charge(c) => {
				let mut said = text(&format!("{total} tokens back: a walk failed\n"));
				said.code(&one_line(c.label.as_deref().unwrap_or(&c.key), MAX_NAME));
				(["kickback", "charge", &user.to_string(), &oldest.id.to_string()].map(str::to_owned), said)
			}
		};
		let telling = Telling {
			person: Some(user),
			link: Some("/user/tokens".to_owned()),
			..Telling::new(Rule::Kickback, derived_event_id(&event.each_ref().map(String::as_str)), now, said)
		};
		self.tell(tx, &telling, now).await?;
		Ok(())
	}

	/// `user` went on in `chat`: what waited there no longer does.
	pub async fn alerts_assistant_answered(&self, user: Uuid, chat: Uuid, now: Timestamp) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		alerts_db::read_linked(&mut tx, user, Rule::AssistantWaiting, &chat_link(chat), now).await?;
		tx.commit().await.wrap_err("committing an assistant's wait over")
	}

	/// `user`'s answer `answer` in chat `chat` ended proposing `what`, after they may have left it.
	pub async fn alerts_assistant_waits(&self, user: Uuid, chat: Uuid, answer: &str, what: &str, now: Timestamp) -> eyre::Result<()> {
		let mut said = text("The assistant waits for you to agree\n");
		said.code(&one_line(what, MAX_NAME));
		let telling = Telling {
			person: Some(user),
			link: Some(chat_link(chat)),
			..Telling::new(Rule::AssistantWaiting, derived_event_id(&["assistant_waiting", answer]), now, said)
		};
		let mut tx = self.store.begin_write().await?;
		self.tell(&mut tx, &telling, now).await?;
		tx.commit().await.wrap_err("committing an assistant's wait")
	}

	/// The user of `r` holds what they asked for.
	pub(crate) async fn tell_granted(&self, tx: &mut SqliteConnection, r: &AccessRequest, now: Timestamp) -> eyre::Result<()> {
		let mut said = text("Access granted: ");
		said.code(&r.need);
		let telling = Telling {
			person: Some(r.user_id),
			..Telling::new(Rule::AccessGranted, derived_event_id(&["access_granted", &r.event_id.to_string()]), now, said)
		};
		self.tell(tx, &telling, now).await?;
		Ok(())
	}

	/// `released`, the next version of `listing`, to whoever's category was taken from it.
	pub(crate) async fn tell_release(&self, tx: &mut SqliteConnection, listing: &Listing, released: &Released, now: Timestamp) -> eyre::Result<()> {
		for (owner, trade, country) in db::adopters(tx, listing.id, listing.author).await? {
			let mut said = text("New version of a category you use: ");
			said.code(&one_line(&listing.title, 40));
			if !released.notes.is_empty() {
				said.push("\n");
				said.code(&one_line(&released.notes, QUOTE));
			}
			let query = form_urlencoded::Serializer::new(String::new())
				.append_pair("trade", &trade)
				.append_pair("country", &country)
				.finish();
			let telling = Telling {
				person: Some(owner),
				link: Some(format!("/?{query}")),
				..Telling::new(
					Rule::ReleaseOut,
					derived_event_id(&["release_out", &released.id.to_string(), &owner.to_string(), &country]),
					now,
					said,
				)
			};
			self.tell(tx, &telling, now).await?;
		}
		Ok(())
	}

	/// What the trash is about to lose.
	pub(crate) async fn tell_purges(&self, tx: &mut SqliteConnection, keep_by_default: bool, now: Timestamp) -> eyre::Result<()> {
		for p in db::purging(tx, now, now + PURGE_SOON, PURGE_KEPT, keep_by_default).await? {
			let mut said = text("Deleted for good ");
			said.push(&p.first.strftime("%d %b").to_string());
			said.push(": ");
			said.code(&one_line(&p.name, 40));
			if p.count > 1 {
				said.push(&format!(" and {} more", p.count - 1));
			}
			said.push("\nRestore from the trash what you still need.");
			let telling = Telling {
				person: Some(p.owner),
				link: Some("/gbps/trash".to_owned()),
				..Telling::new(Rule::TrashPurge, derived_event_id(&["trash_purge", &p.owner.to_string(), &p.day.to_string()]), now, said)
			};
			self.tell(tx, &telling, now).await?;
		}
		Ok(())
	}

	/// `account`'s meters as just read, and what its guard did on its last tick.
	pub(crate) async fn tell_meters(&self, tx: &mut SqliteConnection, account: &str, gauges: &[Gauge], guard: &[Lever], now: Timestamp) -> eyre::Result<()> {
		for g in gauges {
			let Some(why) = g.refuses(1) else { continue };
			let said = format!("{} is at its stop on {account}\n{why}.", g.meter.name);
			let event = derived_event_id(&["meter_held", account, g.meter.id, &g.period.0.to_string()]);
			let telling = Telling {
				link: Some("/admin/tokens".to_owned()),
				..Telling::new(Rule::MeterHeld, event, now, text(&said))
			};
			self.tell(tx, &telling, now).await?;
		}
		// the guard's only action on real money
		let Some(unlinked) = guard.iter().filter_map(|l| l.action.as_deref()).find(|a| a.starts_with("unlink ")) else {
			return Ok(());
		};
		if let Some(event) = self.again(tx, Rule::MeterHeld, &["guard", account], now).await? {
			let said = format!("Billing unlinked on {account} by the guard\nIts Google keys answer nothing until an admin links it again: {unlinked}.");
			let telling = Telling {
				tier: Some(Tier::Urgent),
				link: Some("/admin/tokens".to_owned()),
				..Telling::new(Rule::MeterHeld, event, now, text(&said))
			};
			self.tell(tx, &telling, now).await?;
		}
		Ok(())
	}
}
