//! The queries cache's wire: what was paid for a query, kept by the conductor under
//! `/api/queries_cache` for everyone, so nobody pays for the same query twice wherever it runs
//! (docs/ARCHITECTURE.md, Queries cache).
//!
//! ```text
//! GET    /api/queries_cache/{scheme}/{key}         → 200 Stored | 404
//! POST   /api/queries_cache/{scheme}/{key}/claim   ClaimBody → 200 Claim
//! DELETE /api/queries_cache/{scheme}/{key}/claim   → 204
//! PUT    /api/queries_cache/{scheme}/{key}         Put → 204
//! ```

use std::{fmt, str::FromStr};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::Invalid;

pub const PREFIX: &str = "/api/queries_cache";

/// How long a claim holds a key while its holder buys.
pub const LEASE: jiff::SignedDuration = jiff::SignedDuration::from_secs(30);

/// Whose digest a key is: each client derives its own from its requests.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Scheme {
	/// `sa_places`: the conductor's and review_archive's Places requests.
	Places,
	/// gmaps_optimal_placement's `Work::cached_post`.
	Gop,
}

impl Scheme {
	pub const ALL: [Self; 2] = [Self::Places, Self::Gop];

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Places => "places",
			Self::Gop => "gop",
		}
	}
}

impl FromStr for Scheme {
	type Err = Invalid;

	fn from_str(s: &str) -> Result<Self, Invalid> {
		Self::ALL
			.into_iter()
			.find(|k| k.as_str() == s)
			.ok_or_else(|| Invalid::new(format!("no queries cache scheme {s:?}")))
	}
}

impl fmt::Display for Scheme {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_str())
	}
}

/// A request's SHA-256, 64 hex characters on the wire.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Key(pub [u8; 32]);

impl FromStr for Key {
	type Err = Invalid;

	fn from_str(s: &str) -> Result<Self, Invalid> {
		let bytes = hex::decode(s).map_err(|_| Invalid::new("a query's key is 64 hex characters"))?;
		Ok(Self(bytes.try_into().map_err(|_| Invalid::new("a query's key is 64 hex characters"))?))
	}
}

impl fmt::Display for Key {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&hex::encode(self.0))
	}
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Stored {
	pub answer: serde_json::Value,
	pub fetched_at: Timestamp,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ClaimBody {
	/// An answer fetched before this is as good as none: a reader's age limit, or a re-ask.
	pub fresh_since: Option<Timestamp>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Claim {
	Stored(Stored),
	/// The caller buys it, then PUTs it.
	Granted {
		until: Timestamp,
	},
	/// Another caller is buying it: ask again.
	Held {
		until: Timestamp,
	},
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Put {
	pub answer: serde_json::Value,
	/// What was asked, headers aside: method, URL, field mask and body.
	pub request: String,
}

/// A query's path under the cache's URL (`<origin>/api/queries_cache`, what a client is given).
pub fn under(scheme: Scheme, key: Key) -> String {
	format!("/{scheme}/{key}")
}
