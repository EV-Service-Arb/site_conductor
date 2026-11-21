//! The conductor without I/O: no database, network, clock or randomness. Time comes in as an
//! argument. Feature `api` is the exception: a service's end of the conductor's wire, in its router.
//!
//! - [`gbp`]: a GBP's states and moves, its country and language, and the snapshot the services
//!   are told;
//! - [`agent`]: how far an agent acts on its own; (feature `api`) an operation that is its own agent tool;
//! - [`dashboards`] (feature `api`): a service's Grafana dashboards, over its document;
//! - [`feeds`]: what the conductor's hubs read of a service and push to it, path and page;
//! - [`ids`]: what a source key may name (brands), how its key id looks, and a site key's origin;
//! - [`geo`]: points and rectangles, as Google draws them;
//! - [`signature`]: how a source signs a batch, and how the conductor checks it;
//! - [`queries_cache`]: the queries cache's wire;
//! - [`notify`]: the Telegram rules, the conductor's own messages, the buttons' signed data, and
//!   how a delivery is retried;
//! - [`prices`]: what costs tokens, tool by tool;
//! - [`meters`]: a vendor's own count of what it bills, and its free units;
//! - [`tokens`]: how a balance renews and a charge falls on its payers;
//! - [`usage`]: what was spent on vendors, as a service's feed tells it.

pub mod agent;
#[cfg(feature = "api")]
pub mod dashboards;
pub mod feeds;
pub mod gbp;
pub mod geo;
pub mod ids;
pub mod meters;
pub mod notify;
pub mod prices;
pub mod queries_cache;
pub mod signature;
pub mod tokens;
pub mod usage;

/// Why an input cannot be accepted. The message is what the caller is told, so it names the
/// field and never echoes a secret or PII.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct Invalid(pub String);

impl Invalid {
	pub fn new(msg: impl Into<String>) -> Self {
		Self(msg.into())
	}
}

impl From<sa_auth::UnknownKind> for Invalid {
	fn from(e: sa_auth::UnknownKind) -> Self {
		Self(e.to_string())
	}
}
