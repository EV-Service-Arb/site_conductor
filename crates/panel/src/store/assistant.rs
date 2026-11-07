//! The assistant's chats, their members and each person's marks on them.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};

type SummaryRow = (Uuid, Option<String>, Uuid, String, String, Option<String>, String, bool, Option<i64>, Option<i64>, i64, i64);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Person {
	pub id: Uuid,
	pub name: String,
	pub email: String,
}

/// What a person may do with a chat.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum Role {
	Owner,
	Member,
	/// By general access.
	Viewer,
}

/// Who beyond the owner and members may read a chat.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::EnumString, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum General {
	None,
	/// Whoever holds any `sa` permission.
	View,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Summary {
	pub id: Uuid,
	pub title: Option<String>,
	pub owner: Person,
	pub role: Role,
	/// It has a member, or general access.
	pub shared: bool,
	/// The person's own.
	pub pinned_at: Option<Timestamp>,
	/// The person's own.
	pub archived_at: Option<Timestamp>,
	pub updated_at: Timestamp,
	pub general: General,
	pub version: i64,
}

#[derive(Clone, Copy, Debug)]
pub enum Mark {
	Pinned,
	Archived,
}

/// `$1` the person, `$2` whether they are in Service-Arb; `role` NULL: not theirs to read.
macro_rules! summary {
	($tail:literal) => {
		concat!(
			"SELECT c.id, c.title, c.owner, o.name, o.email, \
	 CASE WHEN c.owner = $1 THEN 'owner' \
	      WHEN EXISTS (SELECT 1 FROM assistant_members m WHERE m.chat = c.id AND m.user_id = $1) THEN 'member' \
	      WHEN c.general = 'view' AND $2 THEN 'viewer' END AS role, \
	 c.general, c.general = 'view' OR EXISTS (SELECT 1 FROM assistant_members m WHERE m.chat = c.id), \
	 k.pinned_at, k.archived_at, c.updated_at, c.version \
	 FROM assistant_chats c JOIN users o ON o.user_id = c.owner \
	 LEFT JOIN assistant_marks k ON k.chat = c.id AND k.user_id = $1 ",
			$tail
		)
	};
}

/// The chats `user` owns or is a member of, newest first.
pub async fn chats(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Vec<Summary>> {
	let rows: Vec<SummaryRow> = sqlx::query_as(summary!("WHERE role IN ('owner', 'member') ORDER BY c.updated_at DESC, c.id"))
		.bind(user)
		.bind(false)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing chats")?;
	Ok(rows.into_iter().map(summary).collect::<eyre::Result<Vec<_>>>()?.into_iter().flatten().collect())
}

/// Chat `id` as `user` sees it; `None`: not theirs to read, or none.
pub async fn chat(conn: &mut SqliteConnection, id: Uuid, user: Uuid, in_sa: bool) -> eyre::Result<Option<Summary>> {
	let row: Option<SummaryRow> = sqlx::query_as(summary!("WHERE c.id = $3"))
		.bind(user)
		.bind(in_sa)
		.bind(id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading a chat")?;
	Ok(row.map(summary).transpose()?.flatten())
}

pub async fn turns(conn: &mut SqliteConnection, id: Uuid) -> eyre::Result<String> {
	sqlx::query_scalar("SELECT turns FROM assistant_chats WHERE id = $1")
		.bind(id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("reading a chat's turns")
}

/// `false`: the id is taken.
pub async fn create(conn: &mut SqliteConnection, id: Uuid, owner: Uuid, title: Option<&str>, turns: &str, now: Timestamp) -> eyre::Result<bool> {
	let added = sqlx::query(
		"INSERT INTO assistant_chats (id, owner, title, general, turns, version, created_at, updated_at) \
		 VALUES ($1, $2, $3, 'none', $4, 1, $5, $5) ON CONFLICT (id) DO NOTHING",
	)
	.bind(id)
	.bind(owner)
	.bind(title)
	.bind(turns)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("creating a chat")?;
	Ok(added.rows_affected() == 1)
}

pub async fn set_turns(conn: &mut SqliteConnection, id: Uuid, turns: &str, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("UPDATE assistant_chats SET turns = $2, version = version + 1, updated_at = $3 WHERE id = $1")
		.bind(id)
		.bind(turns)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("writing a chat's turns")?;
	Ok(())
}

pub async fn set_title(conn: &mut SqliteConnection, id: Uuid, title: Option<&str>) -> eyre::Result<()> {
	sqlx::query("UPDATE assistant_chats SET title = $2 WHERE id = $1")
		.bind(id)
		.bind(title)
		.execute(&mut *conn)
		.await
		.wrap_err("renaming a chat")?;
	Ok(())
}

pub async fn set_general(conn: &mut SqliteConnection, id: Uuid, general: General) -> eyre::Result<()> {
	sqlx::query("UPDATE assistant_chats SET general = $2 WHERE id = $1")
		.bind(id)
		.bind(<&str>::from(general))
		.execute(&mut *conn)
		.await
		.wrap_err("setting a chat's general access")?;
	Ok(())
}

/// The person's mark set at `now`, kept if set already, or cleared.
pub async fn mark(conn: &mut SqliteConnection, id: Uuid, user: Uuid, mark: Mark, on: bool, now: Timestamp) -> eyre::Result<()> {
	let at = on.then_some(to_db(now));
	let sql = match mark {
		Mark::Pinned =>
			"INSERT INTO assistant_marks (chat, user_id, pinned_at) VALUES ($1, $2, $3) \
			 ON CONFLICT (chat, user_id) DO UPDATE SET pinned_at = CASE WHEN $3 IS NULL THEN NULL ELSE coalesce(pinned_at, $3) END",
		Mark::Archived =>
			"INSERT INTO assistant_marks (chat, user_id, archived_at) VALUES ($1, $2, $3) \
			 ON CONFLICT (chat, user_id) DO UPDATE SET archived_at = CASE WHEN $3 IS NULL THEN NULL ELSE coalesce(archived_at, $3) END",
	};
	sqlx::query(sql).bind(id).bind(user).bind(at).execute(&mut *conn).await.wrap_err("marking a chat")?;
	Ok(())
}

/// The owner and the members: whom a change to the chat is told to.
pub async fn audience(conn: &mut SqliteConnection, id: Uuid) -> eyre::Result<Vec<Uuid>> {
	sqlx::query_scalar("SELECT owner FROM assistant_chats WHERE id = $1 UNION SELECT user_id FROM assistant_members WHERE chat = $1")
		.bind(id)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("reading a chat's audience")
}

pub async fn members(conn: &mut SqliteConnection, id: Uuid) -> eyre::Result<Vec<Person>> {
	let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
		"SELECT u.user_id, u.name, u.email FROM assistant_members m JOIN users u ON u.user_id = m.user_id \
		 WHERE m.chat = $1 ORDER BY m.added_at, u.email",
	)
	.bind(id)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a chat's members")?;
	Ok(rows.into_iter().map(|(id, name, email)| Person { id, name, email }).collect())
}

/// `false`: they were the owner or a member already.
pub async fn add_member(conn: &mut SqliteConnection, id: Uuid, user: Uuid, by: Uuid, now: Timestamp) -> eyre::Result<bool> {
	let added = sqlx::query(
		"INSERT INTO assistant_members (chat, user_id, added_by, added_at) \
		 SELECT $1, $2, $3, $4 FROM assistant_chats WHERE id = $1 AND owner != $2 \
		 ON CONFLICT (chat, user_id) DO NOTHING",
	)
	.bind(id)
	.bind(user)
	.bind(by)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("adding a chat's member")?;
	Ok(added.rows_affected() == 1)
}

/// Their membership and marks gone; `false`: they were no member.
pub async fn remove_member(conn: &mut SqliteConnection, id: Uuid, user: Uuid) -> eyre::Result<bool> {
	let gone = sqlx::query("DELETE FROM assistant_members WHERE chat = $1 AND user_id = $2")
		.bind(id)
		.bind(user)
		.execute(&mut *conn)
		.await
		.wrap_err("removing a chat's member")?;
	sqlx::query("DELETE FROM assistant_marks WHERE chat = $1 AND user_id = $2")
		.bind(id)
		.bind(user)
		.execute(&mut *conn)
		.await
		.wrap_err("removing a member's marks")?;
	Ok(gone.rows_affected() == 1)
}

/// With its members and marks.
pub async fn delete(conn: &mut SqliteConnection, id: Uuid) -> eyre::Result<()> {
	sqlx::query("DELETE FROM assistant_chats WHERE id = $1")
		.bind(id)
		.execute(&mut *conn)
		.await
		.wrap_err("deleting a chat")?;
	Ok(())
}

/// The chats `user` archived, each with whether they own it.
pub async fn archived(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Vec<(Uuid, bool)>> {
	sqlx::query_as(
		"SELECT c.id, c.owner = $1 FROM assistant_marks k JOIN assistant_chats c ON c.id = k.chat \
		 WHERE k.user_id = $1 AND k.archived_at IS NOT NULL",
	)
	.bind(user)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing archived chats")
}

/// Everyone who owns or is a member of a chat `user` owns or is a member of, `user` among them.
pub async fn co_members(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Vec<Person>> {
	let rows: Vec<(Uuid, String, String)> = sqlx::query_as(
		"WITH mine (chat) AS (SELECT id FROM assistant_chats WHERE owner = $1 UNION SELECT chat FROM assistant_members WHERE user_id = $1), \
		 them (user_id) AS (SELECT owner FROM assistant_chats WHERE id IN (SELECT chat FROM mine) UNION SELECT user_id FROM assistant_members WHERE chat IN (SELECT chat FROM mine)) \
		 SELECT u.user_id, u.name, u.email FROM users u WHERE u.user_id IN (SELECT user_id FROM them) ORDER BY u.name, u.email",
	)
	.bind(user)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing co-members")?;
	Ok(rows.into_iter().map(|(id, name, email)| Person { id, name, email }).collect())
}

/// Everyone last seen with `email`, compared case-insensitively.
pub async fn by_email(conn: &mut SqliteConnection, email: &str) -> eyre::Result<Vec<Person>> {
	let rows: Vec<(Uuid, String, String)> = sqlx::query_as("SELECT user_id, name, email FROM users WHERE email = $1 COLLATE NOCASE")
		.bind(email)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("looking a person up by email")?;
	Ok(rows.into_iter().map(|(id, name, email)| Person { id, name, email }).collect())
}

fn summary(row: SummaryRow) -> eyre::Result<Option<Summary>> {
	let (id, title, owner, name, email, role, general, shared, pinned_at, archived_at, updated_at, version) = row;
	let Some(role) = role else { return Ok(None) };
	Ok(Some(Summary {
		id,
		title,
		owner: Person { id: owner, name, email },
		role: role.parse().wrap_err_with(|| format!("stored role {role}"))?,
		shared,
		pinned_at: pinned_at.map(from_db).transpose()?,
		archived_at: archived_at.map(from_db).transpose()?,
		updated_at: from_db(updated_at)?,
		general: general.parse().wrap_err_with(|| format!("stored general access {general}"))?,
		version,
	}))
}
