//! What was spent on vendors, as a service tells the conductor: its feed
//! (`GET /api/conductor/usage?after=&limit=`) is read under a Conductor assertion, from a cursor,
//! like the charge feed.

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::prices::Item;

/// Whose bill it lands on.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Bill {
	/// Our keys, accounts and machines.
	Ours,
	/// A person's own key.
	Theirs,
}

/// One Google answer bought, as the queries cache is told it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Billed {
	pub item: Item,
	pub bill: Bill,
}

/// `units` of `item`, done at `at`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Used {
	/// Ascending, never reused.
	pub id: i64,
	pub item: Item,
	/// More than 0, in the item's unit.
	pub units: i64,
	pub at: Timestamp,
}

/// A page of a usage feed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UsagePage {
	/// Ascending by id.
	pub used: Vec<Used>,
	/// The last id of the page; `after` for an empty one.
	pub next: i64,
}
