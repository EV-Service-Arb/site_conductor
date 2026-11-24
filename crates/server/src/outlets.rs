//! The networks alerts go out through (`social_networks_adapters::notify`), each with its
//! configuration or refusing. What a message says and when it goes is the engine's
//! (`sa_panel::alerts`, `sa_panel::deliver`); a message's shape on each network is here.

use jiff::{SignedDuration, Timestamp};
use sa_alerts::Tier;
use sa_core::{
	alerts::Method,
	notify::{self as own, Rendered},
};
use sa_panel::{
	alerts::{Address, Message, Outlets},
	telegram::{CallbackId, ChatInfo},
};
use serde_json::json;
use social_networks_adapters::notify::{
	Button, Entity, Failure, Level, Link, Message as Outgoing, Photo, Rich, discord, email, slack, telegram, webhook, webpush,
};
use url::Url;
use zeroize::Zeroizing;

/// How long a push service keeps an urgent one for a browser that is away; any other, a day.
const URGENT_FOR: SignedDuration = SignedDuration::from_hours(1);

#[derive(Clone, Debug, Default)]
pub struct Networks {
	pub telegram: Option<telegram::Bot>,
	pub email: Option<email::Mailer>,
	pub discord: Option<discord::Webhooks>,
	pub slack: Option<slack::Webhooks>,
	pub push: Option<webpush::Pusher>,
	/// What a browser subscribes to.
	pub push_key: Option<String>,
	pub webhooks: Option<webhook::Webhooks>,
}

impl Networks {
	/// Those that need no configuration, the others left out; `seed` is what the push key is made of,
	/// `origin` the conductor's own.
	pub fn try_new(seed: &Zeroizing<[u8; 32]>, origin: &Url) -> eyre::Result<Self> {
		let vapid = webpush::Vapid::from_seed(**seed);
		let subject = match origin.scheme() {
			"https" => origin.as_str(),
			_ => "mailto:dev@evinvest.ltd", // a push service takes no http subject
		};
		Ok(Self {
			discord: Some(discord::Webhooks::try_new(discord::BASE)?),
			slack: Some(slack::Webhooks::try_new(slack::BASE)?),
			push_key: Some(vapid.public().to_owned()),
			push: Some(webpush::Pusher::try_new(vapid, subject, None)?),
			webhooks: Some(webhook::Webhooks::try_new(webhook::public)?),
			..Self::default()
		})
	}

	/// The methods a destination can be added by here.
	pub fn methods(&self) -> Vec<Method> {
		[
			(self.telegram.is_some(), Method::Telegram),
			(self.email.is_some(), Method::Email),
			(self.discord.is_some(), Method::Discord),
			(self.slack.is_some(), Method::Slack),
			(self.push.is_some(), Method::Push),
			(self.webhooks.is_some(), Method::Webhook),
		]
		.into_iter()
		.filter_map(|(there, method)| there.then_some(method))
		.collect()
	}

	async fn send(&self, to: &Address, m: &Message) -> Result<String, Failure> {
		let method = to.method();
		match to {
			Address::Telegram(chat) => need(&self.telegram, method)?.send(*chat, &whole(m)?).await.map(|id| id.to_string()),
			Address::Email { to } => need(&self.email, method)?.send(to, &headed(m, true)?).await.map(unnamed),
			Address::Discord { url, ping } => {
				let ping = *ping && m.tier == Tier::Urgent;
				need(&self.discord, method)?.send(&url.parse()?, &headed(m, false)?, ping).await.map(unnamed)
			}
			Address::Slack { url } => need(&self.slack, method)?.send(&url.parse()?, &headed(m, false)?).await.map(unnamed),
			Address::Push { endpoint, p256dh, auth } => {
				let (title, body) = split(m);
				let payload = webpush::Payload {
					title,
					body: body.text,
					// the panel's own: the service worker opens no other origin's
					url: match m.notes.as_slice() {
						[one] => format!("/user/alerts?open={one}"),
						_ => "/user/alerts".to_owned(),
					},
					// a digest and a test replace the one before
					tag: match m.notes.as_slice() {
						[one] => format!("alert {one}"),
						_ => "alerts".to_owned(),
					},
					level: level(m.tier),
				};
				let ttl = if m.tier == Tier::Urgent { URGENT_FOR } else { SignedDuration::from_hours(24) };
				let to = webpush::Subscription::try_new(endpoint, p256dh, auth)?;
				need(&self.push, method)?.send(&to, &payload, ttl, level(m.tier), None).await.map(unnamed)
			}
			Address::Webhook { url, secret } => {
				let (title, body) = split(m);
				let said = json!({
					"at": Timestamp::now(),
					"tier": m.tier,
					"rule": m.rule,
					"title": title,
					"body": body.text,
					"url": m.links.last().map(|(_, url)| url),
					"alerts": m.notes,
				});
				let url = url.parse().map_err(|_| Failure::Refused("not a URL".into()))?;
				need(&self.webhooks, method)?.send(&url, secret.as_bytes(), &said).await.map(unnamed)
			}
		}
	}

	fn bot(&self) -> Result<&telegram::Bot, own::Failure> {
		need(&self.telegram, Method::Telegram).map_err(failed)
	}
}

impl Outlets for Networks {
	async fn send_to(&self, to: &Address, message: &Message) -> Result<String, own::Failure> {
		self.send(to, message).await.map_err(failed)
	}

	async fn edit(&self, chat: i64, message_id: i64, text: &Rendered, buttons: &[(String, String)]) -> Result<(), own::Failure> {
		let edited = Outgoing {
			title: None,
			text: rich(text),
			level: Level::Normal,
			links: Vec::new(),
			buttons: pressable(buttons),
			photo: None,
			unsubscribe: None,
		};
		self.bot()?.edit(chat, message_id, &edited).await.map_err(failed)
	}

	async fn answer(&self, callback: &CallbackId, text: &str) -> Result<(), own::Failure> {
		self.bot()?.answer(callback, text).await.map_err(failed)
	}

	async fn chat(&self, chat: &str) -> Result<ChatInfo, own::Failure> {
		self.bot()?.chat(chat).await.map_err(failed)
	}

	async fn is_admin(&self, chat: i64, user: i64) -> Result<bool, own::Failure> {
		self.bot()?.is_admin(chat, user).await.map_err(failed)
	}
}

fn need<T>(network: &Option<T>, method: Method) -> Result<&T, Failure> {
	network.as_ref().ok_or_else(|| Failure::Refused(format!("{} is not configured on this server", method.as_ref())))
}

/// A network that names no message.
fn unnamed((): ()) -> String {
	String::new()
}

fn level(tier: Tier) -> Level {
	match tier {
		Tier::Urgent => Level::High,
		Tier::Important => Level::Normal,
		Tier::Info => Level::Low,
	}
}

fn rich(r: &Rendered) -> Rich {
	let entity = |e: &serde_json::Value| serde_json::from_value::<Entity>(e.clone()).expect("a type, an offset and a length, as checked when the note was taken");
	Rich {
		text: r.text.clone(),
		entities: r.entities.iter().map(entity).collect(),
	}
}

fn pressable(buttons: &[(String, String)]) -> Vec<Button> {
	buttons
		.iter()
		.map(|(label, data)| Button {
			label: label.clone(),
			data: data.clone(),
		})
		.collect()
}

/// Its first line, marked when urgent, and the rest.
fn split(m: &Message) -> (String, Rendered) {
	let (title, rest) = m.text.split_line();
	let urgent = if m.tier == Tier::Urgent { "🔴 " } else { "" };
	(format!("{urgent}{}", title.text), rest)
}

/// As Telegram takes it: the text whole, with its buttons.
fn whole(m: &Message) -> Result<Outgoing, Failure> {
	let links = m.links.iter().map(|(label, url)| {
		let url = Url::parse(url).map_err(|e| Failure::Refused(format!("the link {url:?}: {e}")))?;
		Ok::<_, Failure>(Link { label: label.clone(), url })
	});
	Ok(Outgoing {
		title: None,
		text: rich(&m.text),
		level: level(m.tier),
		links: links.collect::<Result<_, _>>()?,
		buttons: pressable(&m.buttons),
		photo: m.photo.as_ref().map(|bytes| Photo {
			name: "photo.png".to_owned(),
			mime: "image/png".to_owned(),
			bytes: bytes.clone(),
		}),
		unsubscribe: m.unsubscribe.clone(),
	})
}

/// Under a heading, its first line; `subject`: one even when nothing follows it.
fn headed(m: &Message, subject: bool) -> Result<Outgoing, Failure> {
	let (title, rest) = split(m);
	let whole = whole(m)?;
	Ok(match rest.text.trim().is_empty() && !subject {
		true => whole,
		false => Outgoing {
			title: Some(title),
			text: rich(&rest),
			..whole
		},
	})
}

fn failed(f: Failure) -> own::Failure {
	match f {
		Failure::RetryAfter(wait) => own::Failure::RetryAfter(wait),
		Failure::Transient(why) => own::Failure::Transient(why),
		Failure::Gone(why) => own::Failure::Gone(why),
		Failure::Refused(why) => own::Failure::Refused(why),
	}
}
