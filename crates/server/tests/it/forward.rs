//! The forward (docs/ARCHITECTURE.md, Forward): what reaches a service behind the conductor, and
//! what comes back from it.

use axum::{
	Router,
	http::{Method, StatusCode, header},
};
use sa_auth::{Keys, Principal, Service, Signer};
use sa_panel::testing::{TestDb, admin, panel};
use sa_server::testing::{Browser, Log, SEED, upstream};

async fn setup(db: &TestDb) -> (Router, Log) {
	let (base, seen) = upstream(Router::new()).await;
	(sa_server::testing::app_with(panel(db).await, admin(), "dev-admin@localhost", &base, Default::default()), seen)
}

#[tokio::test]
async fn a_signed_in_call_reaches_the_service_with_an_assertion_and_without_the_browsers_cookies() {
	let db = TestDb::create().await;
	let (app, seen) = setup(&db).await;
	let mut b = Browser::default();

	assert_eq!(b.send(&app, Method::GET, "/api/review_archive/me", &[], false).await.0, StatusCode::UNAUTHORIZED, "no session");
	assert!(seen.lock().unwrap().is_empty(), "nothing reached the service");

	b.sign_in(&app).await;
	let (status, headers) = b
		.send(
			&app,
			Method::GET,
			"/api/review_archive/me/overview?x=1",
			&[("x-sa-assertion", "forged"), ("x-member", "3")],
			false,
		)
		.await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(headers["x-upstream"], "yes");
	assert!(headers.get(header::SET_COOKIE).is_none(), "a service sets no cookie on the panel's origin");
	assert!(!b.jar.contains_key("upstream"));

	let got = seen.lock().unwrap().pop().unwrap();
	assert_eq!(got.path_and_query, "/me/overview?x=1", "the prefix is the panel's");
	assert!(got.headers.get(header::COOKIE).is_none(), "the session never leaves the panel");
	assert_eq!(got.headers["x-member"], "3", "the service's own headers pass");
	let keys: Keys = SEED.parse::<Signer>().unwrap().public().parse().unwrap();
	let token = got.headers["x-sa-assertion"].to_str().unwrap();
	let now = jiff::Timestamp::now().as_second();
	let a = sa_auth::verify(&keys, token, Service::ReviewArchive, "GET", "/me/overview", None, now).expect("the panel's assertion, not the forged one");
	let Principal::User(a) = a.principal else { panic!("a session's call is a user's") };
	assert_eq!(a.email, "dev-admin@localhost");
	assert!(a.permissions.iter().all(|p| p.starts_with("sa:review_archive:")), "{:?}", a.permissions);
	assert!(a.permissions.iter().count() == 2, "{:?}", a.permissions);
}

#[tokio::test]
async fn a_forwarded_write_needs_the_panels_csrf_header() {
	let db = TestDb::create().await;
	let (app, seen) = setup(&db).await;
	let mut b = Browser::default();
	b.sign_in(&app).await;

	assert_eq!(b.send(&app, Method::POST, "/api/review_archive/me/gmails", &[], false).await.0, StatusCode::FORBIDDEN);
	assert!(seen.lock().unwrap().is_empty());
	assert_eq!(b.send(&app, Method::POST, "/api/review_archive/me/gmails", &[], true).await.0, StatusCode::OK);
	assert_eq!(b.send(&app, Method::POST, "/api/playbook/search", &[], false).await.0, StatusCode::FORBIDDEN, "playbook's too");
}

#[tokio::test]
async fn an_open_path_passes_without_a_session_and_nobody_vouched_for() {
	let db = TestDb::create().await;
	let (app, seen) = setup(&db).await;
	let mut b = Browser::default();

	assert_eq!(b.send(&app, Method::GET, "/mfe/review_archive/x.js", &[], false).await.0, StatusCode::OK);
	let got = seen.lock().unwrap().pop().unwrap();
	assert!(!got.headers.contains_key("x-sa-assertion"));
	assert_eq!(
		b.send(&app, Method::GET, "/playbook_mcp/token", &[], false).await.0,
		StatusCode::NOT_FOUND,
		"playbook's own OAuth is not forwarded"
	);
}

#[tokio::test]
async fn a_forwarded_path_never_climbs_out_of_its_prefix() {
	let db = TestDb::create().await;
	let (app, seen) = setup(&db).await;
	let mut b = Browser::default();
	b.sign_in(&app).await;

	for path in [
		"/api/review_archive/me/../../x",
		"/api/review_archive/./../x",
		"/api/review_archive/me/%2e%2e/x",
		"/api/review_archive/%2E./x",
		"/api/review_archive/me/..%2f..%2fx",
		"/api/review_archive/..%5cx",
		"/api/review_archive/..",
		"/mfe/review_archive/%2e%2e/me",
	] {
		assert_eq!(b.send(&app, Method::GET, path, &[], false).await.0, StatusCode::BAD_REQUEST, "{path}");
	}
	assert!(seen.lock().unwrap().is_empty(), "nothing reached the service");
}

#[tokio::test]
async fn sites_is_told_the_caller_with_its_own_permissions_only() {
	let db = TestDb::create().await;
	let (app, seen) = setup(&db).await;
	let mut b = Browser::default();

	assert_eq!(b.send(&app, Method::GET, "/api/sites/leads", &[], false).await.0, StatusCode::UNAUTHORIZED, "no session");
	assert!(seen.lock().unwrap().is_empty());
	b.sign_in(&app).await;

	assert_eq!(b.send(&app, Method::GET, "/api/sites/leads?stage=created", &[], false).await.0, StatusCode::OK);
	let got = seen.lock().unwrap().pop().unwrap();
	assert_eq!(got.path_and_query, "/api/v1/leads?stage=created");
	let Principal::User(user) = got.assertion(Service::Sites, None).principal else {
		panic!("a session's call is a user's")
	};
	assert_eq!(user.email, "dev-admin@localhost");
	let held: Vec<&str> = user.permissions.iter().collect();
	assert!(!held.is_empty(), "{held:?}");
	assert!(held.iter().all(|p| p.starts_with("sa:sites:")), "{held:?}");
	assert_eq!(held.len(), sa_auth::SA_ADMIN.members.iter().filter(|p| p.starts_with("sa:sites:")).count());

	assert_eq!(
		b.send(&app, Method::POST, "/api/sites/leads/aquafix/L-1/stage", &[], false).await.0,
		StatusCode::FORBIDDEN,
		"a write needs CSRF"
	);
	assert!(seen.lock().unwrap().is_empty());
	assert_eq!(
		b.send(&app, Method::POST, "/api/sites/leads/aquafix/L-1/stage", &[("idempotency-key", "k1")], true).await.0,
		StatusCode::OK
	);
	let got = seen.lock().unwrap().pop().unwrap();
	assert_eq!((got.method.as_str(), got.path_and_query.as_str()), ("POST", "/api/v1/leads/aquafix/L-1/stage"));
	assert_eq!(got.headers["idempotency-key"], "k1");
	got.assertion(Service::Sites, None);
}

#[tokio::test]
async fn the_sites_bundle_and_the_booking_hooks_pass_without_a_session() {
	let db = TestDb::create().await;
	let (app, seen) = setup(&db).await;
	let mut b = Browser::default();
	for (ours, theirs) in [
		("/mfe/sites/mfe-sites.js", "/mfe/mfe-sites.js"),
		("/api/hooks/booking/cal_com/aquafix", "/api/hooks/booking/cal_com/aquafix"),
	] {
		let method = if ours.starts_with("/api") { Method::POST } else { Method::GET };
		assert_eq!(b.send(&app, method, ours, &[], false).await.0, StatusCode::OK, "{ours}");
		let got = seen.lock().unwrap().pop().unwrap();
		assert_eq!(got.path_and_query, theirs);
		assert!(!got.headers.contains_key(sa_auth::HEADER), "{ours}: nobody vouched for");
	}
	assert_eq!(b.send(&app, Method::GET, "/mfe/sites/%2e%2e/api/v1/leads", &[], false).await.0, StatusCode::BAD_REQUEST);
	assert!(seen.lock().unwrap().is_empty());
}
