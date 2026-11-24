//! Telegram: link tokens, the chats destinations are reached at, the poller's lease and the bot's
//! replies. Every timestamp is the caller's `now`.

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_auth::PermissionSet;
use sa_core::{alerts::Method, notify::ChatId};
use social_networks_adapters::notify::telegram::Account;
use sqlx::{SqliteConnection, types::Json};
use uuid::Uuid;

use super::{alerts, from_db, outbox, to_db};

// ── link tokens ─────────────────────────────────────────────────────────────────────────

/// Stores a link token's hash, in place of any the user had before; expired ones are
/// dropped on the way.
pub async fn insert_link_token(conn: &mut SqliteConnection, hash: &[u8], user: Uuid, permissions: &PermissionSet, display: &str, now: Timestamp, expires_at: Timestamp) -> eyre::Result<()> {
	sqlx::query("DELETE FROM telegram_link_tokens WHERE expires_at <= $1 OR user_id = $2")
		.bind(to_db(now))
		.bind(user)
		.execute(&mut *conn)
		.await
		.wrap_err("pruning link tokens")?;
	sqlx::query("INSERT INTO telegram_link_tokens (token_hash, user_id, permissions, display_name, expires_at, issued_at) VALUES ($1, $2, $3, $4, $5, $6)")
		.bind(hash)
		.bind(user)
		.bind(Json(permissions))
		.bind(display)
		.bind(to_db(expires_at))
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("storing a link token")?;
	Ok(())
}

/// A redeemed link token: whose, their permissions and name, and when it was issued.
#[derive(Clone, Debug)]
pub struct Redeemed {
	pub user_id: Uuid,
	pub permissions: PermissionSet,
	pub display_name: String,
	pub issued_at: Timestamp,
}

/// Redeems a link token: whose it was, if it existed and had not expired. Gone either way,
/// so a token opens at most one link — unless the caller rolls its transaction back.
pub async fn redeem_link_token(conn: &mut SqliteConnection, hash: &[u8], now: Timestamp) -> eyre::Result<Option<Redeemed>> {
	type Row = (Uuid, Json<PermissionSet>, String, i64, Option<i64>);
	let row: Option<Row> = sqlx::query_as("DELETE FROM telegram_link_tokens WHERE token_hash = $1 RETURNING user_id, permissions, display_name, expires_at, issued_at")
		.bind(hash)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("redeeming a link token")?;
	let Some((user_id, Json(permissions), display_name, expires_at, issued_at)) = row else {
		return Ok(None);
	};
	let expires_at = from_db(expires_at)?;
	if expires_at <= now {
		return Ok(None);
	}
	Ok(Some(Redeemed {
		user_id,
		permissions,
		display_name,
		// A token from before `issued_at` was stored: the earliest it could have been issued.
		issued_at: issued_at.map(from_db).transpose()?.unwrap_or(expires_at - crate::telegram::LINK_TTL),
	}))
}

// ── chats ───────────────────────────────────────────────────────────────────────────────

/// What a destination's chat is.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::AsRefStr, strum::EnumString, schemars::JsonSchema, serde::Serialize)]
#[strum(serialize_all = "snake_case")]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "DestinationKind")]
pub enum Kind {
	/// The user's own chat with the bot: the only one PII and buttons go to.
	Private,
	Channel,
	/// A group, or one topic of a forum.
	Group,
}

/// A destination's chat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Chat {
	pub kind: Kind,
	pub id: i64,
	/// A forum's topic.
	pub topic_id: Option<i64>,
}

/// The user's private chat: its destination and chat id.
pub async fn private_of_user(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Option<(i64, i64)>> {
	sqlx::query_as(
		"SELECT t.destination_id, t.chat_id FROM telegram_destinations t JOIN alert_destinations d ON d.id = t.destination_id \
		 WHERE d.user_id = $1 AND t.kind = 'private'",
	)
	.bind(user)
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("reading a user's private chat")
}

/// Whose private chat `chat` is.
pub async fn owner_of_private(conn: &mut SqliteConnection, chat: i64) -> eyre::Result<Option<Uuid>> {
	sqlx::query_scalar(
		"SELECT d.user_id FROM telegram_destinations t JOIN alert_destinations d ON d.id = t.destination_id \
		 WHERE t.chat_id = $1 AND t.kind = 'private'",
	)
	.bind(chat)
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("reading a chat's link")
}

/// Links `chat` to the token's user as their private chat, their permissions confirmed as of the
/// token's issue. A chat they had before is let go, with what the outbox still owed it; the rules
/// they chose stay. The caller has made sure the chat is no one else's.
pub async fn link(conn: &mut SqliteConnection, t: &Redeemed, chat: i64, account: &Account, now: Timestamp) -> eyre::Result<()> {
	let title = account.username.as_ref().map(|u| format!("@{u}")).or_else(|| account.first_name.clone());
	match private_of_user(conn, t.user_id).await? {
		Some((destination, before)) => {
			if before != chat {
				outbox::drop_of_destination(conn, destination, "relinked").await?;
			}
			sqlx::query("UPDATE telegram_destinations SET chat_id = $2 WHERE destination_id = $1")
				.bind(destination)
				.bind(chat)
				.execute(&mut *conn)
				.await
				.wrap_err("relinking a chat")?;
			describe(conn, destination, Kind::Private, title.as_deref()).await?;
			alerts::revive(conn, destination).await?;
		}
		None => {
			insert(conn, t.user_id, Kind::Private, ChatId { v: chat }, None, title.as_deref(), now).await?;
		}
	}
	alerts::access_seen(conn, t.user_id, &t.permissions, &t.display_name, t.issued_at).await
}

/// A destination of the user's, at `chat`.
pub async fn insert(conn: &mut SqliteConnection, user: Uuid, kind: Kind, chat: ChatId, topic: Option<i64>, title: Option<&str>, now: Timestamp) -> eyre::Result<i64> {
	let destination = alerts::add(conn, user, Method::Telegram, kind == Kind::Private, title, None, now)
		.await?
		.expect("only a sealed address conflicts");
	sqlx::query("INSERT INTO telegram_destinations (destination_id, kind, chat_id, topic_id) VALUES ($1, $2, $3, $4)")
		.bind(destination)
		.bind(kind.as_ref())
		.bind(chat.v)
		.bind(topic)
		.execute(&mut *conn)
		.await
		.wrap_err("adding a chat")?;
	Ok(destination)
}

/// Whether a destination stands for `chat` (and `topic`) already.
pub async fn chat_taken(conn: &mut SqliteConnection, chat: i64, topic: Option<i64>) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM telegram_destinations WHERE chat_id = $1 AND ifnull(topic_id, 0) = ifnull($2, 0))")
		.bind(chat)
		.bind(topic)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a chat up")
}

/// Unlinks a private chat (`/stop`): `false` when it was not linked.
pub async fn unlink_private(conn: &mut SqliteConnection, chat: i64) -> eyre::Result<bool> {
	let gone = sqlx::query("DELETE FROM alert_destinations WHERE id = (SELECT destination_id FROM telegram_destinations WHERE chat_id = $1 AND kind = 'private')")
		.bind(chat)
		.execute(&mut *conn)
		.await
		.wrap_err("unlinking a chat")?;
	Ok(gone.rows_affected() == 1)
}

/// What a destination's chat is, as Telegram says now.
pub async fn describe(conn: &mut SqliteConnection, destination: i64, kind: Kind, title: Option<&str>) -> eyre::Result<()> {
	sqlx::query("UPDATE telegram_destinations SET kind = $2 WHERE destination_id = $1")
		.bind(destination)
		.bind(kind.as_ref())
		.execute(&mut *conn)
		.await
		.wrap_err("describing a chat")?;
	sqlx::query("UPDATE alert_destinations SET title = $2 WHERE id = $1")
		.bind(destination)
		.bind(title)
		.execute(&mut *conn)
		.await
		.wrap_err("titling a chat")?;
	Ok(())
}

/// Whether the bot may send an unsolicited reply to `chat` now: at most one per `every`.
pub async fn may_reply(conn: &mut SqliteConnection, chat: i64, now: Timestamp, every: SignedDuration) -> eyre::Result<bool> {
	let taken: Option<i64> = sqlx::query_scalar(
		"INSERT INTO telegram_replies (chat_id, last_at) VALUES ($1, $2) \
		 ON CONFLICT (chat_id) DO UPDATE SET last_at = $2 WHERE telegram_replies.last_at <= $3 RETURNING chat_id",
	)
	.bind(chat)
	.bind(to_db(now))
	.bind(to_db(now - every))
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("throttling a reply")?;
	Ok(taken.is_some())
}

/// Takes or renews the poller's lease for `holder` until `until`: the next update id to ask
/// for, or `None` when another replica holds it.
pub async fn poll_lease(conn: &mut SqliteConnection, holder: Uuid, now: Timestamp, until: Timestamp) -> eyre::Result<Option<i64>> {
	sqlx::query_scalar(
		"UPDATE telegram_poller SET holder = $1, leased_until = $3 \
		 WHERE holder = $1 OR holder IS NULL OR leased_until IS NULL OR leased_until <= $2 RETURNING next_update_id",
	)
	.bind(holder)
	.bind(to_db(now))
	.bind(to_db(until))
	.fetch_optional(&mut *conn)
	.await
	.wrap_err("leasing the poller")
}

/// Moves the offset past a handled update, if `holder` still holds the lease.
pub async fn poll_advance(conn: &mut SqliteConnection, holder: Uuid, next_update_id: i64) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE telegram_poller SET next_update_id = max(next_update_id, $2) WHERE holder = $1")
		.bind(holder)
		.bind(next_update_id)
		.execute(&mut *conn)
		.await
		.wrap_err("advancing the poller")?
		.rows_affected();
	Ok(done == 1)
}

/// Lets the lease go at shutdown, so another replica takes over at once.
pub async fn poll_release(conn: &mut SqliteConnection, holder: Uuid) -> eyre::Result<()> {
	sqlx::query("UPDATE telegram_poller SET holder = NULL, leased_until = NULL WHERE holder = $1")
		.bind(holder)
		.execute(&mut *conn)
		.await
		.wrap_err("releasing the poller")?;
	Ok(())
}
