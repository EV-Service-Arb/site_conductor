//! A GBP as the conductor defines it, for every service that speaks of one: the states it moves
//! through, its country (which its reviews' language follows), and the snapshot review_archive is
//! pushed (`PUT /api/conductor/gmbs`). The `ts` and `utoipa` features derive on these types only.

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Invalid;

/// ISO 3166-1 alpha-2, of the countries GBPs are in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
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
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
	/// Where a GBP in `self` may move.
	pub fn nexts(self) -> &'static [Self] {
		match self {
			Self::Draft => &[Self::Verifying],
			Self::Verifying => &[Self::Verified],
			Self::Verified => &[Self::Suspended, Self::Verifying],
			Self::Suspended => &[Self::Verified, Self::Verifying],
		}
	}

	/// `to`, if [`Self::nexts`] has it.
	pub fn next(self, to: Self) -> Result<Self, Invalid> {
		match self.nexts().contains(&to) {
			true => Ok(to),
			false => Err(Invalid::new(format!("a {self} GBP cannot become {to}"))),
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

/// A [`State`] with a place on Google: every one but a draft.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(rename_all = "lowercase")]
pub enum OnGoogle {
	/// Waiting on Google's verification.
	Verifying,
	/// Live: its track is walked.
	Verified,
	/// Suspended by Google.
	Suspended,
}

impl OnGoogle {
	pub fn as_str(self) -> &'static str {
		State::from(self).as_str()
	}
}

impl From<OnGoogle> for State {
	fn from(s: OnGoogle) -> Self {
		match s {
			OnGoogle::Verifying => Self::Verifying,
			OnGoogle::Verified => Self::Verified,
			OnGoogle::Suspended => Self::Suspended,
		}
	}
}

impl TryFrom<State> for OnGoogle {
	type Error = Invalid;

	fn try_from(s: State) -> Result<Self, Invalid> {
		match s {
			State::Draft => Err(Invalid::new("a GBP on Google is verifying, verified or suspended; a draft is created by its name".to_owned())),
			State::Verifying => Ok(Self::Verifying),
			State::Verified => Ok(Self::Verified),
			State::Suspended => Ok(Self::Suspended),
		}
	}
}

impl FromStr for OnGoogle {
	type Err = Invalid;

	fn from_str(raw: &str) -> Result<Self, Invalid> {
		raw.parse::<State>()?.try_into()
	}
}

impl fmt::Display for OnGoogle {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_str())
	}
}

/// A GBP with a place, as the services are told of it.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Managed {
	/// The conductor's id.
	pub id: i64,
	/// Its gmail's id.
	pub gmail_id: i64,
	/// Its gmail's address.
	pub gmail: String,
	/// The gmail's owner, a concierge `sub`.
	pub owner: Uuid,
	/// The GBP, or its gmail, is removed.
	pub removed: bool,
	/// Its owner's switch; off, the services leave it be.
	pub enabled: bool,
	/// The Google place id.
	pub place_id: String,
	pub state: OnGoogle,
	/// `null` only for a removed GBP from before countries.
	pub country: Option<Country>,
	pub name: String,
	pub address: Option<String>,
}

/// `PUT /api/conductor/gmbs`: every GBP that ever had a place, and every balance, at once.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "utoipa", derive(utoipa::ToSchema))]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
	/// Removed ones included.
	pub gmbs: Vec<Managed>,
	/// By `sub`, renewed up to the snapshot.
	pub balances: BTreeMap<Uuid, i64>,
	/// The last of review_archive's charges in these balances.
	pub charges_through: i64,
}
