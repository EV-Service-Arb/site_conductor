//! A person's categories (docs/ARCHITECTURE.md, A GBP's category): their settings for a trade, one
//! of Google's categories, in a country. Each edit is a new version, never changed; a category's
//! head moves only by a move its history keeps (`edit`, `adopt` or `update` from the library,
//! `revert`), and its lineage is derived from those moves.

use eyre::WrapErr;
use gmaps_optimal_placement_core::{Additional as Tier, Category, Demand, Excluded, Gcid, Keyword};
use jiff::Timestamp;
use sa_core::{Invalid, gbp::Country};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	Panel, Refused,
	google_categories::GoogleCategory,
	library::{Listing, Released},
	store::{categories as db, library as library_db},
	tokens::is_open,
};

/// One person's category: theirs, of a trade, in a country.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Key {
	pub owner: Uuid,
	pub trade: GoogleCategory,
	pub country: Country,
}
impl Key {
	/// `trade` as Google lists it in `country`.
	pub fn try_new(owner: Uuid, trade: &str, country: Country) -> Result<Self, Refused> {
		let trade = GoogleCategory::find(country, trade).map_err(|e| Refused::Invalid(e.to_string()))?;
		Ok(Self { owner, trade, country })
	}

	fn name(self) -> &'static str {
		self.trade.names.get(self.country.lang()).expect("google_categories.json names each in its countries' languages")
	}
}

/// What a version says, by gcid; named only when a study is built of it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
	/// [`gmaps_optimal_placement_core::keywords`]' form, the trade's name among them at weight 1.
	pub keywords: Vec<Keyword>,
	/// In the order they are entered into Google's form: a competitor tier each.
	pub additional: Vec<Additional>,
	pub excluded: Excluded,
	pub demand: Option<Demand>,
}
impl Body {
	/// What a trade picked from Google's list starts from: its name alone, at weight 1.
	pub fn empty(key: Key) -> Self {
		let own = gmaps_optimal_placement_core::keywords(&[key.name().to_owned()]).expect("Google's names are keywords").remove(0);
		Self {
			keywords: vec![Keyword { text: own, weight: Some(1.) }],
			additional: Vec::new(),
			excluded: Excluded::default(),
			demand: None,
		}
	}

	/// Placement's category of it, in placement's `placed`: every gcid named in `country`'s
	/// language. Fails on a gcid Google does not list there.
	fn category(&self, trade: GoogleCategory, country: Country, placed: gmaps_optimal_placement_core::Country) -> Result<Category, Invalid> {
		let named = |g: GoogleCategory| Gcid {
			id: g.id.to_owned(),
			name: g.names.get(country.lang()).expect("google_categories.json names each in its countries' languages").clone(),
		};
		Ok(Category {
			trade: named(trade),
			country: placed,
			additional: self
				.additional
				.iter()
				.map(|a| {
					Ok(Tier {
						google: named(GoogleCategory::find(country, &a.id)?),
						weight: a.weight,
					})
				})
				.collect::<Result<_, Invalid>>()?,
			keywords: self.keywords.clone(),
			excluded: self.excluded.clone(),
			demand: self.demand.clone(),
		})
	}

	/// As stored: keywords in their normalized form, excluded words trimmed lowercase; refused where a
	/// study of it could not be built.
	fn checked(self, key: Key) -> Result<Self, Refused> {
		let invalid = |e: gmaps_optimal_placement_core::Invalid| Refused::Invalid(e.to_string());
		let texts = gmaps_optimal_placement_core::keywords(&self.keywords.iter().map(|k| k.text.clone()).collect::<Vec<_>>()).map_err(invalid)?;
		let body = Self {
			keywords: texts.into_iter().zip(&self.keywords).map(|(text, k)| Keyword { text, weight: k.weight }).collect(),
			excluded: Excluded {
				words: self.excluded.words.iter().map(|w| w.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()).collect(),
				types: self.excluded.types,
			},
			..self
		};
		match key.country.as_str().parse::<gmaps_optimal_placement_core::Country>() {
			Ok(placed) => body.category(key.trade, key.country, placed).map_err(|e| Refused::Invalid(e.0))?.check().map_err(invalid)?,
			// shortcut: a country placement holds no base for has no placement Category, so it is checked part by part (tmp/ongoing_dev/logic_duplication.md)
			Err(_) => {
				for a in &body.additional {
					GoogleCategory::find(key.country, &a.id).map_err(|e| Refused::Invalid(e.to_string()))?;
				}
				gmaps_optimal_placement_core::weighed(&body.keywords).map_err(invalid)?;
				body.excluded.check().map_err(invalid)?;
				body.demand.as_ref().map_or(Ok(()), Demand::check).map_err(invalid)?;
			}
		}
		Ok(body)
	}
}

/// An additional Google category and what one competitor of its tier weighs against one of the
/// trade's.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Additional {
	/// A gcid Google lists in the category's country.
	pub id: String,
	pub weight: f64,
}

/// One version of a trade in a country: written once, read by whoever stands on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Version {
	pub id: i64,
	pub trade: String,
	pub country: Country,
	pub body: Body,
	pub author: Uuid,
	pub at: Timestamp,
}

impl Version {
	/// Placement's category of it, in placement's `placed`, named in its country's language.
	pub fn category(&self, placed: gmaps_optimal_placement_core::Country) -> eyre::Result<Category> {
		// a gcid a refresh of Google's list dropped: its rows need migrating to what Google replaced it with
		let trade = GoogleCategory::find(self.country, &self.trade).wrap_err_with(|| format!("version {}", self.id))?;
		self.body.category(trade, self.country, placed).wrap_err_with(|| format!("version {}", self.id))
	}
}

/// Why a category's head moved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Why {
	/// A version of its own, applied.
	Edit,
	/// A listing's version, taken.
	Adopt,
	/// Its listing's newer version, taken.
	Update,
	/// Back to one of its own history's.
	Revert,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Move {
	pub id: i64,
	pub why: Why,
	/// None: the category's creation.
	pub from: Option<i64>,
	pub to: i64,
	/// An adopt's or update's.
	pub listing_version: Option<i64>,
	pub at: Timestamp,
}

/// A category of the caller's.
#[derive(Clone, Debug)]
pub struct Mine {
	pub trade: GoogleCategory,
	pub country: Country,
	pub head: Version,
	pub lineage: Option<Lineage>,
}

/// Where a category was last adopted or updated from.
#[derive(Clone, Debug)]
pub struct Lineage {
	pub listing: Listing,
	pub from: Released,
	/// Its head is not the version it took.
	pub edited: bool,
	/// The listing's latest, once newer than what it took: never taken without the person's say.
	pub upstream: Option<Released>,
}

impl Panel {
	/// The owner's categories, by trade and country.
	pub async fn categories(&self, owner: Uuid) -> eyre::Result<Vec<Mine>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for categories")?;
		let rows = db::rows(&mut conn, owner).await?;
		let mut out = Vec::with_capacity(rows.len());
		for r in rows {
			out.push(mine(&mut conn, owner, r).await?);
		}
		Ok(out)
	}

	/// `key`, as its owner reads it.
	pub async fn category(&self, key: Key) -> eyre::Result<Option<Mine>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a category")?;
		of(&mut conn, key).await
	}

	/// `body` applied to `key`, set up there if it was not: a new version and a move onto it; a
	/// body equal to its head's moves nothing.
	pub async fn apply(&self, key: Key, body: Body, now: Timestamp) -> eyre::Result<Result<Mine, Refused>> {
		let body = match body.checked(key) {
			Ok(b) => b,
			Err(e) => return Ok(Err(e)),
		};
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let head = db::head(&mut tx, key).await?;
		let same = match head {
			Some(h) => db::version(&mut tx, h).await?.expect("a head is a version").body == body,
			None => false,
		};
		if !same {
			let to = db::add_version(&mut tx, key, &body, now).await?;
			db::moved(&mut tx, key, Why::Edit, head, to, None, now).await?;
		}
		let set = of(&mut tx, key).await?.expect("set up in this transaction");
		tx.commit().await.wrap_err("committing a category's version")?;
		Ok(Ok(set))
	}

	/// `key`'s head moved to `version`: released by `listing_version` (an adopt, or an update from
	/// its lineage's listing; set up there if it was not), or else one of its own history's (a revert).
	pub async fn move_head(&self, key: Key, version: i64, listing_version: Option<i64>, now: Timestamp) -> eyre::Result<Result<Mine, Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let head = db::head(&mut tx, key).await?;
		let (why, listed) = match listing_version {
			Some(lv) => {
				let Some(r) = library_db::release(&mut tx, lv).await? else {
					return Ok(Err(Refused::NotFound));
				};
				if r.version != version {
					return Ok(Err(Refused::Invalid(format!("listing version {lv} releases version {}, not {version}", r.version))));
				}
				let Some(listing) = library_db::listing_of(&mut tx, key.owner, r.listing).await? else {
					return Ok(Err(Refused::NotFound));
				};
				if (listing.trade.as_str(), listing.country) != (key.trade.id, key.country) {
					return Ok(Err(Refused::Invalid(format!("listing {} is of {}·{}", listing.id, listing.trade, listing.country))));
				}
				let lineage = match head {
					Some(_) => of(&mut tx, key).await?.expect("its head read above").lineage,
					None => None,
				};
				let why = match lineage {
					Some(l) if l.listing.id == r.listing && r.n > l.from.n => Why::Update,
					_ => Why::Adopt,
				};
				library_db::seen(&mut tx, key.owner, r.listing, now).await?;
				(why, Some(r.id))
			}
			None => {
				let Some(head) = head else { return Ok(Err(Refused::NotFound)) };
				let ours = db::history(&mut tx, key).await?.iter().any(|m| m.to == version);
				if !ours || version == head {
					return Ok(Err(Refused::Invalid(format!("version {version} is not an earlier one of this category's"))));
				}
				(Why::Revert, None)
			}
		};
		if head != Some(version) {
			db::moved(&mut tx, key, why, head, version, listed, now).await?;
		}
		let moved = of(&mut tx, key).await?.expect("set up in this transaction");
		tx.commit().await.wrap_err("committing a category's move")?;
		Ok(Ok(moved))
	}

	/// `key`'s moves since it was set up, oldest first.
	pub async fn history(&self, key: Key) -> eyre::Result<Result<Vec<Move>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a history")?;
		if db::head(&mut conn, key).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		Ok(Ok(db::history(&mut conn, key).await?))
	}

	/// `key` taken away while no live GBP, review draft or brand holds it; its history stays.
	pub async fn delete_category(&self, key: Key) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		match db::holders(&mut tx, key).await? {
			(0, 0, 0) => {}
			(n @ 1.., _, _) => return Ok(Err(Refused::Conflict(format!("{n} GBPs hold it: give them another first")))),
			(_, 1.., _) => return Ok(Err(Refused::Conflict("it has review drafts: their uses are kept".to_owned()))),
			(_, _, _) => return Ok(Err(Refused::Conflict("it has brands: their photos' uses are kept".to_owned()))),
		}
		if !db::delete(&mut tx, key).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a category's deletion")?;
		Ok(Ok(()))
	}

	/// Version `id`, if `reader` may read it: one of their moves went to it, or a listing released it.
	pub async fn version(&self, reader: Uuid, id: i64) -> eyre::Result<Result<Version, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a version")?;
		if !db::readable(&mut conn, reader, id).await? {
			return Ok(Err(Refused::NotFound));
		}
		Ok(Ok(db::version(&mut conn, id).await?.expect("readable, so it exists")))
	}
}

async fn of(conn: &mut sqlx::SqliteConnection, key: Key) -> eyre::Result<Option<Mine>> {
	let rows = db::rows(conn, key.owner).await?;
	let Some(r) = rows.into_iter().find(|r| (r.trade.as_str(), r.country) == (key.trade.id, key.country)) else {
		return Ok(None);
	};
	Ok(Some(mine(conn, key.owner, r).await?))
}

async fn mine(conn: &mut sqlx::SqliteConnection, owner: Uuid, r: db::Row) -> eyre::Result<Mine> {
	// a gcid a refresh of Google's list dropped: its rows need migrating to what Google replaced it with
	let trade = GoogleCategory::find(r.country, &r.trade).wrap_err_with(|| format!("category {}·{}", r.trade, r.country))?;
	let head = db::version(conn, r.head).await?.expect("a head is a version");
	let lineage = match r.listing_version {
		None => None,
		Some(lv) => {
			let from = library_db::release(conn, lv).await?.expect("a move's listing version exists");
			let listing = library_db::listing_of(conn, owner, from.listing)
				.await?
				.expect("a listing its owner adopted stays visible to them");
			let upstream = (listing.latest.n > from.n).then(|| listing.latest.clone());
			Some(Lineage {
				edited: head.id != from.version,
				listing,
				from,
				upstream,
			})
		}
	};
	Ok(Mine {
		trade,
		country: r.country,
		head,
		lineage,
	})
}
