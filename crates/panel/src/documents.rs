//! What a person gave for a GBP (a Kbis, a bill, a photo of its sign): kept sealed, for Google's next ask and the appeal.

use eyre::WrapErr;
use jiff::Timestamp;
use uuid::Uuid;

use crate::{
	Panel, Refused,
	mail::File,
	seal::gmb_document_aad,
	store::{connections::Sealed, documents as db, gmbs as gmbs_db},
};
pub use crate::store::documents::Document;

/// A document's bytes.
pub const DOCUMENT_MAX: usize = 10 << 20;
/// A GBP's documents.
const MOST: usize = 50;
const NAME_MAX: usize = 200;

impl Panel {
	/// The documents of the person's GBP, newest first.
	pub async fn documents(&self, owner: Uuid, gmb: i64) -> eyre::Result<Result<Vec<Document>, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a GBP's documents")?;
		if gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		Ok(Ok(db::of_gmb(&mut conn, gmb).await?))
	}

	/// `file` kept for the person's GBP, as what `given_for` asks. Answers its id.
	pub async fn add_document(&self, owner: Uuid, by: Uuid, gmb: i64, file: &File, given_for: Option<&str>, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		if file.bytes.is_empty() || file.name.is_empty() || file.name.chars().count() > NAME_MAX {
			return Ok(Err(Refused::Invalid(format!("a document has bytes, and a name of at most {NAME_MAX} characters"))));
		}
		if file.bytes.len() > DOCUMENT_MAX {
			return Ok(Err(Refused::TooLarge(format!("a document is at most {} MiB", DOCUMENT_MAX >> 20))));
		}
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmb_of(&mut tx, Some(owner), gmb).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		if db::of_gmb(&mut tx, gmb).await?.len() >= MOST {
			return Ok(Err(Refused::Conflict(format!("a GBP has at most {MOST} documents"))));
		}
		let sealed = Sealed {
			blob: self.key.seal(&gmb_document_aad(gmb, &file.name), &file.bytes)?,
			data_key_fp: self.key.fingerprint().to_vec(),
		};
		let id = db::add(&mut tx, gmb, file, &sealed, given_for, by, now).await?;
		tx.commit().await.wrap_err("committing a document")?;
		Ok(Ok(id))
	}

	/// The person's GBP's document, with its bytes.
	pub async fn document(&self, owner: Uuid, gmb: i64, id: i64) -> eyre::Result<Result<File, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a document")?;
		if gmbs_db::gmb_of(&mut conn, Some(owner), gmb).await?.is_none() {
			return Ok(Err(Refused::NotFound));
		}
		let Some((name, kind, sealed)) = db::bytes(&mut conn, gmb, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		eyre::ensure!(sealed.data_key_fp == self.key.fingerprint(), "a document was sealed under another PANEL_DATA_KEY");
		let bytes = self.key.open(&gmb_document_aad(gmb, &name), &sealed.blob)?.to_vec();
		Ok(Ok(File { name, kind, bytes }))
	}

	pub async fn remove_document(&self, owner: Uuid, gmb: i64, id: i64) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmb_of(&mut tx, Some(owner), gmb).await?.is_none() || !db::remove(&mut tx, gmb, id).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a document removed")?;
		Ok(Ok(()))
	}
}
