//! The documents kept for GBPs, each sealed.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::{connections::Sealed, from_db, to_db};
use crate::mail::File;

/// A document of a GBP; its bytes apart.
#[derive(Clone, Debug)]
pub struct Document {
	pub id: i64,
	pub name: String,
	/// Its media type.
	pub kind: String,
	pub size: i64,
	/// What Google asked that it answers.
	pub given_for: Option<String>,
	pub added_at: Timestamp,
}

pub(crate) async fn of_gmb(conn: &mut SqliteConnection, gmb: i64) -> eyre::Result<Vec<Document>> {
	type Row = (i64, String, String, i64, Option<String>, i64);
	let rows: Vec<Row> = sqlx::query_as("SELECT id, name, kind, size, given_for, added_at FROM gmb_documents WHERE gmb_id = $1 ORDER BY id DESC")
		.bind(gmb)
		.fetch_all(&mut *conn)
		.await
		.wrap_err("listing a GBP's documents")?;
	rows.into_iter()
		.map(|(id, name, kind, size, given_for, at)| {
			Ok(Document {
				id,
				name,
				kind,
				size,
				given_for,
				added_at: from_db(at)?,
			})
		})
		.collect()
}

pub(crate) async fn add(conn: &mut SqliteConnection, gmb: i64, file: &File, sealed: &Sealed, given_for: Option<&str>, by: Uuid, now: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar("INSERT INTO gmb_documents (gmb_id, name, kind, size, bytes_sealed, data_key_fp, given_for, added_by, added_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id")
		.bind(gmb)
		.bind(&file.name)
		.bind(&file.kind)
		.bind(i64::try_from(file.bytes.len()).expect("a document's size is bounded"))
		.bind(&sealed.blob)
		.bind(&sealed.data_key_fp)
		.bind(given_for)
		.bind(by)
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("keeping a document")
}

/// Its name, its media type, its bytes sealed.
pub(crate) async fn bytes(conn: &mut SqliteConnection, gmb: i64, id: i64) -> eyre::Result<Option<(String, String, Sealed)>> {
	let row: Option<(String, String, Vec<u8>, Vec<u8>)> = sqlx::query_as("SELECT name, kind, bytes_sealed, data_key_fp FROM gmb_documents WHERE id = $1 AND gmb_id = $2")
		.bind(id)
		.bind(gmb)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("loading a document")?;
	Ok(row.map(|(name, kind, blob, data_key_fp)| (name, kind, Sealed { blob, data_key_fp })))
}

pub(crate) async fn remove(conn: &mut SqliteConnection, gmb: i64, id: i64) -> eyre::Result<bool> {
	let gone = sqlx::query("DELETE FROM gmb_documents WHERE id = $1 AND gmb_id = $2")
		.bind(id)
		.bind(gmb)
		.execute(&mut *conn)
		.await
		.wrap_err("removing a document")?;
	Ok(gone.rows_affected() > 0)
}
