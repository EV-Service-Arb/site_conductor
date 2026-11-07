//! The gmails a person manages GBPs with, and those GBPs. The ids are the conductor's, and every
//! service stores them, so nothing is deleted: a gmail or a GBP is removed, a gmail with its GBPs.
//! A GBP moves through [`State`]s: a draft (a name, no place yet), then on Google. One on or off
//! by its owner's switch. review_archive hears of every change through the snapshot
//! ([`crate::tokens`]).

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_gbp::{Country, OnGoogle, State};
use sa_places::{Ask, LatLng, Place};
use uuid::Uuid;

pub use crate::store::gmbs::{GbpCountry, Gmail, Gmb, Point};
use crate::{
	Panel, Refused,
	store::gmbs::{self as db, NewGmb},
	tokens::is_open,
};

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

/// A GBP that is on Google already, added under a gmail.
#[derive(Clone, Debug)]
pub struct Existing {
	pub gmail_id: i64,
	/// What a person pastes: a place id, a Maps URL.
	pub place: String,
	pub state: OnGoogle,
	/// Used when Google gives none, as `country`.
	pub name: Option<String>,
	pub country: Option<Country>,
}

/// What changes of a GBP; each absent is left.
#[derive(Clone, Debug, Default)]
pub struct Edit {
	/// A draft's only: on Google, its name is Google's.
	pub name: Option<String>,
	/// Another gmail of its owner's.
	pub gmail_id: Option<i64>,
	/// `Some(None)` clears it.
	pub phone: Option<Option<String>>,
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

fn name(raw: &str) -> Result<String, Refused> {
	match raw.trim() {
		"" => Err(Refused::Invalid("a GBP's name is not empty".to_owned())),
		n => Ok(n.to_owned()),
	}
}

fn phone(raw: &str) -> Result<String, Refused> {
	let phone = raw.trim();
	match !phone.is_empty() && phone.chars().all(|c| c.is_ascii_digit() || "+-(). ".contains(c)) {
		true => Ok(phone.to_owned()),
		false => Err(Refused::Invalid(format!("{raw:?} is not a phone number"))),
	}
}

/// Google's country of `place`: one it puts in a country not served is refused.
fn google_country(place: Option<&Place>) -> Result<Option<Country>, Refused> {
	match place.and_then(|p| p.country.as_deref()) {
		None => Ok(None),
		Some(c) => c.parse().map(Some).map_err(|e: sa_gbp::Invalid| Refused::Invalid(format!("Google places it in a {e}"))),
	}
}

impl Panel {
	/// The owner's gmails and GBPs; the removed left out.
	pub async fn gmails(&self, owner: Uuid) -> eyre::Result<(Vec<Gmail>, Vec<Gmb>)> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for gmails")?;
		Ok((db::gmails(&mut conn, owner).await?, db::gmbs(&mut conn, owner).await?))
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

	/// A GBP not on Google yet: its name, which no live GBP has.
	pub async fn create_draft(&self, owner: Uuid, raw: &str, now: Timestamp) -> eyre::Result<Result<Gmb, Refused>> {
		let name = match name(raw) {
			Ok(n) => n,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if let Some(why) = db::taken(&mut tx, Some(&name), None, None).await? {
			return Ok(Err(Refused::Conflict(why)));
		}
		let new = NewGmb {
			owner,
			gmail_id: None,
			place_id: None,
			name: &name,
			address: None,
			state: State::Draft,
			country: None,
			country_by_hand: false,
			at: None,
		};
		let added = db::add_gmb(&mut tx, None, &new, now).await?;
		tx.commit().await.wrap_err("committing a draft")?;
		self.snapshot_due.notify_one();
		Ok(Ok(added))
	}

	/// A GBP on Google already, under the gmail `e` names: named, addressed, located and put in
	/// its country by Places when there is a key, `e` filling what Google lacks. `owner` is whose
	/// the gmail must be; `None`, the operator's CLI.
	pub async fn add_gmb(&self, owner: Option<Uuid>, e: Existing, now: Timestamp) -> eyre::Result<Result<Gmb, Refused>> {
		let (place_id, place) = match self.google.resolve(&e.place).await {
			Ok(found) => found,
			Err(err) => return Ok(Err(places_error(err)?)),
		};
		let named = match place.as_ref().and_then(|p| p.name.as_deref()).or(e.name.as_deref()) {
			Some(raw) => name(raw),
			None => Err(Refused::Invalid(format!("Google gives {place_id} no name: give it its name"))),
		};
		let country = match google_country(place.as_ref()) {
			Ok(Some(c)) => Ok((c, false)),
			Ok(None) => e
				.country
				.map(|c| (c, true))
				.ok_or_else(|| Refused::Invalid(format!("Google gives {place_id} no country: give it its country"))),
			Err(r) => Err(r),
		};
		let ((country, by_hand), name) = match (country, named) {
			(Ok(c), Ok(n)) => (c, n),
			(Err(r), _) | (_, Err(r)) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let gmail_owner = match db::gmail_owner(&mut tx, e.gmail_id).await? {
			Some(o) if owner.is_none_or(|owner| owner == o) => o,
			_ => return Ok(Err(Refused::NotFound)),
		};
		if let Some(why) = db::taken(&mut tx, Some(&name), Some(&place_id), None).await? {
			return Ok(Err(Refused::Conflict(why)));
		}
		let new = NewGmb {
			owner: gmail_owner,
			gmail_id: Some(e.gmail_id),
			place_id: Some(&place_id),
			name: &name,
			address: place.as_ref().and_then(|p| p.address.as_deref()),
			state: e.state.into(),
			country: Some(country),
			country_by_hand: by_hand,
			at: place.as_ref().and_then(|p| p.location),
		};
		let added = db::add_gmb(&mut tx, None, &new, now).await?;
		tx.commit().await.wrap_err("committing a GMB")?;
		self.snapshot_due.notify_one();
		Ok(Ok(added))
	}

	/// The owner's GBP `id` with `edit` taken whole, or none of it.
	pub async fn edit_gmb(&self, owner: Uuid, id: i64, edit: Edit) -> eyre::Result<Result<Gmb, Refused>> {
		let renamed = match edit.name.as_deref().map(name).transpose() {
			Ok(n) => n,
			Err(r) => return Ok(Err(r)),
		};
		let phone = match edit.phone.map(|p| p.as_deref().map(phone).transpose()).transpose() {
			Ok(p) => p,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gbp) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Some(n) = &renamed {
			if gbp.state != State::Draft {
				return Ok(Err(Refused::Conflict(format!("GBP {id} is on Google: its name is Google's"))));
			}
			if let Some(why) = db::taken(&mut tx, Some(n), None, Some(id)).await? {
				return Ok(Err(Refused::Conflict(why)));
			}
			db::rename(&mut tx, id, n).await?;
		}
		if let Some(gmail) = edit.gmail_id {
			if db::gmail_owner(&mut tx, gmail).await? != Some(owner) {
				return Ok(Err(Refused::NotFound));
			}
			db::move_to(&mut tx, id, gmail).await?;
		}
		if let Some(p) = &phone {
			db::set_phone(&mut tx, id, p.as_deref()).await?;
		}
		let edited = db::gmb_of(&mut tx, Some(owner), id).await?.expect("read in this transaction");
		tx.commit().await.wrap_err("committing a GMB's edit")?;
		self.snapshot_due.notify_one();
		Ok(Ok(edited))
	}

	/// GBP `id` in `to`, as [`State::next`] allows. A draft goes onto Google with its place
	/// (`place`, as a person pastes it): its gmail and its country, Google's or set by hand,
	/// needed. `owner` is whose it must be; `None`, the operator's CLI.
	pub async fn advance(&self, owner: Option<Uuid>, id: i64, to: State, place: Option<&str>) -> eyre::Result<Result<Gmb, Refused>> {
		let Some(read) = db::gmb_of(&mut *self.store.pool().acquire().await.wrap_err("a connection for a GMB")?, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let placed = match (read.state, place) {
			(State::Draft, Some(input)) => match self.google.resolve(input).await {
				Ok(found) => Some(found),
				Err(e) => return Ok(Err(places_error(e)?)),
			},
			(State::Draft, None) if to == State::Verifying => return Ok(Err(Refused::Invalid("a draft goes onto Google with its place: paste it".to_owned()))),
			(_, Some(_)) => return Ok(Err(Refused::Invalid(format!("GBP {id} is on Google already: only a draft takes a place")))),
			(_, None) => None,
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gbp) = db::gmb_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Err(e) = gbp.state.next(to) {
			return Ok(Err(Refused::Invalid(e.to_string())));
		}
		match placed {
			Some(_) if gbp.state != read.state => return Ok(Err(Refused::Conflict(format!("GBP {id} became {} meanwhile", gbp.state)))),
			Some((place_id, place)) => {
				let google = match google_country(place.as_ref()) {
					Ok(c) => c,
					Err(r) => return Ok(Err(r)),
				};
				if gbp.gmail_id.is_none() {
					return Ok(Err(Refused::Invalid(format!("GBP {id} goes onto Google under a gmail: give it its gmail first"))));
				}
				if google.is_none() && gbp.country.is_none() {
					return Ok(Err(Refused::Invalid(format!("Google gives {place_id} no country: set GBP {id}'s first"))));
				}
				if let Some(why) = db::taken(&mut tx, None, Some(&place_id), Some(id)).await? {
					return Ok(Err(Refused::Conflict(why)));
				}
				let place = place.as_ref();
				db::place_draft(&mut tx, id, &place_id, place.and_then(|p| p.address.as_deref()), place.and_then(|p| p.location), google).await?;
			}
			None => db::set_state(&mut tx, id, to).await?,
		}
		let moved = db::gmb_of(&mut tx, owner, id).await?.expect("read in this transaction");
		tx.commit().await.wrap_err("committing a GMB's state")?;
		self.snapshot_due.notify_one();
		Ok(Ok(moved))
	}

	/// Every GBP on Google without a point or a country of Google's, asked of Places (its cache
	/// first: the field mask keys it, so an answer from before countries is asked again).
	pub async fn locate(&self) -> eyre::Result<Located> {
		let unlocated = db::unlocated(&mut *self.store.pool().acquire().await.wrap_err("a connection for the unlocated")?).await?;
		let mut out = Located::default();
		for (id, place_id, pointless) in unlocated {
			let place = match self.google.details(&place_id, Ask::Cached).await {
				Ok(p) => p,
				Err(sa_places::Error::Invalid(why)) => {
					out.refused.push((place_id, why));
					continue;
				}
				Err(e) => return Err(eyre::eyre!(e).wrap_err(format!("locating {place_id}"))),
			};
			let country = match google_country(Some(&place)) {
				Ok(c) => c,
				Err(r) => {
					out.refused.push((place_id, r.to_string()));
					continue;
				}
			};
			let mut tx = self.store.begin_write().await?;
			match place.location {
				Some(at) => db::set_point(&mut tx, id, at).await?,
				None if pointless => out.no_point.push(place_id.clone()),
				None => {}
			}
			match country {
				Some(c) => db::set_country(&mut tx, id, c).await?,
				None => out.no_country.push(place_id.clone()),
			}
			tx.commit().await.wrap_err("committing a GMB's point and country")?;
			if place.location.is_some() || country.is_some() {
				out.located += 1;
			}
		}
		self.snapshot_due.notify_one();
		Ok(out)
	}

	/// GBP `id` in `country`, while Google gives it none. `owner` is whose it must be; `None`, the
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

	/// The owner's GBP `id` placed at `at` on the map: its point while Google has none.
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
