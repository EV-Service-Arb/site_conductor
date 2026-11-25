//! Areas: geography, everyone's. The kit lists a caller's GBPs by one; a study is framed by one.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	gbp::Country,
	prices::{PlacesRequest, Spender},
};
use sa_places::{LatLng, Locality, Suggestion};
use uuid::Uuid;

use crate::{Panel, Refused, store::areas as db};

pub const MAX_RADIUS_M: i64 = 100_000;

/// A circle on the map, everyone's: the kit's `/<slug>/` lists a caller's GBPs in it, a study's frame.
#[derive(Clone, Debug)]
pub struct Area {
	pub slug: String,
	pub name: String,
	/// ISO 3166-1 alpha-2.
	pub country: String,
	pub center: LatLng,
	pub radius_m: i64,
	/// The locality's place it was drawn from: an outline to see it by, not its edge.
	pub city: Option<String>,
	pub created_by: Uuid,
	pub created_at: Timestamp,
}

/// An area as asked for.
#[derive(Clone, Debug)]
pub struct NewArea {
	pub name: String,
	pub center: LatLng,
	pub radius_m: i64,
	pub city: Option<String>,
}

impl Panel {
	pub async fn area(&self, slug: &str) -> eyre::Result<Option<Area>> {
		db::area(&mut *self.store.pool().acquire().await.wrap_err("a connection for an area")?, slug).await
	}

	pub async fn areas(&self) -> eyre::Result<Vec<Area>> {
		db::areas(&mut *self.store.pool().acquire().await.wrap_err("a connection for areas")?).await
	}

	/// Area `slug` drawn as `new` asks, over what it was if `user` drew it; its country its city's
	/// (Places), else its centre's.
	pub async fn put_area(&self, by: Spender<PlacesRequest>, slug: &str, new: NewArea, now: Timestamp) -> eyre::Result<Result<Area, Refused>> {
		let user = by.user();
		if !sa_core::ids::is_slug(slug) || slug.contains('_') {
			return Ok(Err(Refused::Invalid(format!("{slug:?}: an area's link is lowercase letters, digits and dashes"))));
		}
		let NewArea { name, center, radius_m, city } = new;
		let Some(name) = Some(name.trim().to_owned()).filter(|n| !n.is_empty()) else {
			return Ok(Err(Refused::Invalid("an area has a name".to_owned())));
		};
		if !(1..=MAX_RADIUS_M).contains(&radius_m) {
			return Ok(Err(Refused::Invalid("an area's radius is 1 m to 100 km".to_owned())));
		}
		let country = match &city {
			Some(place_id) => match self.locality(by, place_id).await? {
				Ok(l) => l.country,
				Err(e) => return Ok(Err(e)),
			},
			None => {
				let purse = self.purse(by);
				let payer = self.payer(by.user(), None, Some(&purse)).await?;
				match self.google.country_at(center, &payer).await {
					Ok(Some(c)) => c,
					Ok(None) => return Ok(Err(Refused::Invalid("an area's centre is in a country".to_owned()))),
					Err(e) => return Ok(Err(crate::gmbs::places_error(e)?)),
				}
			}
		};
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
			radius_m,
			city,
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
