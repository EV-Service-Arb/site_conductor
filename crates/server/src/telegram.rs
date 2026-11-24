//! The conductor's Telegram bot polled for updates. What a message says and where it goes, and what
//! a button does, is the engine's (`sa_panel::alerts`, `sa_panel::telegram`).
//!
//! **Updates arrive by long polling, not a webhook.** A webhook would need a public route and a
//! secret header check on it — surface for anyone on the internet — while `getUpdates` needs
//! only egress to `api.telegram.org`. Telegram answers a second poller 409, so one replica
//! polls, under a lease in the database that another takes over when it lapses; the offset lives
//! there too, so a new holder resumes where the last one stopped. Every update is handled
//! idempotently (a link token redeems once, a button's press carries the same Idempotency-Key),
//! so an update handled twice across a takeover does nothing twice.
//!
//! Without `TELEGRAM_BOT_TOKEN` none of this runs: `serve` warns, and Telegram is not among the
//! methods the Alerts page offers.

use std::{
	sync::{Arc, OnceLock},
	time::Duration,
};

use jiff::{SignedDuration, Timestamp};
use sa_panel::alerts::{Feeds, Hub, Outlets};
use social_networks_adapters::notify::telegram::{Bot, parse_update};
use tokio::sync::watch;
use uuid::Uuid;

use crate::concierge::Concierge;

/// How long `getUpdates` holds a poll open when nothing arrives.
const POLL_SECONDS: u64 = 25;

/// The poller's lease: renewed every poll, so it lapses only when its holder is gone.
const POLL_LEASE: SignedDuration = SignedDuration::from_secs(60);

/// A replica without the poller's lease asks for it again this often.
const STANDBY_EVERY: Duration = Duration::from_secs(10);

/// The bot's username, once known (from `TELEGRAM_BOT_USERNAME`, or `getMe` at start).
#[derive(Clone, Debug, Default)]
pub struct BotName(Arc<OnceLock<String>>);
impl BotName {
	/// Its name is `known`, or learned later by [`Self::learn`].
	pub fn new(known: Option<String>) -> Self {
		let cell = OnceLock::new();
		if let Some(name) = known {
			cell.set(name.trim_start_matches('@').to_owned()).expect("a fresh cell");
		}
		Self(Arc::new(cell))
	}

	pub fn learn(&self, name: String) {
		// A name already set (from the environment) wins over getMe's.
		let _kept = self.0.set(name);
	}

	pub fn get(&self) -> Option<&str> {
		self.0.get().map(String::as_str)
	}
}

/// Polls for updates and handles them until `shutdown` turns true, on the replica holding the
/// poller's lease; first learns the bot's name, which the link waits for.
pub async fn poll<O: Outlets, F: Feeds>(hub: &Hub<O, Concierge, F>, api: &Bot, name: &BotName, mut shutdown: watch::Receiver<bool>) {
	let holder = Uuid::now_v7();
	let stopped = |s: &watch::Receiver<bool>| *s.borrow();
	while name.get().is_none() && !stopped(&shutdown) {
		match api.me().await {
			Ok(username) => {
				tracing::info!(username, "telegram: the bot is up");
				name.learn(username);
			}
			Err(f) => {
				tracing::warn!(failure = ?f, "telegram: getMe failed; linking waits for it");
				tokio::select! {
					() = tokio::time::sleep(STANDBY_EVERY) => {}
					_ = shutdown.changed() => {}
				}
			}
		}
	}
	while !stopped(&shutdown) {
		let now = Timestamp::now();
		let offset = match hub.panel.telegram_poll_lease(holder, now, now + POLL_LEASE).await {
			Ok(Some(offset)) => offset,
			Ok(None) => {
				tokio::select! {
					() = tokio::time::sleep(STANDBY_EVERY) => {}
					_ = shutdown.changed() => {}
				}
				continue;
			}
			Err(e) => {
				crate::report(&e, "leasing the telegram poller");
				tokio::time::sleep(STANDBY_EVERY).await;
				continue;
			}
		};
		let updates = tokio::select! {
			u = api.updates(offset, Duration::from_secs(POLL_SECONDS)) => u,
			_ = shutdown.changed() => break,
		};
		let updates = match updates {
			Ok(u) => u,
			Err(f) => {
				tracing::warn!(failure = ?f, "telegram: getUpdates failed");
				tokio::time::sleep(Duration::from_secs(3)).await;
				continue;
			}
		};
		for (id, raw) in updates {
			if let Some(update) = parse_update(&raw)
				&& let Err(e) = hub.handle(update, Timestamp::now()).await
			{
				// Past it anyway: an update that fails every time must not stop the bot; the
				// user presses again.
				crate::report(&e, "handling a telegram update");
			}
			match hub.panel.telegram_poll_advance(holder, id + 1).await {
				Ok(true) => {}
				Ok(false) => break,
				Err(e) => {
					crate::report(&e, "advancing the telegram poller");
					break;
				}
			}
		}
	}
	if let Err(e) = hub.panel.telegram_poll_release(holder).await {
		crate::report(&e, "releasing the telegram poller");
	}
}
