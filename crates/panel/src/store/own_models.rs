//! A person's own credentials with the providers of models, sealed.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::prices::Provider;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{
	connections::{Checked, Sealed},
	from_db, to_db,
};

/// A person's Claude, as they see it: never its token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Claude {
	pub set_at: Timestamp,
	pub expires_at: Timestamp,
	pub checked: Checked,
}

/// A person's OpenAI key, as they see it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OpenAi {
	pub last4: String,
	pub set_at: Timestamp,
	pub checked: Checked,
}

fn table(of: Provider) -> &'static str {
	match of {
		Provider::Claude => "user_claude",
		Provider::OpenAi => "user_openai",
	}
}

pub(crate) async fn claude(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Option<(Claude, Sealed)>> {
	type Row = (Vec<u8>, Vec<u8>, i64, i64, i64, Option<String>);
	let row: Option<Row> = sqlx::query_as("SELECT token_sealed, data_key_fp, set_at, expires_at, checked_at, check_error FROM user_claude WHERE user_id = $1")
		.bind(user)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a person's Claude")?;
	row.map(|(blob, data_key_fp, set_at, expires_at, at, error)| {
		let claude = Claude {
			set_at: from_db(set_at)?,
			expires_at: from_db(expires_at)?,
			checked: Checked { at: from_db(at)?, error },
		};
		Ok((claude, Sealed { blob, data_key_fp }))
	})
	.transpose()
}

/// Checked when set: a refused one is never stored.
pub(crate) async fn set_claude(conn: &mut SqliteConnection, user: Uuid, token: &Sealed, expires_at: Timestamp, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO user_claude (user_id, token_sealed, data_key_fp, set_at, expires_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $4, NULL) \
		 ON CONFLICT (user_id) DO UPDATE SET token_sealed = excluded.token_sealed, data_key_fp = excluded.data_key_fp, set_at = excluded.set_at, \
		 expires_at = excluded.expires_at, checked_at = excluded.checked_at, check_error = NULL",
	)
	.bind(user)
	.bind(&token.blob)
	.bind(&token.data_key_fp)
	.bind(to_db(now))
	.bind(to_db(expires_at))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a person's Claude")?;
	Ok(())
}

pub(crate) async fn openai(conn: &mut SqliteConnection, user: Uuid) -> eyre::Result<Option<(OpenAi, Sealed)>> {
	type Row = (Vec<u8>, Vec<u8>, String, i64, i64, Option<String>);
	let row: Option<Row> = sqlx::query_as("SELECT key_sealed, data_key_fp, last4, set_at, checked_at, check_error FROM user_openai WHERE user_id = $1")
		.bind(user)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a person's OpenAI key")?;
	row.map(|(blob, data_key_fp, last4, set_at, at, error)| {
		let openai = OpenAi {
			last4,
			set_at: from_db(set_at)?,
			checked: Checked { at: from_db(at)?, error },
		};
		Ok((openai, Sealed { blob, data_key_fp }))
	})
	.transpose()
}

/// Checked when set: a refused one is never stored.
pub(crate) async fn set_openai(conn: &mut SqliteConnection, user: Uuid, last4: &str, key: &Sealed, now: Timestamp) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO user_openai (user_id, key_sealed, data_key_fp, last4, set_at, checked_at, check_error) VALUES ($1, $2, $3, $4, $5, $5, NULL) \
		 ON CONFLICT (user_id) DO UPDATE SET key_sealed = excluded.key_sealed, data_key_fp = excluded.data_key_fp, last4 = excluded.last4, \
		 set_at = excluded.set_at, checked_at = excluded.checked_at, check_error = NULL",
	)
	.bind(user)
	.bind(&key.blob)
	.bind(&key.data_key_fp)
	.bind(last4)
	.bind(to_db(now))
	.execute(&mut *conn)
	.await
	.wrap_err("setting a person's OpenAI key")?;
	Ok(())
}

pub(crate) async fn checked(conn: &mut SqliteConnection, user: Uuid, of: Provider, c: &Checked) -> eyre::Result<()> {
	sqlx::query(sqlx::AssertSqlSafe(format!("UPDATE {} SET checked_at = $2, check_error = $3 WHERE user_id = $1", table(of))))
		.bind(user)
		.bind(to_db(c.at))
		.bind(&c.error)
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("recording a check of a person's {of}"))?;
	Ok(())
}

/// `false`: none was set.
pub(crate) async fn delete(conn: &mut SqliteConnection, user: Uuid, of: Provider) -> eyre::Result<bool> {
	let done = sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {} WHERE user_id = $1", table(of))))
		.bind(user)
		.execute(&mut *conn)
		.await
		.wrap_err_with(|| format!("deleting a person's {of}"))?;
	Ok(done.rows_affected() > 0)
}
