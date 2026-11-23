//! For tests only (feature `testing`): a throwaway database per test, and signed requests.
//!
//! Each test gets its own SQLite file in the system's temp directory, removed when the
//! [`TestDb`] is dropped: no server, nothing to set up, so the database tests run everywhere
//! `cargo test` does, CI included. A file, not `:memory:`: an in-memory database is one per
//! connection, and the tests open several pools on one database, as replicas would.

use std::{
	collections::BTreeSet,
	path::{Path, PathBuf},
	sync::Arc,
	time::Duration,
};

use jiff::Timestamp;
use sa_core::signature;
use sqlx::sqlite::SqlitePoolOptions;
use tokio::sync::broadcast::{Receiver, error::TryRecvError};
use uuid::Uuid;

use crate::{
	Panel,
	live::Signal,
	seal::DataKey,
	store::{self, Store},
};

/// Each item sold at that many tokens, at cost (×1).
pub async fn priced(panel: &Panel, items: &[(sa_core::prices::Item, i64)]) {
	let multipliers: Vec<_> = items.iter().map(|&(i, _)| (i, 1000)).collect();
	let costs: Vec<_> = items.iter().map(|&(i, t)| (i, t * sa_core::prices::TOKEN_USD_MICROS)).collect();
	panel.set_prices(&multipliers, &costs, "admin@localhost", Timestamp::now()).await.unwrap().unwrap();
}

/// A database and its queries cache's history, that live as long as this value.
pub struct TestDb {
	path: PathBuf,
	history: PathBuf,
}

impl TestDb {
	/// A fresh database, migrated: a copy of [`migrated`]'s.
	pub async fn create() -> Self {
		let db = Self::empty().await;
		let (path, history) = migrated().await;
		std::fs::copy(path, &db.path).expect("copying the migrated database");
		std::fs::copy(history, &db.history).expect("copying the migrated history");
		db
	}

	/// A fresh database with nothing in it, not even the migrations.
	pub async fn empty() -> Self {
		static TRACED: std::sync::Once = std::sync::Once::new();
		TRACED.call_once(|| {
			if let Some(spans) = sa_trace::layer() {
				use tracing_subscriber::prelude::*;
				tracing_subscriber::registry().with(spans).init();
			}
		});
		let id = Uuid::now_v7().simple();
		let dir = std::env::temp_dir();
		Self {
			path: dir.join(format!("panel_test_{id}.db")),
			history: dir.join(format!("panel_test_{id}_history.db")),
		}
	}

	pub fn path(&self) -> &Path {
		&self.path
	}

	pub fn history(&self) -> &Path {
		&self.history
	}

	/// A store on this database, migrated (opening migrates).
	pub async fn store(&self) -> Store {
		Store::open(&self.path, &self.history, crate::live::Bus::default()).await.expect("opening the test database")
	}

	/// A plain pool on this database, for assertions the engine has no API for. Migrated
	/// first, so it can be asked for before any store.
	pub async fn pool(&self) -> sqlx::SqlitePool {
		drop(self.store().await);
		SqlitePoolOptions::new()
			.max_connections(2)
			.connect_with(store::options(&self.path))
			.await
			.expect("connecting to the test database")
	}
}

/// Once every connection of `panel`'s pool is back in it: what each committed is told by then,
/// a connection's writes being told as it goes back.
pub async fn settled(panel: &Panel) {
	let pool = panel.store().pool();
	let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
	while pool.num_idle() < usize::try_from(pool.size()).expect("8 fits a usize") {
		assert!(tokio::time::Instant::now() < deadline, "a connection stays out of the pool");
		tokio::time::sleep(Duration::from_millis(1)).await;
	}
}

/// The topics told on `rx` since it was last drained, once `panel`'s pool has [`settled`].
pub async fn told(panel: &Panel, rx: &mut Receiver<Signal>) -> BTreeSet<String> {
	settled(panel).await;
	std::iter::from_fn(|| match rx.try_recv() {
		Ok(signal) => Some(Some(signal)),
		Err(TryRecvError::Empty) => None,
		Err(e) => panic!("the bus behind a test: {e}"),
	})
	.flatten()
	.filter_map(|signal| match signal {
		Signal::Changed(c) => Some(c.topic),
		_ => None,
	})
	.collect()
}

/// `conductor.<table>` for each of `tables`.
pub fn topics(tables: &[&str]) -> BTreeSet<String> {
	tables.iter().map(|t| format!("conductor.{t}")).collect()
}

/// A panel on a fresh store with a fresh data key.
pub async fn panel(db: &TestDb) -> Panel {
	let key = DataKey::from_hex(&DataKey::generate_hex().expect("random")).expect("a fresh key");
	panel_with(db.store().await, key)
}

/// A panel on `store`: tokens renewing 15 a day up to 500 from 500, places resolving ids only, no
/// Google account.
pub fn panel_with(store: Store, key: DataKey) -> Panel {
	let keyless = sa_places::Google::try_new(sa_places::Keys::new(vec![]), Arc::new(sa_places::InOrder), Arc::new(store.clone())).expect("a Places client");
	panel_on(store, key, keyless, sa_places::Accounts::new(vec![], vec![]))
}

/// A panel on `store` asking `google` on `accounts`, tokens as [`panel_with`].
pub fn panel_on(store: Store, key: DataKey, google: sa_places::Google, accounts: sa_places::Accounts) -> Panel {
	let renewal = sa_core::tokens::Renewal {
		daily: 15,
		cap: 500,
		start_cap_multiple: 1.,
	}
	.validate()
	.expect("a valid renewal");
	Panel::new(store, key, renewal, google, Arc::new(accounts))
}

/// What every [`account`] names its `allow` of: the meters' slice alone.
pub fn slices() -> Vec<sa_core::google::Slice> {
	vec![sa_core::google::Slice {
		name: sa_core::meters::SLICE.to_owned(),
		service: "(?i)maps|places|geocoding".to_owned(),
		sku: None,
	}]
}

/// An account of `name` with only the keys given, free units only, its export never read.
pub fn account(name: &str, places: Option<&str>, browser: Option<&str>) -> sa_places::Account {
	sa_places::Account {
		name: name.to_owned(),
		export: Some(sa_places::Export {
			table: format!("{name}.billing.export"),
			reader: "{}".to_owned(),
			since: jiff::civil::date(2026, 10, 1),
		}),
		credits: vec![],
		provides: sa_places::Provides {
			places: places.map(|k| sa_places::Key { key: k.to_owned() }),
			browser: browser.map(|k| sa_places::Key { key: k.to_owned() }),
		},
	}
}

/// A request as a source sends it: the body, and the headers' values.
pub struct Signed {
	pub key_id: String,
	pub timestamp: String,
	pub signature: String,
	pub body: Vec<u8>,
}
impl Signed {
	pub fn batch(&self) -> crate::SignedBatch<'_> {
		crate::SignedBatch {
			key_id: &self.key_id,
			timestamp: &self.timestamp,
			signature: &self.signature,
			body: &self.body,
		}
	}
}

/// A source's key, as the source holds it.
#[derive(Clone, Copy)]
pub struct SourceKey<'a> {
	pub id: &'a str,
	pub secret: &'a str,
}

/// `body`, signed at `at` with `key`, as a source sends a batch.
pub fn sign(key: SourceKey<'_>, body: &[u8], at: Timestamp) -> Signed {
	let timestamp = at.as_second().to_string();
	Signed {
		key_id: key.id.to_owned(),
		signature: signature::sign(key.secret.as_bytes(), &timestamp, body),
		timestamp,
		body: body.to_vec(),
	}
}

/// A `GET` signed as a source signs a batch: what a bot sends to look a lead up by its ref
/// (`GET /api/ingest/v1/leads/by-ref/…`), the MAC over `GET <path>` in place of a body.
pub fn sign_get(key: SourceKey<'_>, path: &str, at: Timestamp) -> Signed {
	let timestamp = at.as_second().to_string();
	Signed {
		key_id: key.id.to_owned(),
		signature: signature::sign(key.secret.as_bytes(), &timestamp, format!("GET {path}").as_bytes()),
		timestamp,
		body: Vec::new(),
	}
}

/// What `sa:operator` holds.
pub fn operator() -> sa_auth::PermissionSet {
	sa_auth::SA_OPERATOR.members.iter().chain(sa_auth::SA_EVERYONE.members).copied().collect()
}

/// What `sa:admin` holds.
pub fn admin() -> sa_auth::PermissionSet {
	sa_auth::SA_ADMIN.members.iter().chain(sa_auth::SA_EVERYONE.members).copied().collect()
}

/// A vendor that counts `units` of every one of its meters on `account`, each `net` after credits
/// (millionths), its record reaching `through`.
pub struct Told {
	pub account: &'static str,
	pub units: i64,
	pub net: i64,
	pub through: Timestamp,
	pub bill: crate::meters::Bill,
	/// What was published beside its export, oldest first.
	pub published: std::sync::Mutex<Vec<sa_core::google::Definition>>,
}

/// A database and a history migrated once per state of migrations/ and history_migrations/, shared by
/// every test process: migrating one takes seconds.
#[tracing::instrument]
async fn migrated() -> (PathBuf, PathBuf) {
	let mut h = std::hash::DefaultHasher::new();
	for m in store::migrator().iter().chain(store::history_migrator().iter()) {
		std::hash::Hash::hash(&(m.version, &*m.checksum), &mut h);
	}
	let path = std::env::temp_dir().join(format!("panel_test_migrated_{:x}.db", std::hash::Hasher::finish(&h)));
	let history = path.with_extension("history.db");
	let lock = std::fs::File::create(path.with_extension("lock")).expect("the migrated test database's lock");
	lock.lock().expect("locking the migrated test database"); // one process migrates; the rest wait for it
	if !path.exists() {
		let (part, history_part) = (path.with_extension("part"), path.with_extension("history.part"));
		Store::open(&part, &history_part, crate::live::Bus::default())
			.await
			.expect("migrating the shared test database")
			.close()
			.await;
		for p in [&part, &history_part] {
			assert!(!Path::new(&format!("{}-wal", p.display())).exists(), "the last connection's close checkpoints the WAL in");
		}
		std::fs::rename(&history_part, &history).expect("publishing the migrated test history");
		std::fs::rename(&part, &path).expect("publishing the migrated test database"); // last: its presence says both are there
	}
	(path, history)
}

impl Drop for TestDb {
	fn drop(&mut self) {
		// The WAL and its index sit beside the file; a pool still open only keeps them a while.
		for (file, suffix) in [&self.path, &self.history].into_iter().flat_map(|f| ["", "-wal", "-shm"].map(|s| (f, s))) {
			let mut path = file.clone().into_os_string();
			path.push(suffix);
			match std::fs::remove_file(&path) {
				Ok(()) => {}
				Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
				Err(e) => eprintln!("could not remove test database {}: {e}", PathBuf::from(path).display()),
			}
		}
	}
}

impl crate::meters::Reader for Told {
	fn vendor(&self) -> sa_core::meters::Vendor {
		sa_core::meters::Vendor::Google
	}

	fn account(&self) -> &str {
		self.account
	}

	async fn read(&self, meters: &[&'static sa_core::meters::Meter], now: Timestamp) -> eyre::Result<Vec<crate::meters::Reading>> {
		Ok(meters
			.iter()
			.map(|m| crate::meters::Reading {
				account: self.account.to_owned(),
				meter: m,
				period_start: m.vendor.period(now).0,
				units: self.units,
				cost_micros: self.net.max(0),
				net_micros: self.net,
				currency: "EUR".to_owned(),
				through: self.through,
			})
			.collect())
	}

	async fn bill(&self, _: jiff::civil::Date) -> eyre::Result<crate::meters::Bill> {
		Ok(self.bill.clone())
	}

	async fn published(&self, _: Timestamp) -> eyre::Result<Option<String>> {
		Ok(self.published.lock().expect("never poisoned").last().map(sa_core::google::Definition::digest))
	}

	async fn publish(&self, definition: &sa_core::google::Definition, _: Timestamp) -> eyre::Result<()> {
		self.published.lock().expect("never poisoned").push(definition.clone());
		Ok(())
	}
}
