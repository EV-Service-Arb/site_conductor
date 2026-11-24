//! A gmail's connections, their secrets sealed.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};
use crate::own_alerts::Lost;

/// What the last check said: `None`, it went through.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checked {
	pub at: Timestamp,
	pub error: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Imap {
	pub host: String,
	pub username: String,
	pub set_at: Timestamp,
	pub checked: Checked,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MapsKey {
	pub last4: String,
	pub set_at: Timestamp,
	pub checked: Checked,
}

/// A gmail's Business Profile client and its grant to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Business {
	pub client_id: String,
	pub set_at: Timestamp,
	/// `None`: the gmail has not granted `business.manage` yet.
	pub granted_at: Option<Timestamp>,
	/// `None`: Google not asked since it was set.
	pub checked: Option<Checked>,
}

/// Which connection of a gmail's.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display, strum::EnumString, strum::VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum Connection {
	Imap,
	MapsKey,
	Business,
}
impl Connection {
	fn table(self) -> &'static str {
		match self {
			Self::Imap => "gmail_imap",
			Self::MapsKey => "gmail_maps_keys",
			Self::Business => "gmail_business",
		}
	}
}

/// Where an inbox is, and whose.
#[derive(Clone, Copy)]
pub(crate) struct Inbox<'a> {
	pub host: &'a str,
	pub username: &'a str,
}

/// A secret as it sits in the database.
pub(crate) struct Sealed {
	pub blob: Vec<u8>,
	pub data_key_fp: Vec<u8>,
}

pub(crate) async fn imap(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<Option<(Imap, Sealed)>> {
	type Row = (String, String, Vec<u8>, Vec<u8>, i64, i64, Option<String>);
	let row: Option<Row> = sqlx::query_as("SELECT host, username, password_sealed, data_key_fp, set_at, checked_at, check_error FROM gmail_imap WHERE gmail_id = $1")
		.bind(gmail_id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a gmail's IMAP")?;
	row.map(|(host, username, blob, data_key_fp, set_at, at, error)| {
		let imap = Imap {
			host,
			username,
			set_at: from_db(set_at)?,
			checked: Checked { at: from_db(at)?, error },
		};
		Ok((imap, Sealed { blob, data_key_fp }))
	})
	.transpose()
}

pub(crate) async fn maps_key(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<Option<(MapsKey, Sealed)>> {
	type Row = (String, Vec<u8>, Vec<u8>, i64, i64, Option<String>);
	let row: Option<Row> = sqlx::query_as("SELECT last4, key_sealed, data_key_fp, set_at, checked_at, check_error FROM gmail_maps_keys WHERE gmail_id = $1")
		.bind(gmail_id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a gmail's Maps key")?;
	row.map(|(last4, blob, data_key_fp, set_at, at, error)| {
		let key = MapsKey {
			last4,
			set_at: from_db(set_at)?,
			checked: Checked { at: from_db(at)?, error },
		};
		Ok((key, Sealed { blob, data_key_fp }))
	})
	.transpose()
}

/// The gmail's Business Profile client, its sealed secret, and its sealed grant once given.
pub(crate) async fn business(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<Option<(Business, Sealed, Option<Sealed>)>> {
	type Row = (String, Vec<u8>, Option<Vec<u8>>, Vec<u8>, i64, Option<i64>, Option<i64>, Option<String>);
	let row: Option<Row> = sqlx::query_as("SELECT client_id, secret_sealed, grant_sealed, data_key_fp, set_at, granted_at, checked_at, check_error FROM gmail_business WHERE gmail_id = $1")
		.bind(gmail_id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a gmail's Business Profile client")?;
	row.map(|(client_id, secret, grant, data_key_fp, set_at, granted_at, checked_at, error)| {
		let business = Business {
			client_id,
			set_at: from_db(set_at)?,
			granted_at: granted_at.map(from_db).transpose()?,
			checked: checked_at.map(|at| eyre::Ok(Checked { at: from_db(at)?, error })).transpose()?,
		};
		let grant = grant.map(|blob| Sealed {
			blob,
			data_key_fp: data_key_fp.clone(),
		});
		Ok((business, Sealed { blob: secret, data_key_fp }, grant))
	})
	.transpose()
}

pub(crate) async fn address(conn: &mut SqliteConnection, gmail_id: i64) -> eyre::Result<String> {
	sqlx::query_scalar("SELECT gmail FROM gmails WHERE id = $1")
		.bind(gmail_id)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("loading a gmail's address")
}

/// A client set anew drops the grant given to the one before.
pub(crate) async fn set_business(conn: &mut SqliteConnection, gmail_id: i64, client_id: &str, secret: &Sealed, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO gmail_business (gmail_id, client_id, secret_sealed, grant_sealed, data_key_fp, set_at, granted_at, checked_at, check_error) 		 VALUES ($1, $2, $3, NULL, $4, $5, NULL, NULL, NULL) 		 ON CONFLICT (gmail_id) DO UPDATE SET client_id = excluded.client_id, secret_sealed = excluded.secret_sealed, grant_sealed = NULL, 		 data_key_fp = excluded.data_key_fp, set_at = excluded.set_at, granted_at = NULL, checked_at = NULL, check_error = NULL",
	)
	.bind(gmail_id)
	.bind(client_id)
	.bind(&secret.blob)
	.bind(&secret.data_key_fp)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a gmail's Business Profile client")?;
	Ok(())
}

/// `false`: the client is not `client_id` any more, or its secret was sealed under another key.
pub(crate) async fn set_business_grant(conn: &mut SqliteConnection, gmail_id: i64, client_id: &str, grant: &Sealed, now: Timestamp) -> eyre::Result<bool> {
	let done = sqlx::query("UPDATE gmail_business SET grant_sealed = $3, granted_at = $5 WHERE gmail_id = $1 AND client_id = $2 AND data_key_fp = $4")
		.bind(gmail_id)
		.bind(client_id)
		.bind(&grant.blob)
		.bind(&grant.data_key_fp)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("setting a gmail's Business Profile grant")?;
	Ok(done.rows_affected() > 0)
}

/// Whether another gmail, not removed, has the inbox.
pub(crate) async fn inbox_elsewhere(conn: &mut SqliteConnection, gmail_id: i64, username: &str) -> eyre::Result<bool> {
	sqlx::query_scalar(
		"SELECT EXISTS (SELECT 1 FROM gmail_imap i JOIN gmails g ON g.id = i.gmail_id \
		 WHERE i.gmail_id != $1 AND lower(i.username) = lower($2) AND g.removed_at IS NULL)",
	)
	.bind(gmail_id)
	.bind(username)
	.fetch_one(&mut *conn)
	.await
	.wrap_err("looking for another gmail of an inbox")
}

/// Checked when set: a refused one is never stored.
pub(crate) async fn set_imap(conn: &mut SqliteConnection, gmail_id: i64, inbox: Inbox<'_>, password: &Sealed, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $6, $6, NULL) \
		 ON CONFLICT (gmail_id) DO UPDATE SET host = excluded.host, username = excluded.username, password_sealed = excluded.password_sealed, \
		 data_key_fp = excluded.data_key_fp, set_at = excluded.set_at, checked_at = excluded.checked_at, check_error = NULL",
	)
	.bind(gmail_id)
	.bind(inbox.host)
	.bind(inbox.username)
	.bind(&password.blob)
	.bind(&password.data_key_fp)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a gmail's IMAP")?;
	Ok(())
}

/// Checked when set: a refused one is never stored.
pub(crate) async fn set_maps_key(conn: &mut SqliteConnection, gmail_id: i64, last4: &str, key: &Sealed, now: Timestamp) -> eyre::Result<()> {
	set_key(conn, ("gmail_maps_keys", "gmail_id"), gmail_id, last4, key, now).await
}

/// `owner`'s key in `table`, whose owner is its `whose`: one shown by its last four.
pub(super) async fn set_key<O>(conn: &mut SqliteConnection, (table, whose): (&'static str, &'static str), owner: O, last4: &str, key: &Sealed, now: Timestamp) -> eyre::Result<()>
where
	O: for<'q> sqlx::Encode<'q, sqlx::Sqlite> + sqlx::Type<sqlx::Sqlite> + Send + 'static, {
	sqlx::query(sqlx::AssertSqlSafe(format!(
		"INSERT INTO {table} ({whose}, key_sealed, data_key_fp, last4, set_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $5, NULL) \
		 ON CONFLICT ({whose}) DO UPDATE SET key_sealed = excluded.key_sealed, data_key_fp = excluded.data_key_fp, last4 = excluded.last4, \
		 set_at = excluded.set_at, checked_at = excluded.checked_at, check_error = NULL"
	)))
	.bind(owner)
	.bind(&key.blob)
	.bind(&key.data_key_fp)
	.bind(last4)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err_with(|| format!("setting a key in {table}"))?;
	Ok(())
}

pub(crate) async fn checked(conn: &mut SqliteConnection, gmail_id: i64, which: Connection, c: &Checked) -> eyre::Result<()> {
	sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {} SET checked_at = $2, check_error = $3 WHERE gmail_id = $1", which.table())))
		.bind(gmail_id)
		.bind(to_db(c.at))
		.bind(&c.error)
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("recording a check of a gmail's {which}"))?;
	Ok(())
}

/// The grants, Maps keys and proxies of gmails not removed, last asked at or before `answering`, or
/// `failing` when they did not answer then: whose, of which gmail, what.
pub(crate) async fn unasked(conn: &mut SqliteConnection, answering: Timestamp, failing: Timestamp) -> eyre::Result<Vec<(Uuid, i64, Lost)>> {
	let rows: Vec<(Uuid, i64, String)> = sqlx::query_as(
		"SELECT g.owner, g.id, c.what FROM ( \
		   SELECT gmail_id, 'business' AS what, checked_at, check_error FROM gmail_business WHERE grant_sealed IS NOT NULL \
		   UNION ALL SELECT gmail_id, 'maps_key', checked_at, check_error FROM gmail_maps_keys \
		   UNION ALL SELECT gmail_id, 'proxy', checked_at, check_error FROM gmail_browsers \
		 ) c JOIN gmails g ON g.id = c.gmail_id \
		 WHERE g.removed_at IS NULL AND (c.checked_at IS NULL OR c.checked_at <= iif(c.check_error IS NULL, $1, $2)) ORDER BY g.id, c.what",
	)
	.bind(to_db(answering))
	.bind(to_db(failing))
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing the connections to ask again")?;
	rows.into_iter().map(|(owner, gmail, what)| Ok((owner, gmail, what.parse()?))).collect()
}

/// `false`: none was set.
pub(crate) async fn delete(conn: &mut SqliteConnection, gmail_id: i64, which: Connection) -> eyre::Result<bool> {
	let done = sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {} WHERE gmail_id = $1", which.table())))
		.bind(gmail_id)
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("deleting a gmail's {which}"))?;
	Ok(done.rows_affected() > 0)
}

/// The owner's Maps keys, sealed: `first`'s, then those of their other gmails by id.
pub(crate) async fn own_keys(conn: &mut SqliteConnection, owner: Uuid, first: Option<i64>) -> eyre::Result<Vec<(i64, Sealed)>> {
	let rows: Vec<(i64, Vec<u8>, Vec<u8>)> = sqlx::query_as(
		"SELECT k.gmail_id, k.key_sealed, k.data_key_fp FROM gmail_maps_keys k JOIN gmails g ON g.id = k.gmail_id \
		 WHERE g.owner = $1 AND g.removed_at IS NULL ORDER BY k.gmail_id IS NOT $2, k.gmail_id",
	)
	.bind(owner)
	.bind(first)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing a person's Maps keys")?;
	Ok(rows.into_iter().map(|(id, blob, data_key_fp)| (id, Sealed { blob, data_key_fp })).collect())
}
