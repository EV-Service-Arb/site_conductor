//! A gmail's mail with Google lands in its GBPs' logs: on the GBP it names, else the gmail's, each
//! message once however often its inbox is read.

use std::sync::Mutex;

use jiff::Timestamp;
use sa_core::{gbp::Country, prices::Spender};
use sa_panel::{
	gmbs::{Event, EventKind, Existing},
	import::Export,
	inbox::{Account, BATCH, Batch, Cursor, Mail, Mailbox},
	seal::{DataKey, gmail_imap_aad},
	testing::{TestDb, panel_with},
	tokens::User,
};
use uuid::Uuid;

fn now() -> Timestamp {
	"2026-11-19T12:00:00Z".parse().unwrap()
}

/// All Mail as Gmail would answer it: its UIDVALIDITY and its messages with Google.
struct Inbox {
	validity: Mutex<u32>,
	mails: Vec<Mail>,
}

impl Mailbox for Inbox {
	async fn read(&self, account: Account<'_>, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
		assert_eq!((account.username, account.password), ("a@gmail.com", "app-password"), "read with the gmail's own credentials");
		let validity = *self.validity.lock().unwrap();
		let from = after.filter(|c| c.validity == validity).map_or(0, |c| c.uid);
		Ok(Batch {
			validity,
			mails: self.mails.iter().filter(|m| m.uid > from).take(limit).cloned().collect(),
		})
	}
}

fn mail(uid: u32, subject: &str, text: &str) -> Mail {
	Mail {
		uid,
		message: 1_800_000_000_000_000_000 + u64::from(uid),
		thread: 0x18c0_0000_0000_0000 + u64::from(uid),
		date: "2026-11-18T09:30:00Z".parse().unwrap(),
		subject: Some(subject.to_owned()),
		counterparts: vec!["Google Business Profile <businessprofile-noreply@google.com>".to_owned()],
		from: Some("businessprofile-noreply@google.com".to_owned()),
		reply_to: None,
		to: vec!["a@gmail.com".to_owned()],
		message_id: None,
		refs: vec![],
		authentic: true,
		sent: false,
		text: text.to_owned(),
		whole: true,
	}
}

#[tokio::test]
async fn a_thread_naming_a_gbp_is_its_and_one_naming_none_the_gmails_each_once() {
	let db = TestDb::create().await;
	let hex = DataKey::generate_hex().unwrap();
	let p = panel_with(db.store().await, DataKey::from_hex(&hex).unwrap());
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
	let owner = Uuid::now_v7();
	p.user_seen(
		&User {
			user_id: owner,
			email: "owner@localhost".into(),
			name: "owner".into(),
		},
		now(),
	)
	.await
	.unwrap();
	let gmail = p.add_gmail(owner, "a@gmail.com", now()).await.unwrap().unwrap().id;
	let gbp = |place: &str, name: &str| Existing {
		gmail_id: gmail,
		place: place.to_owned(),
		state: sa_core::gbp::OnGoogle::Verified,
		name: Some(name.to_owned()),
		country: Some(Country::FR),
		trade: None,
	};
	let dupont = p
		.add_gmb(Some(Spender::new(owner)), gbp("ChIJLU7jZClu5kcR4PcOOO6p3I0", "Plomberie Dupont"), now())
		.await
		.unwrap()
		.unwrap()
		.id;
	let other = p
		.add_gmb(Some(Spender::new(owner)), gbp("ChIJD7fiBh9u5kcRYJSMaMOCCwQ", "Serrurerie Martin"), now())
		.await
		.unwrap()
		.unwrap()
		.id;
	let sealed = DataKey::from_hex(&hex).unwrap().seal(&gmail_imap_aad(gmail), b"app-password").unwrap();
	sqlx::query("INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at) VALUES ($1, 'imap.gmail.com', 'a@gmail.com', $2, $3, 0, 0)")
		.bind(gmail)
		.bind(sealed)
		.bind(DataKey::from_hex(&hex).unwrap().fingerprint().to_vec())
		.execute(&db.pool().await)
		.await
		.unwrap();
	assert_eq!(p.inboxes().await.unwrap(), vec![gmail]);

	let inbox = Inbox {
		validity: Mutex::new(7),
		mails: vec![
			mail(3, "Your verification code", "Verify PLOMBERIE DUPONT on Google: enter 12345."),
			mail(5, "Your account was signed in from a new device", "Someone signed in to a@gmail.com."),
		],
	};
	let later = now() + jiff::SignedDuration::from_mins(1);
	assert!(!p.follow_inbox(gmail, &inbox, later).await.unwrap(), "two messages, not a full batch of {BATCH}");
	// read again, and once its UIDVALIDITY changed: all of it again, none logged twice
	p.follow_inbox(gmail, &inbox, later).await.unwrap();
	*inbox.validity.lock().unwrap() = 8;
	p.follow_inbox(gmail, &inbox, later).await.unwrap();

	let emails = |gmb: i64| {
		let p = p.clone();
		async move {
			p.gmb_log(owner, gmb, None, &[EventKind::Email])
				.await
				.unwrap()
				.unwrap()
				.into_iter()
				.map(|l| match l.event {
					Event::Email { subject, thread, snippet, .. } => (subject.unwrap(), thread, snippet, l.of_gmail),
					e => panic!("asked for emails: {e:?}"),
				})
				.collect::<Vec<_>>()
		}
	};
	let own = (
		"Your account was signed in from a new device".to_owned(),
		"18c0000000000005".to_owned(),
		"Someone signed in to a@gmail.com.".to_owned(),
		true,
	);
	assert_eq!(
		emails(dupont).await,
		vec![
			own.clone(),
			(
				"Your verification code".to_owned(),
				"18c0000000000003".to_owned(),
				"Verify PLOMBERIE DUPONT on Google: enter 12345.".to_owned(),
				false
			)
		],
		"the code names it: its own; the sign-in names no GBP: the gmail's, in each GBP's log"
	);
	assert_eq!(emails(other).await, vec![own]);
}
