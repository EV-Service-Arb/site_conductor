//! Access requests: a signed-in user who lacks a permission asks the admins for it, who grant
//! it in concierge's cabinet; the `access_requested` rule tells them, the
//! asker is told once they hold it.

use std::sync::LazyLock;

use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_auth::{Catalog, PermissionSet};
use uuid::Uuid;

pub use crate::store::access::AccessRequest;
use crate::{Panel, store::access as db};

/// A request stands this long: asked again within it, nothing changes and nobody is told again.
pub const ASK_AGAIN_AFTER: SignedDuration = SignedDuration::from_hours(24);

/// Every `sa` permission and alias.
pub static CATALOG: LazyLock<Catalog> = LazyLock::new(|| Catalog::collect("sa", 0));

/// Whether `permissions` hold everything `need` stands for; `None` for a need outside the catalog.
pub fn holds(permissions: &PermissionSet, need: &str) -> Option<bool> {
	let has = |p: &str| permissions.iter().any(|q| q == p);
	if CATALOG.permissions.contains(need) {
		return Some(has(need));
	}
	CATALOG.aliases.get(need).map(|members| members.iter().all(|m| has(m)))
}

/// Who asks.
pub struct Asker<'a> {
	pub user: Uuid,
	pub email: &'a str,
	pub name: &'a str,
}

impl Panel {
	/// The user's request for `need`: the standing one (`false`), or a new one made at `now` (`true`).
	pub async fn request_access(&self, asker: Asker<'_>, need: &str, now: Timestamp) -> eyre::Result<(AccessRequest, bool)> {
		let Asker { user, email, name } = asker;
		let now = crate::store::from_db(crate::store::to_db(now))?; // as it will read back
		let mut tx = self.store.begin_write().await?;
		if let Some(standing) = db::of_user(&mut tx, user).await?.into_iter().find(|r| r.need == need && now < r.requested_at + ASK_AGAIN_AFTER) {
			return Ok((standing, false));
		}
		let request = AccessRequest {
			user_id: user,
			need: need.to_owned(),
			email: email.to_owned(),
			name: name.to_owned(),
			requested_at: now,
			event_id: crate::alerts::derived_event_id(&["access_requested", &user.to_string(), need, &now.to_string()]),
		};
		db::put(&mut tx, &request).await?;
		tx.commit().await.wrap_err("committing an access request")?;
		tracing::info!(user_id = %user, need, "access requested");
		Ok((request, true))
	}

	/// The user's requests still standing: those whose need `permissions` now hold are dropped, and
	/// they are told.
	pub async fn pending_access(&self, user: Uuid, permissions: &PermissionSet) -> eyre::Result<Vec<AccessRequest>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for access requests")?;
		let (held, pending): (Vec<_>, Vec<_>) = db::of_user(&mut conn, user)
			.await?
			.into_iter()
			.partition(|r| holds(permissions, &r.need) == Some(true));
		for r in held {
			self.granted(&mut conn, &r, Timestamp::now()).await?;
		}
		Ok(pending)
	}

	/// Tells whoever asked for access and holds it, by what concierge last confirmed of them.
	pub(crate) async fn tell_accesses(&self, conn: &mut sqlx::SqliteConnection, now: Timestamp) -> eyre::Result<()> {
		for (r, permissions) in db::confirmed(conn).await? {
			if holds(&permissions, &r.need) == Some(true) {
				self.granted(conn, &r, now).await?;
			}
		}
		Ok(())
	}

	async fn granted(&self, conn: &mut sqlx::SqliteConnection, r: &AccessRequest, now: Timestamp) -> eyre::Result<()> {
		let mut tx = crate::store::begin_write(conn).await?;
		self.tell_granted(&mut tx, r, now).await?;
		db::delete(&mut tx, r.user_id, &r.need).await?;
		tx.commit().await.wrap_err("committing an access granted")
	}
}
