//! A use a service's feed tells is charged, in the write that takes it, to whom it names at the
//! admin's price; past the balance it is let go, and a page that breaks the feed's rules is refused
//! whole (docs/ARCHITECTURE.md, Tokens and GBPs).

use std::collections::BTreeSet;

use jiff::Timestamp;
use sa_core::{
	prices::{BrowserStart, Item, PhoneTime, Scope, WalkStep},
	usage::{UsagePage, Used},
};
use sa_panel::{
	Panel,
	import::Export,
	testing::{TestDb, panel},
	tokens::User,
};
use uuid::Uuid;

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
	.unwrap()
	.unwrap();
	let prices = [
		(Item::from(PhoneTime::Running), Some(4)),
		(BrowserStart::Gologin.into(), Some(1)),
		(WalkStep::Open.into(), Some(1)),
	];
	p.set_prices(&prices, &[], "admin@localhost", Timestamp::now()).await.unwrap().unwrap();
	p
}

fn used(id: i64, item: impl Into<Item>, units: i64, user: Option<Uuid>) -> Used {
	Used {
		id,
		item: item.into(),
		units,
		at: Timestamp::now(),
		user,
	}
}

async fn take(p: &Panel, scope: Scope, used: Vec<Used>, after: i64) -> eyre::Result<usize> {
	let next = used.last().map_or(after, |u| u.id);
	let items: BTreeSet<_> = Item::all().filter(|i| i.scope() == scope).collect();
	p.take_usage(scope.into(), &items, UsagePage { used, next }, after).await
}

async fn person(p: &Panel) -> Uuid {
	let user_id = Uuid::now_v7();
	let user = User {
		user_id,
		email: format!("{user_id}@localhost"),
		name: "Someone".to_owned(),
	};
	p.user_seen(&user, Timestamp::now()).await.unwrap();
	user_id
}

async fn balance(p: &Panel, user: Uuid) -> i64 {
	p.tokens(user, Timestamp::now()).await.unwrap().unwrap().0
}

#[tokio::test]
async fn a_runners_use_is_charged_to_whom_it_names_and_past_the_balance_let_go() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let (a, b) = (person(&p).await, person(&p).await);
	let start = balance(&p, a).await;

	let page = vec![
		used(1, PhoneTime::Running, 3, Some(a)),
		used(2, BrowserStart::Gologin, 1, Some(b)),
		used(3, PhoneTime::Running, 1000, Some(b)),
	];
	assert_eq!(take(&p, Scope::Browsers, page, 0).await.unwrap(), 3);
	assert_eq!(balance(&p, a).await, start - 3 * 4);
	assert_eq!(balance(&p, b).await, 0, "a start, then minutes past what is left");
	let (_, entry) = &p.ledger(a, None, 1).await.unwrap().unwrap()[0];
	let charged = entry.charged.as_ref().unwrap();
	assert_eq!((entry.delta, charged.service.as_str(), charged.key.as_str()), (-12, "phone", "browsers:1"));
}

#[tokio::test]
async fn a_page_breaking_the_feeds_rules_is_refused_whole() {
	let db = TestDb::create().await;
	let p = opened(&db).await;
	let a = person(&p).await;
	let start = balance(&p, a).await;

	for (scope, page) in [
		(Scope::Browsers, vec![used(1, BrowserStart::Gologin, 1, Some(a)), used(2, PhoneTime::Running, 1, None)]),
		(Scope::ReviewArchive, vec![used(1, WalkStep::Open, 1, Some(a))]),
		(Scope::Browsers, vec![used(1, BrowserStart::Gologin, 1, Some(a)), used(2, BrowserStart::Mimic, 1, Some(a))]),
	] {
		assert!(take(&p, scope, page.clone(), 0).await.is_err(), "{page:?}");
	}
	assert_eq!(balance(&p, a).await, start, "nothing charged");
	assert_eq!(
		take(&p, Scope::ReviewArchive, vec![used(1, WalkStep::Open, 1, None)], 0).await.unwrap(),
		1,
		"a walk's, charged through its charge feed"
	);
}
