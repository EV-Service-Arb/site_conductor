//! The gmails a person manages GBPs with, and those GBPs. The ids are the conductor's, and every
//! service stores them, so nothing is deleted: a gmail or a GBP is removed, a gmail with its GBPs.
//! A GBP moves through [`State`]s: a draft (a name, no place yet), then on Google. One on or off
//! by its owner's switch. review_archive hears of every change through the snapshot
//! ([`crate::tokens`]).

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::gbp::{Country, GmailRole, OnGoogle, State};
use sa_places::{Ask, LatLng, Place};
use uuid::Uuid;

pub use crate::store::gmbs::{Category, GbpCountry, Gmail, Gmb, Point, Trade};
use crate::{
	Panel, Refused,
	store::gmbs::{self as db, NewGmb, Unique},
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
	/// The owner's.
	pub trade: Option<i64>,
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
	pub profile: Option<Option<String>>,
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
			profile: each(self.profile, text("profile"))?,
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

fn places_error(e: sa_places::Error) -> eyre::Result<Refused> {
	match e {
		sa_places::Error::Invalid(why) => Ok(Refused::Invalid(why)),
		e @ sa_places::Error::OwnKey { .. } => Ok(Refused::Invalid(format!("{e}; replace it on its gmail's Connections"))),
		sa_places::Error::Unpaid(why) => Ok(Refused::Conflict(why)),
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

	/// The owner's gmail `id` in `role`.
	pub async fn set_gmail_role(&self, owner: Uuid, id: i64, role: GmailRole) -> eyre::Result<Result<Gmail, Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !db::set_role(&mut tx, owner, id, role).await? {
			return Ok(Err(Refused::NotFound));
		}
		let set = db::gmails(&mut tx, owner).await?.into_iter().find(|g| g.id == id).expect("set in this transaction");
		tx.commit().await.wrap_err("committing a gmail's role")?;
		Ok(Ok(set))
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

	/// Every place `input` may mean (see [`sa_places::Google::candidates`]), asked for `owner`:
	/// their `gmail`'s key first, if they name one of theirs.
	pub async fn place_candidates(&self, owner: Uuid, gmail: Option<i64>, input: &str) -> eyre::Result<Result<Vec<(String, Option<Place>)>, Refused>> {
		if let Some(g) = gmail
			&& db::gmail_owner(&mut *self.store.pool().acquire().await.wrap_err("a connection for a gmail")?, g).await? != Some(owner)
		{
			return Ok(Err(Refused::NotFound));
		}
		let purse = self.purse(owner);
		let payer = self.payer(owner, gmail, Some(&purse)).await?;
		match self.google.candidates(input, &payer).await {
			Ok(places) => Ok(Ok(places)),
			Err(e) => Ok(Err(places_error(e)?)),
		}
	}

	/// A GBP not on Google yet: its name, which no live GBP has, and the owner's `trade`.
	pub async fn create_draft(&self, owner: Uuid, raw: &str, trade: Option<i64>, now: Timestamp) -> eyre::Result<Result<Gmb, Refused>> {
		let name = match name(raw) {
			Ok(n) => n,
			Err(r) => return Ok(Err(r)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if !trade_is(&mut tx, owner, trade).await? {
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
			gmail_id: None,
			place_id: None,
			name: &name,
			address: None,
			state: State::Draft,
			country: None,
			country_by_hand: false,
			at: None,
			trade_id: trade,
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
		let gmail_owner = match db::gmail_owner(&mut *self.store.pool().acquire().await.wrap_err("a connection for a gmail")?, e.gmail_id).await? {
			Some(o) if owner.is_none_or(|owner| owner == o) => o,
			_ => return Ok(Err(Refused::NotFound)),
		};
		let purse = self.purse(gmail_owner);
		let payer = self.payer(gmail_owner, Some(e.gmail_id), owner.map(|_| &purse as &dyn sa_places::Purse)).await?;
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
			trade_id: e.trade,
		};
		let added = db::add_gmb(&mut tx, None, &new, now).await?;
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
		if let Some(gmail) = edit.gmail_id {
			if db::gmail_owner(&mut tx, gmail).await? != Some(owner) {
				return Ok(Err(Refused::NotFound));
			}
			db::move_to(&mut tx, id, gmail).await?;
		}
		db::set_fields(&mut tx, id, &edit).await?;
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
			(State::Draft, Some(_)) if read.gmail_id.is_none() => {
				return Ok(Err(Refused::Invalid(format!("GBP {id} goes onto Google under a gmail: give it its gmail first"))));
			}
			(State::Draft, Some(input)) => {
				let gmail = read.gmail_id.expect("refused above without one");
				let Some(gmail_owner) = db::gmail_owner(&mut *self.store.pool().acquire().await.wrap_err("a connection for a gmail")?, gmail).await? else {
					return Ok(Err(Refused::NotFound)); // its gmail removed since it was read, and the GBP with it
				};
				let purse = self.purse(gmail_owner);
				let payer = self.payer(gmail_owner, Some(gmail), owner.map(|_| &purse as &dyn sa_places::Purse)).await?;
				match self.google.resolve(input, &payer).await {
					Ok(found) => Some(found),
					Err(e) => return Ok(Err(places_error(e)?)),
				}
			}
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
	/// first: the field mask keys it, so an answer from before countries is asked again). Its
	/// gmail's keys first; the house pays for the rotation: the operator asks, not the owner.
	pub async fn locate(&self) -> eyre::Result<Located> {
		let unlocated = db::unlocated(&mut *self.store.pool().acquire().await.wrap_err("a connection for the unlocated")?).await?;
		let mut out = Located::default();
		for (id, place_id, pointless, owner, gmail) in unlocated {
			let payer = self.payer(owner, Some(gmail), None).await?;
			let place = match self.google.details(&place_id, Ask::Cached, &payer).await {
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
			0 => db::delete_trade(&mut tx, id).await?,
			n => return Ok(Err(Refused::Conflict(format!("{n} GBPs hold trade {id}: give them another first")))),
		}
		tx.commit().await.wrap_err("committing a trade's deletion")?;
		Ok(Ok(()))
	}

	/// The owner's trade `id` set up for `c.country` as `c` says, over what it was: the keywords in
	/// [`sa_core::gbp::keywords`]' form, each text trimmed and not empty.
	pub async fn set_up(&self, owner: Uuid, id: i64, c: Category) -> eyre::Result<Result<Trade, Refused>> {
		let text = |what: &str, raw: Option<String>| match raw.as_deref().map(str::trim) {
			Some("") => Err(Refused::Invalid(format!("a category's {what} is not empty: null clears it"))),
			t => Ok(t.map(str::to_owned)),
		};
		let c = match (sa_core::gbp::keywords(&c.keywords), text("Google category", c.google_category), text("study", c.study)) {
			(Ok(keywords), Ok(google_category), Ok(study)) => Category {
				country: c.country,
				keywords,
				google_category,
				study,
			},
			(Err(e), _, _) => return Ok(Err(Refused::Invalid(e.to_string()))),
			(_, Err(r), _) | (_, _, Err(r)) => return Ok(Err(r)),
		};
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
		if db::trade_owner(&mut tx, id).await? != Some(owner) || !db::remove_category(&mut tx, id, country).await? {
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
		if !trade_is(&mut tx, owner, trade).await? || !db::set_trades(&mut tx, owner, &ids, trade).await? {
			return Ok(Err(Refused::NotFound));
		}
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
