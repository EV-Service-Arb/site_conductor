//! Google's mail against a scripted Claude API: read into cases, a reply written and sent by its
//! operator, each person's their own.

use std::sync::{Arc, Mutex};

use axum::{Json, Router, http::Method, routing::post};
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	gbp::Country,
	prices::{Item, ModelTokens, Spender},
};
use sa_panel::{
	Panel,
	gmbs::Existing,
	import::Export,
	inbox::{Account, Batch, Cursor, Mail, Mailbox},
	seal::{DataKey, gmail_imap_aad},
	testing::{Outbox, TestDb, operator, panel_sending, priced},
	tokens::User,
};
use sa_server::{
	claude::{Model, Origins},
	concierge::{Concierge, DevIdentity},
	http,
	mail::Worker,
	signin::{SignIn, SignInConfig},
	testing::{Browser, ORIGIN},
};
use serde_json::{Value, json};
use uuid::Uuid;

const EMAIL: &str = "operator@x.com";
const SUPPORT: &str = "googlebusinessprofile-support@google.com";

type Sent = Arc<Mutex<Vec<Value>>>;

/// A Claude API that answers each request with the next of `script`: a text, as JSON.
async fn claude(script: Vec<Value>) -> (String, Sent) {
	let sent = Sent::default();
	let script = Arc::new(Mutex::new(script.into_iter()));
	let seen = sent.clone();
	let app = Router::new().route(
		"/v1/messages",
		post(move |Json(req): Json<Value>| {
			seen.lock().unwrap().push(req);
			let said = script.lock().unwrap().next().expect("the script ran out");
			let content = match said.get("content") {
				Some(content) => content.clone(),
				None => json!([{ "type": "thinking", "thinking": "" }, { "type": "text", "text": said.to_string() }]),
			};
			let stop = if content[0]["type"] == "tool_use" { "tool_use" } else { "end_turn" };
			let reply = json!({
				"model": "claude-opus-5-5",
				"stop_reason": stop,
				"content": content,
				"usage": { "input_tokens": 1000, "cache_creation_input_tokens": 0, "cache_read_input_tokens": 0, "output_tokens": 100 },
			});
			async move { Json(reply) }
		}),
	);
	let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
	(format!("http://{addr}"), sent)
}

fn reading(kind: &str) -> Value {
	json!({ "topic": "verification", "kind": kind, "gmb": null, "summary": format!("{kind} was said"), "asks": [], "reasons": [], "deadline": null, "answer_within_hours": null })
}

/// All Mail as Gmail would answer it.
struct Inbox(Mutex<Vec<Mail>>);

impl Mailbox for Inbox {
	async fn read(&self, _: Account<'_>, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
		let mails = self.0.lock().unwrap();
		Ok(Batch {
			validity: 7,
			mails: mails.iter().filter(|m| m.uid > after.map_or(0, |c| c.uid)).take(limit).cloned().collect(),
		})
	}
}

struct World {
	p: Panel,
	app: Router,
	b: Browser,
	worker: Worker,
	outbox: Arc<Outbox>,
	inbox: Inbox,
	owner: Uuid,
	gmail: i64,
	gmb: i64,
	asked: Sent,
	_db: TestDb,
}
impl World {
	/// `ours`: the panel has a Claude.
	async fn new(script: Vec<Value>, ours: bool) -> Self {
		let db = TestDb::create().await;
		let hex = DataKey::generate_hex().unwrap();
		let outbox = Arc::new(Outbox::default());
		let store = db.store().await;
		let keyless = sa_places::Google::try_new(sa_places::Keys::new(vec![]), Arc::new(sa_places::InOrder), Arc::new(store.clone())).unwrap();
		let p = panel_sending(store, DataKey::from_hex(&hex).unwrap(), keyless, sa_places::Accounts::new(vec![], vec![]), outbox.clone());
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
		let now = Timestamp::now();
		let owner = DevIdentity::named(EMAIL);
		let user = User {
			user_id: owner,
			email: EMAIL.into(),
			name: "operator".into(),
		};
		p.user_seen(&user, now).await.unwrap();
		let gmail = p.add_gmail(owner, "a@gmail.com", now).await.unwrap().unwrap().id;
		let new = Existing {
			gmail_id: gmail,
			place: "ChIJLU7jZClu5kcR4PcOOO6p3I0".into(),
			state: sa_core::gbp::OnGoogle::Verifying,
			name: Some("Plomberie Dupont - Lyon".into()),
			country: Some(Country::FR),
			trade: None,
		};
		let gmb = p.add_gmb(Some(Spender::new(owner)), new, now).await.unwrap().unwrap().id;
		let sealed = DataKey::from_hex(&hex).unwrap().seal(&gmail_imap_aad(gmail), b"app-password").unwrap();
		sqlx::query("INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at) VALUES ($1, 'imap.gmail.com', 'a@gmail.com', $2, $3, 0, 0)")
			.bind(gmail)
			.bind(sealed)
			.bind(DataKey::from_hex(&hex).unwrap().fingerprint().to_vec())
			.execute(&db.pool().await)
			.await
			.unwrap();
		let (base, asked) = claude(script).await;
		let model = Model::new(Origins { api: base.clone(), oauth: base }, ours.then(|| "ours".to_owned().into()));
		let who = DevIdentity {
			user_id: owner,
			permissions: operator(),
			email: EMAIL.into(),
		};
		let config = SignInConfig {
			panel_origin: ORIGIN.to_owned(),
			concierge_origin: ORIGIN.to_owned(),
		};
		let app = http::app(
			SignIn::new(p.clone(), Concierge::dev(who), config),
			None,
			None,
			Default::default(),
			None,
			None,
			Some(model.clone()),
			sa_server::testing::sandbox(),
			None,
		);
		let mut b = Browser::default();
		b.sign_in(&app).await;
		Self {
			worker: Worker {
				panel: p.clone(),
				model,
				by_itself: true,
				tell: None,
			},
			p,
			app,
			b,
			outbox,
			inbox: Inbox(Mutex::default()),
			owner,
			gmail,
			gmb,
			asked,
			_db: db,
		}
	}

	async fn arrives(&self, hours_ago: i64, from: &str, subject: &str, text: &str) {
		{
			let mut mails = self.inbox.0.lock().unwrap();
			let uid = u32::try_from(mails.len()).unwrap() + 1;
			mails.push(Mail {
				uid,
				message: 1_800_000_000_000_000_000 + u64::from(uid),
				thread: 0xaa,
				date: Timestamp::now() - SignedDuration::from_hours(hours_ago),
				subject: Some(subject.to_owned()),
				counterparts: vec![from.to_owned()],
				from: Some(from.to_owned()),
				reply_to: None,
				to: vec!["a@gmail.com".into()],
				message_id: Some(format!("m{uid}@mail")),
				refs: vec![],
				authentic: true,
				sent: false,
				text: text.to_owned(),
			});
		}
		self.p.follow_inbox(self.gmail, &self.inbox, Timestamp::now()).await.unwrap();
	}

	async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (u16, Value) {
		let (status, answer) = self.b.call(&self.app, method, &format!("/api/v1{path}"), body).await;
		(status.as_u16(), answer)
	}

	async fn cases(&mut self) -> Vec<Value> {
		let (status, answer) = self.call(Method::GET, "/mail/cases", None).await;
		assert_eq!(status, 200, "{answer}");
		answer["cases"].as_array().unwrap().clone()
	}
}

#[tokio::test]
async fn a_persons_mail_is_read_answered_by_a_draft_and_sent_by_them() {
	let mut asks = reading("asks");
	asks["asks"] = json!(["a photo of the signage"]);
	let mut w = World::new(
		vec![
			asks,
			reading("video_rejected"),
			json!({ "body": "The recorded video does not work for this profile: enable the live call.", "needs": ["a photo of the signage"], "nothing": null }),
			json!({ "body": "La photo est jointe. Activez l'appel.", "needs": [], "nothing": null }),
		],
		true,
	)
	.await;
	priced(&w.p, &[(Item::from(ModelTokens::Input), 1000), (Item::from(ModelTokens::Output), 1000)]).await;
	w.arrives(3, "businessprofile-noreply@google.com", "Action required: Submit a new verification video", "Your verification video for Plomberie Dupont -  \nLyon didn’t meet <https://business.google.com/verify/l/15?trk=x>")
		.await;
	w.arrives(2, SUPPORT, "[1-1234000056789] Your Request to Google", "About “Plomberie Dupont - Lyon”: please share a photo of the signage.").await;
	w.worker.read_and_write().await.unwrap();
	let cases = w.cases().await;
	assert_eq!(cases.len(), 2);
	let (case, rejected) = (&cases[0], &cases[1]);
	assert_eq!((&case["gmb_id"], &case["topic"], &case["asks"]), (&json!(w.gmb), &json!("verification"), &json!(["a photo of the signage"])));
	assert_eq!(case["state"], json!({ "is": "needs_you", "why": { "code": "needs", "needs": ["a photo of the signage"] } }));
	assert_eq!(case["draft"]["state"], "proposed");
	assert_eq!(rejected["state"], json!({ "is": "done" }), "the conversation, newer, carries it");
	let id = case["id"].as_i64().unwrap();

	{
		let asked = w.asked.lock().unwrap();
		assert_eq!(asked.len(), 3, "each message once, a notice not answered");
		let writer = &asked[2];
		let context = writer["messages"][0]["content"].as_str().unwrap();
		assert!(context.contains("video_rejected was said"), "what Google said elsewhere of the GBP: {context}");
		assert!(context.contains("name: Plomberie Dupont - Lyon"), "{context}");
		assert!(writer["tool_choice"].is_null());
		assert_eq!(writer["output_config"]["format"]["type"], "json_schema");
		assert_eq!(writer["system"][0]["text"], "You are Claude Code, Anthropic's official CLI for Claude.");
	}
	let charged: Vec<String> = w
		.p
		.ledger(w.owner, None, 10)
		.await
		.unwrap()
		.unwrap()
		.into_iter()
		.filter_map(|(_, e)| e.charged?.label)
		.collect();
	assert_eq!(charged, ["A reply to Google written", "Google's mail read", "Google's mail read"], "on the panel's Claude, the gmail's owner pays");

	let (status, detail) = w.call(Method::GET, &format!("/mail/cases/{id}"), None).await;
	assert_eq!(status, 200);
	assert_eq!(detail["recipient"], SUPPORT);
	assert_eq!(detail["messages"][0]["read_by"], "model");
	assert_eq!((&detail["attempts"][0]["method"], &detail["attempts"][0]["outcome"]), (&json!("video"), &json!("rejected")));
	let (_, of_gmb) = w.call(Method::GET, &format!("/gmbs/{}/mail", w.gmb), None).await;
	assert_eq!((of_gmb["attempts"].as_array().unwrap().len(), of_gmb["cases"].as_array().unwrap().len()), (1, 2));
	let (_, waiting) = w.call(Method::GET, "/mail/cases?state=waiting", None).await;
	assert_eq!(waiting["cases"], json!([]));

	let (status, drafted) = w.call(Method::POST, &format!("/mail/cases/{id}/draft"), Some(json!({ "instruction": "en français, la photo est jointe" }))).await;
	assert_eq!(status, 200, "{drafted}");
	assert_eq!(drafted["case"]["state"], json!({ "is": "reply_ready" }));
	assert!(w.asked.lock().unwrap()[3]["messages"][0]["content"].as_str().unwrap().contains("en français"));
	let draft = drafted["case"]["draft"]["id"].as_i64().unwrap();
	let (status, edited) = w.call(Method::PUT, &format!("/mail/drafts/{draft}"), Some(json!({ "body": "La photo est jointe." }))).await;
	assert_eq!((status, &edited["case"]["draft"]["body"]), (200, &json!("La photo est jointe.")));
	let send = |base64: &str| json!({ "body": "La photo est jointe. Activez l'appel.", "files": [{ "name": "sign.jpg", "type": "image/jpeg", "base64": base64 }] });
	let (status, _) = w.call(Method::POST, &format!("/mail/drafts/{draft}/send"), Some(send("not base64"))).await;
	assert_eq!(status, 400);
	let (status, sent) = w.call(Method::POST, &format!("/mail/drafts/{draft}/send"), Some(send("AQID"))).await;
	assert_eq!(status, 200, "{sent}");
	assert_eq!(sent["case"]["state"]["is"], "waiting");
	{
		let out = w.outbox.sent.lock().unwrap();
		assert_eq!((out.len(), out[0].to.as_str(), out[0].files[0].bytes.as_slice()), (1, SUPPORT, [1, 2, 3].as_slice()));
	}
	let (status, _) = w.call(Method::POST, &format!("/mail/drafts/{draft}/send"), Some(send("AQID"))).await;
	assert_eq!(status, 409, "once");
	w.worker.read_and_write().await.unwrap();
	assert_eq!(w.asked.lock().unwrap().len(), 4, "nothing more to ask");

	let mut other = Browser::default();
	other.sign_in_as(&w.app, "other@x.com").await;
	for (method, path, body) in [
		(Method::GET, format!("/mail/cases/{id}"), None),
		(Method::PATCH, format!("/mail/cases/{id}"), Some(json!({ "done": true }))),
		(Method::PATCH, format!("/mail/messages/{}", detail["messages"][0]["id"]), Some(json!({ "kind": "closed" }))),
		(Method::POST, format!("/mail/cases/{id}/draft"), Some(json!({ "instruction": null }))),
		(Method::PUT, format!("/mail/drafts/{draft}"), Some(json!({ "body": "x" }))),
		(Method::POST, format!("/mail/drafts/{draft}/send"), Some(json!({ "body": "x", "files": [] }))),
		(Method::DELETE, format!("/mail/drafts/{draft}"), None),
		(Method::PUT, "/mail/auto".to_owned(), Some(json!({ "gmail": w.gmail, "topic": "reviews", "on": true }))),
		(Method::GET, format!("/gmbs/{}/mail", w.gmb), None),
	] {
		let (status, _) = other.call(&w.app, method.clone(), &format!("/api/v1{path}"), body).await;
		assert_eq!(status.as_u16(), 404, "{method} {path}: another's");
	}
	let (_, theirs) = other.call(&w.app, Method::GET, "/api/v1/mail/cases", None).await;
	assert_eq!(theirs["cases"], json!([]));

	let (status, marked) = w.call(Method::PATCH, &format!("/mail/cases/{id}"), Some(json!({ "done": true }))).await;
	assert_eq!((status, &marked["case"]["state"]["is"]), (200, &json!("done")));
	let (status, corrected) = w
		.call(Method::PATCH, &format!("/mail/messages/{}", detail["messages"][0]["id"]), Some(json!({ "kind": "call_offered" })))
		.await;
	assert_eq!((status, &corrected["messages"][0]["reading"]["kind"], &corrected["messages"][0]["read_by"]), (200, &json!("call_offered"), &json!("person")));
	let (status, auto) = w.call(Method::PUT, "/mail/auto", Some(json!({ "gmail": w.gmail, "topic": "reviews", "on": true }))).await;
	assert_eq!((status, &auto["on"]), (200, &json!([{ "gmail_id": w.gmail, "topic": "reviews" }])));
	let (status, _) = w.call(Method::PUT, "/mail/auto", Some(json!({ "gmail": w.gmail, "topic": "suspension", "on": true }))).await;
	assert_eq!(status, 400);
}

#[tokio::test]
async fn mail_waits_without_a_claude_and_a_reading_that_never_reads_is_a_persons() {
	let mut w = World::new(vec![], false).await;
	w.arrives(2, SUPPORT, "[1-1234000056789] Your Request", "Hello").await;
	w.worker.read_and_write().await.unwrap();
	let case = w.cases().await.remove(0);
	assert_eq!(case["state"]["why"]["code"], "stuck", "{case}");
	let (status, refused) = w.call(Method::POST, &format!("/mail/cases/{}/draft", case["id"]), Some(json!({ "instruction": null }))).await;
	assert_eq!((status, &refused["code"]), (503, &json!("unavailable")));

	let invented = json!({ "topic": "verification", "kind": "a_kind_of_its_own", "gmb": null, "summary": "", "asks": [], "reasons": [], "deadline": null, "answer_within_hours": null });
	let mut w = World::new(vec![invented.clone(), invented.clone(), invented], true).await;
	w.arrives(2, SUPPORT, "[1-1234000056789] Your Request", "Hello").await;
	for _ in 0..4 {
		w.worker.read_and_write().await.unwrap();
	}
	assert_eq!(w.asked.lock().unwrap().len(), 3);
	let why = w.cases().await.remove(0)["state"]["why"].clone();
	assert_eq!(why["code"], "unread");
	assert!(why["error"].as_str().unwrap().contains("a_kind_of_its_own"), "{why}");
}

#[tokio::test]
async fn an_agent_sends_only_what_its_person_agreed_to() {
	let send = json!({ "content": [{ "type": "tool_use", "id": "t1", "name": "call", "input": { "operation": "send_mail_draft", "path": { "id": 1 }, "body": { "body": "x", "files": [] } } }] });
	let done = json!({ "content": [{ "type": "text", "text": "Confirm to send it." }] });
	let mut w = World::new(vec![send, done], true).await;
	let chat = Uuid::now_v7();
	let (status, _) = w.call(Method::POST, "/assistant/chats", Some(json!({ "id": chat, "title": null, "turns": [] }))).await;
	assert_eq!(status, 201);
	let question = json!({ "from": 0, "text": "answer Google", "mentions": [], "files": [], "url": "/email", "bindings": [], "approval": "spend" });
	let (status, answer) = w.call(Method::POST, &format!("/assistant/chats/{chat}/ask"), Some(question)).await;
	assert_eq!(status, 200, "{answer}");
	assert_eq!(answer["turns"][1]["actions"][0]["state"], "proposed", "{answer}");
	assert!(w.outbox.sent.lock().unwrap().is_empty());
}
