//! What the conductor hands placement to have a study built: its area and its category, named as
//! the conductor names them, addressed by the SHA-256 of their canonical form; and what a pull
//! bought, as placement answers it.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
	Invalid,
	gbp::{Country, keywords},
	geo::{LatLng, Viewport},
	prices::Item,
};

/// The header on every answer to a pull: its [`Billed`], as JSON.
pub const BILLED: &str = "x-billed";

/// What a pull bought, each item's count: its Places searches as `places.search`.
pub type Billed = BTreeMap<Item, u64>;

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
	pub area: Area,
	/// None is the area on its own: the grid's layers, and not one billed call.
	pub category: Option<Category>,
}
impl Definition {
	/// Fails on the first field the conductor could not have handed in.
	pub fn check(&self) -> Result<(), Invalid> {
		let ensure = |ok: bool, why: String| if ok { Ok(()) } else { Err(Invalid(why)) };
		let a = &self.area;
		let slug = a.slug();
		ensure(
			!slug.is_empty() && slug.len() <= 64 && slug.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
			format!("area slug {slug:?} is not [a-z0-9-]{{1,64}}"),
		)?;
		let distance = |m: f64| m.is_finite() && m >= 0.;
		match a {
			Area::City { viewport: v, margin_m, .. } => {
				ensure(v.south < v.north && v.west < v.east, format!("viewport {v:?} is not ordered south < north, west < east"))?;
				ensure(distance(*margin_m), format!("margin_m {margin_m} is not a distance"))?;
			}
			Area::Circle { center, radius_m, margin_m, .. } => {
				ensure(
					(-90. ..=90.).contains(&center.lat) && (-180. ..=180.).contains(&center.lng),
					format!("center {center:?} is not on the globe"),
				)?;
				ensure(distance(*radius_m) && *radius_m > 0., format!("radius_m {radius_m} is not a distance"))?;
				ensure(distance(*margin_m), format!("margin_m {margin_m} is not a distance"))?;
			}
		}
		let Some(c) = &self.category else { return Ok(()) };
		let named = format!("category {}·{}", c.trade, c.country);
		ensure(c.country == a.country(), format!("{named} applied to an area in {}", a.country()))?;
		ensure(!c.study.is_empty() && c.study.trim() == c.study, format!("{named} names study {:?}", c.study))?;
		ensure(!c.keywords.is_empty(), format!("{named} carries no keyword"))?;
		ensure(keywords(&c.keywords)? == c.keywords, format!("{named}'s keywords are not as a category stores them"))?;
		ensure(
			c.google_additional.len() <= 9,
			format!("{named} carries {} additional Google categories, not ≤ 9", c.google_additional.len()),
		)?;
		for g in std::iter::once(&c.google_primary).chain(&c.google_additional) {
			ensure(g.id.starts_with("gcid:") && g.id.len() > 5, format!("Google category {:?} is not a gcid", g.id))?;
			ensure(!g.name.trim().is_empty(), format!("Google category {} has no name", g.id))?;
		}
		Ok(())
	}

	/// Keywords and additional categories are sets to a study, so their order is not part of what
	/// it is.
	pub fn canonical(mut self) -> Self {
		if let Some(c) = &mut self.category {
			c.keywords.sort();
			c.google_additional.sort();
		}
		self
	}

	/// SHA-256 of the canonical form, hex: the study's address, since one slug or one category can
	/// be redefined under the same name.
	pub fn id(&self) -> String {
		let bytes = serde_json::to_vec(&self.clone().canonical()).expect("a definition serialises");
		hex::encode(Sha256::digest(bytes))
	}

	/// `<study>·<CC>·<slug>`, or the slug alone without a category.
	pub fn name(&self) -> String {
		match &self.category {
			Some(c) => format!("{}·{}·{}", c.study, c.country, self.area.slug()),
			None => self.area.slug().to_owned(),
		}
	}
}

/// An area as the conductor keeps it: a city's viewport or a circle, and a margin around it that
/// is still "next to it".
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Area {
	City {
		slug: String,
		country: Country,
		viewport: Viewport,
		margin_m: f64,
	},
	Circle {
		slug: String,
		country: Country,
		center: LatLng,
		radius_m: f64,
		margin_m: f64,
	},
}
impl Area {
	pub fn slug(&self) -> &str {
		match self {
			Self::City { slug, .. } | Self::Circle { slug, .. } => slug,
		}
	}

	pub fn country(&self) -> Country {
		match self {
			Self::City { country, .. } | Self::Circle { country, .. } => *country,
		}
	}
}

/// A trade set up for a country. `study` names the placement trade that carries what only an
/// expert writes: the demand model, the tier vocabulary, λ.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Category {
	pub trade: u64,
	pub country: Country,
	pub study: String,
	/// [`keywords`]' form.
	pub keywords: Vec<String>,
	pub google_primary: Gcid,
	pub google_additional: Vec<Gcid>,
}

/// One of Google's Business Profile categories, named in the country's language.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Gcid {
	pub id: String,
	pub name: String,
}

#[cfg(test)]
mod tests {
	use super::*;

	/// What placement's work dir already keeps a study under, from before this type was sa_core's.
	#[test]
	fn a_study_keeps_its_address() {
		let def = Definition {
			area: Area::City {
				slug: "lyon".into(),
				country: Country::FR,
				viewport: Viewport {
					south: 45.62,
					west: 4.7,
					north: 45.9,
					east: 5.02,
				},
				margin_m: 0.,
			},
			category: Some(Category {
				trade: 1,
				country: Country::FR,
				study: "plumbing".into(),
				keywords: vec!["plombier".into(), "fuite d'eau".into()],
				google_primary: Gcid {
					id: "gcid:plumber".into(),
					name: "Plombier".into(),
				},
				google_additional: vec![],
			}),
		};
		def.check().unwrap();
		assert_eq!(def.id(), PINNED);
	}

	const PINNED: &str = "f43fc15bc1974d6e73621c240d7648c66a025e989ae05f443ccf5a7409c258b2";
}
