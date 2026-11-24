//! Google's mail as cases: what each message joins, whose it is, what its reply may do.

use std::sync::{Arc, Mutex};

use jiff::{SignedDuration, Timestamp};
use sa_alerts::Tier;
use sa_core::{
	gbp::Country,
	mail::{DraftState, Halt, Kind, Method, Outcome, Reading, Said, Settled, State, Topic, Why, attempt},
	notify::Locale,
	prices::Spender,
};
use sa_panel::{
	Panel, Refused,
	alerts::Sought,
	gmbs::Existing,
	import::Export,
	inbox::{Account, Batch, Cursor, Folder, Mail, Mailbox},
	mail::{Case, CaseEdit, File, Outgoing, ReadBy, ReadingEdit, SendError, Sent, SentBy, Written},
	seal::{DataKey, gmail_imap_aad},
	testing::{Outbox, TestDb, panel_sending},
	tokens::User,
};
use uuid::Uuid;

const SUPPORT: &str = "googlebusinessprofile-support@google.com";
const NOTICE: &str = "businessprofile-noreply@google.com";
const DONE: State = State::Done { by: Settled::Here };

const REJECTED: &str = "Action required: Submit a new verification video";

fn t(s: &str) -> Timestamp {
	s.parse().unwrap()
}

/// All Mail and Spam as Gmail would answer them.
#[derive(Default)]
struct Inbox {
	mails: Mutex<Vec<Mail>>,
}

impl Mailbox for Inbox {
	async fn read(&self, _: Account<'_>, folder: Folder, after: Option<Cursor>, limit: usize) -> eyre::Result<Batch> {
		let mails = self.mails.lock().unwrap();
		let there = |m: &&Mail| m.spam == (folder == Folder::Spam) && m.uid > after.map_or(0, |c| c.uid);
		Ok(Batch {
			validity: 7,
			mails: mails.iter().filter(there).take(limit).cloned().collect(),
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
		self.p.follow_inbox(self.gmail, &self.inbox, t(now)).await.unwrap();
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
		self.p.draft_mail(self.owner, case, written, asked_for, t(now)).await.unwrap().expect("nothing came meanwhile");
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
		whole: true,
		spam: false,
	}
}

fn bare(body: &str) -> Sent<'_> {
	Sent {
		body,
		files: vec![],
		documents: &[],
		as_is: false,
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
		("2026-10-05T09:00:00Z", rejected("", "10000000000000000001")),
		(
			"2026-10-06T09:00:00Z",
			rejected(" for Plomberie Dupont -  \nLyon, Est (at 9 Av. Basse, 69003 Lyon)", "10000000000000000001"),
		),
		("2026-10-07T09:00:00Z", rejected("", "00000000000000000002")),
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
		[
			State::Done {
				by: Settled::Elsewhere(Said {
					kind: Kind::VideoRejected,
					at: t("2026-10-06T09:00:00Z"),
					case_id: Some(cases[1].id)
				})
			},
			rejection(),
			rejection()
		],
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
		join: None,
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
	w.p.send_mail_draft(w.owner, w.owner, draft, bare("Please enable the live call."), t("2026-10-01T12:00:00Z"))
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
	assert_eq!(brief.texts[1].as_ref().unwrap().as_str(), "Please enable the live call.", "its quote is dropped");

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
	w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, true).await.unwrap().unwrap();
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
	let with = |f: File| Sent {
		files: vec![f],
		..bare("Attached.")
	};
	assert!(matches!(
		w.p.send_mail_draft(w.owner, w.owner, draft, with(huge), at).await.unwrap(),
		Err(Refused::TooLarge(_))
	));
	assert!(
		matches!(w.p.send_mail_draft(w.owner, w.owner, draft, bare("Attached."), at).await.unwrap(), Err(Refused::Conflict(_))),
		"it lacks a photo"
	);
	w.p.send_mail_draft(w.owner, w.owner, draft, with(sign), at).await.unwrap().unwrap();
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
	let settled = w.cases("2026-10-07T10:02:00Z").await;
	let verified = State::Done {
		by: Settled::Elsewhere(Said {
			kind: Kind::Verified,
			at: t("2026-10-07T10:00:00Z"),
			case_id: Some(settled[2].id),
		}),
	};
	assert_eq!(settled.iter().map(|c| c.state.clone()).collect::<Vec<_>>(), [verified.clone(), verified, DONE]);
}

#[tokio::test]
async fn a_switched_on_reply_is_held_then_goes_unless_another_message_comes() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request to Google";
	assert!(matches!(w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Suspension, true).await.unwrap(), Err(Refused::Invalid(_))));
	w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, true).await.unwrap().unwrap();
	assert_eq!(w.p.mail_auto(w.owner).await.unwrap().on, [(w.lyon, Topic::Verification), (w.est, Topic::Verification)]);
	w.arrives(support("2026-10-01T09:00:00Z", 1, request, "Plomberie Dupont - Lyon: follow the prompts.")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	w.drafts(&case, &[], false, "2026-10-01T09:05:00Z").await;
	assert_eq!(w.only("2026-10-01T09:06:00Z").await.state, State::Sending { at: t("2026-10-01T09:20:00Z") });
	w.p.send_held_mail(t("2026-10-01T09:19:00Z")).await.unwrap();
	assert!(w.sent().is_empty(), "held for a quarter of an hour");
	assert_eq!(w.p.send_held_mail(t("2026-10-01T09:20:00Z")).await.unwrap(), 0, "its inbox not read past its moment");
	w.only("2026-10-01T09:20:00Z").await;
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
		w.p.send_mail_draft(w.owner, w.owner, held, bare("x"), t("2026-10-02T10:00:00Z")).await.unwrap(),
		Err(Refused::Conflict(_))
	));

	w.arrives(support("2026-10-03T09:00:00Z", 1, request, "Follow the prompts once more.")).await;
	w.reads("2026-10-03T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-03T09:02:00Z").await;
	w.drafts(&case, &[], false, "2026-10-03T09:05:00Z").await;
	w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, false).await.unwrap().unwrap();
	w.only("2026-10-03T09:30:00Z").await;
	assert_eq!(w.p.send_held_mail(t("2026-10-03T09:30:00Z")).await.unwrap(), 0);
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
		w.p.draft_mail(w.owner, c, written, true, t(now)).await.unwrap().unwrap();
		let draft = w.cases(now).await.into_iter().find(|x| x.id == c.id).unwrap().draft.unwrap().id;
		assert!(matches!(
			w.p.send_mail_draft(w.owner, w.owner, draft, bare("x"), t(now)).await.unwrap(),
			Err(Refused::Invalid(_))
		));
	}
	assert!(w.p.unread_mail(w.owner, t(now)).await.unwrap().is_empty(), "nor does the model read it");
	assert!(w.sent().is_empty());
}

#[tokio::test]
async fn a_reply_goes_nowhere_but_a_google_address_somebody_reads() {
	for reply_to in ["sales@evil.io", "x@mail.google.com", "businessprofile-noreply@google.com", "case-1234@google.com"] {
		let w = World::new().await;
		w.arrives(Mail {
			reply_to: Some(reply_to.into()),
			..support("2026-10-01T09:00:00Z", 1, "[1-1234000056789] Your Request", "Follow the prompts.")
		})
		.await;
		w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
		let case = w.only("2026-10-01T09:02:00Z").await;
		let draft = w.drafts(&case, &[], true, "2026-10-01T09:03:00Z").await;
		let refused = w.p.send_mail_draft(w.owner, w.owner, draft, bare("x"), t("2026-10-01T09:04:00Z")).await.unwrap();
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
	let send = |at: &'static str| w.p.send_mail_draft(w.owner, w.owner, draft, bare("x"), t(at));
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
	let case = w.only("2026-10-01T10:00:00Z").await;
	assert_eq!(case.state, State::NeedsYou { why: Why::MayHaveGoneOut });
	let written = Written::Reply { body: "x".into(), needs: vec![] };
	assert!(
		w.p.draft_mail(w.owner, &case, written, true, t("2026-10-01T10:01:00Z")).await.unwrap().is_none(),
		"another would be a second answer"
	);
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
	w.arrives(support("2026-10-05T09:30:00Z", 1, "[1-1234000056789] Your Request", "Follow the prompts.")).await;
	assert_eq!(tell("2026-10-05T10:00:00Z").await, 0, "not read yet");
	w.reads("2026-10-05T10:00:00Z", Kind::SelfServe, |_| {}).await;
	assert_eq!(tell("2026-10-05T10:00:00Z").await, 0, "a reply is being written");
	let case = w.cases("2026-10-05T10:00:00Z").await.remove(1);
	assert_eq!(case.gmb, None, "it names none: the gmail's owners are told");
	let written = || Written::Reply { body: "x".into(), needs: vec![] };
	w.p.draft_mail(w.owner, &case, written(), false, now).await.unwrap().unwrap();
	assert_eq!(tell("2026-10-05T10:01:00Z").await, 1);
	assert_eq!(tell("2026-10-05T10:02:00Z").await, 0, "once");
	w.p.draft_mail(w.owner, &case, written(), true, now).await.unwrap().unwrap();
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

	w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, true).await.unwrap().unwrap();
	w.arrives(support(
		"2026-10-05T11:40:00Z",
		1,
		"[1-1234000056789] Your Request",
		"Plomberie Dupont - Lyon: follow the prompts again.",
	))
	.await;
	w.reads("2026-10-05T11:41:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.cases("2026-10-05T11:41:00Z").await.into_iter().find(|c| c.id == case.id).unwrap();
	w.p.draft_mail(w.owner, &case, written(), false, t("2026-10-05T11:42:00Z")).await.unwrap().unwrap();
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
	w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, true).await.unwrap().unwrap();
	let send = async |at: &str| w.p.send_held_mail(t(at)).await.unwrap();

	w.arrives(support("2026-10-01T09:00:00Z", 1, request, "Plomberie Dupont - Lyon: how many people work there?"))
		.await;
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
	w.p.send_mail_draft(w.owner, w.owner, draft, bare("Two."), t("2026-10-01T10:00:00Z")).await.unwrap().unwrap();

	w.arrives(support("2026-10-01T11:00:00Z", 1, request, "A specialist answers in 24-48Hrs.")).await;
	w.reads("2026-10-01T11:01:00Z", Kind::Routed, |r| r.answer_within_hours = Some(48)).await;
	assert!(!w.only("2026-10-03T10:59:00Z").await.due, "its 48 hours are waited out");
	let case = w.only("2026-10-03T11:00:00Z").await;
	assert!(case.due);
	w.drafts(&case, &[], false, "2026-10-03T11:00:00Z").await;
	w.only("2026-10-03T11:15:00Z").await;
	assert_eq!(send("2026-10-03T11:15:00Z").await, 1, "silent past them: pressed by itself");
	w.arrives(ours("2026-10-03T11:15:00Z", 1, &format!("Re: {request}"), SUPPORT, &["m2@mail"], "The call is needed."))
		.await;
	assert_eq!(w.only("2026-10-05T11:14:00Z").await.state, State::Waiting { until: t("2026-10-05T11:15:00Z") });
	let silent = w.only("2026-10-05T11:16:00Z").await;
	let since = t("2026-10-05T11:15:00Z");
	assert_eq!((silent.state, silent.due), (State::NeedsYou { why: Why::Unanswered { since } }, false), "once");
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
	let moved = |c: &Case| matches!(c.state, State::Done { by: Settled::Elsewhere(s) } if s.kind == Kind::Verified && s.case_id.is_none());
	assert!(settled.iter().all(|c| moved(c) && !c.due), "{settled:#?}");
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
		join: None,
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
	assert!(matches!(w.p.set_mail_auto(other, &[w.lyon], Topic::Reviews, true).await.unwrap(), Err(Refused::NotFound)));

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
	assert_eq!(w.only(now).await.state, State::Done { by: Settled::Hand });

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

/// What happens in a case over simulated days, its replies switched on.
enum Happens {
	/// Google's, read as that kind by the model.
	Google(Box<Mail>, Kind),
	/// Ours, by hand, in Gmail.
	Ours(&'static str),
	Trashed,
}

const REQUEST: &str = "[1-1234000056789] Your Request to Google";
const LYON: &str = "Plomberie Dupont - Lyon";

fn says(kind: Kind, of: &str) -> Happens {
	Happens::Google(Box::new(support("2026-10-01T09:00:00Z", 1, REQUEST, of)), kind)
}

/// Every hour of `days` from 2026-10-01 09:00, the inbox followed, read, answered, what is held sent and its
/// copy in the inbox, `robot` answering each at once. What the first case came to: `us d<day>` a reply that
/// went, else its state as it turns to a person or over.
async fn lived(w: World, mut happens: Vec<(i64, Happens)>, robot: Option<Kind>, days: i64) -> String {
	for topic in [Topic::Verification, Topic::Reviews] {
		w.p.set_mail_auto(w.owner, &[w.lyon, w.est], topic, true).await.unwrap().unwrap();
	}
	let start = t("2026-10-01T09:00:00Z");
	let mut kinds: Vec<(u32, Kind)> = Vec::new();
	let (mut out, mut sent, mut stood) = (Vec::new(), 0, String::new());
	let keep = |m: Mail, now: Timestamp| {
		let mut mails = w.inbox.mails.lock().unwrap();
		let uid = u32::try_from(mails.len()).unwrap() + 1;
		mails.push(Mail {
			uid,
			message: 1_800_000_000_000_000_000 + u64::from(uid),
			message_id: Some(format!("m{uid}@mail")),
			date: now,
			..m
		});
		uid
	};
	for hour in 0..days * 24 {
		let now = start + SignedDuration::from_hours(hour);
		while happens.first().is_some_and(|(at, _)| *at <= hour) {
			match happens.remove(0).1 {
				Happens::Google(m, kind) => kinds.push((keep(*m, now), kind)),
				Happens::Ours(text) => {
					keep(ours("2026-10-01T09:00:00Z", 1, &format!("Re: {REQUEST}"), SUPPORT, &[], text), now);
				}
				Happens::Trashed => w.p.remove_gmb(w.owner, w.lyon, now).await.unwrap().unwrap(),
			}
		}
		w.p.follow_inbox(w.gmail, &w.inbox, now).await.unwrap();
		for m in w.p.unread_mail(w.owner, now).await.unwrap() {
			let uid = w.inbox.mails.lock().unwrap().iter().find(|x| x.date == m.at && !x.sent).unwrap().uid;
			let kind = kinds.iter().find(|(u, _)| *u == uid).unwrap().1;
			let reading = Reading {
				topic: Topic::Verification,
				kind,
				gmb: None,
				summary: "said".into(),
				asks: if m.text.as_ref().unwrap().contains("or reply with") { vec!["a photo".into()] } else { vec![] },
				reasons: vec![],
				deadline: m.text.as_ref().unwrap().contains("in 3 days").then(|| m.at + SignedDuration::from_hours(72)),
				answer_within_hours: (kind == Kind::Routed).then_some(48),
			};
			w.p.mail_read(m.mail, reading, &m.gmbs, now).await.unwrap();
		}
		for c in w.p.mail_cases(w.owner, now).await.unwrap().into_iter().filter(|c| c.due) {
			let written = Written::Reply {
				body: "The live call is needed.".into(),
				needs: vec![],
			};
			w.p.draft_mail(w.owner, &c, written, false, now).await.unwrap().unwrap();
		}
		w.p.send_held_mail(now).await.unwrap();
		let gone = w.sent();
		for m in &gone[sent..] {
			out.push(format!("us d{}", hour.div_euclid(24)));
			let answered = |x: &&Mail| x.message_id == m.in_reply_to;
			let thread = w.inbox.mails.lock().unwrap().iter().find(answered).unwrap().thread;
			let refs: Vec<&str> = m.references.iter().map(String::as_str).collect();
			keep(ours("2026-10-01T09:00:00Z", thread, &m.subject, SUPPORT, &refs, &m.body), now);
			if let Some(kind) = robot {
				happens.insert(0, (hour + 1, says(kind, LYON)));
			}
		}
		sent = gone.len();
		let stands = match &w.cases(&now.to_string()).await[0].state {
			State::NeedsYou { why: Why::Unread { .. } } | State::Sending { .. } | State::ReplyDue | State::Waiting { .. } => String::new(),
			State::NeedsYou { why } => serde_json::to_value(why).unwrap()["code"].as_str().unwrap().to_owned(),
			State::ReplyReady => "reply_ready".to_owned(),
			State::Done { .. } => "done".to_owned(),
			State::Quiet => "quiet".to_owned(),
		};
		if stands != stood {
			if !stands.is_empty() {
				out.push(format!("{stands} d{}", hour.div_euclid(24)));
			}
			stood = stands;
		}
	}
	out.join(", ")
}

#[tokio::test]
async fn google_is_pressed_over_days_as_far_as_it_should_be() {
	use Happens::{Google, Ours, Trashed};
	use Kind::*;
	let verified = || {
		Google(
			Box::new(notice("2026-10-01T09:00:00Z", 9, "Your profile is verified", "Plomberie Dupont - Lyon is verified")),
			Verified,
		)
	};
	let forwarded = || {
		Google(
			Box::new(Mail {
				to: vec!["b@gmail.com".into()],
				..support("2026-10-01T09:00:00Z", 1, REQUEST, LYON)
			}),
			SelfServe,
		)
	};
	let of = |m: Mail, kind| Google(Box::new(m), kind);
	// the same matter under another number, in a thread of its own
	let anew = |kind| of(support("2026-10-01T09:00:00Z", 2, "[1-9876000054321] Your Request to Google", LYON), kind);
	let told = |kind| of(notice("2026-10-01T09:00:00Z", 9, "About your profile", LYON), kind);
	let staff = || {
		of(
			Mail {
				from: Some("jean@xwf.google.com".into()),
				counterparts: vec!["Jean <jean@xwf.google.com>".into()],
				refs: vec!["m2@mail".into()],
				..support("2026-10-01T09:00:00Z", 8, "Re: your request", LYON)
			},
			Routed,
		)
	};
	let spam = || {
		of(
			Mail {
				spam: true,
				..support("2026-10-01T09:00:00Z", 1, REQUEST, LYON)
			},
			SelfServe,
		)
	};
	type Row = (&'static str, Vec<(i64, Happens)>, Option<Kind>, i64, &'static str);
	let rows: Vec<Row> = vec![
		(
			"acknowledged, then silent",
			vec![(0, says(Acknowledged, LYON))],
			None,
			36,
			"us d2, us d4, us d6, us d8, us d10, us d12, asked_often d14, quiet d33",
		),
		(
			"a specialist, silent past its word",
			vec![(0, says(SelfServe, LYON)), (3, says(Routed, LYON))],
			None,
			26,
			"us d0, us d2, unanswered d4, quiet d23",
		),
		(
			"a robot deflects at once",
			vec![(0, says(SelfServe, LYON))],
			Some(SelfServe),
			6,
			"us d0, us d0, us d1, us d1, us d2, us d2, asked_often d2",
		),
		("the call failed in the thread", vec![(0, says(CallFailed, LYON))], None, 5, "us d0, us d2, us d4"),
		(
			"the call offered, thanked",
			vec![(0, says(CallOffered, LYON)), (24, Ours("Thanks, Tuesday"))],
			None,
			8,
			"kind d0, kind d3",
		),
		("verified, thanked", vec![(0, says(Verified, LYON)), (24, Ours("Merci"))], None, 8, "done d0"),
		(
			"verified elsewhere, then a late deflection",
			vec![(0, verified()), (24, says(SelfServe, LYON))],
			None,
			8,
			"done d0",
		),
		("its GBP trashed", vec![(0, says(Acknowledged, LYON)), (30, Trashed)], None, 8, "quiet d1"),
		("its GBP never named", vec![(0, says(SelfServe, "Hello"))], None, 8, "reply_ready d0"),
		(
			"a survey in the thread",
			vec![(0, says(Acknowledged, LYON)), (24, says(Noise, "How did we do?"))],
			None,
			8,
			"us d2, us d4, us d6",
		),
		("unclear", vec![(0, says(Unclear, LYON))], None, 4, "reply_ready d0"),
		(
			"the prompts, or a photo",
			vec![(0, says(SelfServe, "Plomberie Dupont - Lyon: follow the prompts, or reply with a photo"))],
			None,
			4,
			"reply_ready d0",
		),
		("forwarded from another account", vec![(0, forwarded())], None, 4, "reply_ready d0"),
		(
			"closed, nothing given",
			vec![(0, says(SelfServe, LYON)), (30, says(Closed, LYON))],
			None,
			4,
			"us d0, kind d1",
		),
		("closed, verified before", vec![(0, says(Verified, LYON)), (30, says(Closed, LYON))], None, 4, "done d0"),
		("to be closed unless", vec![(0, says(Closed, "Plomberie Dupont - Lyon: we close it in 3 days unless"))], None, 5, "deadline d0, kind d3"),
		(
			"the call offered, then failed by a mail taking no reply",
			vec![(0, says(CallOffered, LYON)), (30, told(CallFailed))],
			None,
			6,
			"kind d0, us d1, us d3, us d5",
		),
		(
			"the call offered, then failed by a mail naming no GBP",
			vec![(0, says(CallOffered, LYON)), (30, of(notice("2026-10-01T09:00:00Z", 9, "Your chat", "The call failed"), CallFailed))],
			None,
			4,
			"kind d0, us d1, us d3",
		),
		(
			"the same under another number",
			vec![(0, says(SelfServe, LYON)), (30, anew(Acknowledged))],
			None,
			6,
			"us d0, quiet d1, us d3, us d5",
		),
		(
			"two tickets, one call offered",
			vec![(0, says(SelfServe, LYON)), (30, anew(CallOffered))],
			None,
			6,
			"us d0, quiet d1",
		),
		("a duplicate of another", vec![(0, anew(Acknowledged)), (3, says(DuplicateTicket, LYON))], None, 3, "us d2"),
		("a duplicate of none known", vec![(0, says(DuplicateTicket, LYON))], None, 3, "kind d0"),
		("a staff address answers ours", vec![(0, says(SelfServe, LYON)), (5, staff())], None, 6, "us d0, us d2, unanswered d4"),
		("in Spam", vec![(0, spam())], None, 3, "us d0, us d2"),
		("a code failed", vec![(0, told(CodeFailed))], None, 3, "kind d0"),
		("the appeal wants more", vec![(0, told(AppealReceived)), (30, told(AppealVerification))], None, 3, "done d1"),
		("refused", vec![(0, says(ReinstatementRefused, LYON))], None, 3, "us d0, us d2"),
		("seen live already", vec![(0, says(ReviewsLiveAlready, LYON))], None, 3, "us d0, us d2"),
		("partly brought back", vec![(0, says(ReinstatementPartial, LYON))], None, 5, "us d2, us d4"),
	];
	let mut runs = Vec::new();
	for (what, happens, robot, days, want) in rows {
		let w = World::new().await;
		runs.push(async move { (what, want, lived(w, happens, robot, days).await) });
	}
	let wrong: Vec<String> = futures::future::join_all(runs)
		.await
		.into_iter()
		.filter(|(_, want, got)| want != got)
		.map(|(what, want, got)| format!("{what}\n  want {want}\n  got  {got}"))
		.collect();
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[tokio::test]
async fn a_held_reply_is_held_back_by_what_changed() {
	let request = "[1-1234000056789] Your Request to Google";
	enum Then {
		TheGbp,
		TheReading,
		ReadNoMore,
		TheInboxElsewhereToo,
		AnotherAccount,
		Trashed,
	}
	for then in [Then::TheGbp, Then::TheReading, Then::ReadNoMore, Then::TheInboxElsewhereToo, Then::AnotherAccount, Then::Trashed] {
		let w = World::new().await;
		w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, true).await.unwrap().unwrap();
		w.arrives(support("2026-10-01T09:00:00Z", 1, request, "Plomberie Dupont - Lyon: follow the prompts.")).await;
		w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
		let case = w.only("2026-10-01T09:02:00Z").await;
		w.drafts(&case, &[], false, "2026-10-01T09:05:00Z").await;
		let (now, pool) = (t("2026-10-01T09:10:00Z"), w.db.pool().await);
		let imap = async |sql: &'static str| sqlx::query(sql).execute(&pool).await.unwrap();
		match then {
			Then::TheGbp => {
				let edit = CaseEdit {
					gmb: Some(Some(w.est)),
					topic: None,
					done: None,
					join: None,
				};
				w.p.edit_mail_case(w.owner, w.owner, case.id, edit, now).await.unwrap().unwrap();
			}
			Then::TheReading => {
				let edit = ReadingEdit {
					kind: Some(Kind::ReinstatementRefused),
					topic: None,
					gmb: None,
					summary: Some("a\nb".repeat(1000)),
					deadline: None,
				};
				w.p.correct_mail(w.owner, case.last().said.id, edit, now).await.unwrap().unwrap();
				let read = w.only("2026-10-01T09:11:00Z").await.messages[0].said.reading.clone().unwrap();
				assert!(read.summary.chars().count() <= 160 && !read.summary.contains('\n'), "a person's summary is a line too");
			}
			Then::ReadNoMore => {
				assert_eq!(w.p.send_held_mail(t("2026-10-01T09:30:00Z")).await.unwrap(), 0, "waits for its inbox to be read");
				assert_eq!(w.p.send_held_mail(t("2026-10-01T10:21:00Z")).await.unwrap(), 0, "its moment is an hour past");
			}
			Then::TheInboxElsewhereToo => {
				let other = w.p.add_gmail(w.owner, "b@gmail.com", now).await.unwrap().unwrap().id;
				assert_ne!(other, w.gmail);
				imap("INSERT INTO gmail_imap SELECT max(g.id), i.host, upper(i.username), i.password_sealed, i.data_key_fp, 0, 0, NULL FROM gmail_imap i, gmails g").await;
			}
			Then::AnotherAccount => {
				imap("UPDATE gmail_imap SET username = 'c@gmail.com'").await;
			}
			Then::Trashed => w.p.remove_gmb(w.owner, w.lyon, now).await.unwrap().unwrap(),
		}
		w.only("2026-10-01T09:20:00Z").await;
		assert_eq!(w.p.send_held_mail(t("2026-10-01T09:20:00Z")).await.unwrap(), 0);
		assert!(w.sent().is_empty());
		let draft = w.only("2026-10-01T09:21:00Z").await.draft.unwrap();
		assert_eq!(draft.reply.state, DraftState::Proposed, "its operator's again");
	}
}

#[tokio::test]
async fn what_a_reply_was_written_of_is_what_it_answers() {
	let w = World::new().await;
	w.arrives(support(
		"2026-10-01T09:00:00Z",
		1,
		"[1-1234000056789] Your Request",
		"Plomberie Dupont - Lyon: follow the prompts.",
	))
	.await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	let written = || Written::Reply { body: "x".into(), needs: vec![] };
	let edit = CaseEdit {
		gmb: Some(Some(w.est)),
		topic: None,
		done: None,
		join: None,
	};
	w.p.edit_mail_case(w.owner, w.owner, case.id, edit, t("2026-10-01T09:03:00Z")).await.unwrap().unwrap();
	let now = t("2026-10-01T09:04:00Z");
	assert!(w.p.draft_mail(w.owner, &case, written(), false, now).await.unwrap().is_none(), "of the GBP it was");

	let case = w.only("2026-10-01T09:05:00Z").await;
	w.drafts(&case, &[], true, "2026-10-01T09:05:00Z").await;
	for done in [true, false] {
		let edit = CaseEdit {
			gmb: None,
			topic: None,
			done: Some(done),
			join: None,
		};
		w.p.edit_mail_case(w.owner, w.owner, case.id, edit, now).await.unwrap().unwrap();
	}
	let case = w.only("2026-10-01T09:05:00Z").await;
	assert_eq!(case.state, State::ReplyReady, "reopened: its reply as it was");
	let first = case.draft.as_ref().unwrap().id;
	w.p.send_mail_draft(w.owner, w.owner, first, bare("x"), now).await.unwrap().unwrap();
	assert!(w.p.draft_mail(w.owner, &case, written(), true, now).await.unwrap().is_none(), "a reply to it went meanwhile");
	let pool = w.db.pool().await;
	sqlx::query(
		"INSERT INTO mail_drafts SELECT 99, case_id, answers, body_sealed, data_key_fp, needs, 'proposed', NULL, created_at, NULL, NULL, NULL, NULL FROM mail_drafts WHERE state = 'sent'",
	)
	.execute(&pool)
	.await
	.unwrap();
	assert!(matches!(w.p.send_mail_draft(w.owner, w.owner, 99, bare("x"), now).await.unwrap(), Err(Refused::Conflict(_))));
	assert_eq!(w.sent().len(), 1, "answered once");
	let twice = sqlx::query(
		"INSERT INTO mail_drafts SELECT 100, case_id, answers, body_sealed, data_key_fp, needs, 'proposed', NULL, created_at, NULL, NULL, NULL, NULL FROM mail_drafts WHERE id = 99",
	)
	.execute(&pool)
	.await;
	twice.unwrap();
	let twice = sqlx::query("UPDATE mail_drafts SET state = 'sending', send_at = 0, message_id = 'm' WHERE id = 100")
		.execute(&pool)
		.await;
	assert!(twice.unwrap_err().to_string().contains("answered once"), "the schema's too");
}

#[tokio::test]
async fn one_persons_broken_draft_stops_no_one_elses() {
	let w = World::new().await;
	let pool = w.db.pool().await;
	w.p.set_mail_auto(w.owner, &[w.lyon, w.est], Topic::Verification, true).await.unwrap().unwrap();
	w.arrives(support(
		"2026-10-01T09:00:00Z",
		1,
		"[1-1234000056789] Your Request",
		"Plomberie Dupont - Lyon: follow the prompts.",
	))
	.await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	w.drafts(&case, &[], false, "2026-10-01T09:05:00Z").await;
	let other = Uuid::from_u128(1); // told and sent of first
	let user = User {
		user_id: other,
		email: "other@localhost".into(),
		name: "other".into(),
	};
	w.p.user_seen(&user, t("2026-10-01T09:00:00Z")).await.unwrap();
	let gmail = w.p.add_gmail(other, "b@gmail.com", t("2026-10-01T09:00:00Z")).await.unwrap().unwrap().id;
	for sql in [
		"INSERT INTO mail_cases (id, gmail_id, numbers) VALUES (77, $1, '[]')",
		"INSERT INTO mails (id, case_id, gmail_id, message, thread, at, sent, recipients, refs, authentic, repliable, text_sealed, data_key_fp) \
		 VALUES (77, 77, $1, 'x', 'x', 1790845200000000, 0, '[]', '[]', 1, 1, x'00', zeroblob(32))",
		"INSERT INTO mail_drafts (id, case_id, answers, body_sealed, data_key_fp, needs, state, send_at, created_at) SELECT 77, 77, 77, x'00', zeroblob(32), '[]', 'held', 0, 0 WHERE $1",
	] {
		sqlx::query(sql).bind(gmail).execute(&pool).await.unwrap();
	}
	w.only("2026-10-01T09:20:00Z").await;
	assert_eq!(w.p.send_held_mail(t("2026-10-01T09:20:00Z")).await.unwrap(), 1, "the next one's went");
	let broken = w.p.mail_cases(other, t("2026-10-01T09:21:00Z")).await.unwrap().remove(0);
	assert!(broken.draft.unwrap().body.is_none());
	let State::NeedsYou {
		why: Why::Failed { error },
	} = broken.state
	else {
		panic!("{:?}", broken.state)
	};
	assert!(error.contains("does not open"), "{error}");
}

#[tokio::test]
async fn a_running_deadline_and_a_long_silence_are_told() {
	let w = World::new().await;
	let pool = w.db.pool().await;
	let told = async |at: &str| {
		w.p.alerts_access_seen(w.owner, &sa_panel::testing::operator(), "owner", t(at)).await.unwrap();
		w.only(at).await;
		w.p.tell_mail(Locale::En, t(at)).await.unwrap();
		let inbox = w.p.alerts_inbox(w.owner, &sa_panel::testing::operator(), &Sought::default()).await.unwrap();
		(inbox.len(), inbox.first().map(|a| a.tier))
	};
	w.arrives(notice("2026-10-01T09:00:00Z", 1, "Is it still open?", "Plomberie Dupont - Lyon")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::StillActive, |r| r.deadline = Some(t("2026-10-31T09:00:00Z"))).await;
	assert_eq!(told("2026-10-01T09:02:00Z").await, (1, Some(Tier::Urgent)));
	assert_eq!(
		w.only("2026-10-28T09:00:00Z").await.state,
		State::NeedsYou {
			why: Why::Kind { kind: Kind::StillActive }
		},
		"not quiet while it runs"
	);
	assert_eq!(told("2026-10-30T08:59:00Z").await.0, 1);
	assert_eq!(told("2026-10-30T09:01:00Z").await.0, 2, "again a day before");
	assert_eq!(told("2026-10-30T10:01:00Z").await.0, 2, "once");
	assert_eq!(w.only("2026-11-01T09:00:00Z").await.state, State::Quiet);
	sqlx::query("DELETE FROM mails").execute(&pool).await.unwrap();
	sqlx::query("DELETE FROM mail_cases").execute(&pool).await.unwrap();

	w.arrives(notice("2026-11-01T09:00:00Z", 2, "Your appeal", "Plomberie Dupont - Lyon")).await;
	w.reads("2026-11-01T09:01:00Z", Kind::AppealReceived, |r| r.answer_within_hours = Some(240)).await;
	assert_eq!(told("2026-11-11T08:00:00Z").await.0, 2, "waited for");
	assert_eq!(told("2026-11-11T10:00:00Z").await.0, 3, "silent past its ten days");
}

#[tokio::test]
async fn a_mail_not_read_to_its_end_takes_no_reply_and_a_copy_to_support_is_ours() {
	let w = World::new().await;
	let request = "[1-1234000056789] Your Request";
	w.arrives(Mail {
		whole: false,
		..support("2026-10-01T09:00:00Z", 1, request, "Plomberie Dupont - Lyon: follow the prompts.")
	})
	.await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	assert_eq!(w.only("2026-10-01T09:02:00Z").await.state, State::NeedsYou { why: Why::NoReply }, "it may say so past there");
	let mut ours = ours("2026-10-01T10:00:00Z", 1, request, "jean@plomberie.fr", &[], "The call, please");
	ours.to.push(SUPPORT.to_owned());
	w.arrives(ours).await;
	assert_eq!(w.only("2026-10-01T10:01:00Z").await.state, State::Waiting { until: t("2026-10-03T10:00:00Z") });
}

#[tokio::test]
async fn one_conversation_under_two_numbers_is_one_case_once_its_person_says_so() {
	let w = World::new().await;
	let now = "2026-10-03T09:00:00Z";
	for (at, thread, subject) in [
		("2026-10-01T09:00:00Z", 1, REQUEST),
		("2026-10-02T09:00:00Z", 2, "[1-9876000054321] Your Request to Google"),
	] {
		w.arrives(support(at, thread, subject, "Plomberie Dupont - Lyon: follow the prompts.")).await;
		w.reads(now, Kind::SelfServe, |_| {}).await;
	}
	let cases = w.cases(now).await;
	assert_eq!((&cases[0].state, &cases[1].state), (&State::Quiet, &State::ReplyDue), "the newer is asked of");
	let (first, second) = (cases[0].id, cases[1].id);
	let written = Written::Reply { body: "x".into(), needs: vec![] };
	w.p.draft_mail(w.owner, &cases[1], written, true, t(now)).await.unwrap().unwrap();
	let join = async |case, to| {
		let edit = CaseEdit {
			gmb: None,
			topic: None,
			done: None,
			join: Some(to),
		};
		w.p.edit_mail_case(w.owner, w.owner, case, edit, t(now)).await.unwrap()
	};
	assert!(matches!(join(first, Some(first)).await, Err(Refused::Invalid(_))));
	assert!(matches!(join(first, Some(99)).await, Err(Refused::NotFound)));
	assert!(matches!(join(first, None).await, Err(Refused::Invalid(_))));
	join(second, Some(first)).await.unwrap();
	let one = w.only(now).await;
	assert_eq!((one.id, &one.joined, one.messages.len(), one.numbers.len()), (first, &vec![second], 2, 2));
	assert!(one.due, "its draft was of the other alone");
	assert_eq!(w.p.mail_case(w.owner, second, t(now)).await.unwrap().unwrap().case.id, first, "a link to either");
	w.arrives(support("2026-10-03T10:00:00Z", 2, "[1-9876000054321] Your Request to Google", "The call is enabled."))
		.await;
	w.reads(now, Kind::CallOffered, |_| {}).await;
	assert_eq!(w.only(now).await.messages.len(), 3, "what comes in either is its");
	join(second, None).await.unwrap();
	assert_eq!(w.cases(now).await.iter().map(|c| c.messages.len()).collect::<Vec<_>>(), [1, 2]);
	w.arrives(Mail {
		authentic: false,
		..support("2026-10-03T11:00:00Z", 1, REQUEST, "pay")
	})
	.await;
	let forged = w.cases(now).await[2].id;
	for (case, to) in [(forged, first), (first, forged)] {
		assert!(matches!(join(case, Some(to)).await, Err(Refused::Invalid(_))), "nothing forged speaks in a case");
	}
}

#[tokio::test]
async fn a_dismissed_reply_comes_back_until_something_newer_stands() {
	let w = World::new().await;
	w.arrives(support("2026-10-01T09:00:00Z", 1, REQUEST, "Plomberie Dupont - Lyon: follow the prompts.")).await;
	w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	let case = w.only("2026-10-01T09:02:00Z").await;
	let draft = w.drafts(&case, &[], true, "2026-10-01T09:03:00Z").await;
	let restore = async || w.p.restore_mail_draft(w.owner, w.owner, draft, t("2026-10-01T09:05:00Z")).await.unwrap();
	assert!(matches!(restore().await, Err(Refused::Conflict(_))), "not dismissed");
	w.p.dismiss_mail_draft(w.owner, w.owner, draft).await.unwrap().unwrap();
	restore().await.unwrap();
	assert_eq!(w.only("2026-10-01T09:06:00Z").await.state, State::ReplyReady);
	w.p.dismiss_mail_draft(w.owner, w.owner, draft).await.unwrap().unwrap();
	w.arrives(support("2026-10-01T10:00:00Z", 1, REQUEST, "Follow them.")).await;
	assert!(matches!(restore().await, Err(Refused::Conflict(_))), "another message came");
}

#[tokio::test]
async fn what_cannot_be_read_is_said_of_its_case_alone() {
	let w = World::new().await;
	let pool = w.db.pool().await;
	let now = "2026-10-01T10:00:00Z";
	for n in 1..=3 {
		w.arrives(support("2026-10-01T09:00:00Z", n, &format!("[1-123400005678{n}] a"), "Plomberie Dupont - Lyon, Est")).await;
		w.reads(now, Kind::SelfServe, |_| {}).await;
	}
	sqlx::query("UPDATE mails SET reading = json_set(reading, '$.kind', 'of_another_build') WHERE case_id = 1")
		.execute(&pool)
		.await
		.unwrap();
	sqlx::query("UPDATE mails SET text_sealed = x'00' WHERE case_id = 2").execute(&pool).await.unwrap();
	let cases = w.cases(now).await;
	let State::NeedsYou {
		why: Why::Unread { error: Some(error) },
	} = &cases[0].state
	else {
		panic!("{:?}", cases[0].state)
	};
	assert!(error.contains("of_another_build"), "{error}");
	assert_eq!(cases[2].state, State::ReplyDue);
	let brief = w.p.mail_case(w.owner, 2, t(now)).await.unwrap().unwrap();
	assert!(brief.texts[0].is_err() && w.p.unread_mail(w.owner, t(now)).await.unwrap().is_empty());
}

#[tokio::test]
async fn mail_waiting_to_be_read_and_what_is_settled_are_told_once() {
	let w = World::new().await;
	let told = async |at: &str| {
		w.p.alerts_access_seen(w.owner, &sa_panel::testing::operator(), "owner", t(at)).await.unwrap();
		w.only(at).await;
		w.p.tell_mail(Locale::En, t(at)).await.unwrap();
		let inbox = w.p.alerts_inbox(w.owner, &sa_panel::testing::operator(), &Sought::default()).await.unwrap();
		inbox.into_iter().map(|a| (a.tier, a.text.text)).collect::<Vec<_>>()
	};
	w.arrives(Mail {
		spam: true,
		..notice("2026-10-01T09:00:00Z", 9, "Your profile is verified", "Plomberie Dupont - Lyon is verified")
	})
	.await;
	assert!(w.only("2026-10-01T09:30:00Z").await.last().spam);
	assert_eq!(told("2026-10-01T09:59:00Z").await, []);
	let waits = told("2026-10-01T10:00:00Z").await;
	assert!(waits[0].1.contains("mail waits") && waits[0].1.ends_with("Your profile is verified\nIt is in Spam"), "{waits:?}");
	w.p.mail_stuck(w.owner, Some("No Claude")).await.unwrap();
	assert_eq!(told("2026-10-01T11:00:00Z").await.len(), 1, "once, whatever it waits on");
	w.reads("2026-10-01T11:00:00Z", Kind::Verified, |_| {}).await;
	let settled = told("2026-10-01T11:01:00Z").await;
	assert_eq!((settled.len(), settled[0].0), (2, Tier::Info));
	assert!(settled[0].1.contains("settled\nverified · Plomberie Dupont - Lyon"), "{settled:?}");
	assert_eq!(told("2026-10-01T12:00:00Z").await.len(), 2);
	// moved out of Spam: it is in All Mail now, under another UID
	{
		let mut mails = w.inbox.mails.lock().unwrap();
		(mails[0].uid, mails[0].spam) = (9, false);
	}
	assert!(!w.only("2026-10-01T12:01:00Z").await.last().spam);
	w.arrives(Mail {
		authentic: false,
		spam: true,
		..support("2026-10-01T12:02:00Z", 5, "Verify now", "pay")
	})
	.await;
	assert_eq!(w.cases("2026-10-01T12:03:00Z").await.len(), 1, "what Spam holds unproved is spam");
}

#[tokio::test]
async fn a_gbp_under_another_gmail_is_known_where_the_gmails_own_are_not_named() {
	let w = World::new().await;
	let now = "2026-10-01T10:00:00Z";
	let other = w.p.add_gmail(w.owner, "b@gmail.com", t(now)).await.unwrap().unwrap().id;
	let new = Existing {
		gmail_id: other,
		place: "ChIJ5WkNXjZw5kcRxEYGmlxqu3g".to_owned(),
		state: sa_core::gbp::OnGoogle::Verifying,
		name: Some("L'Artisan - Bron".to_owned()),
		country: Some(Country::FR),
		trade: None,
	};
	let bron = w.p.add_gmb(Some(Spender::new(w.owner)), new, t(now)).await.unwrap().unwrap().id;
	w.arrives(notice("2026-10-01T09:00:00Z", 1, "Verified", "L’Artisan – Bron is verified")).await;
	assert_eq!(w.only(now).await.gmb, Some(bron));
	w.arrives(notice("2026-10-01T09:30:00Z", 2, "Verified", "L’Artisan – Bron and Plomberie Dupont - Lyon")).await;
	assert_eq!(w.cases(now).await[1].gmb, Some(w.lyon), "its own first");
}

#[tokio::test]
async fn the_mail_of_a_gbp_not_here_is_never_a_siblings() {
	let w = World::new().await;
	let video = |of: &str, listing: &str| rejected(&format!(" for Plomberie Dupont - Lyon, {of}"), listing);
	w.arrives(notice("2026-10-01T09:00:00Z", 1, REJECTED, &video("Nord", "77"))).await;
	w.reads("2026-10-01T09:01:00Z", Kind::VideoRejected, |_| {}).await;
	assert_eq!(w.only("2026-10-01T09:02:00Z").await.gmb, None, "Lyon's name begins another's");

	let new = Existing {
		gmail_id: w.gmail,
		place: "ChIJ5WkNXjZw5kcRxEYGmlxqu3g".to_owned(),
		state: sa_core::gbp::OnGoogle::Verifying,
		name: Some("Plomberie Dupont - Lyon, Nord".to_owned()),
		country: Some(Country::FR),
		trade: None,
	};
	let nord = w.p.add_gmb(Some(Spender::new(w.owner)), new, t("2026-10-02T00:00:00Z")).await.unwrap().unwrap().id;
	let c = w.only("2026-10-02T00:01:00Z").await;
	assert_eq!((c.gmb, c.gmb_certain), (Some(nord), true), "added since, its mail is its own");
	sqlx::query("UPDATE gmbs SET name = 'Dupont Nord' WHERE id = $1").bind(nord).execute(&w.db.pool().await).await.unwrap();
	sqlx::query("INSERT INTO gmb_events (gmb_id, gmail_id, at, kind, data) VALUES ($1, $2, 1, 'edited', $3)")
		.bind(nord)
		.bind(w.gmail)
		.bind(r#"{"kind":"edited","changes":[{"field":"name","from":"Plomberie Dupont - Lyon, Nord","to":"Dupont Nord"}]}"#)
		.execute(&w.db.pool().await)
		.await
		.unwrap();
	w.arrives(notice("2026-10-03T09:00:00Z", 2, REJECTED, &video("Nord", "78"))).await;
	assert_eq!(w.cases("2026-10-03T09:01:00Z").await[1].gmb, Some(nord), "by the name it had");
}

#[tokio::test]
async fn a_pick_by_hand_says_whose_a_link_is() {
	let w = World::new().await;
	w.arrives(notice("2026-10-01T09:00:00Z", 1, REJECTED, &rejected("", "1559"))).await;
	w.reads("2026-10-01T09:01:00Z", Kind::VideoRejected, |r| r.gmb = Some(w.lyon)).await;
	let seen = w.only("2026-10-01T09:02:00Z").await;
	assert_eq!((seen.gmb, seen.gmb_certain), (Some(w.lyon), false), "as its reader saw it");
	let pick = CaseEdit {
		gmb: Some(Some(w.est)),
		topic: None,
		done: None,
		join: None,
	};
	w.p.edit_mail_case(w.owner, w.owner, seen.id, pick, t("2026-10-01T09:03:00Z")).await.unwrap().unwrap();
	w.arrives(notice("2026-10-02T09:00:00Z", 2, REJECTED, &rejected("", "1559"))).await;
	let next = w.cases("2026-10-02T09:01:00Z").await.remove(1);
	assert_eq!((next.gmb, next.gmb_certain), (Some(w.est), true));
}

#[tokio::test]
async fn a_switch_is_one_gbps_and_off_once_it_moves() {
	let w = World::new().await;
	w.p.set_mail_auto(w.owner, &[w.est], Topic::Verification, true).await.unwrap().unwrap();
	for (n, name) in [(1, "Plomberie Dupont - Lyon"), (2, "Plomberie Dupont - Lyon, Est")] {
		let request = format!("[{n}-1234000056789] Your Request to Google");
		w.arrives(support("2026-10-01T09:00:00Z", n, &request, &format!("{name}: follow the prompts."))).await;
		w.reads("2026-10-01T09:01:00Z", Kind::SelfServe, |_| {}).await;
	}
	let cases = w.cases("2026-10-01T09:02:00Z").await;
	assert_eq!(cases[0].press, Err(Halt::Off));
	assert!(cases[1].press.is_ok());
	for c in &cases {
		let written = Written::Reply {
			body: "Enable the live call.".into(),
			needs: vec![],
		};
		w.p.draft_mail(w.owner, c, written, false, t("2026-10-01T09:05:00Z")).await.unwrap().unwrap();
	}
	w.cases("2026-10-01T09:20:00Z").await;
	assert_eq!(w.p.send_held_mail(t("2026-10-01T09:20:00Z")).await.unwrap(), 1, "Est's alone");
	let id = w.sent()[0].message_id.clone();
	w.arrives(Mail {
		message_id: Some(id.clone()),
		..ours("2026-10-01T09:20:00Z", 2, "Re: [2-1234000056789] Your Request to Google", SUPPORT, &["m2@mail"], "Enable the live call.")
	})
	.await;
	let mut messages = w.cases("2026-10-01T09:21:00Z").await.remove(1).messages;
	assert_eq!(messages.pop().unwrap().sent_by, Some(SentBy::Gmail), "the test's inbox titles each message itself");
	sqlx::query("UPDATE mails SET message_id = $1 WHERE sent").bind(&id).execute(&w.db.pool().await).await.unwrap();
	let mut messages = w.cases("2026-10-01T09:21:00Z").await.remove(1).messages;
	assert_eq!(messages.pop().unwrap().sent_by, Some(SentBy::Itself));

	let other = w.p.add_gmail(w.owner, "b@gmail.com", t("2026-10-02T00:00:00Z")).await.unwrap().unwrap().id;
	sqlx::query("UPDATE gmbs SET gmail_id = $1 WHERE id = $2").bind(other).bind(w.est).execute(&w.db.pool().await).await.unwrap();
	assert_eq!(w.p.mail_auto(w.owner).await.unwrap().on, []);
}
