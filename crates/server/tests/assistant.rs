//! The assistant's chats: who sees and changes them, a stale write refused, and a deletion that
//! takes everything of the chat with it.

use axum::{
	Router,
	body::Body,
	http::{Method, StatusCode},
};
use sa_auth::PermissionSet;
use sa_panel::{
	Panel,
	live::{Audience, Signal},
	testing::{TestDb, operator, panel},
};
use sa_server::{
	concierge::{Concierge, DevIdentity},
	http,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};
use uuid::Uuid;

/// Someone signed in to the conductor on `panel`.
struct Person {
	app: Router,
	browser: Browser,
	id: Uuid,
}

impl Person {
	async fn new(panel: &Panel, email: &str, permissions: PermissionSet) -> Self {
		let who = DevIdentity {
			user_id: DevIdentity::named(email),
			permissions,
			email: email.to_owned(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let app = http::app(SignIn::new(panel.clone(), Concierge::dev(who), config), None, None, Default::default(), None, None, None);
		let mut browser = Browser::default();
		browser.sign_in(&app).await;
		let mut me = Self {
			app,
			browser,
			id: DevIdentity::named(email),
		};
		assert_eq!(me.call(Method::GET, "/api/v1/me", None).await.0, StatusCode::OK, "seen by the gate, so a user");
		me
	}

	async fn call(&mut self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
		let json = [("content-type", "application/json")];
		let (status, _, bytes) = self
			.browser
			.send_body(
				&self.app,
				method,
				uri,
				if body.is_some() { &json } else { &[] },
				true,
				body.map_or(Body::empty(), |b| Body::from(b.to_string())),
			)
			.await;
		(status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
	}
}

fn question(id: &str, text: &str) -> Value {
	json!({ "role": "user", "id": id, "at": 1, "text": text, "mentions": [], "files": [] })
}

#[tokio::test]
async fn who_sees_a_chat_and_who_changes_it() {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let mut owner = Person::new(&p, "owner@x.com", operator()).await;
	let mut member = Person::new(&p, "member@x.com", operator()).await;
	let mut viewer = Person::new(&p, "viewer@x.com", operator()).await;
	let mut outsider = Person::new(&p, "outsider@x.com", PermissionSet::from_iter(Vec::<String>::new())).await;

	let chat = Uuid::now_v7();
	let url = format!("/api/v1/assistant/chats/{chat}");
	let (status, created) = owner
		.call(
			Method::POST,
			"/api/v1/assistant/chats",
			Some(json!({ "id": chat, "title": "  ", "turns": [question("q1", "hi")] })),
		)
		.await;
	assert_eq!(status, StatusCode::CREATED, "{created}");
	assert_eq!((created["version"].as_i64(), &created["title"], &created["role"]), (Some(1), &Value::Null, &json!("owner")));
	let again = owner.call(Method::POST, "/api/v1/assistant/chats", Some(json!({ "id": chat, "title": null, "turns": [] }))).await;
	assert_eq!(again.0, StatusCode::CONFLICT);

	// Not shared yet: nobody else can tell it exists.
	for who in [&mut member, &mut viewer, &mut outsider] {
		assert_eq!(who.call(Method::GET, &url, None).await.0, StatusCode::NOT_FOUND);
	}
	let (status, found) = owner.call(Method::GET, "/api/v1/assistant/people?q=Member@x.com", None).await;
	assert_eq!(status, StatusCode::OK);
	assert_eq!(found["people"][0]["id"], json!(member.id), "a whole email finds its person: {found}");
	assert_eq!(owner.call(Method::GET, "/api/v1/assistant/people?q=x.com", None).await.1["people"], json!([]), "no directory");

	let (status, access) = owner.call(Method::POST, &format!("{url}/members"), Some(json!({ "users": [member.id, owner.id] }))).await;
	assert_eq!(status, StatusCode::OK, "{access}");
	assert_eq!(access["members"].as_array().unwrap().len(), 1, "the owner is no member");
	assert_eq!(
		owner.call(Method::POST, &format!("{url}/members"), Some(json!({ "users": [Uuid::nil()] }))).await.0,
		StatusCode::BAD_REQUEST
	);
	assert_eq!(owner.call(Method::PUT, &format!("{url}/general"), Some(json!({ "general": "view" }))).await.0, StatusCode::OK);

	let (_, listed) = member.call(Method::GET, "/api/v1/assistant/chats", None).await;
	assert_eq!((&listed["chats"][0]["role"], &listed["chats"][0]["shared"]), (&json!("member"), &json!(true)));
	assert_eq!(viewer.call(Method::GET, "/api/v1/assistant/chats", None).await.1["chats"], json!([]), "a viewer is not listed it");
	let (status, seen) = viewer.call(Method::GET, &url, None).await;
	assert_eq!((status, &seen["role"]), (StatusCode::OK, &json!("viewer")));
	assert_eq!(outsider.call(Method::GET, &url, None).await.0, StatusCode::NOT_FOUND, "general access is for Service-Arb");

	let write = |base: i64| Some(json!({ "base": base, "from": 1, "turns": [question("q2", "and?")] }));
	assert_eq!(viewer.call(Method::PUT, &format!("{url}/turns"), write(1)).await.0, StatusCode::FORBIDDEN);
	assert_eq!(viewer.call(Method::PUT, &format!("{url}/title"), Some(json!({ "title": "x" }))).await.0, StatusCode::FORBIDDEN);
	assert_eq!(outsider.call(Method::PUT, &format!("{url}/turns"), write(1)).await.0, StatusCode::NOT_FOUND);
	assert_eq!(
		member.call(Method::PUT, &format!("{url}/general"), Some(json!({ "general": "none" }))).await.0,
		StatusCode::FORBIDDEN
	);
	assert_eq!(member.call(Method::DELETE, &url, None).await.0, StatusCode::FORBIDDEN);
	assert_eq!(member.call(Method::DELETE, &format!("{url}/members/{}", owner.id), None).await.0, StatusCode::FORBIDDEN);

	let (status, written) = member.call(Method::PUT, &format!("{url}/turns"), write(1)).await;
	assert_eq!(
		(status, written["version"].as_i64(), written["turns"].as_array().unwrap().len()),
		(StatusCode::OK, Some(2), 2),
		"{written}"
	);
	let (status, stale) = owner.call(Method::PUT, &format!("{url}/turns"), write(1)).await;
	assert_eq!(status, StatusCode::CONFLICT, "{stale}");
	assert_eq!(
		owner.call(Method::PUT, &format!("{url}/turns"), Some(json!({ "base": 2, "from": 3, "turns": [] }))).await.0,
		StatusCode::BAD_REQUEST
	);
	let unknown = json!({ "base": 2, "from": 2, "turns": [{ "role": "user", "id": "q", "at": 1, "text": "", "mentions": [], "files": [], "extra": 1 }] });
	assert_eq!(owner.call(Method::PUT, &format!("{url}/turns"), Some(unknown)).await.0, StatusCode::BAD_REQUEST);
	let huge = json!({ "base": 2, "from": 2, "turns": [question("q3", &"a".repeat(5 << 20))] });
	assert_eq!(owner.call(Method::PUT, &format!("{url}/turns"), Some(huge)).await.0, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn a_deleted_chat_takes_its_members_and_marks_and_tells_them() {
	let db = TestDb::create().await;
	let p = panel(&db).await;
	let mut owner = Person::new(&p, "owner@x.com", operator()).await;
	let mut member = Person::new(&p, "member@x.com", operator()).await;
	let chat = Uuid::now_v7();
	let url = format!("/api/v1/assistant/chats/{chat}");
	owner.call(Method::POST, "/api/v1/assistant/chats", Some(json!({ "id": chat, "title": "t", "turns": [] }))).await;
	owner.call(Method::POST, &format!("{url}/members"), Some(json!({ "users": [member.id] }))).await;
	assert_eq!(member.call(Method::PUT, &format!("{url}/pinned"), Some(json!({ "on": true }))).await.0, StatusCode::NO_CONTENT);
	assert!(member.call(Method::GET, "/api/v1/assistant/chats", None).await.1["chats"][0]["pinned_at"].is_string());

	let mut bus = p.bus().subscribe();
	assert_eq!(owner.call(Method::DELETE, &url, None).await.0, StatusCode::NO_CONTENT);
	let mut told = Vec::new();
	while let Ok(Signal::Changed(c)) = bus.try_recv() {
		assert_eq!((c.topic.as_str(), c.id), ("assistant", Some(chat.to_string())));
		told.push(match c.audience {
			Audience::User(u) => u,
			a => panic!("{a:?}"),
		});
	}
	told.sort();
	let mut both = vec![owner.id, member.id];
	both.sort();
	assert_eq!(told, both, "the member is told of a chat gone");

	let pool = db.pool().await;
	for table in ["assistant_chats", "assistant_members", "assistant_marks"] {
		let left: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}"))).fetch_one(&pool).await.unwrap();
		assert_eq!(left, 0, "{table}");
	}
	assert_eq!(member.call(Method::GET, &url, None).await.0, StatusCode::NOT_FOUND);
}
