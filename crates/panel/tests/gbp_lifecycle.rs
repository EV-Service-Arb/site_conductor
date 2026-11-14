//! A GBP's life: a draft named once among the live, placed on Google once, its state moved only
//! along `State::next`; past a draft it has its gmail and its country, and once verified its
//! place.

use jiff::Timestamp;
use sa_core::gbp::{Country, OnGoogle, State};
use sa_panel::{
	Panel, Refused,
	gmbs::{Edit, Existing, Gmb, Unplaced},
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

/// `owner`'s trade, added the first time.
async fn trade(p: &Panel, owner: Uuid) -> i64 {
	match p.trades(owner).await.unwrap().first() {
		Some(t) => t.id,
		None => p.add_trade(owner, "Plumbing", now()).await.unwrap().unwrap().id,
	}
}

async fn add(p: &Panel, owner: Uuid, gmail_id: i64, place: &str, name: &str, country: Option<Country>) -> Result<Gmb, Refused> {
	let existing = Existing {
		gmail_id,
		place: place.to_owned(),
		state: OnGoogle::Verified,
		name: Some(name.to_owned()),
		country,
		trade: trade(p, owner).await,
	};
	p.add_gmb(Some(owner), existing, now()).await.unwrap()
}

async fn create(p: &Panel, owner: Uuid, name: &str, state: State, gmail_id: Option<i64>, country: Option<Country>) -> Result<Gmb, Refused> {
	let unplaced = Unplaced {
		name: name.to_owned(),
		trade: trade(p, owner).await,
		state,
		gmail_id,
		country,
		at: None,
		spots: if state == State::Draft { vec![sa_panel::gmbs::Spot { at: sa_places::LatLng { lat: 45.76, lng: 4.84 }, label: None, note: None }] } else { Vec::new() },
	};
	p.create_gmb(owner, unplaced, now()).await.unwrap()
}

async fn draft(p: &Panel, owner: Uuid, name: &str) -> Result<Gmb, Refused> {
	create(p, owner, name, State::Draft, None, None).await
}

#[tokio::test]
async fn past_a_draft_a_gbp_has_a_gmail_and_a_country() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();

	let refused = add(&p, owner, gmail.id, PROCOPE, "Le Procope", None).await;
	assert!(matches!(refused, Err(Refused::Invalid(_))), "Places gives it none here, nor the caller: {refused:?}");
	let added = add(&p, owner, gmail.id, PROCOPE, "Le Procope", Some(Country::FR)).await.unwrap();
	assert_eq!((added.state, added.country.map(|c| c.code)), (State::Verified, Some(Country::FR)));

	let draft = draft(&p, owner, "Café de Flore").await.unwrap();
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

	let first = draft(&p, owner, "Le Procope").await.unwrap();
	let named = draft(&p, other, "Le Procope").await;
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
	draft(&p, other, "Le Procope").await.unwrap();

	add(&p, owner, mine.id, PROCOPE, "Procope", Some(Country::FR)).await.unwrap();
	let placed = add(&p, other, theirs.id, PROCOPE, "Procope 2", Some(Country::FR)).await;
	assert!(matches!(placed, Err(Refused::Conflict(_))), "under another gmail too: {placed:?}");

	let removed = add(&p, owner, mine.id, FLORE, "Flore", Some(Country::FR)).await.unwrap();
	p.remove_gmb(owner, removed.id, now()).await.unwrap().unwrap();
	add(&p, other, theirs.id, FLORE, "Flore", Some(Country::FR)).await.unwrap();
}

#[tokio::test]
async fn a_state_moves_only_along_its_transitions_and_a_place_is_set_once() {
	use State::*;
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let draft = draft(&p, owner, "Draft").await.unwrap();
	for to in [Draft, Verified, Suspended] {
		let got = p.advance(Some(owner), draft.id, to, None).await.unwrap();
		assert!(matches!(got, Err(Refused::Invalid(_))), "draft → {to}: {got:?}");
	}
	let no_gmail = p.advance(Some(owner), draft.id, Verifying, None).await.unwrap();
	assert!(matches!(no_gmail, Err(Refused::Invalid(_))), "a draft goes to Google under a gmail: {no_gmail:?}");

	let gbp = add(&p, owner, gmail.id, PROCOPE, "Procope", Some(Country::FR)).await.unwrap();
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
	assert!(matches!(replaced, Err(Refused::Invalid(_))), "a place is set once: {replaced:?}");
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
async fn a_place_is_certain_only_once_verified() {
	use State::*;
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();

	let verified = create(&p, owner, "Verified", Verified, Some(gmail.id), Some(Country::FR)).await;
	assert!(matches!(verified, Err(Refused::Invalid(_))), "on Maps, it is added by its place: {verified:?}");
	for (gmail_id, country) in [(None, Some(Country::FR)), (Some(gmail.id), None)] {
		let got = create(&p, owner, "Verifying", Verifying, gmail_id, country).await;
		assert!(matches!(got, Err(Refused::Invalid(_))), "past a draft, a gmail and a country: {got:?}");
	}
	let verifying = create(&p, owner, "Verifying", Verifying, Some(gmail.id), Some(Country::FR)).await.unwrap();
	assert_eq!((verifying.place_id.as_deref(), verifying.country.map(|c| (c.code, c.by_hand))), (None, Some((Country::FR, true))));
	let suspended = create(&p, owner, "Suspended", Suspended, Some(gmail.id), Some(Country::FR)).await.unwrap();
	p.advance(Some(owner), suspended.id, Verifying, None).await.unwrap().unwrap();

	let draft = draft(&p, owner, "Draft").await.unwrap();
	let edit = Edit {
		gmail_id: Some(gmail.id),
		..Edit::default()
	};
	p.edit_gmb(owner, draft.id, edit).await.unwrap().unwrap();
	p.set_country(Some(owner), draft.id, Country::FR).await.unwrap().unwrap();
	let onto = p.advance(Some(owner), draft.id, Verifying, None).await.unwrap().unwrap();
	assert_eq!((onto.state, onto.place_id), (Verifying, None), "a draft goes onto Google without its place");

	let unplaced = p.advance(Some(owner), verifying.id, Verified, None).await.unwrap();
	assert!(matches!(unplaced, Err(Refused::Invalid(_))), "verified with its place: {unplaced:?}");
	assert!(p.snapshot(now()).await.unwrap().unwrap().gmbs.is_empty(), "the archive tracks no GBP without a place");
	let pool = db.pool().await;
	let refused = sqlx::query("UPDATE gmbs SET state = 'verified' WHERE place_id IS NULL AND state = 'verifying'")
		.execute(&pool)
		.await;
	assert!(refused.is_err(), "the schema refuses it too");

	let placed = p.advance(Some(owner), verifying.id, Verified, Some(PROCOPE)).await.unwrap().unwrap();
	assert_eq!((placed.state, placed.place_id.as_deref()), (Verified, Some(PROCOPE)));
	let [tracked] = p.snapshot(now()).await.unwrap().unwrap().gmbs.try_into().unwrap();
	assert_eq!((tracked.id, tracked.state), (verifying.id, OnGoogle::Verified));
}

#[tokio::test]
async fn the_snapshot_carries_placed_gbps_with_their_state_and_country() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let owner = person(&p, "owner@localhost").await;
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap();
	let other = p.add_gmail(owner, "b@gmail.com", now()).await.unwrap().unwrap();
	draft(&p, owner, "Draft").await.unwrap();
	let gbp = add(&p, owner, gmail.id, PROCOPE, "Procope", Some(Country::US)).await.unwrap();
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
