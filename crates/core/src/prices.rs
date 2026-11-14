//! What costs a person tokens. Each tool that charges is an enum of what it charges for, declared
//! once in the `priced!` below: its variants' `message` is their name, their doc what an admin
//! reads under it. The admin's prices page, the conductor's table and the snapshot all walk
//! [`Item::all`], so a variant added here is priced there with nothing else written.
//!
//! A price is the admin's; an item without one is not sold.

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{EnumMessage, VariantArray};

/// What each item costs, in tokens; an item absent is not sold.
pub type Prices = BTreeMap<Item, i64>;

/// Who charges.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum Scope {
	/// At the buy, before Google is asked.
	Conductor,
	/// After the fact, through its charge feed.
	ReviewArchive,
}

/// A Places request on the shared keys, by how Google bills it: both masks ask a display name,
/// which makes them the Pro SKUs.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlacesRequest {
	/// Place Details Pro: a place by its id, with its address and point.
	#[strum(message = "Lookup (details)")]
	Details,
	/// Text Search Pro: places by a name or an address.
	#[strum(message = "Search")]
	Search,
	/// Autocomplete, per request (no session token): suggestions while an address is typed.
	#[strum(message = "Suggestions (as typed)")]
	Autocomplete,
}

/// What a walk down a place's reviews does in its browser, each time it does it.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum WalkStep {
	/// Opening the place's page, and answering the consent page.
	#[strum(message = "Open the place")]
	Open,
	/// The Reviews tab, and its histogram.
	#[strum(message = "Reviews tab")]
	Reviews,
	/// The sort menu and "Newest".
	#[strum(message = "Sort by newest")]
	Sort,
	/// Each click on the sort button past the first.
	#[strum(message = "Sort, again")]
	SortRetry,
	/// One scroll of the feed, and the screen of cards it loads.
	#[strum(message = "Scroll")]
	Step,
}

macro_rules! priced {
	($($tool:ident($ty:ident) = $key:literal, $scope:ident, $title:literal;)+) => {
		/// One priced thing: `<tool>.<variant>` on the wire.
		#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub enum Item {
			$($tool($ty),)+
		}

		impl Item {
			/// Every priced thing, tool by tool, each in its declaration order.
			pub fn all() -> impl Iterator<Item = Self> {
				std::iter::empty()$(.chain($ty::VARIANTS.iter().map(|v| Self::$tool(*v))))+
			}

			pub fn scope(self) -> Scope {
				match self {
					$(Self::$tool(_) => Scope::$scope,)+
				}
			}

			/// The tool's key, the part before the dot.
			pub fn tool(self) -> &'static str {
				match self {
					$(Self::$tool(_) => $key,)+
				}
			}

			/// What the tool is, as an admin reads it.
			pub fn tool_title(self) -> &'static str {
				match self {
					$(Self::$tool(_) => $title,)+
				}
			}

			fn variant(self) -> (&'static str, Option<&'static str>, Option<&'static str>) {
				match self {
					$(Self::$tool(v) => (v.into(), v.get_message(), v.get_documentation()),)+
				}
			}
		}

		$(impl From<$ty> for Item {
			fn from(v: $ty) -> Self {
				Self::$tool(v)
			}
		})+
	};
}

priced! {
	Places(PlacesRequest) = "places", Conductor, "Google Places on the shared keys";
	Walk(WalkStep) = "walk", ReviewArchive, "A walk down a place's reviews";
}

impl Item {
	/// Its name, as an admin reads it.
	pub fn title(self) -> &'static str {
		self.variant().1.expect("every priced variant has a message")
	}

	/// What it is that costs, as an admin reads it.
	pub fn description(self) -> &'static str {
		self.variant().2.expect("every priced variant has a doc")
	}
}

impl fmt::Display for Item {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}.{}", self.tool(), self.variant().0)
	}
}

/// Not one of [`Item::all`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0:?} is nothing priced")]
pub struct UnknownItem(pub String);

impl FromStr for Item {
	type Err = UnknownItem;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		Self::all().find(|i| i.to_string() == s).ok_or_else(|| UnknownItem(s.to_owned()))
	}
}

impl Serialize for Item {
	fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
		s.collect_str(self)
	}
}

impl<'de> Deserialize<'de> for Item {
	fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
		String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn every_item_is_named_described_and_round_trips() {
		for item in Item::all() {
			assert!(!item.title().is_empty(), "{item}");
			assert!(!item.description().is_empty(), "{item}");
			assert_eq!(item.to_string().parse::<Item>(), Ok(item));
		}
		assert_eq!(Item::all().count(), PlacesRequest::VARIANTS.len() + WalkStep::VARIANTS.len());
	}
}
