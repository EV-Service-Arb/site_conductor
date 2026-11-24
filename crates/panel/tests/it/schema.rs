//! Everything the runtime does runs on a freshly migrated database, and the schema itself
//! refuses what the code must never do: a source is only ever revoked. (SQLite has no roles, so
//! the tables' triggers say what a runtime role's grants would.)

use std::collections::BTreeMap;

use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Call, NotePage};
use sa_auth::{PermissionSet, SA_OPERATOR, Service, SourceKind};
use sa_core::{
	ids::{BrandId, SiteOrigin},
	notify::{Failure, Locale, Rendered},
};
use sa_panel::{
	access::Asker,
	alerts::{Address, Directory, DirectoryError, Feeds, Hub, Identity, Message, Outlets},
	seal::DataKey,
	session::{RefreshError, Refresher, SessionKey, Tokens},
	store::Store,
	telegram::{Account, CallbackId, Chat, ChatInfo, Update},
	testing::{SourceKey, TestDb, panel_with, sign},
};
use serde_json::json;
use zeroize::Zeroizing;

/// The migration that made Telegram one method of the alerts hub.
const ALERTS: i64 = 20261105090000;
/// The migration that let a destination be reached by any method.
const METHODS: i64 = 20261124120000;
/// The migration that took a trade's Google categories off Google's list.
const GOOGLE_CATEGORIES: i64 = 20261112090000;
/// The migration that made a category its studies' whole definition.
const DEFINITION: i64 = 20261119130000;
/// The migration that made a trade one of Google's categories.
const GOOGLES: i64 = 20261121180000;

/// A refresher that must not be asked: the tokens above are fresh.
struct Never;

impl Refresher for Never {
	async fn refresh(&self, _: &str) -> Result<Tokens, RefreshError> {
		unreachable!("the tokens are fresh")
	}
}

/// A refresher that rotates once, to `a2`/`r2`.
struct Rotates;

impl Refresher for Rotates {
	async fn refresh(&self, _: &str) -> Result<Tokens, RefreshError> {
		let now = Timestamp::now();
		Ok(Tokens {
			access: Zeroizing::new("a2".into()),
			access_expires_at: now + SignedDuration::from_hours(24 * 400),
			refresh: Zeroizing::new("r2".into()),
			refresh_expires_at: now + SignedDuration::from_hours(24 * 400),
		})
	}
}

/// A bot that delivers everything, or refuses every send as blocked.
struct FakeBot {
	blocked: bool,
}

impl Outlets for FakeBot {
	async fn send_to(&self, _: &Address, _: &Message) -> Result<String, Failure> {
		if self.blocked { Err(Failure::Gone("blocked".into())) } else { Ok("1".to_owned()) }
	}

	async fn edit(&self, _: i64, _: i64, _: &Rendered, _: &[(String, String)]) -> Result<(), Failure> {
		Ok(())
	}

	async fn answer(&self, _: &CallbackId, _: &str) -> Result<(), Failure> {
		Ok(())
	}

	async fn chat(&self, _: &str) -> Result<ChatInfo, Failure> {
		unreachable!("no chat is added here")
	}

	async fn is_admin(&self, _: i64, _: i64) -> Result<bool, Failure> {
		unreachable!("no chat is added here")
	}
}

/// Refreshes nothing; refuses every token.
struct Operators;

impl Refresher for Operators {
	async fn refresh(&self, _: &str) -> Result<Tokens, RefreshError> {
		Err(RefreshError::Rejected)
	}
}

impl Directory for Operators {
	async fn me(&self, _: &str) -> Result<Identity, DirectoryError> {
		Err(DirectoryError::Refused)
	}
}

/// Sites with two new leads to tell of, and nothing else; review_archive not cut over.
struct TwoLeads;

impl Feeds for TwoLeads {
	async fn notes(&self, service: Service, after: i64, _: usize) -> eyre::Result<Option<NotePage>> {
		if service == Service::ReviewArchive {
			return Ok(None);
		}
		let note = |seq: i64| {
			json!({"seq": seq, "rule": "new_lead", "event_id": uuid::Uuid::now_v7(), "created_at": Timestamp::now(),
				"text": {"text": "New lead", "entities": []}, "text_pii": null, "skip_user": null, "buttons": []})
		};
		let notes = if after == 0 { vec![note(1), note(2)] } else { vec![] };
		Ok(Some(serde_json::from_value(json!({"notes": notes, "next": after.max(2)}))?))
	}

	async fn photo(&self, _: Service, _: &str) -> eyre::Result<Vec<u8>> {
		unreachable!("no photo here")
	}

	async fn received(&self) -> eyre::Result<BTreeMap<String, Timestamp>> {
		Ok(BTreeMap::new())
	}

	fn callable(&self, _: Service, _: &Call) -> eyre::Result<()> {
		unreachable!("no button here")
	}

	async fn press(&self, _: &Identity, _: Service, _: &Call, _: &str) -> eyre::Result<u16> {
		unreachable!("no button here")
	}
}

#[tokio::test]
async fn the_runtime_does_its_work_on_a_fresh_database() {
	let db = TestDb::empty().await;
	Store::open(db.path(), db.history(), sa_panel::live::Bus::default()).await.expect("created and migrated");
	Store::open(db.path(), db.history(), sa_panel::live::Bus::default()).await.expect("again: nothing to do");
	{
		let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
		let panel = panel_with(db.store().await, key);
		let now = Timestamp::now();
		let secret = panel
			.add_source("aquafix-site", SourceKind::Site, [BrandId::parse("aquafix").unwrap()].into(), Some(origin("https://aquafix.top")))
			.await
			.unwrap()
			.unwrap()
			.secret
			.to_string();
		let grant = panel
			.authenticate(
				sign(
					SourceKey {
						id: "aquafix-site",
						secret: &secret,
					},
					b"{}",
					now,
				)
				.batch(),
				now,
			)
			.await
			.unwrap();
		assert_eq!((grant.key_id.as_str(), grant.kind), ("aquafix-site", SourceKind::Site));
		assert_eq!(panel.store().sources().await.unwrap().len(), 1);
		assert!(panel.revoke_source("aquafix-site").await.unwrap());
		assert!(
			panel
				.authenticate(
					sign(
						SourceKey {
							id: "aquafix-site",
							secret: &secret
						},
						b"{}",
						now
					)
					.batch(),
					now
				)
				.await
				.is_err(),
			"revoked"
		);

		// Signed-in users: a session, a rotation (its lease, its guarded write), a redeemed state,
		// a sign-out everywhere.
		let tokens = Tokens {
			access: Zeroizing::new("a".into()),
			access_expires_at: now + SignedDuration::from_mins(15),
			refresh: Zeroizing::new("r".into()),
			refresh_expires_at: now + SignedDuration::from_hours(1),
		};
		let user = uuid::Uuid::now_v7();
		let opened = panel.open_session(user, &tokens, now).await.unwrap();
		assert_eq!(panel.session(&opened.cookie, now, &Never).await.unwrap().user_id, user);
		assert!(panel.close_session(&SessionKey::of_cookie(&opened.cookie).unwrap()).await.unwrap());
		let opened = panel.open_session(user, &tokens, now).await.unwrap();
		let stale = now + SignedDuration::from_mins(15);
		assert_eq!(panel.session(&opened.cookie, stale, &Rotates).await.unwrap().access.as_str(), "a2");
		assert!(panel.consume_state("state", now).await.unwrap());
		assert!(!panel.consume_state("state", now).await.unwrap());
		assert!(panel.consume_state("state", now + SignedDuration::from_hours(1)).await.unwrap(), "expired marks are dropped");
		assert_eq!(panel.close_all_sessions(&SessionKey::of_cookie(&opened.cookie).unwrap()).await.unwrap(), Some(user));

		// Access requests, and alerts: a link, rules, sites' notes and the conductor's own into
		// the outbox, a delivery, a dead chat, the access checks with their pruning, the poller.
		let (asked, made) = panel
			.request_access(
				Asker {
					user,
					email: "o@x.io",
					name: "Olga",
				},
				"sa:operator",
				now,
			)
			.await
			.unwrap();
		assert!(made);
		assert!(asked.need == "sa:operator");
		let hub = |blocked| Hub {
			panel: panel.clone(),
			outlets: FakeBot { blocked },
			concierge: Operators,
			feeds: TwoLeads,
			locale: Locale::Ru,
			cabinet: cabinet(),
			origin: cabinet(),
		};
		let operator: PermissionSet = SA_OPERATOR.members.iter().copied().collect();
		let token = panel.telegram_link_token(user, &operator, "Olga", now).await.unwrap();
		let start = |chat| Update::Start {
			chat: Chat { id: chat, private: true },
			payload: Some(token.to_string()),
			from: Account::default(),
		};
		hub(false).handle(start(7), now).await.unwrap();
		panel.alerts_access_seen(user, &operator, "Olga", now).await.unwrap();
		assert_eq!(hub(false).fan_out_own(now).await.unwrap(), 0, "an operator is not told of access requests");
		assert_eq!(hub(false).take_feed(Service::Sites, now).await.unwrap(), Some(2), "sites' two leads");
		assert_eq!(hub(false).take_feed(Service::ReviewArchive, now).await.unwrap(), None);
		assert_eq!(panel.alerts_feed_after(Service::Sites).await.unwrap(), 2);
		assert_eq!(hub(false).deliver(now).await.unwrap().sent, 1);
		assert_eq!(hub(true).deliver(now + SignedDuration::from_secs(2)).await.unwrap().dead, 1);
		assert_eq!(
			hub(false).recheck_access(now + SignedDuration::from_hours(24 * 8)).await.unwrap(),
			0,
			"a dead chat is not rechecked"
		);
		let holder = uuid::Uuid::now_v7();
		assert_eq!(panel.telegram_poll_lease(holder, now, now + SignedDuration::from_mins(1)).await.unwrap(), Some(0));
		assert!(panel.telegram_poll_advance(holder, 5).await.unwrap());
		panel.telegram_poll_release(holder).await.unwrap();
		hub(false)
			.handle(
				Update::Stop {
					chat: Chat { id: 7, private: true },
				},
				now,
			)
			.await
			.unwrap();
		assert!(panel.alerts(user, &operator).await.unwrap().destinations.is_empty());

		let pool = panel.store().pool();
		for (sql, refusal) in [
			("DELETE FROM sources", "sources are never deleted"),
			("UPDATE sources SET brand_ids = '[\"x\"]'", "a source only ever gets revoked"),
			("INSERT INTO telegram_poller (id) VALUES (2)", "CHECK constraint failed"),
			(
				"INSERT INTO sources (key_id, kind, brand_ids, secret_sealed, data_key_fp) VALUES ('x', 'site', '[]', x'00', zeroblob(32))",
				"CHECK constraint failed",
			),
			(
				"INSERT INTO sources (key_id, kind, brand_ids, secret_sealed, data_key_fp) VALUES ('Bad Key', 'site', '[\"x\"]', x'00', zeroblob(32))",
				"CHECK constraint failed",
			),
		] {
			let err = sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await.unwrap_err().to_string();
			assert!(err.contains(refusal), "{sql}: {err}");
		}
		pool.close().await;
	}
}

/// The database the conductor starts from is the panel's whole: the sites' tables go, the
/// conductor's rows stay, and a message sent before keeps buttons that call sites for the lead
/// it named.
#[tokio::test]
async fn the_sites_tables_go_and_sent_buttons_name_their_calls() {
	const MOVED_OUT: i64 = 20261102090000;
	let db = TestDb::create().await;
	let pool = db.pool().await;
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&pool, MOVED_OUT - 1).await.unwrap();
	for sql in [
		"INSERT INTO sources (key_id, kind, brand_ids, secret_sealed, data_key_fp) VALUES ('aquafix-site', 'site', '[\"aquafix\"]', x'00', zeroblob(32))",
		"INSERT INTO events (id, schema, type, type_version, occurred_at, received_at, source_kind, source_id, key_id, brand_id, lead_id, \
		 status, properties, content_mac) VALUES (zeroblob(16), 'sa.funnel.v1', 'lead.created', 1, 1, 1, 'site', 'aquafix-site', 'aquafix-site', \
		 'aquafix', 'L-1', 'registered', '{}', zeroblob(32))",
		"INSERT INTO telegram_outbox (user_id, chat_id, rule, event_id, brand_id, lead_id, buttons, state, next_attempt_at, created_at, sent_at, tg_message_id) \
		 VALUES (zeroblob(16), 7, 'new_lead', zeroblob(16), 'aquafix', 'L-1', '[\"take\",\"no_answer\"]', 'sent', 1, 1, 1, 9)",
		"INSERT INTO telegram_outbox (user_id, chat_id, rule, event_id, buttons, text_sealed, data_key_fp, next_attempt_at, created_at) \
		 VALUES (zeroblob(16), 7, 'access_requested', zeroblob(16), '[]', x'00', zeroblob(32), 1, 1)",
	] {
		sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&pool).await.unwrap();
	}

	migrator.run_to(ALERTS - 1, &pool).await.unwrap();
	let tables: Vec<String> =
		sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name NOT LIKE '\\_%' ESCAPE '\\' AND name <> 'sqlite_sequence' ORDER BY name")
			.fetch_all(&pool)
			.await
			.unwrap();
	assert_eq!(
		tables,
		[
			"access_requests",
			"charge_cursors",
			"consumed_states",
			"gmails",
			"gmbs",
			"gmbs_off",
			"sessions",
			"sources",
			"telegram_fanout",
			"telegram_link_tokens",
			"telegram_links",
			"telegram_outbox",
			"telegram_poller",
			"telegram_replies",
			"telegram_rules",
			"token_kickbacks",
			"token_ledger",
			"users"
		]
	);
	let sources: i64 = sqlx::query_scalar("SELECT count(*) FROM sources").fetch_one(&pool).await.unwrap();
	assert_eq!(sources, 1);
	let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT buttons, last_error FROM telegram_outbox ORDER BY id").fetch_all(&pool).await.unwrap();
	let sent: serde_json::Value = serde_json::from_str(&rows[0].0).unwrap();
	let call = |b: &serde_json::Value| (b["id"].clone(), b["call"].clone(), b["leaves"].clone());
	assert_eq!(
		sent.as_array().unwrap().iter().map(call).collect::<Vec<_>>(),
		[
			(
				json!("take"),
				json!({"method": "POST", "path": "/api/v1/leads/aquafix/L-1/stage", "body": {"stage": "contacted"}}),
				json!([])
			),
			(
				json!("no_answer"),
				json!({"method": "POST", "path": "/api/v1/leads/aquafix/L-1/no_answer", "body": null}),
				json!(["take"])
			),
		]
	);
	assert_eq!(rows[1].1.as_deref(), Some("cut-over"), "what was queued is given up: its text is of the earlier shape");

	// Later migrations empty the outbox; a button's shape is moved by this one alone.
	let buttons = || async {
		sqlx::query_scalar::<_, String>("SELECT buttons FROM telegram_outbox ORDER BY id LIMIT 1")
			.fetch_one(&pool)
			.await
			.unwrap()
	};
	let before = buttons().await;
	sqlx::raw_sql(include_str!("../../migrations/20261117090000_buttons_by_operation.up.sql"))
		.execute(&pool)
		.await
		.unwrap();
	let sent: Vec<sa_alerts::Button> = serde_json::from_str(&buttons().await).unwrap();
	assert_eq!(
		sent.iter()
			.map(|b| (b.id.as_str(), b.call.operation.as_str(), json!(b.call.path), &b.call.body))
			.collect::<Vec<_>>(),
		[
			("take", "move_lead", json!({"brand": "aquafix", "lead": "L-1"}), &json!({"stage": "contacted"})),
			("no_answer", "no_answer", json!({"brand": "aquafix", "lead": "L-1"}), &json!(null)),
		],
		"a button names its operation and that path's parameters"
	);
	sqlx::raw_sql(include_str!("../../migrations/20261117090000_buttons_by_operation.down.sql"))
		.execute(&pool)
		.await
		.unwrap();
	assert_eq!(buttons().await, before);

	migrator.undo(&pool, MOVED_OUT - 1).await.unwrap();
	let back: (String, Option<String>, Option<String>) = sqlx::query_as("SELECT buttons, brand_id, lead_id FROM telegram_outbox ORDER BY id LIMIT 1")
		.fetch_one(&pool)
		.await
		.unwrap();
	assert_eq!(back, (r#"["take","no_answer"]"#.to_owned(), Some("aquafix".to_owned()), Some("L-1".to_owned())));
}

/// A Telegram link and a link token made under roles carry, after the migration, exactly what
/// the role's alias held; and go back to the role on the way down.
#[tokio::test]
async fn telegram_roles_become_permissions_both_ways() {
	const PERMISSIONS: i64 = 20261006090000;
	// `sa:admin` and `sa:operator` as the migration wrote them: the aliases of that day, frozen (re-spelled under
	// `sa:sites:` since). A permission added to the alias since (`sa:sites:work:invoices:create`)
	// reaches a link when concierge is next asked.
	const ADMIN_WHEN_WRITTEN: &[&str] = &[
		"sa:admin:sources:manage",
		"sa:sites:analysis:experiments:edit",
		"sa:sites:analysis:read",
		"sa:playbook:mcp:use",
		"sa:review_archive:archive:operate",
		"sa:review_archive:members:act_as",
		"sa:review_archive:tokens:grant",
		"sa:sites:work:leads:edit",
		"sa:sites:work:pii:see",
		"sa:sites:work:places:edit",
		"sa:sites:work:pricing:edit",
		"sa:sites:work:read",
	];
	const OPERATOR_WHEN_WRITTEN: &[&str] = &["sa:sites:analysis:read", "sa:sites:work:leads:edit", "sa:sites:work:pii:see", "sa:sites:work:read"];
	let db = TestDb::create().await;
	let pool = db.pool().await;
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&pool, PERMISSIONS - 1).await.unwrap();
	let (ann, bob) = (uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(2));
	for (user, chat, role) in [(ann, 7, "operator"), (bob, 8, "admin")] {
		sqlx::query("INSERT INTO telegram_links (user_id, chat_id, linked_at, role, role_checked_at, display_name) VALUES ($1, $2, 0, $3, 5, 'x')")
			.bind(user)
			.bind(chat)
			.bind(role)
			.execute(&pool)
			.await
			.unwrap();
	}
	sqlx::query("INSERT INTO telegram_link_tokens (token_hash, user_id, role, display_name, expires_at) VALUES (zeroblob(32), $1, 'admin', 'x', 9)")
		.bind(bob)
		.execute(&pool)
		.await
		.unwrap();

	migrator.run_to(ALERTS - 1, &pool).await.unwrap();
	let held = |sql: &'static str, user: uuid::Uuid| {
		let pool = pool.clone();
		async move {
			let raw: String = sqlx::query_scalar(sql).bind(user).fetch_one(&pool).await.unwrap();
			(serde_json::from_str::<PermissionSet>(&raw).unwrap(), raw)
		}
	};
	let alias = |a: &[&str]| a.iter().copied().collect::<PermissionSet>();
	let links = "SELECT permissions FROM telegram_links WHERE user_id = $1";
	let (operator, raw) = held(links, ann).await;
	assert_eq!(operator, alias(OPERATOR_WHEN_WRITTEN));
	assert_eq!(raw, serde_json::to_string(&operator).unwrap(), "written as the panel writes a set");
	assert_eq!(held(links, bob).await.0, alias(ADMIN_WHEN_WRITTEN));
	assert_eq!(held("SELECT permissions FROM telegram_link_tokens WHERE user_id = $1", bob).await.0, alias(ADMIN_WHEN_WRITTEN));
	let checked: i64 = sqlx::query_scalar("SELECT permissions_checked_at FROM telegram_links WHERE user_id = $1")
		.bind(ann)
		.fetch_one(&pool)
		.await
		.unwrap();
	assert_eq!(checked, 5);

	migrator.undo(&pool, PERMISSIONS - 1).await.unwrap();
	let roles: Vec<(uuid::Uuid, String)> = sqlx::query_as("SELECT user_id, role FROM telegram_links ORDER BY user_id").fetch_all(&pool).await.unwrap();
	assert_eq!(roles, [(ann, "operator".to_owned()), (bob, "admin".to_owned())]);
	let token: String = sqlx::query_scalar("SELECT role FROM telegram_link_tokens").fetch_one(&pool).await.unwrap();
	assert_eq!(token, "admin");
}

/// A site key carries the origin its landing answers on; a key from before origins existed gets
/// one once, and then keeps it.
#[tokio::test]
async fn a_site_origin_is_set_once() {
	let db = TestDb::create().await;
	let key = DataKey::from_hex(&DataKey::generate_hex().unwrap()).unwrap();
	let panel = panel_with(db.store().await, key);
	let aquafix = || [BrandId::parse("aquafix").unwrap()].into();
	assert!(panel.add_source("aquafix-site", SourceKind::Site, aquafix(), None).await.is_err(), "a site key without an origin");
	assert!(
		panel.add_source("aquafix-bot", SourceKind::Bot, aquafix(), Some(origin("https://aquafix.top"))).await.is_err(),
		"an origin on a key that is not a site's"
	);
	panel
		.add_source("vifnet-site", SourceKind::Site, aquafix(), Some(origin("https://vifnet.site")))
		.await
		.unwrap()
		.unwrap();
	panel.add_source("aquafix-bot", SourceKind::Bot, aquafix(), None).await.unwrap().unwrap();

	let pool = panel.store().pool();
	sqlx::query("INSERT INTO sources (key_id, kind, brand_ids, secret_sealed, data_key_fp) VALUES ('aquafix-site', 'site', '[\"aquafix\"]', x'00', zeroblob(32))")
		.execute(pool)
		.await
		.unwrap();
	let origin_of = |key: &'static str| {
		let panel = panel.clone();
		async move {
			panel
				.store()
				.sources()
				.await
				.unwrap()
				.into_iter()
				.find(|s| s.grant.key_id == key)
				.unwrap()
				.origin
				.map(|o| o.to_string())
		}
	};
	assert_eq!(origin_of("aquafix-site").await, None, "made before origins");
	panel.set_site_origin("aquafix-site", &origin("https://aquafix.top")).await.unwrap().unwrap();
	panel
		.set_site_origin("aquafix-site", &origin("https://aquafix.top"))
		.await
		.unwrap()
		.expect("the same again is no change");
	assert!(panel.set_site_origin("aquafix-site", &origin("https://elsewhere.top")).await.unwrap().is_err());
	assert!(panel.set_site_origin("aquafix-bot", &origin("https://aquafix.top")).await.unwrap().is_err());
	assert!(panel.set_site_origin("nobody", &origin("https://aquafix.top")).await.unwrap().is_err());
	assert_eq!(origin_of("aquafix-site").await.as_deref(), Some("https://aquafix.top"));
	assert_eq!(origin_of("vifnet-site").await.as_deref(), Some("https://vifnet.site"));

	for (sql, refusal) in [
		(
			"UPDATE sources SET origin = 'https://elsewhere.top' WHERE key_id = 'aquafix-site'",
			"a source only ever gets revoked",
		),
		("UPDATE sources SET origin = NULL WHERE key_id = 'aquafix-site'", "a source only ever gets revoked"),
		("UPDATE sources SET origin = 'https://aquafix.top' WHERE key_id = 'aquafix-bot'", "CHECK constraint failed"),
	] {
		let err = sqlx::query(sqlx::AssertSqlSafe(sql)).execute(pool).await.unwrap_err().to_string();
		assert!(err.contains(refusal), "{sql}: {err}");
	}
	pool.close().await;
}

/// A linked chat becomes its user's private destination — the rules they chose, their access,
/// what was sent to it (its buttons still pressable under the same ids) — and the sites cursor
/// carries over; on the way down, the same.
#[tokio::test]
async fn telegram_links_become_private_destinations_both_ways() {
	let db = TestDb::create().await;
	let pool = db.pool().await;
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&pool, ALERTS - 1).await.unwrap();
	let (ann, bob) = (uuid::Uuid::from_u128(1), uuid::Uuid::from_u128(2));
	for sql in [
		"INSERT INTO telegram_links (user_id, chat_id, linked_at, permissions, permissions_checked_at, display_name, tg_username) \
		 VALUES (x'00000000000000000000000000000001', 7, 10, '[\"sa:sites:work:read\"]', 11, 'Ann', 'ann')",
		"INSERT INTO telegram_links (user_id, chat_id, linked_at, permissions, permissions_checked_at, display_name, dead_at) \
		 VALUES (x'00000000000000000000000000000002', 8, 12, NULL, 13, 'Bob', 99)",
		"INSERT INTO telegram_rules VALUES (x'00000000000000000000000000000001', 'new_lead', 0)",
		"INSERT INTO telegram_outbox (user_id, chat_id, rule, event_id, buttons, state, next_attempt_at, created_at, sent_at, tg_message_id) \
		 VALUES (x'00000000000000000000000000000001', 7, 'new_lead', zeroblob(16), '[]', 'sent', 1, 1, 1, 9)",
		"INSERT INTO telegram_outbox (user_id, chat_id, rule, event_id, buttons, state, next_attempt_at, created_at, sent_at, tg_message_id) \
		 VALUES (x'00000000000000000000000000000001', 7, 'booked', zeroblob(16), '[]', 'sent', 1, 1, 1, 9)",
		"DELETE FROM telegram_outbox WHERE rule = 'booked'",
		"UPDATE telegram_poller SET notes_after = 42",
	] {
		sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&pool).await.unwrap();
	}

	migrator.run_to(METHODS - 1, &pool).await.unwrap();
	let destinations: Vec<(uuid::Uuid, String, i64, Option<String>, bool)> = sqlx::query_as(
		"SELECT d.user_id, t.kind, t.chat_id, t.title, t.dead_at IS NOT NULL FROM alert_destinations d \
		 JOIN telegram_destinations t ON t.destination_id = d.id ORDER BY d.id",
	)
	.fetch_all(&pool)
	.await
	.unwrap();
	assert_eq!(destinations, [(ann, "private".into(), 7, Some("@ann".into()), false), (bob, "private".into(), 8, None, true)]);
	let rules: Vec<(String, bool)> = sqlx::query_as("SELECT rule, enabled FROM alert_rules").fetch_all(&pool).await.unwrap();
	assert_eq!(rules, [("new_lead".to_owned(), false)]);
	let access: Vec<(Option<String>, i64)> = sqlx::query_as("SELECT permissions, confirmed_at FROM alert_access ORDER BY user_id")
		.fetch_all(&pool)
		.await
		.unwrap();
	assert_eq!(access, [(Some(r#"["sa:sites:work:read"]"#.to_owned()), 11), (None, 13)]);
	let after: i64 = sqlx::query_scalar("SELECT after FROM alert_feeds WHERE service = 'sites'").fetch_one(&pool).await.unwrap();
	assert_eq!(after, 42);
	let sent: Vec<i64> = sqlx::query_scalar("SELECT id FROM telegram_outbox").fetch_all(&pool).await.unwrap();
	assert_eq!(sent, [1], "what was sent, under its id");
	sqlx::query(
		"INSERT INTO telegram_outbox (destination_id, user_id, chat_id, rule, event_id, buttons, state, next_attempt_at, created_at) \
	             SELECT destination_id, $1, 7, 'booked', zeroblob(16), '[]', 'dead', 1, 1 FROM telegram_destinations WHERE chat_id = 7",
	)
	.bind(ann)
	.execute(&pool)
	.await
	.unwrap();
	let next: i64 = sqlx::query_scalar("SELECT max(id) FROM telegram_outbox").fetch_one(&pool).await.unwrap();
	assert_eq!(next, 3, "an id handed out before is never handed out again");

	migrator.undo(&pool, ALERTS - 1).await.unwrap();
	let links: Vec<(uuid::Uuid, i64, Option<String>, Option<i64>)> = sqlx::query_as("SELECT user_id, chat_id, tg_username, dead_at FROM telegram_links ORDER BY user_id")
		.fetch_all(&pool)
		.await
		.unwrap();
	assert_eq!(links, [(ann, 7, Some("ann".into()), None), (bob, 8, None, Some(99))]);
	let rules: Vec<(uuid::Uuid, String, bool)> = sqlx::query_as("SELECT user_id, rule, enabled FROM telegram_rules").fetch_all(&pool).await.unwrap();
	assert_eq!(rules, [(ann, "new_lead".to_owned(), false)]);
	let notes_after: i64 = sqlx::query_scalar("SELECT notes_after FROM telegram_poller").fetch_one(&pool).await.unwrap();
	assert_eq!(notes_after, 42);
}

/// A Telegram destination keeps its chat, what it was scoped to and the rules its person chose, at a
/// cadence for each tier; no message's id is handed out twice; on the way down, the same.
#[tokio::test]
async fn telegram_destinations_become_one_method_of_many_both_ways() {
	let db = TestDb::create().await;
	let pool = db.pool().await;
	// One connection: another may still read the schema as it was before an undo.
	let mut conn = pool.acquire().await.unwrap();
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&mut *conn, METHODS - 1).await.unwrap();
	for sql in [
		"INSERT INTO users VALUES (x'00000000000000000000000000000001', 'ann@localhost', 'Ann', 1, 1)",
		"INSERT INTO gmails (id, owner, gmail, created_at) VALUES (5, x'00000000000000000000000000000001', 'a@gmail.com', 1)",
		"INSERT INTO alert_destinations (id, user_id, method, gmail_id, created_at) VALUES (3, x'00000000000000000000000000000001', 'telegram', 5, 10)",
		"INSERT INTO alert_destinations (id, user_id, method, created_at) VALUES (4, x'00000000000000000000000000000001', 'telegram', 11)",
		"INSERT INTO telegram_destinations (destination_id, kind, chat_id, title) VALUES (3, 'private', 7, '@ann')",
		"INSERT INTO telegram_destinations (destination_id, kind, chat_id, topic_id, title, dead_at, dead_reason) VALUES (4, 'group', -9, 2, 'Ops', 99, 'kicked')",
		"INSERT INTO alert_rules VALUES (3, 'new_lead', 0), (3, 'review_new', 1), (3, 'source_silent', 1)",
		"INSERT INTO telegram_outbox (id, destination_id, user_id, chat_id, rule, event_id, state, next_attempt_at, created_at) \
		 VALUES (41, 3, x'00000000000000000000000000000001', 7, 'booked', zeroblob(16), 'sent', 1, 1)",
	] {
		sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.unwrap();
	}

	migrator.run(&mut *conn).await.unwrap();
	type Row = (i64, bool, Option<String>, String, String, String, String, Option<String>, String, i64, Option<i64>);
	let destinations: Vec<Row> = sqlx::query_as(
		"SELECT d.id, d.private, d.title, d.urgent, d.important, d.info, d.filter, d.dead_reason, t.kind, t.chat_id, t.topic_id \
		 FROM alert_destinations d JOIN telegram_destinations t ON t.destination_id = d.id ORDER BY d.id",
	)
	.fetch_all(&mut *conn)
	.await
	.unwrap();
	let filter = |f: &str| serde_json::from_str::<sa_core::alerts::Filter>(f).unwrap();
	assert_eq!(filter(&destinations[0].6).gmails, [5].into());
	assert_eq!(filter(&destinations[1].6), Default::default());
	let kept: Vec<_> = destinations.into_iter().map(|(id, private, title, u, i, n, _, dead, kind, chat, topic)| (id, private, title, [u, i, n], dead, kind, chat, topic)).collect();
	let cadences = ["now", "now", "off"].map(str::to_owned);
	assert_eq!(
		kept,
		[
			(3, true, Some("@ann".to_owned()), cadences.clone(), None, "private".to_owned(), 7, None),
			(4, false, Some("Ops".to_owned()), cadences, Some("kicked".to_owned()), "group".to_owned(), -9, Some(2)),
		]
	);
	let rules: Vec<(i64, String, String)> = sqlx::query_as("SELECT destination_id, rule, cadence FROM alert_rules ORDER BY 1, 2").fetch_all(&mut *conn).await.unwrap();
	let rule = |d: i64, rule: &str, cadence: &str| (d, rule.to_owned(), cadence.to_owned());
	assert_eq!(
		rules,
		[rule(3, "new_lead", "off"), rule(3, "review_new", "now"), rule(3, "source_silent", "now"), rule(4, "source_silent", "off")],
		"silent sources, off until turned on, stay so"
	);
	let seq: i64 = sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'alert_outbox'").fetch_one(&mut *conn).await.unwrap();
	assert_eq!(seq, 41, "a button's data names its message's id");
	for (sql, refusal) in [
		("INSERT INTO alert_hours (user_id, tz, daily, quiet_to) VALUES (zeroblob(16), 'UTC', 0, 5)", "CHECK constraint failed"),
		("INSERT INTO alert_hours (user_id, tz, daily, quiet_from, quiet_to) VALUES (zeroblob(16), 'UTC', 0, 5, 5)", "CHECK constraint failed"),
	] {
		let err = sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.unwrap_err().to_string();
		assert!(err.contains(refusal), "{sql}: {err}");
	}

	migrator.undo(&mut *conn, METHODS - 1).await.unwrap();
	let back: Vec<(i64, Option<i64>, String, i64, Option<String>)> = sqlx::query_as(
		"SELECT d.id, d.gmail_id, t.kind, t.chat_id, t.dead_reason FROM alert_destinations d JOIN telegram_destinations t ON t.destination_id = d.id ORDER BY d.id",
	)
	.fetch_all(&mut *conn)
	.await
	.unwrap();
	assert_eq!(back, [(3, Some(5), "private".to_owned(), 7, None), (4, None, "group".to_owned(), -9, Some("kicked".to_owned()))]);
	let rules: Vec<(String, bool)> = sqlx::query_as("SELECT rule, enabled FROM alert_rules WHERE destination_id = 3 ORDER BY 1").fetch_all(&mut *conn).await.unwrap();
	assert_eq!(rules, [("new_lead".to_owned(), false), ("review_new".to_owned(), true), ("source_silent".to_owned(), true)]);
}

/// A free-text Google category becomes its gcid by its exact name, none stays none, and a label the
/// migration does not know stops it, named.
#[tokio::test]
async fn google_labels_become_gcids() {
	let db = TestDb::create().await;
	let pool = db.pool().await;
	// One connection: another may still read the schema as it was before an undo.
	let mut conn = pool.acquire().await.unwrap();
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&mut *conn, GOOGLE_CATEGORIES - 1).await.unwrap();
	for sql in [
		"INSERT INTO users VALUES (x'00000000000000000000000000000001', 'ann@localhost', 'Ann', 1, 1)",
		"INSERT INTO trades (owner, name, created_at) VALUES (x'00000000000000000000000000000001', 'Plumbing', 1)",
		"INSERT INTO categories VALUES (1, 'FR', '[]', 'Plombier', NULL)",
		"INSERT INTO categories VALUES (1, 'US', '[]', NULL, NULL)",
	] {
		sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.unwrap();
	}
	migrator.run_to(GOOGLES - 1, &mut *conn).await.unwrap(); // the trades as these migrations left them
	let categories: Vec<(String, Option<String>, String)> = sqlx::query_as("SELECT country, google_primary, google_additional FROM categories ORDER BY country")
		.fetch_all(&mut *conn)
		.await
		.unwrap();
	assert_eq!(categories, [("FR".into(), Some("gcid:plumber".into()), "[]".into()), ("US".into(), None, "[]".into())]);

	migrator.undo(&mut *conn, GOOGLE_CATEGORIES - 1).await.unwrap();
	sqlx::query("INSERT INTO categories VALUES (1, 'GB', '[]', 'Plumbing contractor', NULL)")
		.execute(&mut *conn)
		.await
		.unwrap();
	let err = migrator.run_to(GOOGLES - 1, &mut *conn).await.unwrap_err().to_string();
	assert!(err.contains("GB 'Plumbing contractor'"), "the down migration's gcid stays: {err}");
	assert!(!err.contains("FR"), "the down migration's gcid stays: {err}");
}

/// Keywords become weighed ones, the primary's name among them at weight 1 and the rest unweighed;
/// an additional category, whose weight is a person's to give, stops the migration, named.
#[tokio::test]
async fn a_category_becomes_its_studies_definition() {
	let db = TestDb::create().await;
	let pool = db.pool().await;
	let mut conn = pool.acquire().await.unwrap();
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&mut *conn, DEFINITION - 1).await.unwrap();
	for sql in [
		"INSERT INTO users VALUES (x'00000000000000000000000000000001', 'ann@localhost', 'Ann', 1, 1)",
		"INSERT INTO trades (owner, name, created_at) VALUES (x'00000000000000000000000000000001', 'Plumbing', 1)",
		r#"INSERT INTO categories VALUES (1, 'FR', '["fuite d''eau"]', 'gcid:plumber', '[]', 'plumbing')"#,
		r#"INSERT INTO categories VALUES (1, 'US', '["plumber", "drain"]', 'gcid:plumber', '[]', NULL)"#,
		r#"INSERT INTO categories VALUES (1, 'GB', '["plumber"]', NULL, '[]', NULL)"#,
	] {
		sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.unwrap();
	}
	migrator.run_to(GOOGLES - 1, &mut *conn).await.unwrap(); // the trades as these migrations left them
	let keywords: Vec<(String, String)> = sqlx::query_as("SELECT country, keywords FROM categories ORDER BY country").fetch_all(&mut *conn).await.unwrap();
	let keywords: Vec<(String, serde_json::Value)> = keywords.into_iter().map(|(c, k)| (c, serde_json::from_str(&k).unwrap())).collect();
	assert_eq!(
		keywords,
		[
			("FR".into(), json!([{ "text": "fuite d'eau", "weight": null }, { "text": "plombier", "weight": 1.0 }])),
			("GB".into(), json!([{ "text": "plumber", "weight": null }])),
			("US".into(), json!([{ "text": "plumber", "weight": 1.0 }, { "text": "drain", "weight": null }])),
		]
	);

	migrator.undo(&mut *conn, DEFINITION - 1).await.unwrap();
	sqlx::query(r#"UPDATE categories SET google_additional = '["gcid:heating_contractor"]' WHERE country = 'FR'"#)
		.execute(&mut *conn)
		.await
		.unwrap();
	let err = migrator.run_to(GOOGLES - 1, &mut *conn).await.unwrap_err().to_string();
	assert!(err.contains("1·FR"), "{err}");
}

/// Ann's Plumbing (FR, US) and Heating (FR), Bob's Drains (FR): what every case of
/// [`trades_become_googles_categories`] starts from, one schema before it.
async fn trades_as_they_were(db: &TestDb) -> sqlx::pool::PoolConnection<sqlx::Sqlite> {
	let mut conn = db.pool().await.acquire().await.unwrap();
	sqlx::migrate!("./migrations").undo(&mut *conn, GOOGLES - 1).await.unwrap();
	let category = |trade: i64, country: &str, gcid: &str| {
		format!(
			r#"INSERT INTO categories VALUES ({trade}, '{country}', '[{{"text":"plombier","weight":1.0}}]', '{gcid}', '[{{"id":"gcid:drainage_service","weight":0.5}}]', '{{"words":["magasin"],"types":[]}}', NULL)"#
		)
	};
	for sql in [
		"INSERT INTO users VALUES (x'00000000000000000000000000000001', 'ann@localhost', 'Ann', 1, 1)".to_owned(),
		"INSERT INTO users VALUES (x'00000000000000000000000000000002', 'bob@localhost', 'Bob', 1, 1)".to_owned(),
		"INSERT INTO trades (id, owner, name, created_at) VALUES (1, x'00000000000000000000000000000001', 'Plumbing', 5)".to_owned(),
		"INSERT INTO trades (id, owner, name, created_at) VALUES (2, x'00000000000000000000000000000001', 'Heating', 6)".to_owned(),
		"INSERT INTO trades (id, owner, name, created_at) VALUES (3, x'00000000000000000000000000000002', 'Drains', 7)".to_owned(),
		category(1, "FR", "gcid:plumber"),
		category(1, "US", "gcid:plumber"),
		category(2, "FR", "gcid:heating_contractor"),
		category(3, "FR", "gcid:plumber"),
		"INSERT INTO gmails (id, owner, gmail, created_at) VALUES (1, x'00000000000000000000000000000001', 'ann@gmail.com', 1)".to_owned(),
		"INSERT INTO gmbs (id, owner, gmail_id, name, state, country, created_at, trade_id) VALUES (10, x'00000000000000000000000000000001', 1, 'Lyon 7', 'draft', 'FR', 1, 1)".to_owned(),
		"INSERT INTO gmbs (id, owner, name, state, created_at, trade_id) VALUES (11, x'00000000000000000000000000000001', 'Lyon 8', 'draft', 1, 2)".to_owned(),
		"INSERT INTO brands (id, owner, gmail_id, trade_id, country, name, site, created_at) VALUES (1, x'00000000000000000000000000000001', 1, 1, 'FR', 'Aquafix', 'aquafix', 1)".to_owned(),
		"UPDATE gmbs SET brand_id = 1 WHERE id = 10".to_owned(),
		"INSERT INTO kit_drafts (id, trade_id, country, text, created_at) VALUES (1, 1, 'FR', 'Quick and clean.', 1)".to_owned(),
		format!("INSERT INTO placement_pulls VALUES ('{}', 1)", "a".repeat(64)),
	] {
		sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.unwrap();
	}
	conn
}

/// Each old category becomes its owner's category of its primary Google category, its body one
/// version and one move; GBPs, brands and drafts follow to the gcid; a pull's resume state goes.
#[tokio::test]
async fn trades_become_googles_categories() {
	let db = TestDb::create().await;
	let mut conn = trades_as_they_were(&db).await;
	sqlx::migrate!("./migrations").run(&mut *conn).await.unwrap();
	let categories: Vec<(String, String, String, String)> = sqlx::query_as(
		"SELECT u.name, c.trade, c.country, m.why FROM categories c JOIN users u ON u.user_id = c.owner \
		 JOIN category_moves m ON m.owner = c.owner AND m.trade = c.trade AND m.country = c.country AND m.to_version = c.head AND m.from_version IS NULL \
		 ORDER BY u.name, c.trade, c.country",
	)
	.fetch_all(&mut *conn)
	.await
	.unwrap();
	let row = |who: &str, trade: &str, country: &str| (who.to_owned(), trade.to_owned(), country.to_owned(), "edit".to_owned());
	assert_eq!(
		categories,
		[
			row("Ann", "gcid:heating_contractor", "FR"),
			row("Ann", "gcid:plumber", "FR"),
			row("Ann", "gcid:plumber", "US"),
			row("Bob", "gcid:plumber", "FR")
		]
	);
	let body: String = sqlx::query_scalar("SELECT v.body FROM categories c JOIN category_versions v ON v.id = c.head WHERE c.trade = 'gcid:plumber' AND c.country = 'US'")
		.fetch_one(&mut *conn)
		.await
		.unwrap();
	assert_eq!(
		serde_json::from_str::<serde_json::Value>(&body).unwrap(),
		json!({ "keywords": [{ "text": "plombier", "weight": 1.0 }], "additional": [{ "id": "gcid:drainage_service", "weight": 0.5 }],
			"excluded": { "words": ["magasin"], "types": [] }, "demand": null }),
		"the body as it was, without its primary: the trade is it"
	);
	let gmbs: Vec<(i64, Option<String>)> = sqlx::query_as("SELECT id, trade FROM gmbs ORDER BY id").fetch_all(&mut *conn).await.unwrap();
	assert_eq!(
		gmbs,
		[(10, Some("gcid:plumber".to_owned())), (11, Some("gcid:heating_contractor".to_owned()))],
		"by its category in its country, else the trade's only gcid"
	);
	let brand: (String, String) = sqlx::query_as("SELECT trade, country FROM brands").fetch_one(&mut *conn).await.unwrap();
	assert_eq!(brand, ("gcid:plumber".to_owned(), "FR".to_owned()));
	let draft: (uuid::Uuid, String) = sqlx::query_as("SELECT owner, trade FROM kit_drafts").fetch_one(&mut *conn).await.unwrap();
	assert_eq!(draft, (uuid::Uuid::from_u128(1), "gcid:plumber".to_owned()));
	let pulls: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM placement_pulls").fetch_one(&mut *conn).await.unwrap();
	assert_eq!(pulls, 0, "resume state keyed by definitions that hashed the retired trade ids");
}

/// What the migration cannot map stops it, each offender named.
#[tokio::test]
async fn trades_that_map_to_no_one_gcid_stop_the_migration() {
	for (offend, named) in [
		(
			vec![
				"INSERT INTO trades (id, owner, name, created_at) VALUES (4, x'00000000000000000000000000000002', 'Painting', 1)",
				"INSERT INTO categories VALUES (4, 'FR', '[]', NULL, '[]', '{\"words\":[],\"types\":[]}', NULL)",
			],
			"Painting·FR",
		),
		(
			vec![
				"INSERT INTO trades (id, owner, name, created_at) VALUES (4, x'00000000000000000000000000000002', 'Pipes', 1)",
				"INSERT INTO categories VALUES (4, 'FR', '[]', 'gcid:plumber', '[]', '{\"words\":[],\"types\":[]}', NULL)",
			],
			"gcid:plumber·FR (Drains, Pipes)",
		),
		(
			vec![
				"INSERT INTO trades (id, owner, name, created_at) VALUES (4, x'00000000000000000000000000000002', 'Roofing', 1)",
				"INSERT INTO categories VALUES (4, 'FR', '[]', 'gcid:roofing_contractor', '[]', '{\"words\":[],\"types\":[]}', NULL)",
				"INSERT INTO categories VALUES (4, 'US', '[]', 'gcid:roofer', '[]', '{\"words\":[],\"types\":[]}', NULL)",
				"INSERT INTO gmbs (id, owner, name, state, created_at, trade_id) VALUES (12, x'00000000000000000000000000000002', 'Toits', 'draft', 1, 4)",
			],
			"12 'Toits' (Roofing)",
		),
	] {
		let db = TestDb::create().await;
		let mut conn = trades_as_they_were(&db).await;
		for sql in offend {
			sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&mut *conn).await.unwrap();
		}
		let err = sqlx::migrate!("./migrations").run(&mut *conn).await.unwrap_err().to_string();
		assert!(err.contains(named), "{named}: {err}");
	}
}

fn origin(raw: &str) -> SiteOrigin {
	SiteOrigin::parse(raw).unwrap()
}

fn cabinet() -> url::Url {
	"https://evinvest.test".parse().unwrap()
}
