//! A GBP's life: a draft named once among the live, placed on Google once, its state moved only
//! along `State::next`; a GBP with a place has its gmail and its country.

use jiff::Timestamp;
use sa_gbp::{Country, OnGoogle, State};
use sa_panel::{
	Panel, Refused,
	gmbs::{Edit, Existing},
	import::Export,
	testing::{TestDb, panel},
	tokens::User,
};
use uuid::Uuid;

const PROCOPE: &str = "ChIJLU7jZClu5kcR4PcOOO6p3I0";
const FLORE: &str = "ChIJD7fiBh9u5kcRYJSMaMOCCwQ";

fn now() -> Timestamp {
	"2026-11-08T12:00:00Z".parse().unwrap()
}

async fn opened(db: &TestDb) -> Panel {
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
	p
}

async fn person(p: &Panel, email: &str) -> Uuid {
	let user_id = Uuid::now_v7();
	let user = User {
		user_id,
		email: email.to_owned(),
		name: email.to_owned(),
	};
	p.user_seen(&user, now()).await.unwrap();
	user_id
}

fn existing(gmail_id: i64, place: &str, name: &str, country: Option<Country>) -> Existing {
	Existing {
		gmail_id,
		place: place.to_owned(),
		state: OnGoogle::Verified,
		name: Some(name.to_owned()),
		country,
	}
}

#[tokio::test]
async fn a_gbp_on_google_has_a_country() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();

	let refused = p.add_gmb(Some(owner), existing(gmail.id, PROCOPE, "Le Procope", None), now()).await.unwrap();
	assert!(matches!(refused, Err(Refused::Invalid(_))), "Places gives it none here, nor the caller: {refused:?}");
	let added = p
		.add_gmb(Some(owner), existing(gmail.id, PROCOPE, "Le Procope", Some(Country::FR)), now())
		.await
		.unwrap()
		.unwrap();
	assert_eq!((added.state, added.country.map(|c| c.code)), (State::Verified, Some(Country::FR)));

	let draft = p.create_draft(owner, "Café de Flore", now()).await.unwrap().unwrap();
	let no_gmail = p.advance(Some(owner), draft.id, State::Verifying, Some(FLORE)).await.unwrap();
	assert!(matches!(no_gmail, Err(Refused::Invalid(_))), "{no_gmail:?}");
	let edit = Edit {
		gmail_id: Some(gmail.id),
		..Edit::default()
	};
	p.edit_gmb(owner, draft.id, edit).await.unwrap().unwrap();
	let no_country = p.advance(Some(owner), draft.id, State::Verifying, Some(FLORE)).await.unwrap();
	assert!(matches!(no_country, Err(Refused::Invalid(_))), "{no_country:?}");
	p.set_country(Some(owner), draft.id, Country::FR).await.unwrap().unwrap();
	let verifying = p.advance(Some(owner), draft.id, State::Verifying, Some(FLORE)).await.unwrap().unwrap();
	assert_eq!((verifying.state, verifying.place_id.as_deref()), (State::Verifying, Some(FLORE)));
}

#[tokio::test]
async fn a_name_and_a_place_are_one_live_gbps() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let other = person(&p, "other@localhost").await;
	let mine = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let theirs = p.add_gmail(other, "b@gmail.com", now()).await.unwrap().unwrap();

	let first = p.create_draft(owner, "Le Procope", now()).await.unwrap().unwrap();
	let named = p.create_draft(other, "Le Procope", now()).await.unwrap();
	assert!(matches!(named, Err(Refused::Conflict(_))), "anyone's: {named:?}");
	let renamed = p
		.edit_gmb(
			owner,
			first.id,
			Edit {
				name: Some("Le Procope (Paris)".to_owned()),
				..Edit::default()
			},
		)
		.await
		.unwrap()
		.unwrap();
	assert_eq!(renamed.name, "Le Procope (Paris)");
	p.create_draft(other, "Le Procope", now()).await.unwrap().unwrap();

	p.add_gmb(Some(owner), existing(mine.id, PROCOPE, "Procope", Some(Country::FR)), now()).await.unwrap().unwrap();
	let placed = p.add_gmb(Some(other), existing(theirs.id, PROCOPE, "Procope 2", Some(Country::FR)), now()).await.unwrap();
	assert!(matches!(placed, Err(Refused::Conflict(_))), "under another gmail too: {placed:?}");

	let removed = p.add_gmb(Some(owner), existing(mine.id, FLORE, "Flore", Some(Country::FR)), now()).await.unwrap().unwrap();
	p.remove_gmb(owner, removed.id, now()).await.unwrap().unwrap();
	p.add_gmb(Some(other), existing(theirs.id, FLORE, "Flore", Some(Country::FR)), now()).await.unwrap().unwrap();
}

#[tokio::test]
async fn a_state_moves_only_along_its_transitions_and_a_place_is_set_once() {
	use State::*;
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let draft = p.create_draft(owner, "Draft", now()).await.unwrap().unwrap();
	for to in [Draft, Verified, Suspended] {
		let got = p.advance(Some(owner), draft.id, to, None).await.unwrap();
		assert!(matches!(got, Err(Refused::Invalid(_))), "draft → {to}: {got:?}");
	}
	let unplaced = p.advance(Some(owner), draft.id, Verifying, None).await.unwrap();
	assert!(matches!(unplaced, Err(Refused::Invalid(_))), "a draft goes to Google with its place: {unplaced:?}");

	let gbp = p.add_gmb(Some(owner), existing(gmail.id, PROCOPE, "Procope", Some(Country::FR)), now()).await.unwrap().unwrap();
	let walk = [
		(Verified, false),
		(Draft, false),
		(Verifying, true),
		(Suspended, false),
		(Draft, false),
		(Verified, true),
		(Suspended, true),
		(Draft, false),
		(Verifying, true),
		(Verified, true),
		(Suspended, true),
		(Verified, true),
	];
	for (to, allowed) in walk {
		let got = p.advance(Some(owner), gbp.id, to, None).await.unwrap();
		assert_eq!(got.is_ok(), allowed, "→ {to}: {got:?}");
	}
	let replaced = p.advance(Some(owner), gbp.id, Suspended, Some(FLORE)).await.unwrap();
	assert!(matches!(replaced, Err(Refused::Invalid(_))), "only a draft takes a place: {replaced:?}");
	let renamed = p
		.edit_gmb(
			owner,
			gbp.id,
			Edit {
				name: Some("Another".to_owned()),
				..Edit::default()
			},
		)
		.await
		.unwrap();
	assert!(matches!(renamed, Err(Refused::Conflict(_))), "renamed only as a draft: {renamed:?}");

	let pool = db.pool().await;
	for sql in [
		"UPDATE gmbs SET place_id = 'ChIJelsewhereelsewhere' WHERE place_id IS NOT NULL",
		"UPDATE gmbs SET state = 'draft' WHERE place_id IS NOT NULL",
		"DELETE FROM gmbs",
	] {
		let refused = sqlx::query(sqlx::AssertSqlSafe(sql)).execute(&pool).await;
		assert!(refused.is_err(), "the schema refuses it too: {sql}");
	}
}

#[tokio::test]
async fn the_snapshot_carries_placed_gbps_with_their_state_and_country() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let other = p.add_gmail(owner, "b@gmail.com", now()).await.unwrap().unwrap();
	p.create_draft(owner, "Draft", now()).await.unwrap().unwrap();
	let gbp = p.add_gmb(Some(owner), existing(gmail.id, PROCOPE, "Procope", Some(Country::US)), now()).await.unwrap().unwrap();
	p.advance(Some(owner), gbp.id, State::Suspended, None).await.unwrap().unwrap();
	p.edit_gmb(
		owner,
		gbp.id,
		Edit {
			gmail_id: Some(other.id),
			..Edit::default()
		},
	)
	.await
	.unwrap()
	.unwrap();

	let snap = p.snapshot(now()).await.unwrap().unwrap();
	let [m] = snap.gmbs.as_slice() else { panic!("the draft has no place: {:?}", snap.gmbs) };
	assert_eq!(
		(m.id, m.gmail_id, m.state, m.country, m.name.as_str()),
		(gbp.id, other.id, OnGoogle::Suspended, Some(Country::US), "Procope"),
		"moved to its owner's other gmail"
	);
}
