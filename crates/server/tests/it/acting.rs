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

/// A note on a GBP is read by whoever wrote it and no one else: an actor reads and writes their own, never the subject's.
#[tokio::test]
async fn a_note_on_a_gbp_is_its_writers_alone() {
	let db = TestDb::create().await;
	let (app, ..) = setup(&db, admin()).await;
	let mut derek = Browser::default();
	derek.sign_in_as(&app, "derek@dzcadvertising.com").await;
	let new = json!({
		"name": "Lyon 7", "trade": "gcid:plumber", "state": "draft", "spots": [{"lat": 45.76, "lng": 4.84}], "note": "  by the depot\n"
	});
	let (status, draft) = derek.call(&app, Method::POST, "/api/v1/gmbs", Some(new)).await;
	assert_eq!(status, StatusCode::CREATED, "{draft}");
	let gmb = format!("/api/v1/gmbs/{}", draft["id"]);
	let uri = format!("{gmb}/note");
	assert_eq!(derek.call(&app, Method::GET, &uri, None).await, (StatusCode::OK, json!({"note": "by the depot"})));
	for other in ["/api/v1/gmails".to_owned(), format!("{gmb}/log")] {
		let (_, body) = derek.call(&app, Method::GET, &other, None).await;
		assert!(!body.to_string().contains("by the depot"), "{other} carries no note: {body}");
	}

	let mut actor = Browser::default();
	actor.sign_in(&app).await;
	for (method, body) in [(Method::GET, None), (Method::PUT, Some(json!({"note": "mine"})))] {
		assert_eq!(actor.call(&app, method, &uri, body).await.0, StatusCode::NOT_FOUND, "another's GBP");
	}
	actor.call(&app, Method::PUT, "/api/v1/me/acting", Some(json!({"user_id": DEREK}))).await;
	assert_eq!(actor.call(&app, Method::GET, &uri, None).await, (StatusCode::OK, json!({"note": null})), "never the subject's");
	assert_eq!(actor.call(&app, Method::PUT, &uri, Some(json!({"note": "watch this one"}))).await.0, StatusCode::NO_CONTENT);
	assert_eq!(actor.call(&app, Method::GET, &uri, None).await.1, json!({"note": "watch this one"}));
	assert_eq!(derek.call(&app, Method::GET, &uri, None).await.1, json!({"note": "by the depot"}), "one each");

	for bad in [" ".to_owned(), "é".repeat(4001)] {
		assert_eq!(derek.call(&app, Method::PUT, &uri, Some(json!({"note": bad}))).await.0, StatusCode::BAD_REQUEST);
	}
	assert_eq!(
		derek.call(&app, Method::PUT, &uri, Some(json!({"note": "é".repeat(4000)}))).await.0,
		StatusCode::NO_CONTENT,
		"characters, not bytes"
	);
	assert_eq!(derek.call(&app, Method::PUT, &uri, Some(json!({"note": null}))).await.0, StatusCode::NO_CONTENT);
	assert_eq!(derek.call(&app, Method::GET, &uri, None).await.1, json!({"note": null}));
	assert_eq!(actor.call(&app, Method::GET, &uri, None).await.1, json!({"note": "watch this one"}), "the actor's stays");
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
