//! A person's own Claude and OpenAI key: what their answers run on, each proved by the conductor
//! before it is stored, sealed, and never read back out but to ask on.

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::prices::Provider;
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::store::own_models::{Claude, OpenAi};
use crate::{
	Panel, Refused,
	seal::{user_claude_aad, user_openai_aad},
	store::{connections::Checked, own_models as db},
};

/// A credential of a person's, to ask its provider on.
pub struct Own {
	pub provider: Provider,
	pub secret: Zeroizing<String>,
	/// Its provider refused it, last it was asked.
	pub refused: bool,
}

impl Panel {
	pub async fn claude(&self, user: Uuid) -> eyre::Result<Option<Claude>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's Claude")?;
		Ok(db::claude(&mut conn, user).await?.map(|(c, _)| c))
	}

	pub async fn openai(&self, user: Uuid) -> eyre::Result<Option<OpenAi>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's OpenAI key")?;
		Ok(db::openai(&mut conn, user).await?.map(|(o, _)| o))
	}

	/// What the user's answers run on, in the order asked; none: they connected nothing of their own.
	pub async fn own_models(&self, user: Uuid) -> eyre::Result<Vec<Own>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's own models")?;
		let mut own = Vec::new();
		if let Some((c, sealed)) = db::claude(&mut conn, user).await? {
			own.push(Own {
				provider: Provider::Claude,
				secret: self.unseal(&user_claude_aad(user), &sealed)?,
				refused: c.checked.error.is_some(),
			});
		}
		if let Some((o, sealed)) = db::openai(&mut conn, user).await? {
			own.push(Own {
				provider: Provider::OpenAi,
				secret: self.unseal(&user_openai_aad(user), &sealed)?,
				refused: o.checked.error.is_some(),
			});
		}
		Ok(own)
	}

	/// The user's Claude, once Claude took `token`.
	pub async fn set_claude(&self, user: Uuid, token: &str, expires_at: Timestamp, now: Timestamp) -> eyre::Result<Claude> {
		let sealed = self.seal(&user_claude_aad(user), token)?;
		let mut tx = self.store.begin_write().await?;
		db::set_claude(&mut tx, user, &sealed, expires_at, now).await?;
		tx.commit().await.wrap_err("committing a person's Claude")?;
		self.claude(user).await?.ok_or_else(|| eyre::eyre!("a person's Claude, just written, is gone"))
	}

	/// The user's OpenAI key, once OpenAI took it.
	pub async fn set_openai(&self, user: Uuid, key: &str, now: Timestamp) -> eyre::Result<OpenAi> {
		let sealed = self.seal(&user_openai_aad(user), key)?;
		let mut tx = self.store.begin_write().await?;
		db::set_openai(&mut tx, user, &key[key.len() - 4..], &sealed, now).await?;
		tx.commit().await.wrap_err("committing a person's OpenAI key")?;
		self.openai(user).await?.ok_or_else(|| eyre::eyre!("a person's OpenAI key, just written, is gone"))
	}

	/// The provider refused the user's credential: what it said.
	pub async fn own_model_refused(&self, user: Uuid, of: Provider, said: String, now: Timestamp) -> eyre::Result<()> {
		let mut tx = self.store.begin_write().await?;
		db::checked(&mut tx, user, of, &Checked { at: now, error: Some(said) }).await?;
		tx.commit().await.wrap_err("committing a refused credential")
	}

	pub async fn remove_own_model(&self, user: Uuid, of: Provider) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !db::delete(&mut tx, user, of).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a person's own model removed")?;
		Ok(Ok(()))
	}
}
