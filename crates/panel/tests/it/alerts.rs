//! The hub through networks that keep what they were sent: what each method is sent, the mailed links,
//! and what a network's refusal does to its destination.

use std::{
	collections::BTreeMap,
	sync::{Arc, Mutex},
};

use jiff::{SignedDuration, Timestamp};
use sa_alerts::{Call, NotePage};
use sa_auth::{PermissionSet, SA_OPERATOR, Service};
use sa_core::{
	alerts::Cadence,
	notify::{Failure, Locale, Rendered},
};
use sa_panel::{
	Panel, Refused,
	alerts::{Address, Directory, DirectoryError, Feeds, Hub, Identity, Message, Outlets, Settings},
	seal::DataKey,
	session::{RefreshError, Refresher, Tokens},
	telegram::{CallbackId, ChatInfo},
	testing::{TestDb, panel_with},
};
use serde_json::json;
use uuid::Uuid;

/// What went out, and what the next send to an address answers; then delivered.
#[derive(Clone, Default)]
struct Kept {
	sent: Arc<Mutex<Vec<(Address, Message)>>>,
	next: Arc<Mutex<Vec<(Address, Failure)>>>,
	/// Addresses whose network never answers.
	hung: Arc<Mutex<Vec<Address>>>,
}

impl Kept {
	fn refuse(&self, to: Address, f: Failure) {
		self.next.lock().unwrap().push((to, f));
	}

	/// What went out since asked last.
	fn taken(&self) -> Vec<(Address, Message)> {
		std::mem::take(&mut self.sent.lock().unwrap())
	}
}

impl Outlets for Kept {
	async fn send_to(&self, to: &Address, message: &Message) -> Result<String, Failure> {
		if self.hung.lock().unwrap().contains(to) {
			std::future::pending::<()>().await;
		}
		let mut next = self.next.lock().unwrap();
		if let Some(i) = next.iter().position(|(a, _)| a == to) {
			return Err(next.remove(i).1);
		}
		self.sent.lock().unwrap().push((to.clone(), message.clone()));
		Ok("1".to_owned())
	}

	async fn edit(&self, _: i64, _: i64, _: &Rendered, _: &[(String, String)]) -> Result<(), Failure> {
		unreachable!("no Telegram here")
	}

	async fn answer(&self, _: &CallbackId, _: &str) -> Result<(), Failure> {
		unreachable!("no Telegram here")
	}

	async fn chat(&self, _: &str) -> Result<ChatInfo, Failure> {
		unreachable!("no Telegram here")
	}

	async fn is_admin(&self, _: i64, _: i64) -> Result<bool, Failure> {
		unreachable!("no Telegram here")
	}
}

/// A concierge that is never asked.
struct Never;

impl Refresher for Never {
	async fn refresh(&self, _: &str) -> Result<Tokens, RefreshError> {
		unreachable!("no session here")
	}
}

impl Directory for Never {
	async fn me(&self, _: &str) -> Result<Identity, DirectoryError> {
		unreachable!("no session here")
	}
}

/// Sites, its leads so far.
#[derive(Clone, Default)]
struct Leads(Arc<Mutex<Vec<serde_json::Value>>>);

impl Feeds for Leads {
	async fn notes(&self, _: Service, after: i64, _: usize) -> eyre::Result<Option<NotePage>> {
		let notes: Vec<_> = self.0.lock().unwrap().iter().filter(|n| n["seq"].as_i64().unwrap() > after).cloned().collect();
		let next = notes.last().map_or(after, |n| n["seq"].as_i64().unwrap());
		Ok(Some(serde_json::from_value(json!({"notes": notes, "next": next}))?))
	}

	async fn received(&self) -> eyre::Result<BTreeMap<String, Timestamp>> {
		unreachable!("not asked here")
	}

	fn callable(&self, _: Service, _: &Call) -> eyre::Result<()> {
		unreachable!("no button here")
	}

	async fn press(&self, _: &Identity, _: Option<&sa_auth::Actor>, _: Service, _: &Call, _: &str) -> eyre::Result<u16> {
		unreachable!("no button here")
	}

	async fn photo(&self, _: Service, _: &str) -> eyre::Result<Vec<u8>> {
		unreachable!("no photo here")
	}
}

struct Setup {
	_db: TestDb,
	panel: Panel,
	networks: Kept,
	leads: Leads,
	hub: Hub<Kept, Never, Leads>,
	user: Uuid,
}

impl Setup {
	async fn new() -> Self {
		let db = TestDb::create().await;
		let panel = panel_with(db.store().await, DataKey::from_hex(&"0f".repeat(32)).unwrap());
		let (networks, leads) = (Kept::default(), Leads::default());
		let hub = Hub {
			panel: panel.clone(),
			outlets: networks.clone(),
			concierge: Never,
			feeds: leads.clone(),
			locale: Locale::En,
			cabinet: "https://evinvest.test".parse().unwrap(),
			origin: "https://panel.test".parse().unwrap(),
		};
		Self {
			_db: db,
			panel,
			networks,
			leads,
			hub,
			user: Uuid::now_v7(),
		}
	}

	/// A destination of the user's, told of everything at once.
	async fn add(&self, address: Address, now: Timestamp) -> i64 {
		let id = self.hub.add(self.user, &operator(), "Olga", &address, "there", now).await.unwrap().unwrap();
		let settings = Settings {
			title: None,
			cadences: sa_core::alerts::Cadences {
				urgent: Cadence::Now,
				important: Cadence::Now,
				info: Cadence::Now,
			},
			filter: Default::default(),
			rules: BTreeMap::new(),
		};
		self.panel.alerts_set(self.user, &operator(), id, &settings, now).await.unwrap().unwrap();
		self.networks.taken();
		id
	}

	/// A lead noted at `now`, taken: how many messages it queued.
	async fn lead(&self, now: Timestamp) -> usize {
		{
			let mut leads = self.leads.0.lock().unwrap();
			let seq = leads.len() + 1;
			leads.push(json!({"seq": seq, "rule": "new_lead", "event_id": Uuid::now_v7(), "created_at": now, "skip_user": null, "buttons": [],
				"text": {"text": "New lead", "entities": []}, "text_pii": {"text": "New lead\n+33 6", "entities": []}, "link": format!("/sites/leads?n={seq}")}));
		}
		self.hub.take_feed(Service::Sites, now).await.unwrap().unwrap()
	}

	async fn dead(&self, id: i64) -> Option<String> {
		self.panel.alerts(self.user, &operator()).await.unwrap().destinations.into_iter().find(|d| d.id == id).unwrap().dead
	}
}

fn operator() -> PermissionSet {
	SA_OPERATOR.members.iter().copied().collect()
}

fn email(to: &str) -> Address {
	Address::Email { to: to.to_owned() }
}

fn slack(url: &str) -> Address {
	Address::Slack { url: url.to_owned() }
}

/// Contact details go where the person alone reads; a channel gets the rest, at once as the mail. An
/// address is sealed, comes back as it was, and is added once.
#[tokio::test]
async fn a_channel_reads_no_contact_details() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	s.add(email("olga@x.io"), now).await;
	s.add(slack("https://hooks.slack.test/a"), now).await;
	let again = s.hub.add(s.user, &operator(), "Olga", &email("olga@x.io"), "x", now).await.unwrap();
	assert!(matches!(again, Err(Refused::Conflict(_))), "{again:?}");
	s.networks.taken();
	s.networks.refuse(slack("https://hooks.slack.test/b"), Failure::Refused("no such hook".into()));
	let refused = s.hub.add(s.user, &operator(), "Olga", &slack("https://hooks.slack.test/b"), "x", now).await.unwrap();
	assert!(matches!(refused, Err(Refused::Invalid(m)) if m.contains("no such hook")), "what a test does not reach is not stored");
	let stored: Vec<Vec<u8>> = sqlx::query_scalar("SELECT address_sealed FROM alert_destinations").fetch_all(s.panel.store().pool()).await.unwrap();
	assert!(stored.len() == 2 && stored.iter().all(|a| !String::from_utf8_lossy(a).contains("x.io")));

	assert_eq!(s.lead(now).await, 2);
	assert_eq!(s.hub.deliver(now).await.unwrap().sent, 2);
	let mut sent = s.networks.taken();
	sent.sort_by_key(|(a, _)| matches!(a, Address::Slack { .. }));
	let [(to, mail), (_, channel)] = sent.as_slice() else { panic!("{sent:?}") };
	assert_eq!(*to, email("olga@x.io"));
	assert_eq!((mail.text.text.as_str(), channel.text.text.as_str()), ("New lead\n+33 6", "New lead"));
	let note = mail.notes[0];
	assert_eq!(mail.links, [("Open".to_owned(), format!("https://panel.test/user/alerts?open={note}"))]);
	assert!(mail.unsubscribe.is_some() && channel.unsubscribe.is_none());
}

/// A mail's unsubscribe link ends its destination's alerts alone, what waits with them; another's
/// token, or none of ours, does nothing.
#[tokio::test]
async fn a_mails_link_unsubscribes_its_destination() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	s.add(email("olga@x.io"), now).await;
	s.add(slack("https://hooks.slack.test/a"), now).await;
	s.lead(now).await;
	s.hub.deliver(now).await.unwrap();
	let url = s.networks.taken().into_iter().find_map(|(_, m)| m.unsubscribe).unwrap();
	assert_eq!(url.path(), "/api/alerts/unsubscribe");
	let token = url.query_pairs().find(|(k, _)| k == "token").unwrap().1.into_owned();
	for forged in ["", "x", &token[1..], &s.panel.alerts_email_token(s.user, "a@x.io", now).unwrap()] {
		assert!(!s.panel.alerts_unsubscribe(forged).await.unwrap(), "{forged}");
	}
	assert_eq!(s.lead(now).await, 2);
	assert!(s.panel.alerts_unsubscribe(&token).await.unwrap());
	assert_eq!(s.hub.deliver(now + SignedDuration::from_secs(5)).await.unwrap().sent, 1, "the channel's, the mail's given up");
	assert_eq!(s.lead(now).await, 1);
	assert!(s.panel.alerts_unsubscribe(&token).await.unwrap(), "again: as it is");
}

/// The link mailed to an address proves it to who asked for it, for a day.
#[tokio::test]
async fn a_mailed_link_proves_an_address_to_who_asked() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	s.hub.prove_email(s.user, "team@x.io", "/user/alerts/settings", now).await.unwrap().unwrap();
	let [(to, mail)] = s.networks.taken().try_into().unwrap();
	assert_eq!(to, email("team@x.io"));
	let url: url::Url = mail.links[0].1.parse().unwrap();
	assert_eq!((url.host_str(), url.path()), (Some("panel.test"), "/user/alerts/settings"));
	let token = url.query_pairs().find(|(k, _)| k == "confirm").unwrap().1.into_owned();
	let proved = |user, token: &str, at| s.panel.alerts_email_proved(user, token, at);
	let day = SignedDuration::from_hours(24);
	assert_eq!(proved(s.user, &token, now + day - SignedDuration::from_secs(1)).as_deref(), Some("team@x.io"));
	assert_eq!(proved(s.user, &token, now + day), None, "out of date");
	assert_eq!(proved(Uuid::now_v7(), &token, now), None, "another's");
	assert_eq!(proved(s.user, "nonsense", now), None);
	assert!(s.panel.alerts(s.user, &operator()).await.unwrap().destinations.is_empty(), "nothing is added by asking");
}

/// Whoever is mailed a link to prove an address may be anyone: few in an hour by one person, few in a
/// day to one address.
#[tokio::test]
async fn few_links_are_mailed_to_strangers() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	let ask = async |user, to: &str, at| s.hub.prove_email(user, to, "/user/alerts/settings", at).await.unwrap().is_ok();
	for n in 0..5 {
		assert!(ask(s.user, &format!("{n}@x.io"), now).await);
	}
	assert!(!ask(s.user, "5@x.io", now).await);
	assert!(ask(s.user, "5@x.io", now + SignedDuration::from_hours(1)).await);
	for _ in 0..2 {
		assert!(ask(Uuid::now_v7(), "0@x.io", now).await);
	}
	assert!(!ask(Uuid::now_v7(), "0@x.io", now).await, "whoever asks");
	assert!(ask(Uuid::now_v7(), "0@x.io", now + SignedDuration::from_hours(24)).await);
	assert_eq!(s.networks.taken().len(), 9, "a refusal mails nothing");
}

/// A network that hangs holds its own messages alone: not the rest of its pass, nor the passes after.
#[tokio::test]
async fn a_network_that_hangs_delays_no_other() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	let hook = || slack("https://hooks.slack.test/a");
	s.add(hook(), now).await;
	s.add(email("olga@x.io"), now).await;
	s.networks.hung.lock().unwrap().push(hook());
	assert_eq!(s.lead(now).await, 2);
	let meanwhile = async {
		let sent = async || sqlx::query_scalar::<_, i64>("SELECT count(*) FROM alert_outbox WHERE state = 'sent'").fetch_one(s.panel.store().pool()).await.unwrap();
		while sent().await == 0 {
			tokio::time::sleep(std::time::Duration::from_millis(5)).await;
		}
		assert_eq!(s.networks.taken().len(), 1, "the mail of its pass");
		let later = now + SignedDuration::from_secs(5);
		s.lead(later).await;
		assert_eq!(s.hub.deliver(later).await.unwrap().sent, 1, "the mail; the hook's waits for its first");
	};
	// both polled: a pass left mid-write would hold the database
	tokio::select! {
		_ = s.hub.deliver(now) => panic!("its hook answers"),
		() = meanwhile => {}
	}
}

/// A mailbox that asks to wait holds every mail back; a webhook only its own. What nothing reaches is
/// dead, with what it was owed, until a test arrives.
#[tokio::test]
async fn a_refusal_is_its_methods_or_its_addresss() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	let later = |secs| now + SignedDuration::from_secs(secs);
	let first = s.add(email("a@x.io"), now).await;
	let second = s.add(email("b@x.io"), now).await;
	assert_eq!(s.lead(now).await, 2);
	s.networks.refuse(email("a@x.io"), Failure::RetryAfter(SignedDuration::from_secs(60)));
	let done = s.hub.deliver(now).await.unwrap();
	assert_eq!((done.sent, done.retrying), (1, 1));
	s.lead(later(10)).await;
	assert_eq!(s.hub.deliver(later(59)).await.unwrap(), Default::default(), "every mail waits");
	assert_eq!(s.hub.deliver(later(60)).await.unwrap().sent, 2);
	assert_eq!(s.hub.deliver(later(63)).await.unwrap().sent, 1);

	for mail in [first, second] {
		s.panel.alerts_remove(s.user, mail).await.unwrap();
	}
	let a = || slack("https://hooks.slack.test/a");
	let hook = s.add(a(), later(70)).await;
	s.add(slack("https://hooks.slack.test/b"), later(70)).await;
	assert_eq!(s.lead(later(70)).await, 2);
	s.networks.refuse(a(), Failure::RetryAfter(SignedDuration::from_secs(60)));
	let done = s.hub.deliver(later(70)).await.unwrap();
	assert_eq!((done.sent, done.retrying), (1, 1));
	assert_eq!(s.lead(later(71)).await, 2);
	assert_eq!(s.hub.deliver(later(80)).await.unwrap().sent, 2, "the other's, and the same hook's next");
	assert_eq!(s.hub.deliver(later(130)).await.unwrap().sent, 1);

	s.lead(later(140)).await;
	assert_eq!(s.lead(later(140)).await, 2);
	s.networks.refuse(a(), Failure::Gone("no such hook".into()));
	let done = s.hub.deliver(later(140)).await.unwrap();
	assert_eq!((done.sent, done.dead), (1, 1));
	assert_eq!(s.dead(hook).await.as_deref(), Some("no such hook"));
	assert_eq!(s.hub.deliver(later(150)).await.unwrap().sent, 1, "the other's second; the dead one's is given up");
	assert_eq!(s.lead(later(151)).await, 1);
	s.networks.refuse(a(), Failure::Gone("still not".into()));
	assert!(matches!(s.hub.test(s.user, hook).await.unwrap(), Err(Refused::Invalid(_))));
	assert!(s.dead(hook).await.is_some());
	s.hub.test(s.user, hook).await.unwrap().unwrap();
	assert_eq!(s.dead(hook).await, None);
	assert_eq!(s.lead(later(152)).await, 2);
}

/// A browser is one person's: whoever subscribes it last.
#[tokio::test]
async fn a_browser_pushes_to_who_subscribed_it_last() {
	let s = Setup::new().await;
	let now = Timestamp::now();
	let push = Address::Push {
		endpoint: "https://push.test/1".into(),
		p256dh: "k".into(),
		auth: "a".into(),
	};
	s.add(push.clone(), now).await;
	let other = Uuid::now_v7();
	s.hub.add(other, &operator(), "Oleg", &push, "Firefox", now).await.unwrap().unwrap();
	assert!(s.panel.alerts(s.user, &operator()).await.unwrap().destinations.is_empty());
	assert_eq!(s.panel.alerts(other, &operator()).await.unwrap().destinations.len(), 1);
}
