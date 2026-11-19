//! Acting as another (docs/ARCHITECTURE.md, Signing in): their account, the actor's permissions
//! lent, never a bound one.

use axum::{
	Router,
	http::{Method, StatusCode},
};
use jiff::Timestamp;
use sa_auth::{Own, Principal, Service};
use sa_panel::{
	Panel,
	import::Export,
	testing::{TestDb, admin, operator, panel},
	tokens::User,
};
use sa_server::testing::{self, Browser, Log};
use serde_json::json;
use uuid::Uuid;

const DEREK: &str = "0190a7c4-0000-7000-8000-0000000000d7";

async fn setup(db: &TestDb, permissions: sa_auth::PermissionSet) -> (Router, Panel, Log) {
	let panel = panel(db).await;
	let (base, seen) = testing::upstream(Router::new()).await;
	let app = testing::app_with(panel.clone(), permissions, "dev-admin@localhost", &base.parse().unwrap(), Default::default());
	panel
		.import_review_archive(Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		})
		.await
		.unwrap()
		.unwrap();
	let derek = User {
		user_id: DEREK.parse().unwrap(),
		email: "derek@dzcadvertising.com".into(),
		name: "Derek".into(),
	};
	panel.user_seen(&derek, Timestamp::now()).await.unwrap();
	(app, panel, seen)
}

fn lent(set: sa_auth::PermissionSet) -> Vec<String> {
	set.iter()
		.filter(|p| ![Own::Payments.as_str(), Own::Credentials.as_str()].contains(p))
		.map(str::to_owned)
		.collect()
}

#[tokio::test]
async fn acting_as_another_is_their_account_with_the_actors_permissions_but_the_bound_ones() {
	let db = TestDb::create().await;
	let (app, _, seen) = setup(&db, admin()).await;
	let mut b = Browser::default();
	b.sign_in(&app).await;
	let (_, me) = b.call(&app, Method::GET, "/api/v1/me", None).await;
	let actor = me["user_id"].as_str().unwrap().to_owned();
	assert_eq!(me["acting"], json!(null));

	let (status, members) = b.call(&app, Method::GET, "/api/v1/members", None).await;
	assert_eq!(status, StatusCode::OK, "{members}");
	assert!(members["users"].as_array().unwrap().iter().any(|u| u["user_id"] == DEREK), "{members}");

	assert_eq!(
		b.call(&app, Method::PUT, "/api/v1/me/acting", Some(json!({"user_id": actor}))).await.0,
		StatusCode::BAD_REQUEST,
		"not oneself"
	);
	assert_eq!(
		b.call(&app, Method::PUT, "/api/v1/me/acting", Some(json!({"user_id": Uuid::now_v7()}))).await.0,
		StatusCode::NOT_FOUND,
		"nobody the conductor knows"
	);
	let (status, body) = b.call(&app, Method::PUT, "/api/v1/me/acting", Some(json!({"user_id": DEREK}))).await;
	assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
	assert_eq!(
		b.call(&app, Method::PUT, "/api/v1/me/acting", Some(json!({"user_id": DEREK}))).await.0,
		StatusCode::CONFLICT,
		"acting does not nest"
	);

	let (_, me) = b.call(&app, Method::GET, "/api/v1/me", None).await;
	assert_eq!(me["user_id"], DEREK);
	assert_eq!(me["email"], "derek@dzcadvertising.com");
	assert_eq!(me["preferred_name"], "Derek");
	assert_eq!(me["permissions"], json!(lent(admin())));
	assert_eq!(me["acting"], json!({"sub": actor, "email": "dev-admin@localhost"}));

	b.send(&app, Method::GET, "/api/review_archive/me", &[], false).await;
	let Principal::User(told) = seen.lock().unwrap().pop().unwrap().assertion(Service::ReviewArchive, None).principal else {
		panic!("a session's call is a user's")
	};
	assert_eq!(told.sub, DEREK, "the service serves Derek's data");
	assert_eq!(told.actor.map(|a| a.sub), Some(actor.clone()), "and knows who acts");

	for (method, path) in [
		(Method::GET, "/api/v1/me/claude"),
		(Method::DELETE, "/api/v1/me/claude"),
		(Method::POST, "/api/v1/tokens/claim"),
		(Method::POST, "/api/v1/alerts/telegram/link"),
	] {
		assert_eq!(
			b.call(&app, method.clone(), path, Some(json!({}))).await.0,
			StatusCode::FORBIDDEN,
			"{method} {path} is the subject's own"
		);
	}

	assert_eq!(b.call(&app, Method::DELETE, "/api/v1/me/acting", None).await.0, StatusCode::NO_CONTENT);
	let (_, me) = b.call(&app, Method::GET, "/api/v1/me", None).await;
	assert_eq!((me["user_id"].as_str(), &me["acting"]), (Some(actor.as_str()), &json!(null)));
	assert_eq!(me["permissions"], json!(admin()), "one's own session holds the bound ones");
}

#[tokio::test]
async fn acting_needs_act_as_and_a_browsers_session() {
	let db = TestDb::create().await;
	let (app, _, _) = setup(&db, operator()).await;
	let mut b = Browser::default();
	b.sign_in(&app).await;
	assert_eq!(b.call(&app, Method::GET, "/api/v1/members", None).await.0, StatusCode::FORBIDDEN);
	assert_eq!(b.call(&app, Method::PUT, "/api/v1/me/acting", Some(json!({"user_id": DEREK}))).await.0, StatusCode::FORBIDDEN);
	assert_eq!(b.call(&app, Method::DELETE, "/api/v1/me/acting", None).await.0, StatusCode::NO_CONTENT, "anyone may stop");

	let db = TestDb::create().await;
	let (app, _, _) = setup(&db, admin()).await;
	let (bearer, ..) = sa_server::testing::mcp_grant(&app, "ask").await;
	let auth = format!("Bearer {bearer}");
	let (status, ..) = Browser::default()
		.send_body(
			&app,
			Method::PUT,
			"/api/v1/me/acting",
			&[("authorization", &auth), ("content-type", "application/json")],
			false,
			json!({"user_id": DEREK}).to_string().into(),
		)
		.await;
	assert_eq!(status, StatusCode::BAD_REQUEST, "an MCP client's session never acts");
}
