//! What is removed waits in its owner's trash, restorable, until it is purged.

use jiff::{SignedDuration, Timestamp};
use sa_core::gbp::{Country, OnGoogle};
use sa_panel::{
	Panel, Refused,
	gmbs::Existing,
	import::Export,
	testing::{TestDb, panel},
	tokens::User,
	trash::Trash,
};
use uuid::Uuid;

const ONE: &str = "ChIJLU7jZClu5kcR4PcOOO6p3I0";

const TWO: &str = "ChIJD7fiBh9u5kcRYJSMaMOCCwQ";
const LOOSE: &str = "ChIJAQAAAAAAAAAAAAAAAAAAAAA";
fn now() -> Timestamp {
	"2026-11-01T12:00:00Z".parse().unwrap()
}

fn days(n: i64) -> SignedDuration {
	SignedDuration::from_hours(24 * n)
}

async fn opened(db: &TestDb) -> (Panel, Uuid) {
	let p = panel(db).await;
	p.import_review_archive(Export {
		people: vec![],
		gmails: vec![],
		gmbs: vec![],
		ledger: vec![],
		kickbacks: vec![],
	})
	.await
	.unwrap();
	let owner = Uuid::now_v7();
	p.user_seen(
		&User {
			user_id: owner,
			email: "owner@localhost".to_owned(),
			name: "Owner".to_owned(),
		},
		now(),
	)
	.await
	.unwrap();
	(p, owner)
}

fn existing(trade: i64, gmail_id: i64, place: &str, name: &str) -> Existing {
	Existing {
		gmail_id,
		place: place.to_owned(),
		state: OnGoogle::Verified,
		name: Some(name.to_owned()),
		country: Some(Country::FR),
		trade,
	}
}

/// The gmail's IMAP and Maps key rows.
async fn connections(db: &TestDb, gmail: i64) -> (i64, i64) {
	let pool = db.pool().await;
	let count = |table: &str| sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {table} WHERE gmail_id = $1"))).bind(gmail);
	(count("gmail_imap").fetch_one(&pool).await.unwrap(), count("gmail_maps_keys").fetch_one(&pool).await.unwrap())
}

async fn connect(db: &TestDb, gmail: i64) {
	let pool = db.pool().await;
	let fp = vec![0u8; 32];
	sqlx::query("INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at) VALUES ($1, 'imap.gmail.com', 'a', x'00', $2, 0, 0)")
		.bind(gmail)
		.bind(&fp)
		.execute(&pool)
		.await
		.unwrap();
	sqlx::query("INSERT INTO gmail_maps_keys (gmail_id, key_sealed, data_key_fp, last4, set_at, checked_at) VALUES ($1, x'00', $2, '1234', 0, 0)")
		.bind(gmail)
		.bind(&fp)
		.execute(&pool)
		.await
		.unwrap();
}

fn listed(t: &Trash) -> (Vec<(i64, usize)>, Vec<i64>) {
	(t.gmails.iter().map(|g| (g.id, g.gmbs)).collect(), t.gmbs.iter().map(|b| b.id).collect())
}

#[tokio::test]
async fn a_removed_gmail_waits_in_the_trash_and_comes_back_with_what_went_with_it() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let trade = p.add_trade(owner, "Plumbing", now()).await.unwrap().unwrap().id;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let one = p.add_gmb(Some(owner), existing(trade, gmail.id, ONE, "One"), now()).await.unwrap().unwrap();
	let two = p.add_gmb(Some(owner), existing(trade, gmail.id, TWO, "Two"), now()).await.unwrap().unwrap();
	let loose = p.add_gmb(Some(owner), existing(trade, gmail.id, LOOSE, "Loose"), now()).await.unwrap().unwrap();
	connect(&db, gmail.id).await;

	p.remove_gmb(owner, loose.id, now()).await.unwrap().unwrap();
	let later = now() + days(1);
	p.remove_gmail(owner, gmail.id, later).await.unwrap().unwrap();
	assert_eq!(connections(&db, gmail.id).await, (1, 1), "the trash keeps its connections");

	let trash = p.trash(owner, false).await.unwrap();
	assert_eq!(
		listed(&trash),
		(vec![(gmail.id, 2)], vec![loose.id]),
		"the gmail with the GBPs that went with it, the loose one apart"
	);
	assert_eq!(trash.gmails[0].purge_at, Some(later + days(30)));
	assert!(trash.gmbs[0].blocked.is_some(), "a GBP whose gmail is in the trash waits for it");
	assert!(matches!(p.restore_gmb(owner, loose.id).await.unwrap(), Err(Refused::Conflict(_))));

	p.restore_gmail(owner, gmail.id).await.unwrap().unwrap();
	let (gmails, gmbs) = p.gmails(owner).await.unwrap();
	assert_eq!(gmails.iter().map(|g| g.id).collect::<Vec<_>>(), vec![gmail.id]);
	assert_eq!(gmbs.iter().map(|b| b.id).collect::<Vec<_>>(), vec![one.id, two.id], "removed one by one before, it stays");
	assert_eq!(connections(&db, gmail.id).await, (1, 1));
	assert_eq!(listed(&p.trash(owner, false).await.unwrap()), (vec![], vec![loose.id]));

	p.restore_gmb(owner, loose.id).await.unwrap().unwrap();
	assert_eq!(p.gmails(owner).await.unwrap().1.len(), 3);
	let snapshot = p.snapshot(now()).await.unwrap().unwrap();
	assert!(snapshot.gmbs.iter().all(|m| !m.removed), "review_archive is told they are back");
}

#[tokio::test]
async fn a_restore_is_refused_while_a_live_gbp_holds_its_place_or_a_live_gmail_its_address() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let trade = p.add_trade(owner, "Plumbing", now()).await.unwrap().unwrap().id;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let one = p.add_gmb(Some(owner), existing(trade, gmail.id, ONE, "One"), now()).await.unwrap().unwrap();
	p.remove_gmb(owner, one.id, now()).await.unwrap().unwrap();
	p.add_gmb(Some(owner), existing(trade, gmail.id, ONE, "One again"), now()).await.unwrap().unwrap();

	let trash = p.trash(owner, false).await.unwrap();
	assert!(trash.gmbs[0].blocked.as_deref().is_some_and(|b| b.contains(ONE)), "{:?}", trash.gmbs[0].blocked);
	assert!(matches!(p.restore_gmb(owner, one.id).await.unwrap(), Err(Refused::Conflict(_))));

	p.remove_gmail(owner, gmail.id, now()).await.unwrap().unwrap();
	p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	assert!(p.trash(owner, false).await.unwrap().gmails[0].blocked.is_some());
	assert!(matches!(p.restore_gmail(owner, gmail.id).await.unwrap(), Err(Refused::Conflict(_))));
}

#[tokio::test]
async fn the_purge_ends_a_restore_once_the_kept_days_pass() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let trade = p.add_trade(owner, "Plumbing", now()).await.unwrap().unwrap().id;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let one = p.add_gmb(Some(owner), existing(trade, gmail.id, ONE, "One"), now()).await.unwrap().unwrap();
	connect(&db, gmail.id).await;
	p.remove_gmail(owner, gmail.id, now()).await.unwrap().unwrap();
	p.set_trash_settings(owner, 5, None).await.unwrap().unwrap();

	p.purge(now() + days(4), false).await.unwrap();
	assert_eq!(listed(&p.trash(owner, false).await.unwrap()), (vec![(gmail.id, 1)], vec![]));

	p.purge(now() + days(5), false).await.unwrap();
	assert_eq!(listed(&p.trash(owner, false).await.unwrap()), (vec![], vec![]), "gone for good");
	assert_eq!(connections(&db, gmail.id).await, (0, 0));
	assert!(matches!(p.restore_gmail(owner, gmail.id).await.unwrap(), Err(Refused::NotFound)));
	assert!(matches!(p.restore_gmb(owner, one.id).await.unwrap(), Err(Refused::NotFound)));
	let snapshot = p.snapshot(now()).await.unwrap().unwrap();
	assert!(snapshot.gmbs.iter().any(|m| m.id == one.id && m.removed), "its id stays with review_archive");
}

#[tokio::test]
async fn never_purge_keeps_everything() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	p.remove_gmail(owner, gmail.id, now()).await.unwrap().unwrap();

	assert_eq!(p.trash(owner, true).await.unwrap().gmails[0].purge_at, None, "kept by default: dev sign-in");
	p.purge(now() + days(365), true).await.unwrap();
	assert_eq!(p.trash(owner, true).await.unwrap().gmails.len(), 1);

	p.set_trash_settings(owner, 1, Some(true)).await.unwrap().unwrap();
	p.purge(now() + days(365), false).await.unwrap();
	assert_eq!(p.trash(owner, false).await.unwrap().gmails.len(), 1, "their own say over the default");

	p.set_trash_settings(owner, 1, Some(false)).await.unwrap().unwrap();
	p.purge(now() + days(365), false).await.unwrap();
	assert!(p.trash(owner, false).await.unwrap().gmails.is_empty());

	assert!(matches!(p.set_trash_settings(owner, 91, None).await.unwrap(), Err(Refused::Invalid(_))));
	assert!(matches!(p.set_trash_settings(owner, 0, None).await.unwrap(), Err(Refused::Invalid(_))));
}
