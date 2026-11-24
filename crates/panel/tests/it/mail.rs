//! Google's mail as cases: what each message joins, whose it is, what its reply may do.

use std::sync::{Arc, Mutex};

use jiff::{SignedDuration, Timestamp};
use sa_alerts::Tier;
use sa_core::{
	gbp::Country,
	mail::{Kind, Method, Outcome, Reading, State, Topic, Why, attempt},
	notify::Locale,
	prices::Spender,
};
use sa_panel::{
	Panel, Refused,
	alerts::Sought,
	gmbs::Existing,
	import::Export,
	inbox::{Account, Batch, Cursor, Mail, Mailbox},
	mail::{Case, CaseEdit, File, Outgoing, ReadBy, ReadingEdit, SendError, Written},
	seal::{DataKey, gmail_imap_aad},
	testing::{Outbox, TestDb, panel_sending},
	tokens::User,
};
use uuid::Uuid;

const SUPPORT: &str = "googlebusinessprofile-support@google.com";
const NOTICE: &str = "businessprofile-noreply@google.com";

const REJECTED: &str = "Action required: Submit a new verification video";

fn t(s: &str) -> Timestamp {
	s.parse().unwrap()
}

/// All Mail as Gmail would answer it.
#[derive(Default)]
struct Inbox {
	mails: Mutex<Vec<Mail>>,
}

impl Mailbox for Inbox {
	async fn read(&self, _: Account<'_>, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
		let mails = self.mails.lock().unwrap();
		Ok(Batch {
			validity: 7,
			mails: mails.iter().filter(|m| m.uid > after.map_or(0, |c| c.uid)).take(limit).cloned().collect(),
		})
	}
}

struct World {
	db: TestDb,
	p: Panel,
	owner: Uuid,
	gmail: i64,
	/// "Plomberie Dupont - Lyon" and "Plomberie Dupont - Lyon, Est".
	lyon: i64,
	est: i64,
	inbox: Inbox,
	outbox: Arc<Outbox>,
}
impl World {
	async fn new() -> Self {
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
		let now = t("2026-10-01T00:00:00Z");
		let owner = Uuid::now_v7();
		let user = User {
			user_id: owner,
			email: "owner@localhost".into(),
			name: "owner".into(),
		};
		p.user_seen(&user, now).await.unwrap();
		let gmail = p.add_gmail(owner, "a@gmail.com", now).await.unwrap().unwrap().id;
		let mut gmbs = Vec::new();
		for (place, name) in [
			("ChIJLU7jZClu5kcR4PcOOO6p3I0", "Plomberie Dupont - Lyon"),
			("ChIJD7fiBh9u5kcRYJSMaMOCCwQ", "Plomberie Dupont - Lyon, Est"),
		] {
			let new = Existing {
				gmail_id: gmail,
				place: place.to_owned(),
				state: sa_core::gbp::OnGoogle::Verifying,
				name: Some(name.to_owned()),
				country: Some(Country::FR),
				trade: None,
			};
			gmbs.push(p.add_gmb(Some(Spender::new(owner)), new, now).await.unwrap().unwrap().id);
		}
		let sealed = DataKey::from_hex(&hex).unwrap().seal(&gmail_imap_aad(gmail), b"app-password").unwrap();
		sqlx::query("INSERT INTO gmail_imap (gmail_id, host, username, password_sealed, data_key_fp, set_at, checked_at) VALUES ($1, 'imap.gmail.com', 'a@gmail.com', $2, $3, 0, 0)")
			.bind(gmail)
			.bind(sealed)
			.bind(DataKey::from_hex(&hex).unwrap().fingerprint().to_vec())
			.execute(&db.pool().await)
			.await
			.unwrap();
		Self {
			db,
			p,
			owner,
			gmail,
			lyon: gmbs[0],
			est: gmbs[1],
			inbox: Inbox::default(),
			outbox,
		}
	}

	/// `m` arrives, and the follower reads it.
	async fn arrives(&self, mut m: Mail) {
		{
			let mut mails = self.inbox.mails.lock().unwrap();
			m.uid = u32::try_from(mails.len()).unwrap() + 1;
			m.message = 1_800_000_000_000_000_000 + u64::from(m.uid);
			m.message_id = Some(format!("m{}@mail", m.uid));
			mails.push(m);
		}
		let now = self.inbox.mails.lock().unwrap().last().unwrap().date;
		self.p.follow_inbox(self.gmail, &self.inbox, now).await.unwrap();
	}

	/// The one message unread read as `kind`, as the model would.
	async fn reads(&self, now: &str, kind: Kind, with: impl Fn(&mut Reading)) {
		let unread = self.p.unread_mail(self.owner, t(now)).await.unwrap();
		let [m] = unread.as_slice() else { panic!("{} unread", unread.len()) };
		let mut reading = Reading {
			topic: Topic::Verification,
			kind,
			gmb: None,
			summary: "said".into(),
			asks: vec![],
			reasons: vec![],
			deadline: None,
			answer_within_hours: None,
		};
		with(&mut reading);
		self.p.mail_read(m.mail, reading, &m.gmbs, t(now)).await.unwrap();
	}

	async fn cases(&self, now: &str) -> Vec<Case> {
		let mut cases = self.p.mail_cases(self.owner, t(now)).await.unwrap();
		cases.sort_by_key(|c| c.id);
		cases
	}

	async fn only(&self, now: &str) -> Case {
		let cases = self.cases(now).await;
		assert_eq!(cases.len(), 1, "{cases:#?}");
		cases.into_iter().next().unwrap()
	}

	async fn drafts(&self, case: &Case, needs: &[&str], asked_for: bool, now: &str) -> i64 {
		let written = Written::Reply {
			body: "The recorded video does not work for this profile: enable the live call.".into(),
			needs: needs.iter().map(|s| (*s).to_owned()).collect(),
		};
		self.p
			.draft_mail(self.owner, case.id, case.last().said.id, written, asked_for, t(now))
			.await
			.unwrap()
			.expect("nothing came meanwhile");
		self.only(now).await.draft.unwrap().id
	}

	fn sent(&self) -> Vec<Outgoing> {
		self.outbox.sent.lock().unwrap().clone()
	}
}

fn support(at: &str, thread: u64, subject: &str, text: &str) -> Mail {
	Mail {
		uid: 0,
		message: 0,
		thread,
		date: t(at),
		subject: Some(subject.to_owned()),
		counterparts: vec![SUPPORT.to_owned()],
		from: Some(SUPPORT.to_owned()),
		reply_to: None,
		to: vec!["a@gmail.com".to_owned()],
		message_id: None,
		refs: vec![],
		authentic: true,
		sent: false,
		text: text.to_owned(),
	}
}

fn notice(at: &str, thread: u64, subject: &str, text: &str) -> Mail {
	Mail {
		counterparts: vec![format!("Google Business Profile <{NOTICE}>")],
		from: Some(NOTICE.to_owned()),
		..support(at, thread, subject, text)
	}
}

fn ours(at: &str, thread: u64, subject: &str, to: &str, refs: &[&str], text: &str) -> Mail {
	Mail {
		counterparts: vec![to.to_owned()],
		from: Some("a@gmail.com".to_owned()),
		to: vec![to.to_owned()],
		refs: refs.iter().map(|s| (*s).to_owned()).collect(),
		authentic: false,
		sent: true,
		..support(at, thread, subject, text)
	}
}

fn rejected(of: &str, listing: &str) -> String {
	format!(
		"Your verification failed. Submit a new video.\n\nYour verification video{of} didn’t meet\nall of the requirements.\n\nSubmit new video  \n<https://business.google.com/verify/l/{listing}?trk=https%3A%2F%2Fc.gle%2FAbc>\n\nNo view of surrounding area\n"
	)
}

#[tokio::test]
async fn rejected_videos_are_attempts_of_the_gbp_named_or_known_by_its_link() {
	let w = World::new().await;
	let now = "2026-10-08T12:00:00Z";
	// Gmail threads the notices of every profile together
	for (at, text) in [
		("2026-10-05T09:00:00Z", rejected("", "15597583944401176782")),
		(
			"2026-10-06T09:00:00Z",
			rejected(" for Plomberie Dupont -  \nLyon, Est (at 9 Av. Basse, 69003 Lyon)", "15597583944401176782"),
		),
		("2026-10-07T09:00:00Z", rejected("", "00284230971258484020")),
	] {
		w.arrives(notice(at, 0xaa, REJECTED, &text)).await;
		w.reads(now, Kind::VideoRejected, |r| r.reasons = vec!["No view of surrounding area".into()]).await;
	}
	w.arrives(ours("2026-10-07T10:00:00Z", 0xaa, &format!("Re: {REJECTED}"), NOTICE, &["m2@mail"], "We need a live call"))
		.await;
	let cases = w.cases(now).await;
	assert_eq!(
		cases.iter().map(|c| c.gmb).collect::<Vec<_>>(),
		[Some(w.est), Some(w.est), None],
		"by its link, by its name, unknown yet"
	);
	assert_eq!(cases[1].messages.len(), 2, "our answer is with what it answers");
	let rejection = || State::NeedsYou {
		why: Why::Kind { kind: Kind::VideoRejected },
	};
	assert_eq!(
		cases.iter().map(|c| c.state.clone()).collect::<Vec<_>>(),
		[State::Done, rejection(), rejection()],
		"the newer one stands, and nobody read our answer"
	);
	let attempts: Vec<_> = cases
		.iter()
		.filter(|c| c.gmb == Some(w.est))
		.flat_map(|c| &c.messages)
		.filter_map(|m| attempt(m.said.reading.as_ref()?.kind))
		.collect();
	assert_eq!(attempts, [(Method::Video, Outcome::Rejected); 2]);

	w.arrives(notice(
		"2026-10-08T09:00:00Z",
		0xab,
		"Your profile",
		"Plomberie Dupont - Lyon <https://business.google.com/n/77/profile?trk=x>\nverify <https://business.google.com/settings>",
	))
	.await;
	w.reads(now, Kind::ChangedByGoogle, |r| r.gmb = Some(w.est)).await;
	assert_eq!(w.cases(now).await[3].gmb, Some(w.lyon), "what it names goes before what its reader saw");
	let third = w.cases(now).await[2].id;
	let edit = CaseEdit {
		gmb: Some(Some(w.lyon)),
		topic: None,
		done: None,
	};
	w.p.edit_mail_case(w.owner, w.owner, third, edit, t(now)).await.unwrap().unwrap();
	assert_eq!(w.cases(now).await[2].gmb, Some(w.lyon), "by hand");
}

#[tokio::test]
async fn a_support_conversation_goes_through_its_turns_over_two_threads() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request to Google";
	w.arrives(support(
		"2026-10-01T09:00:00Z",
		1,
		request,
		"To continue, follow the on-screen prompts. You can reply to this email.",
	))
	.await;
	assert_eq!(w.only("2026-10-01T09:01:00Z").await.state, State::NeedsYou { why: Why::Unread { error: None } });
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	assert!(case.due);
	let draft = w.drafts(&case, &[], false, "2026-10-01T09:03:00Z").await;
	assert_eq!(w.only("2026-10-01T09:03:00Z").await.state, State::ReplyReady, "automatic replies are off by default");
	w.p.send_held_mail(t("2026-10-01T12:00:00Z")).await.unwrap();
	assert!(w.sent().is_empty(), "nothing goes without its operator");
	w.p.send_mail_draft(w.owner, w.owner, draft, "Please enable the live call.", vec![], t("2026-10-01T12:00:00Z"))
		.await
		.unwrap()
		.unwrap();
	let [sent] = <[Outgoing; 1]>::try_from(w.sent()).unwrap();
	assert_eq!(
		(sent.to.as_str(), sent.subject.as_str(), sent.body.as_str()),
		(SUPPORT, "Re: [1-1234000056789] Your Request to Google", "Please enable the live call.")
	);
	assert_eq!((sent.in_reply_to.as_deref(), sent.references), (Some("m1@mail"), vec!["m1@mail".to_owned()]));
	assert_eq!(
		w.only("2026-10-01T12:01:00Z").await.state,
		State::Waiting { until: t("2026-10-03T12:00:00Z") },
		"sent, and not in the inbox yet"
	);

	let quoted = "Please enable the live call.\n\nOn Thu, Oct 1, 2026 at 9:00 AM <googlebusinessprofile-support@google.com>\nwrote:\n\n> To continue, follow the prompts.\n";
	w.arrives(ours("2026-10-01T12:00:00Z", 1, &format!("Re: {request}"), SUPPORT, &["m1@mail"], quoted)).await;
	let case = w.only("2026-10-01T12:02:00Z").await;
	let brief = w.p.mail_case(w.owner, case.id, t("2026-10-01T12:02:00Z")).await.unwrap().unwrap();
	assert_eq!(brief.texts[1].as_str(), "Please enable the live call.", "its quote is dropped");

	w.arrives(support(
		"2026-10-02T09:00:00Z",
		1,
		request,
		"I'm routing your query to a specialist. You should hear from us in 24-48Hrs.",
	))
	.await;
	w.reads("2026-10-02T09:01:00Z", Kind::Routed, |r| r.answer_within_hours = Some(48)).await;
	assert_eq!(w.only("2026-10-02T09:02:00Z").await.state, State::Waiting { until: t("2026-10-04T09:00:00Z") });
	assert!(w.only("2026-10-04T09:01:00Z").await.due, "silent past its word");

	let followed = "Follow up to Business Profile Query [1-1234000056789] ";
	w.arrives(support(
		"2026-10-03T09:00:00Z",
		2,
		followed,
		"This email is in reference to “Plomberie\nDupont - Lyon, Est, 9 Av. Basse”. Please share a picture of the permanent signage.",
	))
	.await;
	w.reads("2026-10-03T09:01:00Z", Kind::Asks, |r| r.asks = vec!["a picture of the permanent signage".into()]).await;
	let case = w.only("2026-10-03T09:02:00Z").await;
	assert_eq!((case.gmb, case.topic, case.due), (Some(w.est), Topic::Verification, true), "one case over two threads");
	w.p.set_mail_auto(w.owner, w.gmail, Topic::Verification, true).await.unwrap().unwrap();
	let draft = w.drafts(&case, &["a photo of the signage"], false, "2026-10-03T09:03:00Z").await;
	let needs = State::NeedsYou {
		why: Why::Needs {
			needs: vec!["a photo of the signage".into()],
		},
	};
	assert_eq!(w.only("2026-10-03T12:00:00Z").await.state, needs, "what it lacks is its operator's, switched on or not");
	w.p.send_held_mail(t("2026-10-04T12:00:00Z")).await.unwrap();
	assert_eq!(w.sent().len(), 1);
	let sign = File {
		name: "sign.jpg".into(),
		kind: "image/jpeg".into(),
		bytes: vec![1; 10],
	};
	let huge = File {
		bytes: vec![0; (18 << 20) + 1],
		..sign.clone()
	};
	let at = t("2026-10-03T13:00:00Z");
	assert!(matches!(
		w.p.send_mail_draft(w.owner, w.owner, draft, "Attached.", vec![huge], at).await.unwrap(),
		Err(Refused::TooLarge(_))
	));
	w.p.send_mail_draft(w.owner, w.owner, draft, "Attached.", vec![sign], at).await.unwrap().unwrap();
	assert_eq!(w.sent()[1].files.len(), 1);

	w.arrives(support("2026-10-06T09:00:00Z", 2, followed, "I've provided you with an option for Live video call."))
		.await;
	w.reads("2026-10-06T09:01:00Z", Kind::CallOffered, |_| {}).await;
	assert_eq!(
		w.only("2026-10-06T09:02:00Z").await.state,
		State::NeedsYou {
			why: Why::Kind { kind: Kind::CallOffered }
		}
	);
	// the rejection of a video meanwhile is the conversation's to settle
	w.arrives(notice("2026-10-06T10:00:00Z", 9, REJECTED, &rejected(" for Plomberie Dupont - Lyon, Est", "1559")))
		.await;
	w.reads("2026-10-06T10:01:00Z", Kind::VideoRejected, |_| {}).await;
	assert_eq!(w.cases("2026-10-06T10:02:00Z").await[1].state, State::Quiet);
	w.arrives(notice("2026-10-07T10:00:00Z", 10, "Your profile is verified", "Plomberie Dupont - Lyon, Est is verified"))
		.await;
	w.reads("2026-10-07T10:01:00Z", Kind::Verified, |_| {}).await;
	let states: Vec<_> = w.cases("2026-10-07T10:02:00Z").await.into_iter().map(|c| c.state).collect();
	assert_eq!(states, [State::Done, State::Done, State::Done]);
}

#[tokio::test]
async fn a_switched_on_reply_is_held_then_goes_unless_another_message_comes() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request to Google";
	assert!(matches!(w.p.set_mail_auto(w.owner, w.gmail, Topic::Suspension, true).await.unwrap(), Err(Refused::Invalid(_))));
	w.p.set_mail_auto(w.owner, w.gmail, Topic::Verification, true).await.unwrap().unwrap();
	assert_eq!(w.p.mail_auto(w.owner).await.unwrap(), [(w.gmail, Topic::Verification)]);
	w.arrives(support("2026-10-01T09:00:00Z", 1, request, "Follow the prompts.")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	w.drafts(&case, &[], false, "2026-10-01T09:05:00Z").await;
	assert_eq!(w.only("2026-10-01T09:06:00Z").await.state, State::Sending { at: t("2026-10-01T09:20:00Z") });
	w.p.send_held_mail(t("2026-10-01T09:19:00Z")).await.unwrap();
	assert!(w.sent().is_empty(), "held for a quarter of an hour");
	assert_eq!(w.p.send_held_mail(t("2026-10-01T09:20:00Z")).await.unwrap(), 1);
	assert_eq!(w.sent().len(), 1);
	assert_eq!(w.p.send_held_mail(t("2026-10-01T09:21:00Z")).await.unwrap(), 0, "once");

	w.arrives(support("2026-10-02T09:00:00Z", 1, request, "Follow the prompts again.")).await;
	w.reads("2026-10-02T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-02T09:02:00Z").await;
	let held = w.drafts(&case, &[], false, "2026-10-02T09:05:00Z").await;
	// its operator answered in Gmail meanwhile
	w.arrives(ours("2026-10-02T09:10:00Z", 1, &format!("Re: {request}"), SUPPORT, &["m2@mail"], "We need the live call"))
		.await;
	assert_eq!(w.p.send_held_mail(t("2026-10-02T10:00:00Z")).await.unwrap(), 0);
	assert_eq!(w.sent().len(), 1, "superseded");
	assert!(matches!(
		w.p.send_mail_draft(w.owner, w.owner, held, "x", vec![], t("2026-10-02T10:00:00Z")).await.unwrap(),
		Err(Refused::Conflict(_))
	));

	w.arrives(support("2026-10-03T09:00:00Z", 1, request, "Follow the prompts once more.")).await;
	w.reads("2026-10-03T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-03T09:02:00Z").await;
	w.drafts(&case, &[], false, "2026-10-03T09:05:00Z").await;
	w.p.set_mail_auto(w.owner, w.gmail, Topic::Verification, false).await.unwrap().unwrap();
	assert_eq!(w.p.send_held_mail(t("2026-10-03T10:00:00Z")).await.unwrap(), 0);
	assert_eq!(w.only("2026-10-03T10:00:00Z").await.state, State::ReplyReady, "switched off since: its operator's again");
	assert_eq!(w.sent().len(), 1);
}

#[tokio::test]
async fn a_look_alike_is_an_impersonation_and_is_never_answered() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request to Google";
	w.arrives(support("2026-10-01T09:00:00Z", 1, request, "Follow the prompts.")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let forged = Mail {
		authentic: false,
		..support("2026-10-01T10:00:00Z", 1, request, "Send us your password")
	};
	w.arrives(forged).await;
	w.arrives(Mail {
		counterparts: vec!["Google Business Profile <support@gbp-verify.io>".into()],
		from: Some("support@gbp-verify.io".into()),
		authentic: false,
		..support("2026-10-01T10:05:00Z", 4, "Verify now", "Plomberie Dupont - Lyon: pay here")
	})
	.await;
	// From the gmail itself, which did not send it
	w.arrives(Mail {
		counterparts: vec!["a@gmail.com".into()],
		sent: false,
		..ours("2026-10-01T10:10:00Z", 1, &format!("Re: {request}"), SUPPORT, &["m1@mail"], "Never mind, close it")
	})
	.await;
	let now = "2026-10-01T11:00:00Z";
	let cases = w.cases(now).await;
	assert_eq!((cases.len(), cases[0].messages.len()), (3, 1), "it joins no case, whatever it carries");
	assert_eq!(cases[0].state, State::ReplyDue, "nor quiets one");
	for c in &cases[1..] {
		let read = c.messages[0].said.reading.as_ref().unwrap();
		assert_eq!(
			(read.kind, c.messages[0].read_by, c.gmb, &c.state, c.due),
			(Kind::Impersonation, Some(ReadBy::Rule), None, &State::Quiet, false)
		);
		let written = Written::Reply { body: "x".into(), needs: vec![] };
		w.p.draft_mail(w.owner, c.id, c.last().said.id, written, true, t(now)).await.unwrap().unwrap();
		let draft = w.cases(now).await.into_iter().find(|x| x.id == c.id).unwrap().draft.unwrap().id;
		assert!(matches!(
			w.p.send_mail_draft(w.owner, w.owner, draft, "x", vec![], t(now)).await.unwrap(),
			Err(Refused::Invalid(_))
		));
	}
	assert!(w.p.unread_mail(w.owner, t(now)).await.unwrap().is_empty(), "nor does the model read it");
	assert!(w.sent().is_empty());
}

#[tokio::test]
async fn a_reply_goes_nowhere_but_a_google_address_somebody_reads() {
	for reply_to in ["sales@evil.io", "x@mail.google.com", "businessprofile-noreply@google.com"] {
		let w = World::new().await;
		w.arrives(Mail {
			reply_to: Some(reply_to.into()),
			..support("2026-10-01T09:00:00Z", 1, "[1-1234000056789] Your Request", "Follow the prompts.")
		})
		.await;
		w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
		let case = w.only("2026-10-01T09:02:00Z").await;
		let draft = w.drafts(&case, &[], true, "2026-10-01T09:03:00Z").await;
		let refused = w.p.send_mail_draft(w.owner, w.owner, draft, "x", vec![], t("2026-10-01T09:04:00Z")).await.unwrap();
		assert!(matches!(refused, Err(Refused::Invalid(_))), "{reply_to}: {refused:?}");
		assert!(w.sent().is_empty(), "{reply_to}");
	}
}

#[tokio::test]
async fn a_draft_left_sending_is_never_tried_again_and_one_refused_may_be() {
	let w = World::new().await;
	w.arrives(support("2026-10-01T09:00:00Z", 1, "[1-1234000056789] Your Request", "Follow the prompts.")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	let draft = w.drafts(&case, &[], true, "2026-10-01T09:03:00Z").await;
	*w.outbox.answers.lock().unwrap() = vec![Err(SendError::NotSent("535 bad credentials".into())), Err(SendError::Unknown("timed out".into()))];
	let send = |at: &'static str| w.p.send_mail_draft(w.owner, w.owner, draft, "x", vec![], t(at));
	assert!(matches!(send("2026-10-01T09:04:00Z").await.unwrap(), Err(Refused::Unavailable(_))));
	assert_eq!(
		w.only("2026-10-01T09:04:00Z").await.state,
		State::NeedsYou {
			why: Why::Failed {
				error: "535 bad credentials".into()
			}
		}
	);
	assert!(
		matches!(send("2026-10-01T09:05:00Z").await.unwrap(), Err(Refused::Unavailable(_))),
		"tried again, its end unknown"
	);
	assert_eq!(w.sent().len(), 2);
	assert!(matches!(send("2026-10-01T09:06:00Z").await.unwrap(), Err(Refused::Conflict(_))), "and never after");
	assert!(matches!(w.p.edit_mail_draft(w.owner, w.owner, draft, "y").await.unwrap(), Err(Refused::Conflict(_))));
	assert!(matches!(w.p.dismiss_mail_draft(w.owner, w.owner, draft).await.unwrap(), Err(Refused::Conflict(_))));
	assert_eq!(w.p.send_held_mail(t("2026-10-09T09:00:00Z")).await.unwrap(), 0);
	assert_eq!(w.sent().len(), 2);
	assert_eq!(w.only("2026-10-01T10:00:00Z").await.state, State::NeedsYou { why: Why::MayHaveGoneOut });
	let again = w.drafts(&w.only("2026-10-01T10:00:00Z").await, &[], true, "2026-10-01T10:01:00Z").await;
	assert_ne!(again, draft, "a rewrite is another draft");
	assert_eq!(w.only("2026-10-01T10:02:00Z").await.state, State::ReplyReady);
}

#[tokio::test]
async fn a_turn_to_a_person_is_told_once_to_the_owners_of_the_gbp_else_of_the_gmails() {
	let w = World::new().await;
	let now = t("2026-10-05T10:00:00Z");
	let pool = w.db.pool().await;
	w.p.alerts_access_seen(w.owner, &sa_panel::testing::operator(), "owner", now).await.unwrap();
	sqlx::query(
		"INSERT INTO alert_destinations (id, user_id, method, private, urgent, important, info, filter, created_at) \
		 VALUES (1, $1, 'telegram', 1, 'now', 'now', 'off', '{\"gmails\":[],\"trades\":[],\"brands\":[],\"gmbs\":[]}', 0)",
	)
	.bind(w.owner)
	.execute(&pool)
	.await
	.unwrap();
	sqlx::query("INSERT INTO telegram_destinations (destination_id, kind, chat_id) VALUES (1, 'private', 42)")
		.execute(&pool)
		.await
		.unwrap();
	let tell = async |at: &str| w.p.tell_mail(Locale::En, t(at)).await.unwrap();
	w.arrives(notice("2026-09-20T09:00:00Z", 7, REJECTED, &rejected(" for Plomberie Dupont - Lyon", "15"))).await;
	w.reads("2026-10-05T10:00:00Z", Kind::VideoRejected, |_| {}).await;
	assert_eq!(tell("2026-10-05T10:00:00Z").await, 0, "old news is not told");
	w.arrives(support("2026-10-05T09:00:00Z", 1, "[1-1234000056789] Your Request", "Follow the prompts.")).await;
	assert_eq!(tell("2026-10-05T10:00:00Z").await, 0, "not read yet");
	w.reads("2026-10-05T10:00:00Z", Kind::SelfServe, |_| {}).await;
	assert_eq!(tell("2026-10-05T10:00:00Z").await, 0, "a reply is being written");
	let case = w.cases("2026-10-05T10:00:00Z").await.remove(1);
	assert_eq!(case.gmb, None, "it names none: the gmail's owners are told");
	let written = || Written::Reply { body: "x".into(), needs: vec![] };
	w.p.draft_mail(w.owner, case.id, case.last().said.id, written(), false, now).await.unwrap().unwrap();
	assert_eq!(tell("2026-10-05T10:01:00Z").await, 1);
	assert_eq!(tell("2026-10-05T10:02:00Z").await, 0, "once");
	w.p.draft_mail(w.owner, case.id, case.last().said.id, written(), true, now).await.unwrap().unwrap();
	assert_eq!(tell("2026-10-05T10:03:00Z").await, 0, "nor again for a rewrite");
	w.arrives(notice("2026-10-05T11:00:00Z", 8, "your profile has been suspended", "Plomberie Dupont - Lyon, Est is suspended"))
		.await;
	w.reads("2026-10-05T11:01:00Z", Kind::Suspended, |_| {}).await;
	assert_eq!(tell("2026-10-05T11:02:00Z").await, 0, "its owner's access is an hour old");
	w.p.alerts_access_seen(w.owner, &sa_panel::testing::operator(), "owner", t("2026-10-05T11:02:00Z")).await.unwrap();
	w.arrives(notice("2026-10-05T11:30:00Z", 9, "Is it still open?", "Plomberie Dupont - Lyon, Est")).await;
	w.reads("2026-10-05T11:31:00Z", Kind::StillActive, |r| r.deadline = Some(t("2026-10-09T00:00:00Z"))).await;
	assert_eq!(tell("2026-10-05T11:32:00Z").await, 1);
	let inbox = async || w.p.alerts_inbox(w.owner, &sa_panel::testing::operator(), &Sought::default()).await.unwrap();
	let linked: Vec<_> = inbox().await.into_iter().map(|a| a.link.unwrap()).collect();
	let cases = w.cases("2026-10-05T11:32:00Z").await;
	assert!(linked.iter().all(|l| cases.iter().any(|c| *l == format!("/email?case={}", c.id))), "{linked:?}");

	w.p.set_mail_auto(w.owner, w.gmail, Topic::Verification, true).await.unwrap().unwrap();
	w.arrives(support("2026-10-05T11:40:00Z", 1, "[1-1234000056789] Your Request", "Follow the prompts again.")).await;
	w.reads("2026-10-05T11:41:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.cases("2026-10-05T11:41:00Z").await.into_iter().find(|c| c.id == case.id).unwrap();
	w.p.draft_mail(w.owner, case.id, case.last().said.id, written(), false, t("2026-10-05T11:42:00Z"))
		.await
		.unwrap()
		.unwrap();
	let before = inbox().await.len();
	tell("2026-10-05T11:43:00Z").await;
	tell("2026-10-05T11:44:00Z").await;
	let after = inbox().await;
	assert_eq!(after.len(), before + 1, "a held draft, once");
	assert!(after[0].text.text.contains("a reply goes by itself 2026-10-05 11:57 UTC"), "{}", after[0].text.text);
	assert_eq!(after[0].tier, Tier::Important, "its quarter of an hour runs");
	assert_eq!(after[0].link, Some(format!("/email?case={}", case.id)));

	w.arrives(support("2026-10-05T11:50:00Z", 1, "[1-1234000056789] Your Request", "A specialist answers in 24-48Hrs."))
		.await;
	w.reads("2026-10-05T11:51:00Z", Kind::Routed, |r| r.answer_within_hours = Some(48)).await;
	tell("2026-10-05T11:52:00Z").await;
	tell("2026-10-05T11:53:00Z").await;
	let routed = inbox().await;
	assert_eq!((routed.len(), routed[0].tier), (after.len() + 1, Tier::Info), "passed on, once");
	assert!(
		routed[0].text.text.contains("passed on to a specialist, an answer by 2026-10-07 11:50 UTC"),
		"{}",
		routed[0].text.text
	);
}

#[tokio::test]
async fn by_itself_goes_what_presses_never_what_google_asked_for() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request to Google";
	w.p.set_mail_auto(w.owner, w.gmail, Topic::Verification, true).await.unwrap().unwrap();
	let send = async |at: &str| w.p.send_held_mail(t(at)).await.unwrap();

	w.arrives(support("2026-10-01T09:00:00Z", 1, request, "How many people work there?")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::Asks, |r| r.asks = vec!["how many people work there".into()]).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	assert!(case.due, "a reply is written all the same");
	let draft = w.drafts(&case, &[], false, "2026-10-01T09:03:00Z").await;
	assert_eq!(
		w.only("2026-10-01T10:00:00Z").await.state,
		State::NeedsYou {
			why: Why::Kind { kind: Kind::Asks }
		}
	);
	assert_eq!(send("2026-10-01T10:00:00Z").await, 0, "lacking nothing, and still its operator's");
	w.p.send_mail_draft(w.owner, w.owner, draft, "Two.", vec![], t("2026-10-01T10:00:00Z")).await.unwrap().unwrap();

	w.arrives(support("2026-10-01T11:00:00Z", 1, request, "A specialist answers in 24-48Hrs.")).await;
	w.reads("2026-10-01T11:01:00Z", Kind::Routed, |r| r.answer_within_hours = Some(48)).await;
	assert!(!w.only("2026-10-03T10:59:00Z").await.due, "its 48 hours are waited out");
	let case = w.only("2026-10-03T11:00:00Z").await;
	assert!(case.due);
	w.drafts(&case, &[], false, "2026-10-03T11:00:00Z").await;
	assert_eq!(send("2026-10-03T11:15:00Z").await, 1, "silent past them: pressed by itself");
	w.arrives(ours("2026-10-03T11:15:00Z", 1, &format!("Re: {request}"), SUPPORT, &["m2@mail"], "The call is needed."))
		.await;
	assert_eq!(w.only("2026-10-05T11:14:00Z").await.state, State::Waiting { until: t("2026-10-05T11:15:00Z") });
	let silent = w.only("2026-10-05T11:16:00Z").await;
	assert_eq!((silent.state, silent.due), (State::NeedsYou { why: Why::AskedOften }, false), "once");
}

#[tokio::test]
async fn a_case_is_not_pressed_once_its_gbp_is_verified_nor_when_it_is_old() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request to Google";
	w.arrives(support("2026-10-01T09:00:00Z", 1, request, "Plomberie Dupont - Lyon: follow the prompts.")).await;
	w.arrives(notice("2026-10-01T10:00:00Z", 2, REJECTED, &rejected(" for Plomberie Dupont - Lyon, Est", "15"))).await;
	for m in w.p.unread_mail(w.owner, t("2026-10-01T11:00:00Z")).await.unwrap() {
		let kind = if m.from == SUPPORT { Kind::SelfServe } else { Kind::VideoRejected };
		let reading = Reading {
			topic: Topic::Verification,
			kind,
			gmb: None,
			summary: "said".into(),
			asks: vec![],
			reasons: vec![],
			deadline: None,
			answer_within_hours: None,
		};
		w.p.mail_read(m.mail, reading, &m.gmbs, t("2026-10-01T11:00:00Z")).await.unwrap();
	}
	assert!(w.cases("2026-10-02T09:00:00Z").await[0].due);
	let old = w.cases("2026-10-23T09:00:00Z").await;
	assert_eq!((&old[0].state, old[0].due), (&State::Quiet, false), "first read long after");
	for gmb in [w.lyon, w.est] {
		w.p.advance(Some(Spender::new(w.owner)), gmb, sa_core::gbp::State::Verified, None).await.unwrap().unwrap();
	}
	let settled = w.cases("2026-10-02T09:00:00Z").await;
	assert!(settled.iter().all(|c| c.state == State::Done && !c.due), "{settled:#?}");
}

#[tokio::test]
async fn a_persons_mail_is_theirs_alone_corrected_by_them_and_gone_with_its_gmail() {
	let w = World::new().await;
	w.arrives(notice(
		"2026-10-01T09:00:00Z",
		1,
		"Jean invited you to manage Plomberie Dupont - Lyon",
		"Jean invited a@gmail.com to manage it",
	))
	.await;
	w.arrives(Mail {
		counterparts: vec!["Google Calendar <calendar-notification@google.com>".into()],
		from: Some("calendar-notification@google.com".into()),
		..support("2026-10-01T09:30:00Z", 2, "Invitation", "Lunch")
	})
	.await;
	let now = "2026-10-01T10:00:00Z";
	let case = w.only(now).await;
	assert_eq!(
		case.messages[0].said.reading.as_ref().unwrap().kind,
		Kind::Invited,
		"by rule; the calendar's is kept nowhere but the log"
	);
	assert_eq!((case.gmb, &case.state), (Some(w.lyon), &State::Quiet));

	let other = Uuid::now_v7();
	let user = User {
		user_id: other,
		email: "other@localhost".into(),
		name: "other".into(),
	};
	w.p.user_seen(&user, t(now)).await.unwrap();
	assert!(w.p.mail_cases(other, t(now)).await.unwrap().is_empty());
	assert!(matches!(w.p.mail_case(other, case.id, t(now)).await.unwrap(), Err(Refused::NotFound)));
	let edit = || CaseEdit {
		gmb: None,
		topic: None,
		done: Some(true),
	};
	assert!(matches!(w.p.edit_mail_case(other, other, case.id, edit(), t(now)).await.unwrap(), Err(Refused::NotFound)));
	let correction = || ReadingEdit {
		kind: Some(Kind::OwnershipRequested),
		topic: None,
		gmb: Some(Some(w.est)),
		summary: None,
		deadline: Some(Some(t("2026-10-04T00:00:00Z"))),
	};
	let mail = case.messages[0].said.id;
	assert!(matches!(w.p.correct_mail(other, mail, correction(), t(now)).await.unwrap(), Err(Refused::NotFound)));
	assert!(matches!(w.p.set_mail_auto(other, w.gmail, Topic::Reviews, true).await.unwrap(), Err(Refused::NotFound)));

	w.p.correct_mail(w.owner, mail, correction(), t(now)).await.unwrap().unwrap();
	let case = w.only(now).await;
	assert_eq!((case.gmb, case.topic, case.messages[0].read_by), (Some(w.est), Topic::Access, Some(ReadBy::Person)));
	assert_eq!(
		case.state,
		State::NeedsYou {
			why: Why::Kind { kind: Kind::OwnershipRequested }
		}
	);
	let wrong = ReadingEdit {
		topic: Some(Topic::Reviews),
		..correction()
	};
	assert!(matches!(w.p.correct_mail(w.owner, mail, wrong, t(now)).await.unwrap(), Err(Refused::Invalid(_))));
	w.p.edit_mail_case(w.owner, w.owner, case.id, edit(), t(now)).await.unwrap().unwrap();
	assert_eq!(w.only(now).await.state, State::Done);

	let pool = w.db.pool().await;
	let text: Vec<u8> = sqlx::query_scalar("SELECT text_sealed FROM mails").fetch_one(&pool).await.unwrap();
	assert!(!String::from_utf8_lossy(&text).contains("Jean"), "its text is sealed");
	w.p.remove_gmail(w.owner, w.gmail, t(now)).await.unwrap().unwrap();
	assert!(w.cases(now).await.is_empty(), "in the trash");
	w.p.erase_gmail(w.owner, w.gmail, t(now) + SignedDuration::from_mins(1)).await.unwrap().unwrap();
	let left: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM mails) + (SELECT count(*) FROM mail_cases) + (SELECT count(*) FROM mail_drafts)")
		.fetch_one(&pool)
		.await
		.unwrap();
	assert_eq!(left, 0);
}
