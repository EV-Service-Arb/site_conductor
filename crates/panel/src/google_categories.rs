//! Google's Business Profile categories, as `google_categories.json` lists them (docs/ARCHITECTURE.md,
//! A GBP's category).

use std::{collections::BTreeMap, sync::LazyLock};

use sa_core::{Invalid, gbp::Country};
use serde::Deserialize;

/// The front end's languages: every category is named in each, beside its countries' languages.
const READERS: [&str; 2] = ["en", "ru"];

/// One of Google's categories.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GoogleCategory {
	/// `gcid:plumber`.
	pub id: &'static str,
	/// By language: `fr` → `Plombier`.
	pub names: &'static BTreeMap<String, String>,
}
impl GoogleCategory {
	/// `id` as Google lists it in `country`.
	pub fn find(country: Country, id: &str) -> Result<Self, Invalid> {
		LIST.get_key_value(id)
			.filter(|(_, l)| l.countries.contains(&country))
			.map(|(id, l)| Self { id, names: &l.names })
			.ok_or_else(|| Invalid::new(format!("Google lists no category {id:?} in {country}")))
	}

	/// Every one Google lists in `country`, by id.
	pub fn all(country: Country) -> impl Iterator<Item = Self> {
		LIST.iter().filter(move |(_, l)| l.countries.contains(&country)).map(|(id, l)| Self { id, names: &l.names })
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
	countries: Vec<Country>,
	/// By language.
	names: BTreeMap<String, String>,
}

static LIST: LazyLock<BTreeMap<String, Listed>> = LazyLock::new(|| {
	let list: BTreeMap<String, Listed> = serde_json::from_str(include_str!("../google_categories.json")).expect("google_categories.json is Google's list");
	for (id, listed) in &list {
		assert!(id.starts_with("gcid:"), "{id}: a gcid");
		for c in &listed.countries {
			assert!(listed.names.contains_key(c.lang()), "{id} is listed in {c} without a name in {}", c.lang());
		}
		for lang in READERS {
			assert!(listed.names.contains_key(lang), "{id} has no name in {lang}");
		}
	}
	list
});
