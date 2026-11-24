//! The library (docs/ARCHITECTURE.md, Library): category versions released as listings with
//! numbered versions, some ranked the standard library, and studies published on released versions.
//! What is released is readable by everyone, for good; withdrawn hides it from whoever never used it.

use eyre::WrapErr;
use gmaps_optimal_placement_core::Definition;
use jiff::Timestamp;
use sa_core::gbp::Country;
use uuid::Uuid;

use crate::{
	Panel, Refused,
	categories::Version,
	store::{categories as categories_db, library as db},
	tokens::is_open,
};

#[derive(Clone, Debug)]
pub struct Listing {
	pub id: i64,
	pub trade: String,
	pub country: Country,
	pub author: Uuid,
	pub author_name: String,
	pub title: String,
	pub about: String,
	pub forked_from: Option<i64>,
	/// In the standard library, at this place.
	pub standard_rank: Option<i64>,
	pub created_at: Timestamp,
	pub withdrawn_at: Option<Timestamp>,
	pub latest: Released,
	/// Categories whose lineage is this listing.
	pub used_by: i64,
}

/// A listing's numbered version.
#[derive(Clone, Debug, PartialEq)]
pub struct Released {
	pub id: i64,
	pub listing: i64,
	pub n: i64,
	/// The category version it releases.
	pub version: i64,
	pub notes: String,
	pub published_at: Timestamp,
}

/// A study published on a released category version.
#[derive(Clone, Debug)]
pub struct Published {
	/// Its definition's id.
	pub sha: String,
	/// Frozen: the area's geometry as it was.
	pub definition: Definition,
	pub title: String,
	pub author_name: String,
	pub at: Timestamp,
	pub withdrawn_at: Option<Timestamp>,
	pub listing: i64,
	pub listing_title: String,
	pub trade: String,
	pub country: Country,
	pub on: Released,
}

/// Which listings a browse is of.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
	/// Ranked by a curator.
	Standard,
	/// Everyone else's, unranked.
	Community,
	/// The reader's own.
	Mine,
	/// Those the reader adopted or loaded a study of.
	Used,
}

/// A browse's order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
	/// The standard library's rank first, then the newest.
	Rank,
	/// The newest version first.
	Recent,
	/// The most used first.
	Used,
}

/// A new listing's own words.
#[derive(Clone, Debug)]
pub struct NewListing {
	pub title: String,
	pub about: String,
	pub notes: String,
	pub forked_from: Option<i64>,
}

/// How an unreleased version is released in the same write as what needs it.
#[derive(Clone, Debug)]
pub enum Release {
	New(NewListing),
	/// The next version of one of the author's listings.
	Next {
		listing: i64,
		notes: String,
	},
}

/// A listing and what it is: every version with its body, and the studies published on it.
#[derive(Clone, Debug)]
pub struct Detail {
	pub listing: Listing,
	pub versions: Vec<(Released, Version)>,
	pub studies: Vec<Published>,
}

impl Panel {
	/// The listings `reader` may see, of `trade` and `country` when given, `kind` when given,
	/// matching `q` (in the title, the notes or the trade's names), in `sort`'s order.
	pub async fn library(&self, reader: Uuid, trade: Option<&str>, country: Option<Country>, q: Option<&str>, kind: Option<Kind>, sort: Sort) -> eyre::Result<Vec<Listing>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the library")?;
		let q = q.map(str::to_lowercase);
		let used = db::seen_by(&mut conn, reader).await?;
		let mut out: Vec<Listing> = db::listings(&mut conn, reader, trade, country)
			.await?
			.into_iter()
			.filter(|l| match kind {
				None => true,
				Some(Kind::Standard) => l.standard_rank.is_some(),
				Some(Kind::Community) => l.standard_rank.is_none() && l.author != reader,
				Some(Kind::Mine) => l.author == reader,
				Some(Kind::Used) => used.contains(&l.id),
			})
			.filter(|l| {
				q.as_ref().is_none_or(|q| {
					let names = crate::google_categories::GoogleCategory::find(l.country, &l.trade).map(|g| g.names.values().cloned().collect::<Vec<_>>().join(" "));
					// a gcid a refresh dropped matches by its title alone
					format!("{} {} {}", l.title, l.about, names.unwrap_or_default()).to_lowercase().contains(q.as_str())
				})
			})
			.collect();
		match sort {
			Sort::Rank => out.sort_by_key(|l| (l.standard_rank.is_none(), l.standard_rank, std::cmp::Reverse(l.latest.published_at))),
			Sort::Recent => out.sort_by_key(|l| std::cmp::Reverse(l.latest.published_at)),
			Sort::Used => out.sort_by_key(|l| (std::cmp::Reverse(l.used_by), std::cmp::Reverse(l.latest.published_at))),
		}
		Ok(out)
	}

	/// Listing `id`, if `reader` may see it.
	pub async fn listing(&self, reader: Uuid, id: i64) -> eyre::Result<Result<Detail, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a listing")?;
		let Some(listing) = db::listing_of(&mut conn, reader, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let mut versions = Vec::new();
		for r in db::versions(&mut conn, id).await? {
			let v = categories_db::version(&mut conn, r.version).await?.expect("a release is of a version");
			versions.push((r, v));
		}
		let studies = db::studies(&mut conn, reader, None, None, None, Some(id)).await?;
		Ok(Ok(Detail { listing, versions, studies }))
	}

	/// Version `version`, one of the author's, released as `release` says.
	pub async fn publish(&self, author: Uuid, version: i64, release: Release, now: Timestamp) -> eyre::Result<Result<Listing, Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let released = match self.released(&mut tx, author, version, release, now).await? {
			Ok(r) => r,
			Err(e) => return Ok(Err(e)),
		};
		let listing = db::listing_of(&mut tx, author, released.listing).await?.expect("released in this transaction");
		tx.commit().await.wrap_err("committing a release")?;
		Ok(Ok(listing))
	}

	/// The author's listing `id` withdrawn: hidden from whoever never used it, readable by whoever did.
	pub async fn withdraw(&self, author: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !db::withdraw(&mut tx, author, id, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a withdrawal")?;
		Ok(Ok(()))
	}

	/// Listing `id` in the standard library at `rank`, or out of it.
	pub async fn rank(&self, id: i64, rank: Option<i64>) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !db::rank(&mut tx, id, rank).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a rank")?;
		Ok(Ok(()))
	}

	/// `definition`, a study of category version `version`, published: on that version's latest
	/// release, or released by `release` in the same write; refused [`Refused::Unreleased`] on a
	/// version neither releases.
	pub async fn publish_study(
		&self,
		author: Uuid,
		definition: &Definition,
		version: i64,
		title: &str,
		release: Option<Release>,
		now: Timestamp,
	) -> eyre::Result<Result<Published, Refused>> {
		let Some(title) = Some(title.trim()).filter(|t| !t.is_empty() && t.chars().count() <= 100) else {
			return Ok(Err(Refused::Invalid("a study's title is 1 to 100 characters".to_owned())));
		};
		let sha = definition.id();
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let on = match (db::released_as(&mut tx, version).await?, release) {
			(_, Some(release)) => match self.released(&mut tx, author, version, release, now).await? {
				Ok(r) => r,
				Err(e) => return Ok(Err(e)),
			},
			(Some(r), None) => r,
			(None, None) => return Ok(Err(Refused::Unreleased(format!("version {version} is no listing's: release it to publish a study on it")))),
		};
		let kept = serde_json::to_string(definition).expect("a definition serializes");
		if !db::add_study(&mut tx, &sha, &kept, on.id, author, title, now).await? {
			return Ok(Err(Refused::Conflict(format!("study {sha} is published already"))));
		}
		let published = db::study_of(&mut tx, author, &sha).await?.expect("published in this transaction");
		tx.commit().await.wrap_err("committing a published study")?;
		Ok(Ok(published))
	}

	/// The published studies `reader` may see, of `trade`, `country` and `area` when given.
	pub async fn published_studies(&self, reader: Uuid, trade: Option<&str>, country: Option<Country>, area: Option<&str>) -> eyre::Result<Vec<Published>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for published studies")?;
		db::studies(&mut conn, reader, trade, country, area, None).await
	}

	/// Published study `sha`, if `reader` may see it, marked used by them.
	pub async fn published_study(&self, reader: Uuid, sha: &str, now: Timestamp) -> eyre::Result<Result<Published, Refused>> {
		let mut tx = self.store.begin_write().await?;
		let Some(p) = db::study_of(&mut tx, reader, sha).await? else {
			return Ok(Err(Refused::NotFound));
		};
		db::seen(&mut tx, reader, p.listing, now).await?;
		tx.commit().await.wrap_err("committing a study's use")?;
		Ok(Ok(p))
	}

	/// The author's published study `sha` withdrawn.
	pub async fn withdraw_study(&self, author: Uuid, sha: &str, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !db::withdraw_study(&mut tx, author, sha, now).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a study's withdrawal")?;
		Ok(Ok(()))
	}

	/// `version` released as `release` says: it must be in the author's history, and a next version
	/// must be on a listing of theirs, of its trade and country, not withdrawn.
	async fn released(&self, conn: &mut sqlx::SqliteConnection, author: Uuid, version: i64, release: Release, now: Timestamp) -> eyre::Result<Result<Released, Refused>> {
		let words = |s: &str, at_most: usize, what: &str| {
			let s = s.trim();
			(s.chars().count() <= at_most)
				.then(|| s.to_owned())
				.ok_or_else(|| Refused::Invalid(format!("{what} is at most {at_most} characters")))
		};
		if !categories_db::ours(conn, author, version).await? {
			return Ok(Err(Refused::NotFound));
		}
		let v = categories_db::version(conn, version).await?.expect("one of their moves went to it");
		let listing = match release {
			Release::New(n) => {
				let (title, about, notes) = match (words(&n.title, 100, "a title"), words(&n.about, 4000, "an about"), words(&n.notes, 4000, "a note")) {
					(Ok(t), Ok(a), Ok(no)) if !t.is_empty() => (t, a, no),
					(Ok(_), Ok(_), Ok(_)) => return Ok(Err(Refused::Invalid("a listing has a title".to_owned()))),
					(Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => return Ok(Err(e)),
				};
				if let Some(f) = n.forked_from
					&& db::listing_of(conn, author, f).await?.is_none()
				{
					return Ok(Err(Refused::NotFound));
				}
				let id = db::add_listing(conn, author, &v.trade, v.country, &title, &about, n.forked_from, now).await?;
				return Ok(Ok(db::add_release(conn, id, version, &notes, now).await?));
			}
			Release::Next { listing, notes } => {
				let notes = match words(&notes, 4000, "a note") {
					Ok(n) => n,
					Err(e) => return Ok(Err(e)),
				};
				let Some(l) = db::listing_of(conn, author, listing).await?.filter(|l| l.author == author && l.withdrawn_at.is_none()) else {
					return Ok(Err(Refused::NotFound));
				};
				if (l.trade.as_str(), l.country) != (v.trade.as_str(), v.country) {
					return Ok(Err(Refused::Invalid(format!(
						"listing {listing} is of {}·{}, the version of {}·{}",
						l.trade, l.country, v.trade, v.country
					))));
				}
				if l.latest.version == version {
					return Ok(Err(Refused::Conflict(format!("listing {listing}'s latest is this version already"))));
				}
				(l, notes)
			}
		};
		let released = db::add_release(conn, listing.0.id, version, &listing.1, now).await?;
		self.tell_release(conn, &listing.0, &released, now).await?;
		Ok(Ok(released))
	}
}
