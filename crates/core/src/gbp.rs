//! A GBP's country, which its reviews' language follows, and the states it moves through.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::Invalid;

/// ISO 3166-1 alpha-2, of the countries GBPs are in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Country {
	FR,
	US,
	CA,
	GB,
}

impl Country {
	/// The language its Maps pages, and so its reviews' dates, are read in.
	pub fn lang(self) -> &'static str {
		match self {
			Self::FR => "fr",
			Self::US | Self::CA | Self::GB => "en",
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::FR => "FR",
			Self::US => "US",
			Self::CA => "CA",
			Self::GB => "GB",
		}
	}
}

impl FromStr for Country {
	type Err = Invalid;

	fn from_str(raw: &str) -> Result<Self, Invalid> {
		match raw {
			"FR" => Ok(Self::FR),
			"US" => Ok(Self::US),
			"CA" => Ok(Self::CA),
			"GB" => Ok(Self::GB),
			_ => Err(Invalid::new(format!("country {raw:?}: one of FR, US, CA, GB"))),
		}
	}
}

impl fmt::Display for Country {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_str())
	}
}

/// ```text
/// Draft ─▶ Verifying ─▶ Verified ⇄ Suspended
///             ▲ └──────────┘          │
///             └───────────────────────┘
/// ```
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
	/// No place on Google yet.
	Draft,
	/// On Google, Google verifying it.
	Verifying,
	Verified,
	Suspended,
}

impl State {
	/// `to`, if a GBP in `self` may move there.
	pub fn next(self, to: Self) -> Result<Self, Invalid> {
		use State::*;
		match (self, to) {
			(Draft, Verifying) | (Verifying, Verified) | (Verified, Suspended | Verifying) | (Suspended, Verified | Verifying) => Ok(to),
			_ => Err(Invalid::new(format!("a {self} GBP cannot become {to}"))),
		}
	}

	pub fn as_str(self) -> &'static str {
		match self {
			Self::Draft => "draft",
			Self::Verifying => "verifying",
			Self::Verified => "verified",
			Self::Suspended => "suspended",
		}
	}
}

impl FromStr for State {
	type Err = Invalid;

	fn from_str(raw: &str) -> Result<Self, Invalid> {
		match raw {
			"draft" => Ok(Self::Draft),
			"verifying" => Ok(Self::Verifying),
			"verified" => Ok(Self::Verified),
			"suspended" => Ok(Self::Suspended),
			_ => Err(Invalid::new(format!("state {raw:?}: one of draft, verifying, verified, suspended"))),
		}
	}
}

impl fmt::Display for State {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_str())
	}
}
