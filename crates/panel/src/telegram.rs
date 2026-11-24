//! Telegram, beside what it shares with every method ([`crate::deliver`]): a user's private chat
//! linked by `/start <token>`, a channel, group or forum topic they administer, and what the bot
//! does with the updates it receives — `/start <token>`, `/stop` and the buttons under a message.
//!
//! ```text
//! POST /api/v1/alerts/telegram/link ─ token (256 bits; its hash, 10 min) ─ t.me/<bot>?start=<token>
//!                         /start <token> in a private chat ─ the user's private destination
//! a channel's paste ─ getChat ─ getChatMember(the user's private chat id): an admin ─ a test sent
//! button ─ signed data ─ the chat's user, access asked of concierge now ─ its service, as that user
//! ```
//!
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_alerts::Button;
use sa_auth::PermissionSet;
use sa_core::notify::{self, ChatId, OutboxId, Rendered, Reply};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

pub use social_networks_adapters::notify::telegram::{Account, CallbackId, Chat, ChatInfo, Target, Update};

pub use crate::store::telegram::Kind;
use crate::{
	Panel, Refused,
	alerts::{Access, Address, Directory, Feeds, Hub, Message, Outlets, Pressed},
	deliver::failure_text,
	session::Refresher,
	store::{alerts as hub_db, outbox, telegram as db},
};

/// How long a link token may wait for its `/start`.
pub const LINK_TTL: SignedDuration = SignedDuration::from_mins(10);

/// At most one unsolicited reply (help, "not linked", …) per chat this often.
pub const REPLY_EVERY: SignedDuration = SignedDuration::from_mins(10);

fn link_token_hash(token: &str) -> [u8; 32] {
	let mut h = Sha256::new();
	h.update(b"sa-panel/tg-link/v1/");
	h.update(token.as_bytes());
	h.finalize().into()
}

impl Panel {
	/// A one-time token for `t.me/<bot>?start=<token>`, for a signed-in user: 256 random bits,
	/// base64url (the 43 characters a start payload may hold); only its hash is stored, for
	/// [`LINK_TTL`].
	pub async fn telegram_link_token(&self, user: Uuid, permissions: &PermissionSet, display: &str, now: Timestamp) -> eyre::Result<Zeroizing<String>> {
		let mut raw = Zeroizing::new([0u8; 32]);
		getrandom::fill(raw.as_mut_slice()).map_err(|e| eyre::eyre!("the OS random source failed: {e}"))?;
		let token = Zeroizing::new(URL_SAFE_NO_PAD.encode(raw.as_slice()));
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a link token")?;
		db::insert_link_token(&mut conn, &link_token_hash(&token), user, permissions, display, now, now + LINK_TTL).await?;
		Ok(token)
	}

	/// Takes or renews the poller's lease for `holder`: the update id to ask Telegram for
	/// next, or `None` while another replica polls.
	pub async fn telegram_poll_lease(&self, holder: Uuid, now: Timestamp, until: Timestamp) -> eyre::Result<Option<i64>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the poller")?;
		db::poll_lease(&mut conn, holder, now, until).await
	}

	/// Records that updates before `next_update_id` are handled; `false` when `holder` has
	/// lost the lease meanwhile.
	pub async fn telegram_poll_advance(&self, holder: Uuid, next_update_id: i64) -> eyre::Result<bool> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the poller")?;
		db::poll_advance(&mut conn, holder, next_update_id).await
	}

	pub async fn telegram_poll_release(&self, holder: Uuid) -> eyre::Result<()> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the poller")?;
		db::poll_release(&mut conn, holder).await
	}
}

impl<O: Outlets, C: Refresher + Directory, F: Feeds> Hub<O, C, F> {
	/// A message's buttons in `chat_id`: `(label, signed data)`.
	pub(crate) fn buttons(&self, chat_id: ChatId, message: OutboxId, buttons: &[Button]) -> Vec<(String, String)> {
		let key = self.panel.key.telegram_callback_key();
		buttons
			.iter()
			.map(|b| (b.label.clone(), notify::callback_data(&key, chat_id, message, &b.id)))
			.collect()
	}

	/// Adds a channel, a group or a forum topic, as `named`, as a destination of the
	/// user's: only once the bot sees it, the user administers it (asked with their private chat's
	/// id — they link that first) and a test went out there. Nothing is stored otherwise.
	pub async fn add_chat(
		&self,
		user: Uuid,
		permissions: &PermissionSet,
		display: &str,
		named: tg_types::TelegramDestination,
		now: Timestamp,
	) -> eyre::Result<Result<i64, Refused>> {
		let refuse = |why: String| Ok(Err(Refused::Invalid(why)));
		let (chat, topic) = match &named {
			tg_types::TelegramDestination::Channel(id) | tg_types::TelegramDestination::Group(id) => (id.chat_id_param(), None),
			tg_types::TelegramDestination::GroupTopic { group, topic_id } => (group.chat_id_param(), Some(i64::try_from(*topic_id)?)),
		};
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to add a chat")?;
		let Some((_, own_chat)) = db::private_of_user(&mut conn, user).await? else {
			return refuse("link your own Telegram first: the bot asks Telegram whether that account administers the chat".into());
		};
		drop(conn);
		let info = match self.outlets.chat(&chat).await {
			Ok(info) => info,
			Err(f) => return refuse(format!("the bot cannot see {chat}: add it there first ({})", failure_text(&f))),
		};
		let kind = match info.kind.as_str() {
			"channel" => Kind::Channel,
			"group" | "supergroup" => Kind::Group,
			_ => return refuse(format!("{chat} is a private chat; link your own with the link button")),
		};
		if topic.is_some() && !info.forum {
			return refuse(format!("{chat} has no topics"));
		}
		match self.outlets.is_admin(info.id, own_chat).await {
			Ok(true) => {}
			Ok(false) => return refuse(format!("your linked Telegram account does not administer {chat}")),
			Err(f) => return refuse(format!("Telegram did not say whether you administer {chat} ({})", failure_text(&f))),
		}
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to add a chat")?;
		if db::chat_taken(&mut conn, info.id, topic).await? {
			return Ok(Err(Refused::Conflict(format!("{chat} is a destination already"))));
		}
		drop(conn);
		let to = Address::Telegram(Target { chat_id: info.id, topic_id: topic });
		if let Err(f) = self.outlets.send_to(&to, &self.test_message()).await {
			return refuse(format!("the bot cannot post in {chat}: {}", failure_text(&f)));
		}
		let mut tx = self.panel.store.begin_write().await?;
		if db::chat_taken(&mut tx, info.id, topic).await? {
			return Ok(Err(Refused::Conflict(format!("{chat} is a destination already"))));
		}
		let destination = db::insert(&mut tx, user, kind, ChatId { v: info.id }, topic, info.title.as_deref(), now).await?;
		hub_db::access_seen(&mut tx, user, permissions, display, now).await?;
		tx.commit().await.wrap_err("committing a chat")?;
		tracing::info!(user_id = %user, destination, "telegram: a chat added");
		Ok(Ok(destination))
	}

	async fn say(&self, chat: i64, text: String) {
		let message = Message {
			text: Rendered { text, entities: Vec::new() },
			..self.test_message()
		};
		let to = Address::Telegram(Target { chat_id: chat, topic_id: None });
		if let Err(f) = self.outlets.send_to(&to, &message).await {
			tracing::warn!(failure = ?f, "telegram: a reply was not delivered");
		}
	}

	async fn reply(&self, chat: i64, reply: Reply) {
		self.say(chat, reply.text(self.locale).to_owned()).await;
	}

	/// A reply nobody asked a linked chat for (help, "not linked", …): at most one per chat per
	/// [`REPLY_EVERY`], so a stranger cannot make the bot spend its rate on them.
	async fn reply_unsolicited(&self, chat: i64, reply: Reply, now: Timestamp) -> eyre::Result<()> {
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to throttle a reply")?;
		if db::may_reply(&mut conn, chat, now, REPLY_EVERY).await? {
			drop(conn);
			self.reply(chat, reply).await;
		}
		Ok(())
	}

	/// Handles one update. Groups and channels are ignored whatever they say, and so is any
	/// message that is not a command: the bot only ever talks to a person, one to one, and
	/// only when asked.
	pub async fn handle(&self, update: Update, now: Timestamp) -> eyre::Result<()> {
		match update {
			Update::Start { chat, .. } | Update::Stop { chat } | Update::Other { chat } if !chat.private => Ok(()),
			Update::Other { .. } => Ok(()),
			Update::Start { chat, payload: Some(token), from } => self.start(chat.id, token.trim(), &from, now).await,
			Update::Start { chat, payload: None, .. } => self.reply_unsolicited(chat.id, Reply::Help, now).await,
			Update::Stop { chat } => {
				let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to unlink")?;
				let gone = db::unlink_private(&mut conn, chat.id).await?;
				drop(conn);
				if gone {
					self.reply(chat.id, Reply::Unlinked).await;
					Ok(())
				} else {
					self.reply_unsolicited(chat.id, Reply::NotLinked, now).await
				}
			}
			Update::Callback {
				id,
				chat_id,
				message_id,
				data,
				message,
			} => {
				let (reply, edit) = self.pressed(chat_id, &data, now).await?;
				if let Some(Edit { message: outbox, line, left }) = edit {
					let mut edited = Rendered::default();
					edited.push(&message.text);
					edited.entities = message.entities.iter().map(|e| serde_json::to_value(e).expect("an entity is JSON")).collect();
					edited.push("\n\n");
					edited.push(&line);
					let buttons = self.buttons(ChatId { v: chat_id }, outbox, &left);
					if let Err(f) = self.outlets.edit(chat_id, message_id, &edited, &buttons).await {
						tracing::warn!(failure = ?f, "telegram: a pressed message was not updated");
					}
				}
				if let Err(f) = self.outlets.answer(&id, reply.text(self.locale)).await {
					tracing::warn!(failure = ?f, "telegram: a button press was not answered");
				}
				Ok(())
			}
		}
	}

	/// `/start <token>`: the token redeemed and the chat linked in one transaction — rolled
	/// back, the token kept, when the chat is another panel account's.
	async fn start(&self, chat: i64, token: &str, from: &Account, now: Timestamp) -> eyre::Result<()> {
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection to link")?;
		let mut tx = crate::store::begin_write(&mut conn).await?;
		let Some(redeemed) = db::redeem_link_token(&mut tx, &link_token_hash(token), now).await? else {
			tx.commit().await.wrap_err("committing a spent token")?;
			drop(conn);
			return self.reply_unsolicited(chat, Reply::LinkInvalid, now).await;
		};
		if db::owner_of_private(&mut tx, chat).await?.is_some_and(|user| user != redeemed.user_id) {
			drop(tx);
			drop(conn);
			tracing::info!(user_id = %redeemed.user_id, "telegram: a link refused, the chat is another account's");
			self.reply(chat, Reply::ChatTaken).await;
			return Ok(());
		}
		db::link(&mut tx, &redeemed, chat, from, now).await?;
		tx.commit().await.wrap_err("committing a link")?;
		drop(conn);
		tracing::info!(user_id = %redeemed.user_id, "telegram: chat linked");
		self.say(chat, notify::linked_text(self.locale, &redeemed.display_name)).await;
		Ok(())
	}

	/// A button pressed in `chat_id`: what to answer, and — when something was recorded now —
	/// how the message changes. Nothing in the button is trusted but its signature: the message
	/// it names must be this chat's, the chat must be the private chat of the message's user, and
	/// that user's access is asked of concierge now; its service then judges the call as that user's.
	async fn pressed(&self, chat_id: i64, data: &str, now: Timestamp) -> eyre::Result<(Reply, Option<Edit>)> {
		let Some((message, button)) = notify::parse_callback(&self.panel.key.telegram_callback_key(), ChatId { v: chat_id }, data) else {
			tracing::warn!("telegram: a button with data the panel did not sign");
			return Ok((Reply::BadButton, None));
		};
		let mut conn = self.panel.store.pool().acquire().await.wrap_err("a connection for a button")?;
		let sent = outbox::message(&mut conn, message).await?;
		let owner = db::owner_of_private(&mut conn, chat_id).await?;
		drop(conn);
		let Some(sent) = sent.filter(|m| m.lane == format!("chat {chat_id}")) else {
			return Ok((Reply::BadButton, None));
		};
		if owner != Some(sent.user_id) {
			return Ok((Reply::NotLinked, None));
		}
		let identity = match self.confirm_access(sent.user_id, now).await? {
			Access::Granted(identity) => identity,
			Access::NoSession => return Ok((Reply::OpenPanel, None)),
			Access::Unavailable => return Ok((Reply::TryLater, None)),
		};
		Ok(match self.press(&identity, sent.note_id, &button, now).await? {
			Pressed::Done { line, left } => (Reply::Done, Some(Edit { message, line, left })),
			Pressed::AlreadyDone => (Reply::AlreadyDone, None),
			Pressed::Gone => (Reply::LeadGone, None),
			Pressed::NoAccess => (Reply::NoAccess, None),
			Pressed::TryLater => (Reply::TryLater, None),
			Pressed::Unknown => (Reply::BadButton, None),
		})
	}
}

/// A press recorded: the line its message gains, and the buttons it leaves.
struct Edit {
	message: OutboxId,
	line: String,
	left: Vec<Button>,
}
