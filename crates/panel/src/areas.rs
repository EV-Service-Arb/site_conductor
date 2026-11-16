//! Areas: geography, everyone's. The kit lists a caller's GBPs by one; a study is framed by one.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	gbp::Country,
	prices::{PlacesRequest, Spender},
};
use sa_places::{LatLng, Locality, Suggestion, Viewport};
use uuid::Uuid;

use crate::{Panel, Refused, store::areas as db};

/// A place on the map, everyone's: the kit's `/<slug>/` lists a caller's GBPs in it and next to it, a study's frame.
#[derive(Clone, Debug)]
pub struct Area {
	pub slug: String,
	pub name: String,
	/// ISO 3166-1 alpha-2.
	pub country: String,
	pub center: LatLng,
	pub shape: Shape,
	/// How far outside it a GBP is still next to it.
	pub margin_m: i64,
	pub created_by: Uuid,
	pub created_at: Timestamp,
}

#[derive(Clone, Debug)]
pub enum Shape {
	/// Google's locality: inside its viewport is in.
	City {
		place_id: String,
		viewport: Viewport,
	},
	Circle {
		radius_m: i64,
	},
}

/// An area as asked for: a city by its place, or a circle drawn by hand.
#[derive(Clone, Debug)]
pub enum NewArea {
	City { place_id: String, margin_m: i64 },
	Circle { name: String, center: LatLng, radius_m: i64, margin_m: i64 },
}

impl Panel {
	pub async fn area(&self, slug: &str) -> eyre::Result<Option<Area>> {
		db::area(&mut *self.store.pool().acquire().await.wrap_err("a connection for an area")?, slug).await
	}

	pub async fn areas(&self) -> eyre::Result<Vec<Area>> {
		db::areas(&mut *self.store.pool().acquire().await.wrap_err("a connection for areas")?).await
	}

	/// Area `slug` drawn as `new` asks, over what it was if `user` drew it; a city's viewport and
	/// country asked of Places for them, a circle's country too.
	pub async fn put_area(&self, by: Spender<PlacesRequest>, slug: &str, new: NewArea, now: Timestamp) -> eyre::Result<Result<Area, Refused>> {
		let user = by.user();
		if !sa_core::ids::is_slug(slug) || slug.contains('_') {
			return Ok(Err(Refused::Invalid(format!("{slug:?}: an area's link is lowercase letters, digits and dashes"))));
		}
		let (name, country, center, shape, margin_m) = match new {
			NewArea::City { place_id, margin_m } => {
				let l = match self.locality(by, &place_id).await? {
					Ok(l) => l,
					Err(e) => return Ok(Err(e)),
				};
				(
					l.name,
					l.country,
					l.location,
					Shape::City {
						place_id: l.place_id,
						viewport: l.viewport,
					},
					margin_m,
				)
			}
			NewArea::Circle { name, center, radius_m, margin_m } => {
				let Some(name) = Some(name.trim().to_owned()).filter(|n| !n.is_empty()) else {
					return Ok(Err(Refused::Invalid("an area has a name".to_owned())));
				};
				if !(1..=100_000).contains(&radius_m) {
					return Ok(Err(Refused::Invalid("a circle's radius is 1 m to 100 km".to_owned())));
				}
				let purse = self.purse(by);
				let payer = self.payer(by.user(), None, Some(&purse)).await?;
				let country = match self.google.country_at(center, &payer).await {
					Ok(Some(c)) => c,
					Ok(None) => return Ok(Err(Refused::Invalid("a circle's centre is in a country".to_owned()))),
					Err(e) => return Ok(Err(crate::gmbs::places_error(e)?)),
				};
				(name, country, center, Shape::Circle { radius_m }, margin_m)
			}
		};
		if !(0..=50_000).contains(&margin_m) {
			return Ok(Err(Refused::Invalid("the margin is 0 to 50 km".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		if let Some(was) = db::area(&mut tx, slug).await?
			&& was.created_by != user
		{
			return Ok(Err(Refused::Conflict(format!("/{slug}/ is {} already", was.name))));
		}
		let area = Area {
			slug: slug.to_owned(),
			name,
			country,
			center,
			shape,
			margin_m,
			created_by: user,
			created_at: now,
		};
		db::put_area(&mut tx, &area).await?;
		tx.commit().await.wrap_err("committing an area")?;
		Ok(Ok(area))
	}

	/// Cities and towns `input` may be heading for, in the countries served; paid by `by`.
	pub async fn cities(&self, by: Spender<PlacesRequest>, input: &str) -> eyre::Result<Result<Vec<Suggestion>, Refused>> {
		let purse = self.purse(by);
		let payer = self.payer(by.user(), None, Some(&purse)).await?;
		let regions: Vec<&str> = <Country as strum::VariantArray>::VARIANTS.iter().map(|c| c.as_str()).collect();
		match self.google.suggest(input, &regions, &["(cities)"], &payer).await {
			Ok(found) => Ok(Ok(found)),
			Err(e) => Ok(Err(crate::gmbs::places_error(e)?)),
		}
	}

	pub async fn locality(&self, by: Spender<PlacesRequest>, place_id: &str) -> eyre::Result<Result<Locality, Refused>> {
		let purse = self.purse(by);
		let payer = self.payer(by.user(), None, Some(&purse)).await?;
		match self.google.locality(place_id, &payer).await {
			Ok(l) => Ok(Ok(l)),
			Err(e) => Ok(Err(crate::gmbs::places_error(e)?)),
		}
	}

	/// The city or town `at` lies in; `None` outside any.
	pub async fn locality_at(&self, by: Spender<PlacesRequest>, at: LatLng) -> eyre::Result<Result<Option<Locality>, Refused>> {
		let purse = self.purse(by);
		let payer = self.payer(by.user(), None, Some(&purse)).await?;
		match self.google.locality_at(at, &payer).await {
			Ok(l) => Ok(Ok(l)),
			Err(e) => Ok(Err(crate::gmbs::places_error(e)?)),
		}
	}
}
