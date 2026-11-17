//! What changed, told to whoever is watching: the bus the server's `/api/v1/live` sockets
//! listen on.
//!
//! The conductor runs as one process, so the bus is in-process: a `tokio::sync::broadcast`
//! channel. What publishes:
//!
//! ```text
//! the store's pool      a table the conductor committed to (`conductor.<table>`)      → its topic
//! the services' feeds   a table one committed to (`sites.<table>`, …)                  → its topic
//!                       a rebuild there, a reconnect, the feed lagging                → resync
//! sessions closed       logout, a sign-in replacing one, a refusal  → the sockets of that session end
//! ```
//!
//! Publishing never waits and never fails a write: with nobody subscribed the message is
//! simply dropped, and a subscriber that falls behind by more than the channel holds is told
//! so ([`broadcast::error::RecvError::Lagged`]) instead of the channel growing.
//!
//! A command run in another process publishes on its own bus, which nobody listens to.

use std::time::Duration;

use jiff::Timestamp;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::session::SessionKey;

/// How many messages a subscriber may fall behind before it is told to read everything
/// afresh.
pub const DEFAULT_CAPACITY: usize = 1024;

/// A table committed to, everyone signed in told.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
	/// `<service>.<table>`.
	pub topic: String,
	/// When the conductor heard of it.
	pub at: Timestamp,
}

/// Which sessions have ended, so the sockets they opened close at once rather than at their
/// next check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ended {
	/// Every session of the user (a sign-out).
	User(Uuid),
	/// One session.
	Session(SessionKey),
}

/// What travels on the bus.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Signal {
	Changed(Change),
	/// Too much changed to say what: read everything again.
	Resync,
	SessionsEnded(Ended),
	/// The process is stopping.
	GoingAway,
}

/// The in-process bus. Cloning it gives another handle on the same channel.
#[derive(Clone, Debug)]
pub struct Bus {
	tx: broadcast::Sender<Signal>,
}
impl Bus {
	/// A bus whose subscribers may fall `capacity` messages behind (at least 1).
	pub fn new(capacity: usize) -> Self {
		Self {
			tx: broadcast::channel(capacity.max(1)).0,
		}
	}

	pub fn subscribe(&self) -> broadcast::Receiver<Signal> {
		self.tx.subscribe()
	}

	/// Tells every subscriber; never waits.
	pub fn publish(&self, signal: Signal) {
		// `Err` means nobody is subscribed, and then there is nobody to tell: the write it
		// follows has committed either way.
		let _nobody_listening = self.tx.send(signal);
	}

	pub fn changed(&self, topic: &str) {
		self.publish(Signal::Changed(Change {
			topic: topic.to_owned(),
			at: Timestamp::now(),
		}));
	}

	/// Waits until every subscriber has let go, or `within` has passed: at shutdown, so the
	/// sockets' close frames go out before the process ends.
	pub async fn drained(&self, within: Duration) {
		let deadline = tokio::time::Instant::now() + within;
		while self.tx.receiver_count() > 0 && tokio::time::Instant::now() < deadline {
			// A broadcast sender cannot be awaited for its receivers to go; they go within
			// moments of `GoingAway`, so a short poll is enough.
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	}
}

impl Default for Bus {
	fn default() -> Self {
		Self::new(DEFAULT_CAPACITY)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[tokio::test]
	async fn publishing_with_nobody_listening_is_fine() {
		let bus = Bus::new(2);
		bus.publish(Signal::Resync);
		let mut rx = bus.subscribe();
		bus.publish(Signal::Resync);
		assert_eq!(rx.recv().await.unwrap(), Signal::Resync);
		drop(rx);
		bus.drained(Duration::from_secs(1)).await;
	}

	#[tokio::test]
	async fn a_subscriber_behind_is_told_it_lagged() {
		let bus = Bus::new(2);
		let mut rx = bus.subscribe();
		for _ in 0..5 {
			bus.publish(Signal::Resync);
		}
		assert!(matches!(rx.recv().await, Err(broadcast::error::RecvError::Lagged(3))));
	}
}
