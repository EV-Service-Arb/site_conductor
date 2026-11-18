//! A vendor's own count of what it bills, against its free units a period: each meter declared
//! once in the `metered!` below, and the items of [`crate::prices`] drawing on it name it
//! (`#[strum(props(meter = "<vendor>:<id>"))]`).
//!
//! ```text
//! used = baseline   the vendor's count this period, as of `through`: every DB, every project
//!      + delta      our own uses of its items after `through`
//! ```

use std::{fmt, str::FromStr};

use jiff::{Timestamp, tz::TimeZone};
use serde::{Serialize, Serializer};

use crate::prices::Item;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Vendor {
	/// Its Cloud Billing export: a billing account's every project and service, hours behind.
	Google,
}

impl Vendor {
	/// The period a free allowance runs over, around `now`: `[start, end)`.
	pub fn period(self, now: Timestamp) -> (Timestamp, Timestamp) {
		match self {
			Self::Google => {
				// Google's billing month runs on Pacific time
				let zone = TimeZone::get("America/Los_Angeles").expect("bundled with jiff");
				let first = now.to_zoned(zone.clone()).date().first_of_month();
				let at = |d: jiff::civil::Date| d.to_zoned(zone.clone()).expect("a month's first midnight exists in Los Angeles").timestamp();
				(at(first), at(first.checked_add(jiff::Span::new().months(1)).expect("a month after is a date")))
			}
		}
	}
}

/// What happens once the free units are used.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
	/// Nothing more is bought.
	AtFree,
	/// Watched only.
	Never,
}

/// One line on a vendor's bill.
#[derive(Debug, Eq, PartialEq)]
pub struct Meter {
	pub vendor: Vendor,
	/// The vendor's own id of it: Google's SKU.
	pub id: &'static str,
	/// As the vendor names it.
	pub name: &'static str,
	/// Free units a period.
	pub free: i64,
	pub stop: Stop,
}
impl Meter {
	/// The items drawing on it.
	pub fn items(&'static self) -> impl Iterator<Item = Item> {
		Item::all().filter(move |i| i.meter() == Some(self))
	}
}

macro_rules! metered {
	($($vendor:ident $id:literal $name:literal, $free:literal free, $stop:ident;)+) => {
		/// Every meter.
		pub const ALL: &[Meter] = &[$(Meter { vendor: Vendor::$vendor, id: $id, name: $name, free: $free, stop: Stop::$stop },)+];
	};
}

metered! {
	Google "658E-F885-E11A" "Dynamic Street View", 5000 free, AtFree;
	Google "FAF4-3B2D-51B2" "Dynamic Maps", 10000 free, Never;
	Google "4ED6-464A-2AFC" "Places API Place Details Pro", 5000 free, Never;
	Google "4FDA-34B1-A910" "Places API Text Search Pro", 5000 free, Never;
	Google "4EF4-B17C-B31A" "Autocomplete Requests", 10000 free, Never;
	Google "BAC8-4E68-E261" "Geocoding", 10000 free, Never;
	Google "2D9A-3DE0-3766" "Places API Place Details Enterprise", 1000 free, Never;
	Google "DCD1-FE97-8C71" "Places API Place Details Photos", 1000 free, Never;
}

impl fmt::Display for Meter {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}:{}", <&str>::from(self.vendor), self.id)
	}
}

impl Serialize for &'static Meter {
	fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
		s.collect_str(self)
	}
}

/// Not one of [`ALL`].
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("{0:?} is no meter")]
pub struct UnknownMeter(pub String);

impl FromStr for &'static Meter {
	type Err = UnknownMeter;

	fn from_str(s: &str) -> Result<Self, Self::Err> {
		ALL.iter().find(|m| m.to_string() == s).ok_or_else(|| UnknownMeter(s.to_owned()))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn a_google_month_starts_at_pacific_midnight() {
		let (start, end) = Vendor::Google.period("2026-11-01T07:30:00Z".parse().unwrap());
		assert_eq!(start, "2026-11-01T07:00:00Z".parse().unwrap(), "PDT until Nov 1");
		assert_eq!(end, "2026-12-01T08:00:00Z".parse().unwrap(), "PST in December");
		let (start, _) = Vendor::Google.period("2026-11-01T06:59:59Z".parse().unwrap());
		assert_eq!(start, "2026-10-01T07:00:00Z".parse().unwrap());
	}

	#[test]
	fn every_meter_is_drawn_on() {
		for m in ALL {
			assert!(m.items().next().is_some(), "{m} has no item");
			assert_eq!(m.to_string().parse::<&Meter>(), Ok(m));
		}
	}
}
