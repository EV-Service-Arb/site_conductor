//! A study's pull that has not ended whole (`migrations/…_placement_pulls.up.sql`).

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;

use super::{from_db, to_db};

/// When the first pull of `study` since its last whole one began: `now`, unless one did not end whole.
pub(crate) async fn began(conn: &mut SqliteConnection, study: &str, now: Timestamp) -> eyre::Result<Timestamp> {
	let since: i64 = sqlx::query_scalar("INSERT INTO placement_pulls (study, since) VALUES ($1, $2) ON CONFLICT (study) DO UPDATE SET since = since RETURNING since")
		.bind(study)
		.bind(to_db(now))
		.fetch_one(&mut *conn)
		.await
		.wrap_err("beginning a pull")?;
	from_db(since)
}

/// `study`'s pull ended whole: the next one asks everything again.
pub(crate) async fn ended(conn: &mut SqliteConnection, study: &str) -> eyre::Result<()> {
	sqlx::query("DELETE FROM placement_pulls WHERE study = $1")
		.bind(study)
		.execute(&mut *conn)
		.await
		.wrap_err("ending a pull")?;
	Ok(())
}
