//! A draft's must-haves: stored as given, cleared by `null`, refused rather than fixed; a phone is
//! one live GBP's whatever its spelling.

use jiff::Timestamp;
use sa_panel::{
	Panel, Refused,
	gmbs::{Edit, Gmb},
	import::Export,
	testing::{TestDb, panel},
	tokens::User,
};
use uuid::Uuid;

/// The migration that brought the must-haves.
const MUST_HAVES: i64 = 20261109090000;

fn now() -> Timestamp {
	"2026-11-09T12:00:00Z".parse().unwrap()
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
	let user = User {
		user_id: owner,
		email: "owner@localhost".to_owned(),
		name: "owner".to_owned(),
	};
	p.user_seen(&user, now()).await.unwrap();
	(p, owner)
}

fn areas(n: usize) -> Edit {
	Edit {
		areas: Some(Some((1..=n).map(|i| format!("Lyon {i}")).collect())),
		..Edit::default()
	}
}

#[tokio::test]
async fn must_haves_are_stored_and_cleared() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let draft = p.create_draft(owner, "Plombier Lyon 7", None, now()).await.unwrap().unwrap();
	let trade = p.add_trade(owner, "Plumbing", now()).await.unwrap().unwrap();

	let set = Edit {
		phone: Some(Some(" +33 (4) 78-00.00.01 ".to_owned())),
		trade: Some(Some(trade.id)),
		areas: Some(Some(vec![" Lyon 7e ".to_owned(), "Villeurbanne".to_owned()])),
		hidden_address: Some(Some("12 rue de Marseille, Lyon".to_owned())),
		customers_visit: Some(Some(false)),
		film: Some(Some("verif_vid:42".to_owned())),
		profile: Some(Some("mlx:lyon-7".to_owned())),
		..Edit::default()
	};
	let edited = p.edit_gmb(owner, draft.id, set).await.unwrap().unwrap();
	let fields = |g: &Gmb| {
		(
			g.phone.clone(),
			g.trade_id,
			g.areas.clone(),
			g.hidden_address.clone(),
			g.customers_visit,
			g.film.clone(),
			g.profile.clone(),
		)
	};
	let want = (
		Some("+33478000001".to_owned()),
		Some(trade.id),
		vec!["Lyon 7e".to_owned(), "Villeurbanne".to_owned()],
		Some("12 rue de Marseille, Lyon".to_owned()),
		Some(false),
		Some("verif_vid:42".to_owned()),
		Some("mlx:lyon-7".to_owned()),
	);
	assert_eq!(fields(&edited), want);
	let listed = p.gmails(owner).await.unwrap().1;
	assert_eq!(fields(&listed[0]), want, "read back as written");

	let renamed = Edit {
		name: Some("Plombier Lyon 7e".to_owned()),
		..Edit::default()
	};
	assert_eq!(fields(&p.edit_gmb(owner, draft.id, renamed).await.unwrap().unwrap()), want, "an absent field is left");

	let clear = Edit {
		phone: Some(None),
		trade: Some(None),
		areas: Some(None),
		hidden_address: Some(None),
		customers_visit: Some(None),
		film: Some(None),
		profile: Some(None),
		..Edit::default()
	};
	assert_eq!(fields(&p.edit_gmb(owner, draft.id, clear).await.unwrap().unwrap()), (None, None, vec![], None, None, None, None));
}

#[tokio::test]
async fn what_is_wrong_is_refused_not_fixed() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let draft = p.create_draft(owner, "Draft", None, now()).await.unwrap().unwrap();
	let text = |raw: &str| Some(Some(raw.to_owned()));
	let list = |raw: &[&str]| Some(Some(raw.iter().map(|a| a.to_string()).collect()));
	for (why, edit) in [
		("20 at most", areas(21)),
		(
			"an empty list is a null",
			Edit {
				areas: Some(Some(vec![])),
				..Edit::default()
			},
		),
		(
			"no area empty",
			Edit {
				areas: list(&["Lyon", " "]),
				..Edit::default()
			},
		),
		(
			"none twice",
			Edit {
				areas: list(&["Lyon", " Lyon "]),
				..Edit::default()
			},
		),
		(
			"letters are no phone",
			Edit {
				phone: text("+33 4 PLOMBIER"),
				..Edit::default()
			},
		),
		(
			"a + only leads",
			Edit {
				phone: text("04+78"),
				..Edit::default()
			},
		),
		(
			"an empty text is a null",
			Edit {
				hidden_address: text("  "),
				..Edit::default()
			},
		),
	] {
		let got = p.edit_gmb(owner, draft.id, edit).await.unwrap();
		assert!(matches!(got, Err(Refused::Invalid(_))), "{why}: {got:?}");
	}
	p.edit_gmb(owner, draft.id, areas(20)).await.unwrap().unwrap();

	let pool = db.pool().await;
	let twenty_one = serde_json::to_string(&(1..=21).collect::<Vec<_>>()).unwrap();
	for areas in [twenty_one.as_str(), "[]", "{\"a\": 1}", "Lyon"] {
		let refused = sqlx::query("UPDATE gmbs SET areas = $1").bind(areas).execute(&pool).await;
		assert!(refused.is_err(), "the schema refuses {areas} too");
	}
}

#[tokio::test]
async fn a_phone_is_one_live_gbps_however_spelled() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let first = p.create_draft(owner, "First", None, now()).await.unwrap().unwrap();
	let second = p.create_draft(owner, "Second", None, now()).await.unwrap().unwrap();
	let phone = |raw: &str| Edit {
		phone: Some(Some(raw.to_owned())),
		..Edit::default()
	};
	p.edit_gmb(owner, first.id, phone("+33 4 78 00 00 01")).await.unwrap().unwrap();
	p.edit_gmb(owner, first.id, phone("+33478000001")).await.unwrap().unwrap();

	let taken = p.edit_gmb(owner, second.id, phone("+33 (4) 78-00-00-01")).await.unwrap();
	assert!(matches!(taken, Err(Refused::Conflict(_))), "{taken:?}");

	p.remove_gmb(owner, first.id, now()).await.unwrap().unwrap();
	p.edit_gmb(owner, second.id, phone("+33.4.78.00.00.01")).await.unwrap().unwrap();
}

#[tokio::test]
async fn the_migration_rewrites_phones_to_one_spelling() {
	let db = TestDb::create().await;
	let (p, owner) = opened(&db).await;
	let spelled = ["+33 4 78 00 00 01", "(04) 78-00-00-02", "04.78.00.00.03", "0478000004"];
	let mut drafts = Vec::new();
	for (i, _) in spelled.iter().enumerate() {
		drafts.push(p.create_draft(owner, &format!("Draft {i}"), None, now()).await.unwrap().unwrap().id);
	}
	let pool = db.pool().await;
	let migrator = sqlx::migrate!("./migrations");
	migrator.undo(&pool, MUST_HAVES - 1).await.unwrap();
	for (id, phone) in drafts.iter().zip(spelled) {
		sqlx::query("UPDATE gmbs SET phone = $2 WHERE id = $1").bind(id).bind(phone).execute(&pool).await.unwrap();
	}

	migrator.run(&pool).await.unwrap();
	let listed: Vec<Option<String>> = p.gmails(owner).await.unwrap().1.into_iter().map(|g| g.phone).collect();
	let canonical: Vec<Option<String>> = spelled.iter().map(|raw| Some(sa_core::gbp::phone(raw).unwrap())).collect();
	assert_eq!(listed, canonical, "as sa_core spells them");
}
