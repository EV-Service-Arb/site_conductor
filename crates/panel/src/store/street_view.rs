//! The Street View panoramas opened.

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::SqliteConnection;
use uuid::Uuid;

use super::to_db;

/// Panoramas opened by anyone from `since` on.
pub(crate) async fn opened_since(conn: &mut SqliteConnection, since: Timestamp) -> eyre::Result<i64> {
	sqlx::query_scalar("SELECT COUNT(*) FROM street_views WHERE at >= $1")
		.bind(to_db(since))
		.fetch_one(conn)
		.await
		.wrap_err("counting panoramas opened")
}

pub(crate) async fn opened(conn: &mut SqliteConnection, user: Uuid, at: Timestamp, lat: f64, lng: f64) -> eyre::Result<()> {
	sqlx::query("INSERT INTO street_views (user_id, at, lat, lng) VALUES ($1, $2, $3, $4)")
		.bind(user)
		.bind(to_db(at))
		.bind(lat)
		.bind(lng)
		.execute(conn)
		.await
		.wrap_err("recording a panorama opened")?;
	Ok(())
}
