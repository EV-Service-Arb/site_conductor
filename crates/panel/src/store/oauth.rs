//! MCP clients and their codes.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;

use super::{from_db, to_db};

pub async fn insert_client(conn: &mut SqliteConnection, client_id: &str, redirect_uris: &str, name: Option<&str>, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("INSERT INTO oauth_clients (client_id, redirect_uris, name, created_at) VALUES ($1, $2, $3, $4)")
		.bind(client_id)
		.bind(redirect_uris)
		.bind(name)
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("registering an MCP client")?;
	Ok(())
}

/// Its redirect URIs (a JSON array) and name.
pub async fn client(conn: &mut SqliteConnection, client_id: &str) -> eyre::Result<Option<(String, Option<String>)>> {
	sqlx::query_as("SELECT redirect_uris, name FROM oauth_clients WHERE client_id = $1")
		.bind(client_id)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("reading an MCP client")
}

/// A code as issued, its bearer still sealed.
pub struct CodeRow {
	pub session: Vec<u8>,
	pub redirect_uri: String,
	pub challenge: String,
	pub bearer_sealed: Vec<u8>,
	pub data_key_fp: Vec<u8>,
	pub expires_at: Timestamp,
}

pub async fn insert_code(conn: &mut SqliteConnection, code_hash: &[u8], code: &CodeRow) -> eyre::Result<()> {
	sqlx::query("INSERT INTO oauth_codes (code_hash, session, redirect_uri, challenge, bearer_sealed, data_key_fp, expires_at) VALUES ($1, $2, $3, $4, $5, $6, $7)")
		.bind(code_hash)
		.bind(&code.session)
		.bind(&code.redirect_uri)
		.bind(&code.challenge)
		.bind(&code.bearer_sealed)
		.bind(&code.data_key_fp)
		.bind(to_db(code.expires_at))
		.execute(&mut *conn)
		.await
		.wrap_err("issuing an MCP code")?;
	Ok(())
}

/// Deletes the code and answers it: a code is presented once, whatever comes of it.
pub async fn take_code(conn: &mut SqliteConnection, code_hash: &[u8]) -> eyre::Result<Option<CodeRow>> {
	type Row = (Vec<u8>, String, String, Vec<u8>, Vec<u8>, i64);
	let row: Option<Row> = sqlx::query_as("DELETE FROM oauth_codes WHERE code_hash = $1 RETURNING session, redirect_uri, challenge, bearer_sealed, data_key_fp, expires_at")
		.bind(code_hash)
		.fetch_optional(&mut *conn)
		.await
		.wrap_err("redeeming an MCP code")?;
	row.map(|(session, redirect_uri, challenge, bearer_sealed, data_key_fp, expires_at)| {
		Ok(CodeRow {
			session,
			redirect_uri,
			challenge,
			bearer_sealed,
			data_key_fp,
			expires_at: from_db(expires_at)?,
		})
	})
	.transpose()
}

/// Closes the sessions whose code lapsed unredeemed: no client ever held them.
pub async fn prune_codes(conn: &mut SqliteConnection, now: Timestamp) -> eyre::Result<()> {
	sqlx::query("DELETE FROM sessions WHERE id_hash IN (SELECT session FROM oauth_codes WHERE expires_at <= $1)")
		.bind(to_db(now))
		.execute(&mut *conn)
		.await
		.wrap_err("pruning lapsed MCP codes")?;
	Ok(())
}
