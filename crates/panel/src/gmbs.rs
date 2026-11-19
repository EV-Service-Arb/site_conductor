//! The gmails a person manages GBPs with, and those GBPs. The ids are the conductor's, and every
//! service stores them, so nothing is deleted: a gmail or a GBP is removed, a gmail with its GBPs.
//! A GBP moves through [`State`]s: a draft (a name, no place yet), then on Google, its place certain
//! only once verified. One on or off
//! by its owner's switch. review_archive hears of every change through the snapshot
//! ([`crate::tokens`]).

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	gbp::{Country, OnGoogle, Role, State},
	prices::{PlacesRequest, Spender},
};
use sa_places::{Ask, LatLng, Place, Suggestion};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	store::gmbs::{self as db, NewGmb, Unique},
	tokens::is_open,
};
pub use crate::{
	google_categories::GoogleCategory,
	store::gmbs::{Category, Changed, Event, EventKind, Field, GbpCountry, Gmail, Gmb, GmbId, GmbPhoto, Logged, Point, Spot, Trade, TradeId},
};

/// The rows of a page of a GBP's log.
pub const LOG_PAGE: u32 = 100;
const MAX_SPOTS: usize = 20;

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
	/// The owner's.
	pub trade: Option<i64>,
}

/// A GBP no place is known of yet: a draft, or one on Google not on Maps.
#[derive(Clone, Debug)]
pub struct Unplaced {
	pub name: String,
	/// The owner's.
	pub trade: i64,
	/// Not verified: a verified GBP is on Maps, added by its place ([`Existing`]).
	pub state: State,
	/// The owner's; past a draft, required.
	pub gmail_id: Option<i64>,
	/// Set by hand; past a draft, required.
	pub country: Option<Country>,
	/// Placed by hand; past a draft only.
	pub at: Option<LatLng>,
	/// A draft's, at least one.
	pub spots: Vec<Spot>,
}

/// What changes of a GBP; each absent is left, `Some(None)` clears it.
#[derive(Clone, Debug, Default)]
pub struct Edit {
	/// A draft's only: on Google, its name is Google's.
	pub name: Option<String>,
	/// Another gmail of its owner's.
	pub gmail_id: Option<i64>,
	/// One live GBP's.
	pub phone: Option<Option<String>>,
	/// The owner's.
	pub trade: Option<Option<i64>>,
	/// 1 to 20, none twice.
	pub areas: Option<Option<Vec<String>>>,
	pub hidden_address: Option<Option<String>>,
	pub customers_visit: Option<Option<bool>>,
	pub film: Option<Option<String>>,
	/// Needs a gmail; moving to another clears it.
	pub role: Option<Option<Role>>,
	/// A manager's, an address.
	pub owner_gmail: Option<Option<String>>,
}

impl Edit {
	/// `self` as stored, or why not: each text trimmed and not empty, the phone in
	/// [`sa_core::gbp::phone`]'s form.
	fn checked(self) -> Result<Self, Refused> {
		fn each<T>(field: Option<Option<T>>, check: impl FnOnce(T) -> Result<T, Refused>) -> Result<Option<Option<T>>, Refused> {
			field.map(|v| v.map(check).transpose()).transpose()
		}
		let text = |what: &'static str| {
			move |raw: String| match raw.trim() {
				"" => Err(Refused::Invalid(format!("a GBP's {what} is not empty: null clears it"))),
				t => Ok(t.to_owned()),
			}
		};
		Ok(Self {
			name: self.name.as_deref().map(name).transpose()?,
			gmail_id: self.gmail_id,
			phone: each(self.phone, |p| sa_core::gbp::phone(&p).map_err(|e| Refused::Invalid(e.to_string())))?,
			trade: self.trade,
			areas: each(self.areas, areas)?,
			hidden_address: each(self.hidden_address, text("hidden address"))?,
			customers_visit: self.customers_visit,
			film: each(self.film, text("film"))?,
			role: self.role,
			owner_gmail: each(self.owner_gmail, |raw| match address(&raw) {
				Ok(a) if crate::assistant::is_email(&a) => Ok(a),
				_ => Err(Refused::Invalid(format!("owner gmail {raw:?} is not an address"))),
			})?,
		})
	}
}

/// `trade`, if any, is `owner`'s.
async fn trade_is(conn: &mut sqlx::SqliteConnection, owner: Uuid, trade: Option<i64>) -> eyre::Result<bool> {
	Ok(match trade {
		None => true,
		Some(t) => db::trade_owner(conn, t).await? == Some(owner),
	})
}

pub(crate) fn places_error(e: sa_places::Error) -> eyre::Result<Refused> {
	match e {
		sa_places::Error::Invalid(why) => Ok(Refused::Invalid(why)),
		e @ sa_places::Error::OwnKey { .. } => Ok(Refused::Invalid(format!("{e}; replace it on its gmail's Connections"))),
		sa_places::Error::Unpaid(why) => Ok(Refused::Conflict(why)),
		e @ sa_places::Error::Unbought => Ok(Refused::Conflict(e.to_string())),
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

fn areas(raw: Vec<String>) -> Result<Vec<String>, Refused> {
	let mut seen = BTreeSet::new();
	let mut out = Vec::with_capacity(raw.len());
	for area in raw {
		let area = area.trim();
		if area.is_empty() {
			return Err(Refused::Invalid("a service area is not empty".to_owned()));
		}
		if !seen.insert(area.to_owned()) {
			return Err(Refused::Invalid(format!("{area:?} is a service area twice")));
		}
		out.push(area.to_owned());
	}
	match out.len() {
		0 => Err(Refused::Invalid("a GBP's service areas are not empty: null clears them".to_owned())),
		21.. => Err(Refused::Invalid(format!("{} service areas: Google takes 20", out.len()))),
		_ => Ok(out),
	}
}

fn on_earth(at: LatLng) -> Result<(), Refused> {
	match at.lat.is_finite() && at.lng.is_finite() && (-90.0..=90.0).contains(&at.lat) && (-180.0..=180.0).contains(&at.lng) {
		true => Ok(()),
		false => Err(Refused::Invalid(format!("({}, {}) is no point on Earth", at.lat, at.lng))),
	}
}

/// A draft's spots: 1 to 20, each on Earth, its texts trimmed and not empty.
fn spots(spots: Vec<Spot>) -> Result<Vec<Spot>, Refused> {
	if !(1..=MAX_SPOTS).contains(&spots.len()) {
		return Err(Refused::Invalid(format!("a draft has 1 to {MAX_SPOTS} spots, not {}", spots.len())));
	}
	let text = |t: Option<String>| match t.as_deref().map(str::trim) {
		Some("") => Err(Refused::Invalid("a spot's label and note are not empty: null for none".to_owned())),
		trimmed => Ok(trimmed.map(str::to_owned)),
	};
	spots
		.into_iter()
		.map(|s| {
			on_earth(s.at)?;
			Ok(Spot {
				at: s.at,
				label: text(s.label)?,
				note: text(s.note)?,
			})
		})
		.collect()
}

/// Google's country of `place`: one it puts in a country not served is refused.
fn google_country(place: Option<&Place>) -> Result<Option<Country>, Refused> {
	match place.and_then(|p| p.country.as_deref()) {
		None => Ok(None),
		Some(c) => c.parse().map(Some).map_err(|e: sa_core::Invalid| Refused::Invalid(format!("Google places it in a {e}"))),
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
		if db::gmail_owner(&mut tx, id).await? == Some(owner) && crate::store::browsers::browser(&mut tx, id).await?.is_some() {
			return Ok(Err(Refused::Conflict("remove its browser first: its provider's profile and its session go with it".to_owned())));
		}
		let going = db::gmbs(&mut tx, owner).await?.into_iter().filter(|b| b.gmail_id == Some(id)).collect::<Vec<_>>();
		if !db::remove_gmail(&mut tx, owner, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		for b in going {
			db::log(&mut tx, Some(b.id), Some(id), Some(owner), now, &Event::Removed).await?;
		}
		tx.commit().await.wrap_err("committing a gmail's removal")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// Every place `input` may mean (see [`sa_places::Google::candidates`]), asked for `owner`:
	/// their `gmail`'s key first, if they name one of theirs.
	/// Each with Google's country of it: one in a country not served refuses them all, as adding it
	/// would.
	pub async fn place_candidates(&self, by: Spender<PlacesRequest>, gmail: Option<i64>, input: &str) -> eyre::Result<Result<Vec<(String, Option<Place>, Option<Country>)>, Refused>> {
		let owner = by.user();
		if !self.gmail_is(owner, gmail).await? {
			return Ok(Err(Refused::NotFound));
		}
		let purse = self.purse(by);
		let payer = self.payer(owner, gmail, Some(&purse)).await?;
		let places = match self.google.candidates(input, &payer).await {
			Ok(places) => places,
			Err(e) => return Ok(Err(places_error(e)?)),
		};
		Ok(places.into_iter().map(|(id, place)| google_country(place.as_ref()).map(|c| (id, place, c))).collect())
	}

	/// The places `input` may be heading for, in the countries served (see
	/// [`sa_places::Google::suggest`]), paid for as [`Self::place_candidates`].
	pub async fn suggest(&self, by: Spender<PlacesRequest>, gmail: Option<i64>, input: &str) -> eyre::Result<Result<Vec<Suggestion>, Refused>> {
		let owner = by.user();
		if !self.gmail_is(owner, gmail).await? {
			return Ok(Err(Refused::NotFound));
		}
		let purse = self.purse(by);
		let payer = self.payer(owner, gmail, Some(&purse)).await?;
		let regions: Vec<&str> = <Country as strum::VariantArray>::VARIANTS.iter().map(|c| c.as_str()).collect();
		match self.google.suggest(input, &regions, &[], &payer).await {
			Ok(found) => Ok(Ok(found)),
			Err(e) => Ok(Err(places_error(e)?)),
		}
	}

	/// `gmail`, if any, is `owner`'s.
	async fn gmail_is(&self, owner: Uuid, gmail: Option<i64>) -> eyre::Result<bool> {
		Ok(match gmail {
			None => true,
			Some(g) => db::gmail_owner(&mut *self.store.pool().acquire().await.wrap_err("a connection for a gmail")?, g).await? == Some(owner),
		})
	}

	/// A GBP no place is known of: its name, which no live GBP has, the owner's trade; past a
	/// draft, the owner's gmail and its country.
	pub async fn create_gmb(&self, owner: Uuid, u: Unplaced, now: Timestamp) -> eyre::Result<Result<Gmb, Refused>> {
		let checked = name(&u.name).and_then(|n| {
			if u.state == State::Verified {
				return Err(Refused::Invalid("a verified GBP is on Maps: add it by its place".to_owned()));
			}
			if u.state != State::Draft && (u.gmail_id.is_none() || u.country.is_none()) {
				return Err(Refused::Invalid(format!("a {} GBP is on Google under a gmail, in a country: give both", u.state)));
			}
			let spots = match u.state {
				State::Draft if u.at.is_some() => return Err(Refused::Invalid("a draft is where its first spot is: give spots, not a location".to_owned())),
				State::Draft => spots(u.spots)?,
				_ if !u.spots.is_empty() => return Err(Refused::Invalid(format!("spots are a draft's: a {} GBP has a location", u.state))),
				_ => Vec::new(),
			};
			u.at.map(on_earth).transpose()?;
			Ok((n, spots))
		});
		let (name, spots) = match checked {
			Ok(n) => n,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !trade_is(&mut tx, owner, Some(u.trade)).await? {
			return Ok(Err(Refused::NotFound));
		}
		if let Some(g) = u.gmail_id
			&& db::gmail_owner(&mut tx, g).await? != Some(owner)
		{
			return Ok(Err(Refused::NotFound));
		}
		if let Some(why) = db::taken(
			&mut tx,
			Unique {
				name: Some(&name),
				..Unique::default()
			},
			None,
		)
		.await?
		{
			return Ok(Err(Refused::Conflict(why)));
		}
		let new = NewGmb {
			owner,
			gmail_id: u.gmail_id,
			place_id: None,
			name: &name,
			address: None,
			state: u.state,
			country: u.country,
			country_by_hand: u.country.is_some(),
			at: u.at,
			at_by_hand: u.at.is_some(),
			trade_id: Some(u.trade),
		};
		let mut added = db::add_gmb(&mut tx, None, &new, now).await?;
		if !spots.is_empty() {
			db::set_spots(&mut tx, owner, added.id, &spots).await?;
			added = db::gmb_of(&mut tx, None, added.id).await?.expect("added in this transaction");
		}
		db::log(&mut tx, Some(added.id), added.gmail_id, Some(owner), now, &Event::Added { state: added.state }).await?;
		tx.commit().await.wrap_err("committing a GBP without a place")?;
		self.snapshot_due.notify_one();
		Ok(Ok(added))
	}

	/// A GBP on Google already, under the gmail `e` names: named, addressed, located and put in
	/// its country by Places when there is a key, `e` filling what Google lacks. `by` is whose
	/// the gmail must be; `None`, the operator's CLI, the house paying.
	pub async fn add_gmb(&self, by: Option<Spender<PlacesRequest>>, e: Existing, now: Timestamp) -> eyre::Result<Result<Gmb, Refused>> {
		let owner = by.map(|b| b.user());
		let gmail_owner = match db::gmail_owner(&mut *self.store.pool().acquire().await.wrap_err("a connection for a gmail")?, e.gmail_id).await? {
			Some(o) if owner.is_none_or(|owner| owner == o) => o,
			_ => return Ok(Err(Refused::NotFound)),
		};
		let purse = by.map(|b| self.purse(b));
		let payer = self.payer(gmail_owner, Some(e.gmail_id), purse.as_ref().map(|p| p as &dyn sa_places::Purse)).await?;
		let (place_id, place) = match self.google.resolve(&e.place, &payer).await {
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
		if !trade_is(&mut tx, gmail_owner, e.trade).await? {
			return Ok(Err(Refused::NotFound));
		}
		if let Some(why) = db::taken(
			&mut tx,
			Unique {
				name: Some(&name),
				place_id: Some(&place_id),
				..Unique::default()
			},
			None,
		)
		.await?
		{
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
			at_by_hand: false,
			trade_id: e.trade,
		};
		let added = db::add_gmb(&mut tx, None, &new, now).await?;
		db::log(&mut tx, Some(added.id), added.gmail_id, owner, now, &Event::Added { state: added.state }).await?;
		tx.commit().await.wrap_err("committing a GMB")?;
		self.snapshot_due.notify_one();
		Ok(Ok(added))
	}

	/// The owner's GBP `id` with `edit` taken whole, or none of it.
	pub async fn edit_gmb(&self, owner: Uuid, id: i64, edit: Edit) -> eyre::Result<Result<Gmb, Refused>> {
		let edit = match edit.checked() {
			Ok(e) => e,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gbp) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if edit.name.is_some() && gbp.state != State::Draft {
			return Ok(Err(Refused::Conflict(format!("GBP {id} is on Google: its name is Google's"))));
		}
		let unique = Unique {
			name: edit.name.as_deref(),
			phone: edit.phone.as_ref().and_then(Option::as_deref),
			..Unique::default()
		};
		if let Some(why) = db::taken(&mut tx, unique, Some(id)).await? {
			return Ok(Err(Refused::Conflict(why)));
		}
		if let Some(n) = &edit.name {
			db::rename(&mut tx, id, n).await?;
		}
		if !trade_is(&mut tx, owner, edit.trade.flatten()).await? {
			return Ok(Err(Refused::NotFound));
		}
		let moved = edit.gmail_id.is_some_and(|g| gbp.gmail_id != Some(g));
		let role = edit.role.unwrap_or(if moved { None } else { gbp.role });
		if role.is_some() && edit.gmail_id.or(gbp.gmail_id).is_none() {
			return Ok(Err(Refused::Invalid(format!("GBP {id} has no gmail to have a role on"))));
		}
		if edit.owner_gmail.as_ref().is_some_and(Option::is_some) && role != Some(Role::Manager) {
			return Ok(Err(Refused::Invalid(format!("GBP {id}'s gmail is not its manager: no owner gmail"))));
		}
		if let Some(gmail) = edit.gmail_id {
			if db::gmail_owner(&mut tx, gmail).await? != Some(owner) {
				return Ok(Err(Refused::NotFound));
			}
			db::move_to(&mut tx, GmbId { v: id }, gmail).await?;
		}
		db::set_fields(&mut tx, id, &edit).await?;
		let edited = db::gmb_of(&mut tx, Some(owner), id).await?.expect("read in this transaction");
		db::record(&mut tx, Some(owner), Timestamp::now(), &gbp, &edited, false).await?;
		tx.commit().await.wrap_err("committing a GMB's edit")?;
		self.snapshot_due.notify_one();
		Ok(Ok(edited))
	}

	/// GBP `id` in `to`, as [`State::next`] allows; past a draft, with its gmail and its country.
	/// One without a place takes it (`place`, as a person pastes it) on any move onto Google, and
	/// must on one to verified. `by` is whose it must be; `None`, the operator's CLI, the house paying.
	pub async fn advance(&self, by: Option<Spender<PlacesRequest>>, id: i64, to: State, place: Option<&str>) -> eyre::Result<Result<Gmb, Refused>> {
		let owner = by.map(|b| b.user());
		let Some(read) = db::gmb_of(&mut *self.store.pool().acquire().await.wrap_err("a connection for a GMB")?, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Err(e) = read.state.next(to) {
			return Ok(Err(Refused::Invalid(e.to_string())));
		}
		let placed = match (place, &read.place_id) {
			(Some(_), Some(had)) => return Ok(Err(Refused::Invalid(format!("GBP {id} is on {had} already: a place is set once")))),
			(None, None) if to == State::Verified => return Ok(Err(Refused::Invalid(format!("a verified GBP is on Maps: paste GBP {id}'s place")))),
			(None, _) => None,
			(Some(_), None) if read.gmail_id.is_none() => {
				return Ok(Err(Refused::Invalid(format!("GBP {id} goes onto Google under a gmail: give it its gmail first"))));
			}
			(Some(input), None) => {
				let gmail = read.gmail_id.expect("refused above without one");
				let Some(gmail_owner) = db::gmail_owner(&mut *self.store.pool().acquire().await.wrap_err("a connection for a gmail")?, gmail).await? else {
					return Ok(Err(Refused::NotFound)); // its gmail removed since it was read, and the GBP with it
				};
				let purse = by.map(|b| self.purse(b));
				let payer = self.payer(gmail_owner, Some(gmail), purse.as_ref().map(|p| p as &dyn sa_places::Purse)).await?;
				match self.google.resolve(input, &payer).await {
					Ok(found) => Some(found),
					Err(e) => return Ok(Err(places_error(e)?)),
				}
			}
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gbp) = db::gmb_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if gbp.state != read.state || gbp.place_id != read.place_id {
			return Ok(Err(Refused::Conflict(format!("GBP {id} changed meanwhile: read it again"))));
		}
		let google = match placed.as_ref().map(|(_, p)| google_country(p.as_ref())).transpose() {
			Ok(c) => c.flatten(),
			Err(r) => return Ok(Err(r)),
		};
		if gbp.gmail_id.is_none() {
			return Ok(Err(Refused::Invalid(format!("GBP {id} goes onto Google under a gmail: give it its gmail first"))));
		}
		if google.is_none() && gbp.country.is_none() {
			return Ok(Err(Refused::Invalid(format!("GBP {id} goes onto Google in a country: set its country first"))));
		}
		match placed {
			Some((place_id, place)) => {
				if let Some(why) = db::taken(
					&mut tx,
					Unique {
						place_id: Some(&place_id),
						..Unique::default()
					},
					Some(id),
				)
				.await?
				{
					return Ok(Err(Refused::Conflict(why)));
				}
				let place = place.as_ref();
				db::place_on_google(&mut tx, id, &place_id, place.and_then(|p| p.address.as_deref()), place.and_then(|p| p.location), google, to).await?;
			}
			None => db::set_state(&mut tx, id, to).await?,
		}
		let moved = db::gmb_of(&mut tx, owner, id).await?.expect("read in this transaction");
		db::record(&mut tx, owner, Timestamp::now(), &gbp, &moved, false).await?;
		tx.commit().await.wrap_err("committing a GMB's state")?;
		self.snapshot_due.notify_one();
		Ok(Ok(moved))
	}

	/// The owner's verified GBP `id` overwritten with its profile on Google, asked afresh and paid by
	/// `by`: its name, address, phone, website, hours, primary category, country, point (none on
	/// Google: one placed by hand kept) and photos.
	pub async fn sync(&self, by: Spender<PlacesRequest>, id: i64) -> eyre::Result<Result<Gmb, Refused>> {
		let owner = by.user();
		let Some(read) = db::gmb_of(&mut *self.store.pool().acquire().await.wrap_err("a connection for a GBP")?, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if read.state != State::Verified {
			return Ok(Err(Refused::Conflict(format!("GBP {id} is {}: a verified one's profile is synced", read.state))));
		}
		let place_id = read.place_id.clone().expect("a verified GBP is on Maps");
		let purse = self.purse(by);
		let payer = self.payer(owner, read.gmail_id, Some(&purse)).await?;
		let profile = match self.google.profile(&place_id, Ask::Afresh, &payer).await {
			Ok(p) => p,
			Err(e) => return Ok(Err(places_error(e)?)),
		};
		if profile.place.place_id != place_id {
			return Ok(Err(Refused::Conflict(format!(
				"Google now names GBP {id}'s place {}: a place is set once",
				profile.place.place_id
			))));
		}
		let checked = (|| {
			let named = name(profile.place.name.as_deref().ok_or_else(|| Refused::Invalid(format!("Google gives {place_id} no name")))?)?;
			let phone = profile
				.phone
				.as_deref()
				.map(sa_core::gbp::phone)
				.transpose()
				.map_err(|e| Refused::Invalid(format!("Google's phone for GBP {id}: {e}")))?;
			Ok((named, phone, google_country(Some(&profile.place))?))
		})();
		let (named, phone, country) = match checked {
			Ok(c) => c,
			Err(r) => return Ok(Err(r)),
		};
		let google_primary = profile.primary_type.as_deref().zip(country.or(read.country.map(|c| c.code))).and_then(|(t, c)| {
			GoogleCategory::find(c, &format!("gcid:{t}")).ok().map(|g| g.id) // a type the catalog lacks is no category of the country's
		});
		let mut photos = Vec::with_capacity(profile.photos.len());
		for p in profile.photos.iter().take(10) {
			match self.google.photo(&p.name, &payer).await {
				Ok(bytes) => photos.push((bytes, p.author.clone())),
				Err(e) => return Ok(Err(places_error(e)?)),
			}
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gbp) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if gbp.state != read.state || gbp.place_id != read.place_id {
			return Ok(Err(Refused::Conflict(format!("GBP {id} changed meanwhile: read it again"))));
		}
		let unique = Unique {
			name: Some(&named),
			phone: phone.as_deref(),
			..Unique::default()
		};
		if let Some(why) = db::taken(&mut tx, unique, Some(id)).await? {
			return Ok(Err(Refused::Conflict(why)));
		}
		// country first: the schema refuses a placed GBP without one
		if let Some(c) = country {
			db::set_country(&mut tx, id, c).await?;
		}
		if let Some(at) = profile.place.location {
			db::set_point(&mut tx, id, at).await?;
		}
		let synced = db::Synced {
			name: &named,
			address: profile.place.address.as_deref(),
			phone: phone.as_deref(),
			website: profile.website.as_deref(),
			hours: &profile.hours,
			google_primary,
			photos: &photos,
		};
		let now = Timestamp::now();
		db::sync(&mut tx, id, &synced, now).await?;
		let after = db::gmb_of(&mut tx, Some(owner), id).await?.expect("read in this transaction");
		db::record(&mut tx, Some(owner), now, &gbp, &after, true).await?;
		tx.commit().await.wrap_err("committing a synced GBP")?;
		self.snapshot_due.notify_one();
		Ok(Ok(after))
	}

	/// A page of the owner's GBP `id`'s log before event `before`, newest first, of `kinds` (none: every kind).
	pub async fn gmb_log(&self, owner: Uuid, id: i64, before: Option<i64>, kinds: &[EventKind]) -> eyre::Result<Result<Vec<Logged>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a GBP's log")?;
		Ok(db::events(&mut conn, owner, id, before, kinds, LOG_PAGE).await?.ok_or(Refused::NotFound))
	}

	/// `event` of the owner's gmail `gmail`'s browser in the log of `gmb` it ran for, else of each GBP under the gmail.
	pub async fn log_browser(&self, owner: Uuid, gmail: i64, gmb: Option<i64>, event: &Event) -> eyre::Result<()> {
		assert!(matches!(event, Event::Launched { .. } | Event::Stopped), "a browser's own: {event:?}");
		let mut tx = self.store.begin_write().await?;
		db::log(&mut tx, gmb, Some(gmail), Some(owner), Timestamp::now(), event).await?;
		tx.commit().await.wrap_err("committing a browser's event")
	}

	/// Photo `photo` of the owner's GBP `id`, as its last Sync took it.
	pub async fn gmb_photo(&self, owner: Uuid, id: i64, photo: i64) -> eyre::Result<Result<Vec<u8>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a GBP's photo")?;
		Ok(db::photo(&mut conn, owner, id, photo).await?.ok_or(Refused::NotFound))
	}

	/// GBP `id` in `country`, while Google gives it none. `owner` is whose it must be; `None`, the
	/// operator's CLI.
	pub async fn set_country(&self, owner: Option<Uuid>, id: i64, country: Country) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(before) = db::gmb_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Err(r) = db::set_country_by_hand(&mut tx, owner, id, country).await? {
			return Ok(Err(r));
		}
		db::record_since(&mut tx, owner, Timestamp::now(), &[before]).await?;
		tx.commit().await.wrap_err("committing a country")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// The owner's GBP `id` placed at `at` on the map: its point while Google has none.
	pub async fn place_gmb(&self, owner: Uuid, id: i64, at: LatLng) -> eyre::Result<Result<(), Refused>> {
		if let Err(r) = on_earth(at) {
			return Ok(Err(r));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(before) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Err(r) = db::place(&mut tx, owner, id, at).await? {
			return Ok(Err(r));
		}
		db::record_since(&mut tx, Some(owner), Timestamp::now(), &[before]).await?;
		tx.commit().await.wrap_err("committing a placed point")?;
		Ok(Ok(()))
	}

	/// Draft `id`'s spots, in order: the first its point.
	pub async fn set_spots(&self, owner: Uuid, id: i64, given: Vec<Spot>) -> eyre::Result<Result<Gmb, Refused>> {
		let given = match spots(given) {
			Ok(s) => s,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gbp) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if gbp.state != State::Draft {
			return Ok(Err(Refused::Conflict(format!("GBP {id} is {}: spots are a draft's", gbp.state))));
		}
		db::set_spots(&mut tx, owner, id, &given).await?;
		let set = db::gmb_of(&mut tx, Some(owner), id).await?.expect("read in this transaction");
		db::record(&mut tx, Some(owner), Timestamp::now(), &gbp, &set, false).await?;
		tx.commit().await.wrap_err("committing a draft's spots")?;
		Ok(Ok(set))
	}

	/// The point the owner placed on GBP `id` taken back.
	pub async fn unplace_gmb(&self, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(before) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Err(r) = db::unplace(&mut tx, owner, id).await? {
			return Ok(Err(r));
		}
		db::record_since(&mut tx, Some(owner), Timestamp::now(), &[before]).await?;
		tx.commit().await.wrap_err("committing a point taken back")?;
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
		let befores = db::of_ids(&mut tx, &ids).await?;
		if !db::switch(&mut tx, owner, &ids, on).await? {
			return Ok(Err(Refused::NotFound));
		}
		db::record_since(&mut tx, Some(owner), Timestamp::now(), &befores).await?;
		tx.commit().await.wrap_err("committing a switch")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	pub async fn remove_gmb(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(gone) = db::gmb_of(&mut tx, Some(owner), id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		assert!(db::remove_gmb(&mut tx, owner, id, now).await?, "read live in this transaction");
		db::log(&mut tx, Some(id), gone.gmail_id, Some(owner), now, &Event::Removed).await?;
		tx.commit().await.wrap_err("committing a GMB's removal")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// The owner's trades, each with the countries it is set up for.
	pub async fn trades(&self, owner: Uuid) -> eyre::Result<Vec<Trade>> {
		db::trades(&mut *self.store.pool().acquire().await.wrap_err("a connection for trades")?, owner).await
	}

	/// A trade of the owner's, set up nowhere yet: its name, no other of theirs whatever its case.
	pub async fn add_trade(&self, owner: Uuid, raw: &str, now: Timestamp) -> eyre::Result<Result<Trade, Refused>> {
		let Some(name) = trade_name(raw) else {
			return Ok(Err(Refused::Invalid("a trade's name is not empty".to_owned())));
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if db::trade_named(&mut tx, owner, &name, None).await?.is_some() {
			return Ok(Err(Refused::Conflict(format!("{name} is one of your trades already"))));
		}
		let id = db::add_trade(&mut tx, owner, &name, now).await?;
		let added = trade_of(&mut tx, owner, id).await?.expect("added in this transaction");
		tx.commit().await.wrap_err("committing a trade")?;
		Ok(Ok(added))
	}

	pub async fn rename_trade(&self, owner: Uuid, id: i64, raw: &str) -> eyre::Result<Result<Trade, Refused>> {
		let Some(name) = trade_name(raw) else {
			return Ok(Err(Refused::Invalid("a trade's name is not empty".to_owned())));
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if db::trade_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		if db::trade_named(&mut tx, owner, &name, Some(id)).await?.is_some() {
			return Ok(Err(Refused::Conflict(format!("{name} is one of your trades already"))));
		}
		db::rename_trade(&mut tx, id, &name).await?;
		let renamed = trade_of(&mut tx, owner, id).await?.expect("read in this transaction");
		tx.commit().await.wrap_err("committing a trade's name")?;
		Ok(Ok(renamed))
	}

	/// The owner's trade `id` and its categories, while no live GBP holds it.
	pub async fn delete_trade(&self, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if db::trade_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		match db::trade_holders(&mut tx, id).await? {
			0 => {}
			n => return Ok(Err(Refused::Conflict(format!("{n} GBPs hold trade {id}: give them another first")))),
		}
		if crate::store::kit::held(&mut tx, id, None).await? > 0 {
			return Ok(Err(Refused::Conflict(format!("trade {id} has review drafts: their uses are kept"))));
		}
		if crate::store::brands::ever(&mut tx, id, None).await? > 0 {
			return Ok(Err(Refused::Conflict(format!("trade {id} has brands: their photos' uses are kept"))));
		}
		db::delete_trade(&mut tx, id).await?;
		tx.commit().await.wrap_err("committing a trade's deletion")?;
		Ok(Ok(()))
	}

	/// The owner's trade `id` set up for `c.country` as `c` says, over what it was: the keywords in
	/// [`sa_core::gbp::keywords`]' form, the study trimmed and not empty, at most 9 additional
	/// Google categories beside a primary, none twice.
	pub async fn set_up(&self, owner: Uuid, id: i64, c: Category) -> eyre::Result<Result<Trade, Refused>> {
		let study = match c.study.as_deref().map(str::trim) {
			Some("") => return Ok(Err(Refused::Invalid("a category's study is not empty: null clears it".to_owned()))),
			t => t.map(str::to_owned),
		};
		let keywords = match sa_core::gbp::keywords(&c.keywords) {
			Ok(k) => k,
			Err(e) => return Ok(Err(Refused::Invalid(e.to_string()))),
		};
		if c.google_additional.len() > 9 {
			return Ok(Err(Refused::Invalid("Google takes at most 9 additional categories".to_owned())));
		}
		if c.google_primary.is_none() && !c.google_additional.is_empty() {
			return Ok(Err(Refused::Invalid("additional Google categories need a primary one".to_owned())));
		}
		let mut seen = BTreeSet::new();
		if let Some(twice) = c.google_primary.iter().chain(&c.google_additional).find(|g| !seen.insert(g.id)) {
			return Ok(Err(Refused::Invalid(format!("Google category {} twice", twice.id))));
		}
		let c = Category { keywords, study, ..c };
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if db::trade_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		db::set_up(&mut tx, id, &c).await?;
		let set = trade_of(&mut tx, owner, id).await?.expect("read in this transaction");
		tx.commit().await.wrap_err("committing a category")?;
		Ok(Ok(set))
	}

	/// The owner's trade `id` no longer set up in `country`: its GBPs there are not set up.
	pub async fn remove_category(&self, owner: Uuid, id: i64, country: Country) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if db::trade_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		if crate::store::kit::held(&mut tx, id, Some(country)).await? > 0 {
			return Ok(Err(Refused::Conflict(format!("{country} has review drafts: their uses are kept"))));
		}
		if crate::store::brands::ever(&mut tx, id, Some(country)).await? > 0 {
			return Ok(Err(Refused::Conflict(format!("{country} has brands: their photos' uses are kept"))));
		}
		if !db::remove_category(&mut tx, id, country).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a category's removal")?;
		Ok(Ok(()))
	}

	/// Every one of `ids` given the owner's `trade` (`None`: none), or none of them: each must be
	/// the owner's, not removed.
	pub async fn set_trades(&self, owner: Uuid, ids: &[i64], trade: Option<i64>) -> eyre::Result<Result<(), Refused>> {
		let ids: BTreeSet<i64> = ids.iter().copied().collect();
		if ids.is_empty() {
			return Ok(Err(Refused::Invalid("no GBP to give a trade".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let befores = db::of_ids(&mut tx, &ids).await?;
		if !trade_is(&mut tx, owner, trade).await? || !db::set_trades(&mut tx, owner, &ids, trade).await? {
			return Ok(Err(Refused::NotFound));
		}
		db::record_since(&mut tx, Some(owner), Timestamp::now(), &befores).await?;
		tx.commit().await.wrap_err("committing GBPs' trade")?;
		Ok(Ok(()))
	}
}

fn trade_name(raw: &str) -> Option<String> {
	Some(raw.trim()).filter(|n| !n.is_empty()).map(str::to_owned)
}

async fn trade_of(conn: &mut sqlx::SqliteConnection, owner: Uuid, id: i64) -> eyre::Result<Option<Trade>> {
	Ok(db::trades(conn, owner).await?.into_iter().find(|t| t.id == id))
}
