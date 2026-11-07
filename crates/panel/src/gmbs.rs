//! The gmails a person manages GMBs with, and those GMBs. The ids are the conductor's, and every
//! service stores them, so nothing is deleted: a gmail or a GMB is removed, and a removed gmail's
//! GMBs count as removed. A GMB is on or off by its owner's switch. review_archive hears of every
//! change through the snapshot
//! ([`crate::tokens`]).

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::Country;
use sa_places::{Ask, LatLng, Place};
use uuid::Uuid;

pub use crate::store::gmbs::{Gmail, Gmb, Point};
use crate::{Panel, Refused, store::gmbs as db, tokens::is_open};

/// What [`Panel::locate`] did.
#[derive(Debug, Default, PartialEq)]
pub struct Located {
	/// Places given a point or a country.
	pub located: usize,
	/// Service-area places not placed yet: for the map to place.
	pub no_point: Vec<String>,
	/// For a person to set the country of.
	pub no_country: Vec<String>,
	/// Places Places refused, or put in a country not served, and why.
	pub refused: Vec<(String, String)>,
}

fn places_error(e: sa_places::Error) -> eyre::Result<Refused> {
	match e {
		sa_places::Error::Invalid(why) => Ok(Refused::Invalid(why)),
		e => Err(eyre::eyre!(e).wrap_err("asking the Places API")),
	}
}

/// A gmail as a person types it: trimmed, lowercased, no space. An address or an alias for one,
/// as review_archive took them.
fn address(raw: &str) -> Result<String, Refused> {
	let gmail = raw.trim().to_lowercase();
	match gmail.is_empty() || gmail.contains(char::is_whitespace) {
		true => Err(Refused::Invalid(format!("{raw:?} is not an address or an alias for one"))),
		false => Ok(gmail),
	}
}

impl Panel {
	/// The owner's gmails, each with its GMBs; the removed left out.
	pub async fn gmails(&self, owner: Uuid) -> eyre::Result<Vec<(Gmail, Vec<Gmb>)>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for gmails")?;
		let gmails = db::gmails(&mut conn, owner).await?;
		let mut gmbs = db::gmbs(&mut conn, owner).await?;
		Ok(gmails
			.into_iter()
			.map(|g| {
				let (mine, rest) = std::mem::take(&mut gmbs).into_iter().partition(|b| b.gmail_id == g.id);
				gmbs = rest;
				(g, mine)
			})
			.collect())
	}

	pub async fn add_gmail(&self, owner: Uuid, raw: &str, now: Timestamp) -> eyre::Result<Result<Gmail, Refused>> {
		let gmail = match address(raw) {
			Ok(g) => g,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(added) = db::add_gmail(&mut tx, None, owner, &gmail, now).await? else {
			return Ok(Err(Refused::Conflict(format!("{gmail} is one of your gmails already"))));
		};
		tx.commit().await.wrap_err("committing a gmail")?;
		self.snapshot_due.notify_one();
		Ok(Ok(added))
	}

	pub async fn remove_gmail(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !db::remove_gmail(&mut tx, owner, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a gmail's removal")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// Every place `input` may mean (see [`sa_places::Google::candidates`]).
	pub async fn place_candidates(&self, input: &str) -> eyre::Result<Result<Vec<(String, Option<Place>)>, Refused>> {
		match self.google.candidates(input).await {
			Ok(places) => Ok(Ok(places)),
			Err(e) => Ok(Err(places_error(e)?)),
		}
	}

	/// The place `input` names, as a GMB under `gmail_id`: named, addressed and located by the
	/// Places API when there is a key. `owner` is whose the gmail must be; `None`, the operator's CLI.
	pub async fn add_gmb(&self, owner: Option<Uuid>, gmail_id: i64, input: &str, now: Timestamp) -> eyre::Result<Result<Gmb, Refused>> {
		let (place_id, place) = match self.google.resolve(input).await {
			Ok(found) => found,
			Err(e) => return Ok(Err(places_error(e)?)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		match db::gmail_owner(&mut tx, gmail_id).await? {
			Some(o) if owner.is_none_or(|owner| owner == o) => {}
			_ => return Ok(Err(Refused::NotFound)),
		}
		let country = match place.as_ref().and_then(|p| p.country.as_deref()).map(str::parse::<Country>).transpose() {
			Ok(c) => c,
			Err(e) => return Ok(Err(Refused::Invalid(format!("Google places {place_id} in a {e}")))),
		};
		if let Some(at) = place.as_ref().and_then(|p| p.location) {
			db::set_point(&mut tx, &place_id, at).await?;
		}
		let (name, address) = place.as_ref().map_or((None, None), |p| (p.name.as_deref(), p.address.as_deref()));
		let Some(added) = db::add_gmb(&mut tx, None, gmail_id, &place_id, name, address, country, now).await? else {
			return Ok(Err(Refused::Conflict(format!("gmail {gmail_id} has place {place_id} already"))));
		};
		tx.commit().await.wrap_err("committing a GMB")?;
		self.snapshot_due.notify_one();
		Ok(Ok(added))
	}

	/// Every place a GMB is on without a point or a country of Google's, asked of Places (its
	/// cache first: the field mask keys it, so an answer from before countries is asked again).
	pub async fn locate(&self) -> eyre::Result<Located> {
		let unlocated = db::unlocated(&mut *self.store.pool().acquire().await.wrap_err("a connection for the unlocated")?).await?;
		let mut out = Located::default();
		for (place_id, pointless) in unlocated {
			let place = match self.google.details(&place_id, Ask::Cached).await {
				Ok(p) => p,
				Err(sa_places::Error::Invalid(why)) => {
					out.refused.push((place_id, why));
					continue;
				}
				Err(e) => return Err(eyre::eyre!(e).wrap_err(format!("locating {place_id}"))),
			};
			let country = match place.country.as_deref().map(str::parse::<Country>).transpose() {
				Ok(c) => c,
				Err(e) => {
					out.refused.push((place_id, e.to_string()));
					continue;
				}
			};
			let mut tx = self.store.begin_write().await?;
			match place.location {
				Some(at) => db::set_point(&mut tx, &place_id, at).await?,
				None if pointless => out.no_point.push(place_id.clone()),
				None => {}
			}
			match country {
				Some(c) => db::set_country(&mut tx, &place_id, c).await?,
				None => out.no_country.push(place_id.clone()),
			}
			tx.commit().await.wrap_err("committing a place's point and country")?;
			if place.location.is_some() || country.is_some() {
				out.located += 1;
			}
		}
		self.snapshot_due.notify_one();
		Ok(out)
	}

	/// GMB `id` in `country`, while Google gives it none. `owner` is whose it must be; `None`, the
	/// operator's CLI.
	pub async fn set_country(&self, owner: Option<Uuid>, id: i64, country: Country) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if let Err(r) = db::set_country_by_hand(&mut tx, owner, id, country).await? {
			return Ok(Err(r));
		}
		tx.commit().await.wrap_err("committing a country")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// The owner's GMB `id` placed at `at` on the map: its place's point while Places has none.
	pub async fn place_gmb(&self, owner: Uuid, id: i64, at: LatLng) -> eyre::Result<Result<(), Refused>> {
		if !(at.lat.is_finite() && at.lng.is_finite() && (-90.0..=90.0).contains(&at.lat) && (-180.0..=180.0).contains(&at.lng)) {
			return Ok(Err(Refused::Invalid(format!("({}, {}) is no point on Earth", at.lat, at.lng))));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if let Err(r) = db::place(&mut tx, owner, id, at).await? {
			return Ok(Err(r));
		}
		tx.commit().await.wrap_err("committing a placed point")?;
		Ok(Ok(()))
	}

	/// Every one of `ids` on or off, or none: each must be the owner's, not removed.
	pub async fn switch_gmbs(&self, owner: Uuid, ids: &[i64], on: bool) -> eyre::Result<Result<(), Refused>> {
		let ids: BTreeSet<i64> = ids.iter().copied().collect();
		if ids.is_empty() {
			return Ok(Err(Refused::Invalid("no GMB to switch".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !db::switch(&mut tx, owner, &ids, on).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a switch")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	pub async fn remove_gmb(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !db::remove_gmb(&mut tx, owner, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a GMB's removal")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}
}
