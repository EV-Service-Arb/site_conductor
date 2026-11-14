//! A gmail's connections, their secrets sealed.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{from_db, to_db};

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

/// Which connection of a gmail's.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::Display, strum::EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum Connection {
	Imap,
	MapsKey,
}
impl Connection {
	fn table(self) -> &'static str {
		match self {
			Self::Imap => "gmail_imap",
			Self::MapsKey => "gmail_maps_keys",
		}
	}
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

/// Checked when set: a refused one is never stored.
pub(crate) async fn set_imap(conn: &mut SqliteConnection, gmail_id: i64, host: &str, username: &str, password: &Sealed, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $6, $6, NULL) \
		 ON CONFLICT (gmail_id) DO UPDATE SET host = excluded.host, username = excluded.username, password_sealed = excluded.password_sealed, \
		 data_key_fp = excluded.data_key_fp, set_at = excluded.set_at, checked_at = excluded.checked_at, check_error = NULL",
	)
	.bind(gmail_id)
	.bind(host)
	.bind(username)
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
	sqlx::query(
		"INSERT INTO gmail_maps_keys (gmail_id, key_sealed, data_key_fp, last4, set_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $5, NULL) \
		 ON CONFLICT (gmail_id) DO UPDATE SET key_sealed = excluded.key_sealed, data_key_fp = excluded.data_key_fp, last4 = excluded.last4, \
		 set_at = excluded.set_at, checked_at = excluded.checked_at, check_error = NULL",
	)
	.bind(gmail_id)
	.bind(&key.blob)
	.bind(&key.data_key_fp)
	.bind(last4)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a gmail's Maps key")?;
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
