//! The assistant's chats: the conductor's own state, nothing forwards them. A chat is its owner's;
//! members ask in it, rename and invite; general access lets anyone in Service-Arb read it. Whoever
//! may not read it is told it does not exist ([`Refused::NotFound`]). The turns' shape is the
//! caller's `T`: stored as the JSON it serializes to.
//!
//! Every change is told on the bus (topic `assistant`, the chat's id) to its owner and members,
//! those it removed among them; a mark only to whose it is.

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use serde::{Serialize, de::DeserializeOwned};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub use crate::store::assistant::{General, Mark, Person, Role, Summary};
use crate::{
	Panel, Refused, live,
	store::{assistant as db, users},
};

/// A chat's stored turns past this are refused.
pub const MAX_TURNS_BYTES: usize = 16 << 20;

const MAX_PEOPLE: usize = 20;

/// Who asks of a chat.
#[derive(Clone, Copy, Debug)]
pub struct Reader {
	pub user: Uuid,
	/// Holds any `sa` permission: a chat under general access is open to them.
	pub in_sa: bool,
}

#[derive(Clone, Debug)]
pub struct Chat<T> {
	pub summary: Summary,
	pub turns: Vec<T>,
}

#[derive(Clone, Debug)]
pub struct Access {
	pub owner: Person,
	pub members: Vec<Person>,
	pub general: General,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Needs {
	Read,
	Write,
	Own,
}

/// The chat as `r` sees it, if they may do what `needs`.
async fn may(conn: &mut SqliteConnection, id: Uuid, r: Reader, needs: Needs) -> eyre::Result<Result<Summary, Refused>> {
	let Some(chat) = db::chat(conn, id, r.user, r.in_sa).await? else {
		return Ok(Err(Refused::NotFound));
	};
	let allowed = match needs {
		Needs::Read => true,
		Needs::Write => chat.role != Role::Viewer,
		Needs::Own => chat.role == Role::Owner,
	};
	Ok(if allowed { Ok(chat) } else { Err(Refused::Forbidden) })
}

fn parse<T: DeserializeOwned>(id: Uuid, raw: &str) -> eyre::Result<Vec<T>> {
	serde_json::from_str(raw).wrap_err_with(|| format!("chat {id}'s stored turns"))
}

fn stored<T: Serialize>(turns: &[T]) -> eyre::Result<Result<String, Refused>> {
	let raw = serde_json::to_string(turns).wrap_err("serializing turns")?;
	Ok(match raw.len() > MAX_TURNS_BYTES {
		true => Err(Refused::TooLarge(format!("a chat's turns are at most {MAX_TURNS_BYTES} bytes"))),
		false => Ok(raw),
	})
}

fn title(raw: Option<&str>) -> Option<&str> {
	raw.map(str::trim).filter(|t| !t.is_empty())
}

async fn access(conn: &mut SqliteConnection, chat: Summary) -> eyre::Result<Access> {
	Ok(Access {
		members: db::members(conn, chat.id).await?,
		owner: chat.owner,
		general: chat.general,
	})
}

/// `q` as a whole address, which finds its person even outside the caller's chats.
fn is_email(q: &str) -> bool {
	q.split_once('@')
		.is_some_and(|(local, domain)| !local.is_empty() && !domain.contains('@') && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.'))
}

impl Panel {
	fn tell(&self, chat: Uuid, users: impl IntoIterator<Item = Uuid>) {
		let at = Timestamp::now();
		for user in users {
			self.live.changed(live::Change {
				topic: "assistant".into(),
				brand: None,
				id: Some(chat.to_string()),
				at,
				audience: live::Audience::User(user),
			});
		}
	}

	/// The chats `user` owns or is a member of, with their marks, newest first.
	pub async fn assistant_chats(&self, user: Uuid) -> eyre::Result<Vec<Summary>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for chats")?;
		db::chats(&mut conn, user).await
	}

	pub async fn assistant_chat<T: DeserializeOwned>(&self, r: Reader, id: Uuid) -> eyre::Result<Result<Chat<T>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a chat")?;
		let summary = match may(&mut conn, id, r, Needs::Read).await? {
			Ok(s) => s,
			Err(e) => return Ok(Err(e)),
		};
		let turns = parse(id, &db::turns(&mut conn, id).await?)?;
		Ok(Ok(Chat { summary, turns }))
	}

	/// A chat of `user`'s under the id their browser chose.
	pub async fn assistant_create<T: Serialize>(&self, user: Uuid, id: Uuid, raw_title: Option<&str>, turns: Vec<T>, now: Timestamp) -> eyre::Result<Result<Chat<T>, Refused>> {
		let raw = match stored(&turns)? {
			Ok(raw) => raw,
			Err(e) => return Ok(Err(e)),
		};
		let mut tx = self.store.begin_write().await?;
		if !db::create(&mut tx, id, user, title(raw_title), &raw, now).await? {
			return Ok(Err(Refused::Conflict(format!("chat {id} exists already"))));
		}
		let summary = db::chat(&mut tx, id, user, false).await?.expect("created just now, by them");
		tx.commit().await.wrap_err("committing a chat")?;
		self.tell(id, [user]);
		Ok(Ok(Chat { summary, turns }))
	}

	/// The turns from `from` on replaced by `turns`, if the chat is still at version `base`.
	pub async fn assistant_turns<T: Serialize + DeserializeOwned>(
		&self,
		r: Reader,
		id: Uuid,
		base: i64,
		from: usize,
		turns: Vec<T>,
		now: Timestamp,
	) -> eyre::Result<Result<Chat<T>, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let chat = match may(&mut tx, id, r, Needs::Write).await? {
			Ok(c) => c,
			Err(e) => return Ok(Err(e)),
		};
		if chat.version != base {
			return Ok(Err(Refused::Conflict(format!("the chat is at version {}, not {base}", chat.version))));
		}
		let mut all: Vec<T> = parse(id, &db::turns(&mut tx, id).await?)?;
		if from > all.len() {
			return Ok(Err(Refused::Invalid(format!("the chat has {} turns, none at {from}", all.len()))));
		}
		all.truncate(from);
		all.extend(turns);
		let raw = match stored(&all)? {
			Ok(raw) => raw,
			Err(e) => return Ok(Err(e)),
		};
		db::set_turns(&mut tx, id, &raw, now).await?;
		let summary = db::chat(&mut tx, id, r.user, r.in_sa).await?.expect("theirs to write, a moment ago");
		let audience = db::audience(&mut tx, id).await?;
		tx.commit().await.wrap_err("committing turns")?;
		self.tell(id, audience);
		Ok(Ok(Chat { summary, turns: all }))
	}

	/// Named `title`, trimmed; unnamed when it is empty.
	pub async fn assistant_rename<T: DeserializeOwned>(&self, r: Reader, id: Uuid, raw_title: Option<&str>) -> eyre::Result<Result<Chat<T>, Refused>> {
		let mut tx = self.store.begin_write().await?;
		if let Err(e) = may(&mut tx, id, r, Needs::Write).await? {
			return Ok(Err(e));
		}
		db::set_title(&mut tx, id, title(raw_title)).await?;
		let summary = db::chat(&mut tx, id, r.user, r.in_sa).await?.expect("theirs to write, a moment ago");
		let turns = parse(id, &db::turns(&mut tx, id).await?)?;
		let audience = db::audience(&mut tx, id).await?;
		tx.commit().await.wrap_err("committing a title")?;
		self.tell(id, audience);
		Ok(Ok(Chat { summary, turns }))
	}

	/// The caller's own pin or archive of a chat.
	pub async fn assistant_mark(&self, r: Reader, id: Uuid, mark: Mark, on: bool, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if let Err(e) = may(&mut tx, id, r, Needs::Write).await? {
			return Ok(Err(e));
		}
		db::mark(&mut tx, id, r.user, mark, on, now).await?;
		tx.commit().await.wrap_err("committing a mark")?;
		self.tell(id, [r.user]);
		Ok(Ok(()))
	}

	/// The owner's chat, with its members and marks.
	pub async fn assistant_delete(&self, r: Reader, id: Uuid) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if let Err(e) = may(&mut tx, id, r, Needs::Own).await? {
			return Ok(Err(e));
		}
		let audience = db::audience(&mut tx, id).await?;
		db::delete(&mut tx, id).await?;
		tx.commit().await.wrap_err("committing a chat's deletion")?;
		self.tell(id, audience);
		Ok(Ok(()))
	}

	/// Every chat `user` archived: deleted if theirs, else left.
	pub async fn assistant_clear_archived(&self, user: Uuid) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		let mut told = Vec::new();
		for (id, owned) in db::archived(&mut tx, user).await? {
			told.push((id, db::audience(&mut tx, id).await?));
			match owned {
				true => db::delete(&mut tx, id).await?,
				false => assert!(db::remove_member(&mut tx, id, user).await?, "a chat marked is the owner's or a member's"),
			}
		}
		tx.commit().await.wrap_err("committing the archived chats' clearing")?;
		for (id, audience) in told {
			self.tell(id, audience);
		}
		Ok(())
	}

	pub async fn assistant_access(&self, r: Reader, id: Uuid) -> eyre::Result<Result<Access, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a chat's access")?;
		match may(&mut conn, id, r, Needs::Read).await? {
			Ok(chat) => Ok(Ok(access(&mut conn, chat).await?)),
			Err(e) => Ok(Err(e)),
		}
	}

	/// `users` made members; the owner and members already are left as they are.
	pub async fn assistant_invite(&self, r: Reader, id: Uuid, invited: &[Uuid], now: Timestamp) -> eyre::Result<Result<Access, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let chat = match may(&mut tx, id, r, Needs::Write).await? {
			Ok(c) => c,
			Err(e) => return Ok(Err(e)),
		};
		let mut added = false;
		for &user in invited {
			if !users::exists(&mut tx, user).await? {
				return Ok(Err(Refused::Invalid(format!("no user {user}"))));
			}
			added |= db::add_member(&mut tx, id, user, r.user, now).await?;
		}
		let audience = db::audience(&mut tx, id).await?;
		let access = access(&mut tx, chat).await?;
		tx.commit().await.wrap_err("committing members")?;
		if added {
			self.tell(id, audience);
		}
		Ok(Ok(access))
	}

	/// `member` no longer one: by the owner, or by themselves.
	pub async fn assistant_remove_member(&self, r: Reader, id: Uuid, member: Uuid) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		let chat = match may(&mut tx, id, r, Needs::Write).await? {
			Ok(c) => c,
			Err(e) => return Ok(Err(e)),
		};
		if chat.role != Role::Owner && member != r.user {
			return Ok(Err(Refused::Forbidden));
		}
		let audience = db::audience(&mut tx, id).await?;
		if !db::remove_member(&mut tx, id, member).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a member's removal")?;
		self.tell(id, audience);
		Ok(Ok(()))
	}

	pub async fn assistant_set_general(&self, r: Reader, id: Uuid, general: General) -> eyre::Result<Result<Access, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let mut chat = match may(&mut tx, id, r, Needs::Own).await? {
			Ok(c) => c,
			Err(e) => return Ok(Err(e)),
		};
		db::set_general(&mut tx, id, general).await?;
		chat.general = general;
		let audience = db::audience(&mut tx, id).await?;
		let access = access(&mut tx, chat).await?;
		tx.commit().await.wrap_err("committing general access")?;
		self.tell(id, audience);
		Ok(Ok(access))
	}

	/// Whom `user` may invite matching `q`: those they share a chat with, by name or email; and
	/// whoever has `q` as their whole email. Nobody else is ever listed.
	pub async fn assistant_people(&self, user: Uuid, q: &str) -> eyre::Result<Vec<Person>> {
		let q = q.trim().to_lowercase();
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for people")?;
		let mut people: Vec<Person> = db::co_members(&mut conn, user)
			.await?
			.into_iter()
			.filter(|p| p.name.to_lowercase().contains(&q) || p.email.to_lowercase().contains(&q))
			.collect();
		if is_email(&q) {
			people.extend(db::by_email(&mut conn, &q).await?);
		}
		let mut seen = BTreeSet::from([user]);
		people.retain(|p| seen.insert(p.id));
		people.truncate(MAX_PEOPLE);
		Ok(people)
	}
}
