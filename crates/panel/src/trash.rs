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
	pub gmbs: usize,
	/// Why it cannot be restored now.
	pub blocked: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrashedGmb {
	pub id: i64,
	pub name: String,
	pub state: State,
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

impl Panel {
	pub async fn trash(&self, owner: Uuid, keep_by_default: bool) -> eyre::Result<Trash> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the trash")?;
		let (keep_days, never_purge) = db::settings(&mut conn, owner).await?;
		let settings = Settings {
			keep_days,
			never_purge: never_purge.unwrap_or(keep_by_default),
		};
		let purge_at = |removed: Timestamp| (!settings.never_purge).then(|| removed + SignedDuration::from_hours(24 * keep_days));
		let mut gmails = Vec::new();
		for g in db::gmails(&mut conn, owner, None).await? {
			let blocked = db::gmail_blocked(&mut conn, owner, &g).await?;
			gmails.push(TrashedGmail {
				id: g.id,
				gmail: g.gmail,
				removed_at: g.removed_at,
				purge_at: purge_at(g.removed_at),
				gmbs: g.gmbs,
				blocked,
			});
		}
		let mut gmbs = Vec::new();
		for b in db::gmbs(&mut conn, owner).await? {
			let blocked = db::gmb_blocked(&mut conn, &b).await?;
			gmbs.push(TrashedGmb {
				id: b.id,
				name: b.name,
				state: b.state,
				gmail_id: b.gmail_id,
				gmail: b.gmail,
				removed_at: b.removed_at,
				purge_at: purge_at(b.removed_at),
				blocked,
			});
		}
		Ok(Trash { settings, gmails, gmbs })
	}

	/// `never_purge` `None`: left as it was.
	pub async fn set_trash_settings(&self, owner: Uuid, keep_days: i64, never_purge: Option<bool>) -> eyre::Result<Result<(), Refused>> {
		if ![1, 7, 30, 90].contains(&keep_days) {
			return Ok(Err(Refused::Invalid(format!("{keep_days} days: the trash keeps 1, 7, 30 or 90"))));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for trash settings")?;
		db::set_settings(&mut conn, owner, keep_days, never_purge).await?;
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
		db::restore(&mut tx, Some(id), None).await?;
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
		tx.commit().await.wrap_err("committing a GBP's restore")?;
		self.snapshot_due.notify_one();
		Ok(Ok(()))
	}

	/// Everyone's trash past their kept days.
	pub async fn purge(&self, now: Timestamp, keep_by_default: bool) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		db::purge(&mut tx, now, keep_by_default).await?;
		tx.commit().await.wrap_err("committing a purge")
	}
}
