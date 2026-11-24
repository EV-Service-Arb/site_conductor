//! Where the follower of each gmail's inbox stands, and the mail it has logged.

use eyre::WrapErr;
use sa_core::mail::Named;
use sqlx::SqliteConnection;

use crate::inbox::{Cursor, Folder};

/// The gmails with an inbox set, not removed.
pub(crate) async fn followed(conn: &mut SqliteConnection) -> eyre::Result<Vec<i64>> {
	sqlx::query_scalar("SELECT i.gmail_id FROM gmail_imap i JOIN gmails g ON g.id = i.gmail_id WHERE g.removed_at IS NULL ORDER BY i.gmail_id")
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing the inboxes followed")
}

/// The account it was read on, and where.
pub(crate) async fn cursor(conn: &mut SqliteConnection, gmail: i64, folder: Folder) -> eyre::Result<Option<(String, Cursor)>> {
	let row: Option<(String, i64, i64)> = sqlx::query_as("SELECT username, uid_validity, last_uid FROM gmail_inbox_cursors WHERE gmail_id = $1 AND spam = $2")
		.bind(gmail)
		.bind(folder == Folder::Spam)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading an inbox's cursor")?;
	row.map(|(username, validity, uid)| {
		Ok((
			username,
			Cursor {
				validity: u32::try_from(validity).wrap_err("a stored UIDVALIDITY")?,
				uid: u32::try_from(uid).wrap_err("a stored UID")?,
			},
		))
	})
	.transpose()
}

pub(crate) async fn set_cursor(conn: &mut SqliteConnection, gmail: i64, folder: Folder, username: &str, c: Cursor) -> eyre::Result<()> {
	sqlx::query(
		"INSERT INTO gmail_inbox_cursors (gmail_id, spam, username, uid_validity, last_uid) VALUES ($1, $2, $3, $4, $5) \
		 ON CONFLICT (gmail_id, spam) DO UPDATE SET username = excluded.username, uid_validity = excluded.uid_validity, last_uid = excluded.last_uid",
	)
	.bind(gmail)
	.bind(folder == Folder::Spam)
	.bind(username)
	.bind(i64::from(c.validity))
	.bind(i64::from(c.uid))
	.execute(&mut *conn)
	.await
	.wrap_err("moving an inbox's cursor")?;
	Ok(())
}

/// Whether Gmail's message `message` is in the gmail's GBPs' logs already.
pub(crate) async fn logged(conn: &mut SqliteConnection, gmail: i64, message: &str) -> eyre::Result<bool> {
	sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM gmb_events WHERE kind = 'email' AND gmail_id = $1 AND data ->> 'message' = $2)")
		.bind(gmail)
		.bind(message)
		.fetch_one(&mut *conn)
		.await
		.wrap_err("looking a message up in the log")
}

/// The live GBPs of the gmail's owner, as a message may name them: the gmail's, and their others'.
pub(crate) async fn gmbs(conn: &mut SqliteConnection, gmail: i64) -> eyre::Result<(Vec<Named>, Vec<Named>)> {
	type Row = (i64, Option<String>, Option<String>, Option<String>, bool);
	let rows: Vec<Row> = sqlx::query_as(
		"SELECT b.id, b.name, b.place_id, b.address, b.gmail_id IS g.id FROM gmbs b JOIN gmails g ON g.owner = b.owner WHERE g.id = $1 AND b.removed_at IS NULL ORDER BY b.id",
	)
	.bind(gmail)
	.fetch_all(&mut *conn)
	.await
	.wrap_err("listing the GBPs of a gmail's owner")?;
	let (own, others): (Vec<Row>, Vec<Row>) = rows.into_iter().partition(|(.., own)| *own);
	let named = |rows: Vec<Row>| rows.into_iter().map(|(id, name, place_id, address, _)| Named { id, name, place_id, address }).collect();
	Ok((named(own), named(others)))
}
