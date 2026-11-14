//! What costs: every action a vendor bills us for, or we bill a person for. Each tool is an enum of
//! its actions, declared once in the `priced!` below with where it happens, whether it is sold and
//! its unit: its variants' `message` is their name, their doc what an admin reads under it. The
//! admin's prices page, the conductor's tables, the snapshot and the usage all walk [`Item::all`],
//! so a variant added here is priced, costed and counted there with nothing else written.
//!
//! A price is the admin's; a sold item without one is not sold. A cost per unit is the admin's too.

use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{EnumMessage, VariantArray};

/// What each item costs, in tokens; an item absent is not sold.
pub type Prices = BTreeMap<Item, i64>;

/// Where it happens: who counts it, and charges for it if it is sold.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, VariantArray, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Scope {
	/// At the buy, before Google is asked.
	Conductor,
	/// After the fact, through its charge feed.
	ReviewArchive,
	/// sa_browsers' runner: nobody is charged.
	Browsers,
	/// gmaps_optimal_placement's studies: the house's for now.
	Placement,
}

/// Whether a person pays tokens for it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, VariantArray, strum::IntoStaticStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Sale {
	/// At the admin's price; unpriced, refused.
	Sold,
	/// Counted and costed; nobody is charged.
	Metered,
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
	/// Geocoding: the town a point on the map lies in, when a review kit's area is drawn.
	#[strum(message = "Town by a point")]
	Geocode,
}

/// What Street View on the browser's Maps key loads: Google bills each panorama opened (Dynamic
/// Street View), past its free ones a month.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum StreetViewLoad {
	/// The pegman dropped on the map: one panorama opened, walked around in for free.
	#[strum(message = "Panorama")]
	Panorama,
}

/// What the map in a person's browser loads on the browser's Maps key.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MapLoad {
	/// A `google.maps.Map` built: one a page session, kept while the page is away.
	#[strum(message = "Map")]
	Map,
}

/// A gmail's cloud phone running: Multilogin bills it by the minute.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PhoneTime {
	/// Each minute a cloud phone runs, from its start to its stop, a started one whole.
	#[strum(message = "Cloud phone")]
	Running,
}

/// A gmail's antidetect browser started: the providers bill a plan, a start is what it is spent on.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum BrowserStart {
	/// GoLogin's Orbita, headless on the runner.
	#[strum(message = "GoLogin")]
	Gologin,
	/// Multilogin's Mimic, through the Multilogin app on the runner.
	#[strum(message = "Multilogin browser")]
	Mimic,
}

/// What a study asks the Google Ads API for: free per request, within the developer token's
/// daily quota.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AdsRequest {
	/// `geoTargetConstants:suggest`: the Ads location a study's place is.
	#[strum(message = "Location")]
	Location,
	/// `generateKeywordIdeas`: a keyword group's monthly search volumes there.
	#[strum(message = "Search volumes")]
	KeywordIdeas,
}

/// What a study buys of DataForSEO, by the task.
#[derive(Clone, Copy, Debug, EnumMessage, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DataForSeoTask {
	/// `keywords_for_keywords/live`: keyword ideas and their volumes from Google Ads' data.
	#[strum(message = "Keywords for keywords")]
	KeywordsForKeywords,
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
	($($tool:ident($ty:ident) = $key:literal, $scope:ident, $sale:ident, $unit:literal, $title:literal;)+) => {
		/// One costly action: `<tool>.<variant>` on the wire.
		#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
		pub enum Item {
			$($tool($ty),)+
		}

		impl Item {
			/// Every costly action, tool by tool, each in its declaration order.
			pub fn all() -> impl Iterator<Item = Self> {
				std::iter::empty()$(.chain($ty::VARIANTS.iter().map(|v| Self::$tool(*v))))+
			}

			pub fn scope(self) -> Scope {
				match self {
					$(Self::$tool(_) => Scope::$scope,)+
				}
			}

			pub fn sale(self) -> Sale {
				match self {
					$(Self::$tool(_) => Sale::$sale,)+
				}
			}

			/// What one is counted in.
			pub fn unit(self) -> &'static str {
				match self {
					$(Self::$tool(_) => $unit,)+
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
	Places(PlacesRequest) = "places", Conductor, Sold, "request", "Google Places on the shared keys";
	StreetView(StreetViewLoad) = "street_view", Conductor, Sold, "panorama", "Google Street View on the map";
	Map(MapLoad) = "map", Conductor, Metered, "load", "Google Maps in the browser";
	Walk(WalkStep) = "walk", ReviewArchive, Sold, "step", "A walk down a place's reviews";
	Phone(PhoneTime) = "phone", Browsers, Metered, "minute", "Multilogin's cloud phones";
	Ads(AdsRequest) = "ads", Placement, Metered, "request", "Google Ads API";
	DataForSeo(DataForSeoTask) = "dataforseo", Placement, Metered, "task", "DataForSEO";
	Browser(BrowserStart) = "browser", Browsers, Metered, "start", "Antidetect browsers";
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
	}
}
