//! The conductor without I/O: no database, network, clock or randomness. Time comes in as an
//! argument.
//!
//! - [`gbp`]: a GBP's states and moves, its country and language, a category's keywords, and the
//!   snapshot the services are told;
//! - [`ids`]: what a source key may name (brands), how its key id looks, and a site key's origin;
//! - [`geo`]: points and rectangles, as Google draws them;
//! - [`placement`]: the study the conductor hands placement, and its address;
//! - [`signature`]: how a source signs a batch, and how the conductor checks it;
//! - [`queries_cache`]: the queries cache's wire;
//! - [`notify`]: the Telegram rules, the conductor's own messages, the buttons' signed data, and
//!   how a delivery is retried;
//! - [`prices`]: what costs tokens, tool by tool;
//! - [`meters`]: a vendor's own count of what it bills, and its free units;
//! - [`tokens`]: how a balance renews and a charge falls on its payers;
//! - [`usage`]: what was spent on vendors, as a service's feed tells it.

pub mod gbp;
pub mod geo;
pub mod ids;
pub mod meters;
pub mod notify;
pub mod placement;
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
