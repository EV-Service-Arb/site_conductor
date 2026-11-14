//! A GBP as the conductor defines it, for every service that speaks of one: the states it moves
//! through, its country (which its reviews' and its category's keywords' language follows), and
//! the snapshot review_archive is pushed (`PUT /api/conductor/gmbs`). The `ts` and `utoipa`
//! features derive on these types only.

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Invalid;

/// ISO 3166-1 alpha-2, of the countries GBPs are in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, strum::VariantArray)]
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

/// A phone as a GBP holds it, one live GBP's: an optional `+`, then digits, from what a person
/// types (digits, spaces, `+-().`).
pub fn phone(raw: &str) -> Result<String, Invalid> {
	let typed = raw.trim();
	let refused = || Invalid::new(format!("{raw:?} is not a phone number"));
	if typed.is_empty() || !typed.chars().all(|c| c.is_ascii_digit() || "+-(). ".contains(c)) {
		return Err(refused());
	}
	let canonical: String = typed.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
	let digits = canonical.strip_prefix('+').unwrap_or(&canonical);
	match !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
		true => Ok(canonical),
		false => Err(refused()),
	}
}

/// A category's keywords as stored: each trimmed, its inner whitespace one space, lowercased, at
/// most 80 chars; at most 50, none twice. A keyword names no city: the grid point is the location,
/// and a city term is one location's own. No weights: a term weighs its search volume where it is
/// read.
pub fn keywords(raw: &[String]) -> Result<Vec<String>, Invalid> {
	let mut out: Vec<String> = Vec::with_capacity(raw.len());
	for typed in raw {
		let k = typed.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
		if k.is_empty() {
			return Err(Invalid::new("a keyword is not empty"));
		}
		if k.chars().count() > 80 {
			return Err(Invalid::new(format!("{k:?} is over 80 characters")));
		}
		if out.contains(&k) {
			return Err(Invalid::new(format!("{typed:?} is the keyword {k:?} twice")));
		}
		out.push(k);
	}
	match out.len() {
		51.. => Err(Invalid::new(format!("{} keywords: a category takes 50", out.len()))),
		_ => Ok(out),
	}
}

/// What a gmail is on the GBPs it manages.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum GmailRole {
	Owner,
	Manager,
}

impl GmailRole {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Owner => "owner",
			Self::Manager => "manager",
		}
	}
}

impl FromStr for GmailRole {
	type Err = Invalid;

	fn from_str(raw: &str) -> Result<Self, Invalid> {
		match raw {
			"owner" => Ok(Self::Owner),
			"manager" => Ok(Self::Manager),
			_ => Err(Invalid::new(format!("role {raw:?}: owner or manager"))),
		}
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
	/// Not on Google yet: no place.
	Draft,
	/// On Google, Google verifying it: not on Maps, so its place may be unknown.
	Verifying,
	/// On Maps: its place is certain.
	Verified,
	/// Off Maps: its place, if it had none, may still be unknown.
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

/// A [`State`] on Google: every one but a draft.
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

/// A GBP with a place, as the services are told of it: one on Google without one is not.
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

/// `PUT /api/conductor/gmbs`: every GBP that ever had a place, every balance and every price, at once.
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
	/// What its walks cost; a step without a price is not walked.
	#[cfg_attr(feature = "utoipa", schema(value_type = BTreeMap<String, i64>))]
	pub prices: crate::prices::Prices,
}
