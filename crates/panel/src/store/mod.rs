//! SQLite: runtime `sqlx` queries, migrations embedded from `migrations/` and applied on open.
//! Every query here is executed by the integration tests (`tests/`), which stand in for the
//! compile-time checks the `query!` macros would give. Every committed write is told on the bus
//! by its tables (`ev_lib::sqlite`).
//!
//! Two files: the panel's, and the queries cache's history ([`history_migrator`]), on a pool of
//! its own that no client follows. One writer at a time on each (WAL: readers never wait for it). Every transaction that
//! writes begins `BEGIN IMMEDIATE` ([`begin_write`]): it takes the write lock up front, so two
//! of them queue on `busy_timeout` instead of both reading under a plain `BEGIN` and the
//! second failing with SQLITE_BUSY when it tries to upgrade to a write. A lone statement
//! outside a transaction is atomic by itself and waits on `busy_timeout` the same way.

pub mod access;
pub mod alerts;
pub mod areas;
pub mod assistant;
pub mod brands;
pub mod browsers;
pub mod claude;
pub mod connections;
pub mod gmbs;
pub(crate) mod inbox;
pub mod kit;
pub mod meters;
pub mod oauth;
pub(crate) mod placement;
mod queries_cache;
pub mod sessions;
pub mod sources;
pub mod telegram;
pub mod tokens;
pub(crate) mod trash;
pub mod usage;
pub mod users;

use std::{path::Path, time::Duration};

use eyre::WrapErr;
use jiff::Timestamp;
use sqlx::{
	Sqlite, SqliteConnection, SqlitePool, Transaction,
	migrate::Migrator,
	sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tracing::Instrument;

use crate::live::Bus;

/// How long a statement waits for another connection's write lock before it fails. Writes are
/// short.
const BUSY_TIMEOUT: Duration = Duration::from_secs(10);

/// The connection settings every connection to the panel's file gets.
pub fn options(path: &Path) -> SqliteConnectOptions {
	SqliteConnectOptions::new()
		.filename(path)
		.create_if_missing(true)
		// Readers do not block the writer nor it them; and litestream replicates the WAL.
		.journal_mode(SqliteJournalMode::Wal)
		// Durable at every checkpoint, not every commit: in WAL a crash loses at most the
		// last commits, never consistency — and litestream ships the WAL off the pod anyway.
		.synchronous(SqliteSynchronous::Normal)
		.foreign_keys(true)
		.busy_timeout(BUSY_TIMEOUT)
}

/// The panel's database.
#[derive(Clone, Debug)]
pub struct Store {
	pool: SqlitePool,
	/// The queries cache's every version of an answer but the latest, in a file of its own.
	history: SqlitePool,
	bus: Bus,
}
impl Store {
	/// Opens (creating them if need be) the database at `path` and the queries cache's history at
	/// `history`, and applies the migrations each lacks, the panel's writes told on `bus`. There is
	/// no separate migrate step: the process that owns the files migrates them.
	#[tracing::instrument(name = "store open", skip_all)]
	pub async fn open(path: &Path, history: &Path, bus: Bus) -> eyre::Result<Self> {
		let told = bus.clone();
		// A request waits at most 3 s for a connection, then fails: a pool drained by a burst
		// answers 500 quickly instead of stacking every request behind it.
		let pool = ev_lib::sqlite::pool("conductor", options(path), 8, Duration::from_secs(3), move |topic| told.changed(topic));
		let opening = || format!("opening the database at {}", path.display());
		migrator()
			.run(&pool)
			.instrument(tracing::info_span!("migrate"))
			.await
			.wrap_err("applying migrations")
			.wrap_err_with(opening)?;
		let mut tx = pool.begin_with("BEGIN IMMEDIATE").await.wrap_err("beginning a write").wrap_err_with(opening)?;
		tokens::init_rates(&mut tx, jiff::Timestamp::now()).await?;
		tx.commit().await.wrap_err("committing the first terms")?;
		// Written only under the panel's write lock (`keep_query`): two connections are for reads beside it.
		let history_pool = SqlitePoolOptions::new()
			.max_connections(2)
			.acquire_timeout(Duration::from_secs(3))
			.connect_lazy_with(options(history));
		history_migrator()
			.run(&history_pool)
			.instrument(tracing::info_span!("migrate history"))
			.await
			.wrap_err_with(|| format!("applying the migrations of the queries cache's history at {}", history.display()))?;
		Ok(Self { pool, history: history_pool, bus })
	}

	/// Both pools closed, each file's WAL checkpointed in.
	pub async fn close(&self) {
		self.pool.close().await;
		self.history.close().await;
	}

	pub fn pool(&self) -> &SqlitePool {
		&self.pool
	}

	/// Where its committed writes are told.
	pub fn bus(&self) -> &Bus {
		&self.bus
	}

	/// A transaction that will write: the write lock is taken at its start (see the module).
	pub async fn begin_write(&self) -> eyre::Result<Transaction<'static, Sqlite>> {
		self.pool.begin_with("BEGIN IMMEDIATE").await.wrap_err("beginning a write")
	}
}

/// [`Store::begin_write`] on a connection already in hand.
pub async fn begin_write(conn: &mut SqliteConnection) -> eyre::Result<Transaction<'_, Sqlite>> {
	sqlx::Connection::begin_with(conn, "BEGIN IMMEDIATE").await.wrap_err("beginning a write")
}

/// The schema, embedded from `migrations/`.
pub(crate) fn migrator() -> Migrator {
	let mut m = sqlx::migrate!("./migrations");
	// A migration the database has and this build does not know is a rollback onto a schema
	// moved on: fine, as it was on Postgres.
	m.set_ignore_missing(true);
	m
}

/// The queries cache's history's schema, embedded from `history_migrations/`.
pub(crate) fn history_migrator() -> Migrator {
	let mut m = sqlx::migrate!("./history_migrations");
	m.set_ignore_missing(true);
	m
}

/// A timestamp as the database holds it: microseconds since the epoch, the precision Postgres'
/// timestamptz had. Anything finer is dropped.
pub(crate) fn to_db(ts: Timestamp) -> i64 {
	ts.as_microsecond()
}

pub(crate) fn from_db(us: i64) -> eyre::Result<Timestamp> {
	Timestamp::from_microsecond(us).wrap_err_with(|| format!("stored timestamp {us}"))
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn timestamps_round_trip() {
		for s in ["2026-09-30T10:00:00.123456Z", "1969-12-31T23:59:59.5Z", "1970-01-01T00:00:00Z"] {
			let ts: Timestamp = s.parse().unwrap();
			assert_eq!(from_db(to_db(ts)).unwrap(), ts, "{s}");
		}
		let fine: Timestamp = "2026-09-30T10:00:00.123456789Z".parse().unwrap();
		assert_eq!(from_db(to_db(fine)).unwrap().to_string(), "2026-09-30T10:00:00.123456Z", "nanoseconds are dropped");
	}
}
