//! A removed gmail or GBP waits in its owner's trash, restorable, until it is purged after their
//! kept days: then it is gone for good, its row kept (the services hold its id) and a gmail's
//! connections deleted. `keep_by_default` is whether a person who has not said keeps everything.

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_core::gbp::State;
use uuid::Uuid;

use crate::{Panel, Refused, store::trash as db, tokens::is_open};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Settings {
	/// 1 to 90.
	pub keep_days: i64,
	pub never_purge: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedGmail {
	pub id: i64,
	pub gmail: String,
	pub removed_at: Timestamp,
	/// `None`: kept until restored.
	pub purge_at: Option<Timestamp>,
	/// Its GBPs removed with it, which come back with it.
	pub gmbs: Vec<Listing>,
	/// Why it cannot be restored now.
	pub blocked: Option<String>,
}

/// What tells a trashed GBP apart.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Listing {
	pub id: i64,
	pub name: String,
	pub state: State,
	pub place_id: Option<String>,
	pub address: Option<String>,
	pub phone: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedGmb {
	pub listing: Listing,
	pub gmail_id: Option<i64>,
	pub gmail: Option<String>,
	pub removed_at: Timestamp,
	pub purge_at: Option<Timestamp>,
	pub blocked: Option<String>,
}

/// A person's trash: the gmails, and the GBPs removed on their own.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Trash {
	pub settings: Settings,
	pub gmails: Vec<TrashedGmail>,
	pub gmbs: Vec<TrashedGmb>,
}

impl From<db::TrashedGmb> for Listing {
	fn from(b: db::TrashedGmb) -> Self {
		Self {
			id: b.id,
			name: b.name,
			state: b.state,
			place_id: b.place_id,
			address: b.address,
			phone: b.phone,
		}
	}
}

impl Panel {
	pub async fn trash(&self, owner: Uuid, keep_by_default: bool) -> eyre::Result<Trash> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the trash")?;
		let (keep_days, never_purge, counted_from) = db::settings(&mut conn, owner).await?;
		let settings = Settings {
			keep_days,
			never_purge: never_purge.unwrap_or(keep_by_default),
		};
		let purge_at = |removed: Timestamp| (!settings.never_purge).then(|| counted_from.map_or(removed, |c| c.max(removed)) + SignedDuration::from_hours(24 * keep_days));
		let mut gmails = Vec::new();
		for g in db::gmails(&mut conn, owner, None).await? {
			let blocked = db::gmail_blocked(&mut conn, owner, &g).await?;
			let with = db::coming_back(&mut conn, owner, Some(g.id), None).await?;
			gmails.push(TrashedGmail {
				id: g.id,
				gmail: g.gmail,
				removed_at: g.removed_at,
				purge_at: purge_at(g.removed_at),
				gmbs: with.into_iter().map(Listing::from).collect(),
				blocked,
			});
		}
		let mut gmbs = Vec::new();
		for b in db::gmbs(&mut conn, owner).await? {
			let blocked = db::gmb_blocked(&mut conn, &b).await?;
			let (gmail_id, gmail, removed_at) = (b.gmail_id, b.gmail.clone(), b.removed_at);
			gmbs.push(TrashedGmb {
				listing: b.into(),
				gmail_id,
				gmail,
				removed_at,
				purge_at: purge_at(removed_at),
				blocked,
			});
		}
		Ok(Trash { settings, gmails, gmbs })
	}

	/// `never_purge` `None`: left as it was. Turned off at `now`, what is in the trash counts its
	/// kept days from `now`.
	pub async fn set_trash_settings(&self, owner: Uuid, keep_days: i64, never_purge: Option<bool>, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if ![1, 7, 30, 90].contains(&keep_days) {
			return Ok(Err(Refused::Invalid(format!("{keep_days} days: the trash keeps 1, 7, 30 or 90"))));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for trash settings")?;
		db::set_settings(&mut conn, owner, keep_days, never_purge, now).await?;
		Ok(Ok(()))
	}

	/// The owner's trashed gmail `id` back, with the GBPs removed with it.
	pub async fn restore_gmail(&self, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(g) = db::gmails(&mut tx, owner, Some(id)).await?.pop() else {
			return Ok(Err(Refused::NotFound));
		};
		if let Some(why) = db::gmail_blocked(&mut tx, owner, &g).await? {
			return Ok(Err(Refused::Conflict(why)));
		}
		let back = db::coming_back(&mut tx, owner, Some(id), None).await?;
		db::restore(&mut tx, Some(id), None).await?;
		let now = Timestamp::now();
		for b in back {
			crate::store::gmbs::log(&mut tx, Some(b.id), b.gmail_id, Some(owner), now, &crate::store::gmbs::Event::Restored).await?;
		}
		tx.commit().await.wrap_err("committing a gmail's restore")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// The owner's trashed GBP `id` back, its gmail live.
	pub async fn restore_gmb(&self, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		let Some(b) = db::gmb_of(&mut tx, owner, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		if let Some(why) = db::gmb_blocked(&mut tx, &b).await? {
			return Ok(Err(Refused::Conflict(why)));
		}
		db::restore(&mut tx, None, Some(id)).await?;
		crate::store::gmbs::log(&mut tx, Some(id), b.gmail_id, Some(owner), Timestamp::now(), &crate::store::gmbs::Event::Restored).await?;
		tx.commit().await.wrap_err("committing a GBP's restore")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// The owner's trashed gmail `id` gone for good at `now`, with every GBP of it in the trash.
	pub async fn erase_gmail(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if db::gmails(&mut tx, owner, Some(id)).await?.is_empty() {
			return Ok(Err(Refused::NotFound));
		}
		db::erase(&mut tx, Some(id), None, now).await?;
		tx.commit().await.wrap_err("committing a gmail's erase")?;
		Ok(Ok(()))
	}

	/// The owner's trashed GBP `id` gone for good at `now`.
	pub async fn erase_gmb(&self, owner: Uuid, id: i64, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if db::gmb_of(&mut tx, owner, id).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		db::erase(&mut tx, None, Some(id), now).await?;
		tx.commit().await.wrap_err("committing a GBP's erase")?;
		Ok(Ok(()))
	}

	/// Everyone's trash past their kept days.
	pub async fn purge(&self, now: Timestamp, keep_by_default: bool) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		db::purge(&mut tx, now, keep_by_default).await?;
		tx.commit().await.wrap_err("committing a purge")
	}
}
