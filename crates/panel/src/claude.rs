//! A person's own Claude: the token their assistant's answers run on, proved by the conductor
//! before it is stored, sealed, and never read back out but by the ask.

use eyre::WrapErr;
use jiff::Timestamp;
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::store::claude::Claude;
use crate::{
	Panel, Refused,
	seal::user_claude_aad,
	store::claude::{self as db, Checked},
};

impl Panel {
	pub async fn claude(&self, user: Uuid) -> eyre::Result<Option<Claude>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's Claude")?;
		Ok(db::get(&mut conn, user).await?.map(|(c, _)| c))
	}

	/// The token the user's answers run on.
	pub async fn claude_token(&self, user: Uuid) -> eyre::Result<Option<Zeroizing<String>>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's Claude")?;
		db::get(&mut conn, user).await?.map(|(_, sealed)| self.unseal(&user_claude_aad(user), &sealed)).transpose()
	}

	/// The user's Claude, once Claude took `token`.
	pub async fn set_claude(&self, user: Uuid, token: &str, expires_at: Timestamp, now: Timestamp) -> eyre::Result<Claude> {
		let sealed = self.seal(&user_claude_aad(user), token)?;
		let mut tx = self.store.begin_write().await?;
		db::set(&mut tx, user, &sealed, expires_at, now).await?;
		tx.commit().await.wrap_err("committing a person's Claude")?;
		self.claude(user).await?.ok_or_else(|| eyre::eyre!("a person's Claude, just written, is gone"))
	}

	/// Claude refused the user's token: what it said.
	pub async fn claude_refused(&self, user: Uuid, said: String, now: Timestamp) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		db::checked(&mut tx, user, &Checked { at: now, error: Some(said) }).await?;
		tx.commit().await.wrap_err("committing a refused Claude")
	}

	pub async fn remove_claude(&self, user: Uuid) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !db::delete(&mut tx, user).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a person's Claude removed")?;
		Ok(Ok(()))
	}
}
