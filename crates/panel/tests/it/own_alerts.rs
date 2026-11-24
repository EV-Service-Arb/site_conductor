//! The conductor's own alerts, told in the writes that make them so: each once for what it is about.

use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Rule, Tier};
use sa_auth::{PermissionSet, SA_ADMIN, Service};
use sa_core::{
	prices::{Item, PhoneTime},
	tokens::ChargePage,
	usage::{Used, UsagePage},
};
use sa_panel::{
	Panel,
	access::Asker,
	alerts::{Alert, Sought},
	import::Export,
	inbox::{Account, Batch, Cursor, Mail, Mailbox},
	seal::{DataKey, gmail_imap_aad},
	testing::{TestDb, panel_with, priced},
	tokens::{Change, User},
};
use uuid::Uuid;

fn now() -> Timestamp {
	"2026-11-19T12:00:00Z".parse().unwrap()
}

fn days(n: i64) -> SignedDuration {
	SignedDuration::from_hours(24 * n)
}

fn admin() -> PermissionSet {
	SA_ADMIN.members.iter().copied().collect()
}

struct Setup {
	db: TestDb,
	hex: String,
	panel: Panel,
	user: Uuid,
}

impl Setup {
	async fn new() -> Self {
		let db = TestDb::create().await;
		let hex = DataKey::generate_hex().unwrap();
		let panel = panel_with(db.store().await, DataKey::from_hex(&hex).unwrap());
		let empty = Export {
			people: vec![],
			gmails: vec![],
			gmbs: vec![],
			ledger: vec![],
			kickbacks: vec![],
		};
		panel.import_review_archive(empty).await.unwrap().unwrap();
		let user = Uuid::now_v7();
		let seen = User {
			user_id: user,
			email: "owner@localhost".into(),
			name: "owner".into(),
		};
		panel.user_seen(&seen, now()).await.unwrap();
		panel.alerts_access_seen(user, &admin(), "owner", now()).await.unwrap();
		Self { db, hex, panel, user }
	}

	/// What the user was told, oldest first.
	async fn told(&self) -> Vec<Alert> {
		let mut all = self.panel.alerts_inbox(self.user, &admin(), &Sought::default()).await.unwrap();
		all.reverse();
		all
	}

	async fn titles(&self) -> Vec<String> {
		self.told().await.into_iter().map(|a| a.text.text.lines().next().unwrap().to_owned()).collect()
	}
}

struct Inbox(Vec<Mail>);

impl Mailbox for Inbox {
	async fn read(&self, _: Account<'_>, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
		let from = after.map_or(0, |c| c.uid);
		Ok(Batch {
			validity: 7,
			mails: self.0.iter().filter(|m| m.uid > from).take(limit).cloned().collect(),
		})
	}
}

fn mail(uid: u32, date: Timestamp, subject: &str, sent: bool) -> Mail {
	Mail {
		uid,
		message: u64::from(uid),
		thread: u64::from(uid),
		date,
		subject: Some(subject.to_owned()),
		counterparts: vec!["Google <noreply@google.com>".to_owned()],
		sent,
		text: "Enter 12345.".to_owned(),
	}
}

#[tokio::test]
async fn mail_from_google_is_told_once_and_never_a_mailboxs_backlog() {
	let s = Setup::new().await;
	let gmail = s.panel.add_gmail(s.user, "a@gmail.com", now()).await.unwrap().unwrap().id;
	let key = DataKey::from_hex(&s.hex).unwrap();
	sqlx::query("INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at) VALUES ($1, 'imap.gmail.com', 'a@gmail.com', $2, $3, 0, 0)")
		.bind(gmail)
		.bind(key.seal(&gmail_imap_aad(gmail), b"app-password").unwrap())
		.bind(key.fingerprint().to_vec())
		.execute(&s.db.pool().await)
		.await
		.unwrap();
	let inbox = Inbox(vec![
		mail(1, now() - days(3), "Your profile was suspended", false),
		mail(2, now(), "Votre fiche a été suspendue", false),
		mail(3, now(), "Re: appeal", true),
		mail(4, now(), "Your verification code", false),
		mail(5, now(), "Your performance in November", false),
	]);
	s.panel.follow_inbox(gmail, &inbox, now()).await.unwrap();
	s.panel.follow_inbox(gmail, &inbox, now()).await.unwrap();
	let told = s.told().await;
	let said: Vec<(Tier, &str)> = told.iter().map(|a| (a.tier, a.text.text.lines().next().unwrap())).collect();
	assert_eq!(
		said,
		[
			(Tier::Urgent, "Google · a@gmail.com: Votre fiche a été suspendue"),
			(Tier::Important, "Google · a@gmail.com: Your verification code"),
			(Tier::Info, "Google · a@gmail.com: Your performance in November"),
		],
		"neither the backlog, nor what the gmail sent"
	);
	assert!(told[1].text.text.ends_with("\nEnter 12345."), "its owner reads what it says");
}

#[tokio::test]
async fn what_goes_on_failing_is_said_again_after_a_day() {
	let s = Setup::new().await;
	let stalled = |at: Timestamp| s.panel.alerts_feed_stalled(Service::Sites, "502", at);
	let evening: Timestamp = "2026-11-19T23:50:00Z".parse().unwrap();
	for minutes in [0, 1, 20, 23 * 60] {
		stalled(evening + SignedDuration::from_mins(minutes)).await.unwrap();
	}
	assert_eq!(s.told().await.len(), 1, "over midnight too");
	stalled(evening + days(1) + SignedDuration::from_mins(1)).await.unwrap();
	assert_eq!(s.told().await.len(), 2);
}

#[tokio::test]
async fn running_low_is_told_once_a_top_up_and_a_phone_run_dry_at_its_stop() {
	let s = Setup::new().await;
	priced(&s.panel, &[(Item::Phone(PhoneTime::Running), 10)]).await;
	let set = |n| s.panel.change_balance(s.user, Change::Set { n, note: None }, "admin@localhost", now());
	set(1_000).await.unwrap().unwrap();
	let ran = |id, minutes, after| {
		let page = UsagePage {
			used: vec![Used {
				id,
				item: Item::Phone(PhoneTime::Running),
				units: minutes,
				at: now(),
				user: Some(s.user),
			}],
			next: id,
		};
		let panel = &s.panel;
		async move {
			panel.take_usage("browsers", &[Item::Phone(PhoneTime::Running)].into(), page, after).await.unwrap();
			panel.snapshot(Timestamp::now()).await.unwrap();
		}
	};
	ran(1, 60, 0).await;
	assert_eq!(s.titles().await, Vec::<String>::new());
	ran(2, 39, 1).await;
	let told = s.told().await;
	assert_eq!(told.iter().map(|a| a.rule).collect::<Vec<_>>(), [Rule::TokensLow], "{:?}", s.titles().await);
	ran(3, 5, 2).await;
	s.panel.snapshot(Timestamp::now()).await.unwrap();
	assert_eq!(s.told().await.iter().map(|a| a.rule).collect::<Vec<_>>(), [Rule::TokensLow, Rule::BrowserStopped, Rule::TokensLow], "low, then out, each once");
	set(1_000).await.unwrap().unwrap();
	ran(4, 99, 3).await;
	assert_eq!(s.told().await.len(), 4, "a top-up starts over");
}

#[tokio::test]
async fn kick_backs_waiting_to_be_claimed_are_told_once_a_grant_each() {
	let s = Setup::new().await;
	s.panel.change_balance(s.user, Change::Set { n: 100, note: None }, "admin@localhost", now()).await.unwrap().unwrap();
	let failed = |id: i64| -> ChargePage {
		serde_json::from_value(serde_json::json!({
			"charges": [{"id": id, "key": format!("run:{id}"), "label": "Plomberie Dupont", "at": now(), "payers": [s.user], "spent": 5, "used": {"walk.open": 1}, "kick_back": true}],
			"next": id,
		}))
		.unwrap()
	};
	s.panel.take_charges(failed(1), 0, now()).await.unwrap();
	s.panel.take_charges(failed(2), 1, now()).await.unwrap();
	assert_eq!(s.titles().await, ["5 tokens back: a walk failed"]);
	let grant = || Change::Grant { n: 20, note: "sorry".into() };
	s.panel.change_balance(s.user, grant(), "admin@localhost", now()).await.unwrap().unwrap();
	s.panel.change_balance(s.user, grant(), "admin@localhost", now()).await.unwrap().unwrap();
	s.panel.claim(s.user, now()).await.unwrap().unwrap();
	s.panel.take_charges(failed(3), 2, now()).await.unwrap();
	assert_eq!(
		s.titles().await,
		["5 tokens back: a walk failed", "20 tokens granted to you", "20 tokens granted to you", "5 tokens back: a walk failed"]
	);
}

#[tokio::test]
async fn access_asked_for_and_then_held_is_told_and_no_longer_stands() {
	let s = Setup::new().await;
	let nothing: PermissionSet = std::iter::empty::<&str>().collect();
	let asker = Asker {
		user: s.user,
		email: "owner@localhost",
		name: "owner",
	};
	s.panel.request_access(asker, "sa:admin", now()).await.unwrap();
	assert_eq!(s.panel.pending_access(s.user, &nothing).await.unwrap().len(), 1);
	let cabinet = "https://evinvest.ltd".parse().unwrap();
	s.panel.alerts_fan_out_own(now(), sa_core::notify::Locale::En, &cabinet, &[].into()).await.unwrap();
	assert_eq!(s.told().await.iter().map(|a| a.rule).collect::<Vec<_>>(), [Rule::AccessGranted], "concierge last said they hold it; nobody else asked");
	assert!(s.panel.pending_access(s.user, &nothing).await.unwrap().is_empty());
}

#[tokio::test]
async fn what_the_trash_is_about_to_lose_is_told_once_a_day_lost() {
	let s = Setup::new().await;
	for (gmail, at) in [("a@gmail.com", now()), ("b@gmail.com", now() + SignedDuration::from_mins(1))] {
		let id = s.panel.add_gmail(s.user, gmail, now()).await.unwrap().unwrap().id;
		s.panel.remove_gmail(s.user, id, at).await.unwrap().unwrap();
	}
	for d in [1, 26, 28, 29] {
		s.panel.purge(now() + days(d), false).await.unwrap();
	}
	assert_eq!(s.titles().await, ["Deleted for good 19 Dec: a@gmail.com and 1 more"]);
}
