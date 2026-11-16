//! What costs: every action a vendor bills us for, or we bill a person for. Each tool is an enum of
//! its actions, declared once in the `priced!` below with where it happens, whether it is sold and
//! its unit: its variants' `message` is their name, their doc what an admin reads under it. The
//! admin's prices page, the conductor's tables, the snapshot and the usage all walk [`Item::all`],
//! so a variant added here is priced, costed and counted there with nothing else written.
//!
//! A price is the admin's; a sold item without one is not sold. A cost per unit is the admin's too.

use std::{collections::BTreeMap, fmt, marker::PhantomData, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{EnumMessage, EnumProperty, VariantArray};
use uuid::Uuid;

use crate::meters::Meter;

/// What each item costs, in tokens; an item absent is not sold.
pub type Prices = BTreeMap<Item, i64>;

#[cfg(feature = "api")]
pub use spends::{Payer, Spends};

/// What one operation may buy: a tool's items, or several tools'.
pub trait Tool {
	fn items() -> Vec<Item>;
}

/// Where it happens: who counts it, and charges for it if it is sold.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, VariantArray, strum::IntoStaticStr, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Scope {
	/// At the buy, before Google is asked.
	Conductor,
	/// After the fact, through its charge feed.
	ReviewArchive,
	/// sa_browsers' runner: nobody is charged.
	Browsers,
	/// gmaps_optimal_placement's studies: a pull's searches charged at the conductor's forward.
	Placement,
}

/// Whether a person pays tokens for it.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, VariantArray, strum::IntoStaticStr, schemars::JsonSchema)]
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
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlacesRequest {
	/// Place Details Pro: a place by its id, with its address and point.
	#[strum(message = "Lookup (details)", props(meter = "google:4ED6-464A-2AFC"))]
	Details,
	/// Text Search Pro: places by a name or an address.
	#[strum(message = "Search", props(meter = "google:4FDA-34B1-A910"))]
	Search,
	/// Autocomplete, per request (no session token): suggestions while an address is typed.
	#[strum(message = "Suggestions (as typed)", props(meter = "google:4EF4-B17C-B31A"))]
	Autocomplete,
	/// Geocoding: the town a point on the map lies in, when a review kit's area is drawn.
	#[strum(message = "Town by a point", props(meter = "google:BAC8-4E68-E261"))]
	Geocode,
}

/// What Street View on the browser's Maps key loads: Google bills each panorama opened (Dynamic
/// Street View), past its free ones a month.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum StreetViewLoad {
	/// The pegman dropped on the map: one panorama opened, walked around in for free.
	#[strum(message = "Panorama", props(meter = "google:658E-F885-E11A"))]
	Panorama,
}

/// What the map in a person's browser loads on the browser's Maps key.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum MapLoad {
	/// A `google.maps.Map` built: one a page session, kept while the page is away.
	#[strum(message = "Map", props(meter = "google:FAF4-3B2D-51B2"))]
	Map,
}

/// A gmail's cloud phone running: Multilogin bills it by the minute.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PhoneTime {
	/// Each minute a cloud phone runs, from its start to its stop, a started one whole.
	#[strum(message = "Cloud phone")]
	Running,
}

/// A gmail's antidetect browser started: the providers bill a plan, a start is what it is spent on.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum BrowserStart {
	/// GoLogin's Orbita, headless on the runner.
	#[strum(message = "GoLogin")]
	Gologin,
	/// Multilogin's Mimic, through the Multilogin app on the runner.
	#[strum(message = "Multilogin browser")]
	Mimic,
}

/// What a study's pull buys, charged to whoever pulls it.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum PlacementRequest {
	/// A Places search a study's pull buys.
	#[strum(message = "Search")]
	Search,
}

/// What a study asks the Google Ads API for: free per request, within the developer token's
/// daily quota.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum AdsRequest {
	/// `geoTargetConstants:suggest`: the Ads location a study's place is.
	#[strum(message = "Location")]
	Location,
	/// `generateKeywordIdeas`: a keyword group's monthly search volumes there.
	#[strum(message = "Search volumes")]
	KeywordIdeas,
	/// `generateKeywordHistoricalMetrics`: a category's keywords' volumes, which weigh its terms.
	#[strum(message = "Keyword volumes")]
	HistoricalMetrics,
}

/// What a study buys of DataForSEO, by the task.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum DataForSeoTask {
	/// `keywords_for_keywords/live`: keyword ideas and their volumes from Google Ads' data.
	#[strum(message = "Keywords for keywords")]
	KeywordsForKeywords,
	/// `search_volume/live`: a category's keywords' volumes, which weigh its terms.
	#[strum(message = "Search volume")]
	SearchVolume,
}

/// What the assistant's model reads and writes, by how Anthropic bills it.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum ModelTokens {
	/// The prompt's tokens not read from its cache.
	#[strum(message = "Input")]
	Input,
	/// The prompt's tokens written to its cache.
	#[strum(message = "Cache write")]
	CacheWrite,
	/// The prompt's tokens read from its cache.
	#[strum(message = "Cache read")]
	CacheRead,
	/// What the model wrote, its thinking with it.
	#[strum(message = "Output")]
	Output,
}

/// What a walk down a place's reviews does in its browser, each time it does it.
#[derive(Clone, Copy, Debug, EnumMessage, EnumProperty, Eq, Hash, Ord, PartialEq, PartialOrd, VariantArray, strum::IntoStaticStr)]
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

			/// The vendor's meter it draws on, if it has one.
			pub fn meter(self) -> Option<&'static Meter> {
				let key = match self {
					$(Self::$tool(v) => v.get_str("meter"),)+
				};
				key.map(|k| k.parse().expect("a meter prop names one of meters::ALL"))
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
		}

		impl Tool for $ty {
			fn items() -> Vec<Item> {
				$ty::VARIANTS.iter().map(|v| Item::$tool(*v)).collect()
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
	Placement(PlacementRequest) = "placement", Placement, Sold, "search", "A study's pull";
	Ads(AdsRequest) = "ads", Placement, Metered, "request", "Google Ads API";
	DataForSeo(DataForSeoTask) = "dataforseo", Placement, Metered, "task", "DataForSEO";
	Browser(BrowserStart) = "browser", Browsers, Metered, "start", "Antidetect browsers";
	Claude(ModelTokens) = "claude", Conductor, Metered, "token", "Claude, the assistant's model";
}

impl<A: Tool, B: Tool> Tool for (A, B) {
	fn items() -> Vec<Item> {
		[A::items(), B::items()].concat()
	}
}

/// Who buys of `T`'s items are made for (`W`, as their service names a person), charged for those
/// sold. What spends takes one, so a handler gets it from its `Spends` and its operation says what
/// it spends.
pub struct Spender<T, W = Uuid> {
	who: W,
	tool: PhantomData<fn() -> T>,
}

impl<T, W: Copy> Spender<T, W> {
	pub fn new(who: W) -> Self {
		Self { who, tool: PhantomData }
	}

	pub fn user(&self) -> W {
		self.who
	}
}

impl<T, W: Copy> Clone for Spender<T, W> {
	fn clone(&self) -> Self {
		*self
	}
}
impl<T, W: Copy> Copy for Spender<T, W> {}

#[cfg(feature = "api")]
mod spends {
	use axum::{extract::FromRequestParts, http::request::Parts};
	use ev_lib::api::{
		GenContext, OperationInput,
		openapi::{Operation, Response, StatusCode},
	};

	use super::{Spender, Tool};

	/// A service's caller, as whom they pay.
	pub trait Payer {
		type Who: Copy;
		fn payer(&self) -> Self::Who;
	}

	impl Payer for sa_auth::User {
		type Who = uuid::Uuid;

		fn payer(&self) -> uuid::Uuid {
			self.sub.parse().expect("the conductor asserts a concierge user id")
		}
	}

	/// A handler's caller `E`, and the [`Spender`] its buys of `T`'s items are made for: the
	/// operation's `x-spends` is those items.
	pub struct Spends<T, E: Payer>(pub E, pub Spender<T, E::Who>);

	impl<S: Send + Sync, T, E: FromRequestParts<S> + Payer + Send> FromRequestParts<S> for Spends<T, E> {
		type Rejection = E::Rejection;

		async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
			let caller = E::from_request_parts(parts, state).await?;
			let by = Spender::new(caller.payer());
			Ok(Self(caller, by))
		}
	}

	impl<T: Tool, E: OperationInput + Payer> OperationInput for Spends<T, E> {
		fn operation_input(ctx: &mut GenContext, op: &mut Operation) {
			E::operation_input(ctx, op);
			let items = T::items().iter().map(|i| serde_json::Value::String(i.to_string())).collect();
			assert!(op.extensions.insert("x-spends".into(), serde_json::Value::Array(items)).is_none(), "one Spends a handler");
		}

		fn inferred_early_responses(ctx: &mut GenContext, op: &mut Operation) -> Vec<(Option<StatusCode>, Response)> {
			E::inferred_early_responses(ctx, op)
		}
	}
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

impl schemars::JsonSchema for Item {
	fn schema_name() -> std::borrow::Cow<'static, str> {
		"PricedItem".into()
	}

	fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
		schemars::json_schema!({ "type": "string", "enum": Self::all().map(|i| i.to_string()).collect::<Vec<_>>() })
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
			item.meter();
		}
	}
}
