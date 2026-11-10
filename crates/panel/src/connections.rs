//! A gmail's connections: its inbox over IMAP and its own Maps key, each checked before it is
//! stored, sealed, and never read back out but by the conductor's own use. And who a Places
//! request is made for: the gmail's key, its owner's other gmails', then the shared rotation, paid
//! in tokens at the price an admin sets ([`sa_places::Payer`]).
//!
//! ```text
//! own keys ── free ──▶ the rotation: purse(user) holds — price set, balance ≥ price, else refused
//!                      before anything is sent — the buy, its charge in the ledger
//! ```

use std::sync::{Arc, Mutex};

use eyre::WrapErr;
use jiff::Timestamp;
use sa_places::{Bought, Hold, Payer, Pending, Purse, Sku};
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::store::connections::{Checked, Connection, Imap, MapsKey};
use crate::{
	Panel, Refused, imap,
	seal::{gmail_imap_aad, gmail_maps_key_aad},
	store::{
		browsers as browsers_db,
		connections::{self as db, Sealed},
		gmbs as gmbs_db,
		tokens::{self as ledger, Charged, Entry, Kind},
	},
	tokens::is_open,
};

/// The ledger's name for what the shared Places keys charge.
const PLACES: &str = "places";

/// One lock per person whose buys reach the rotation; see [`Held`].
pub(crate) type Purses = Arc<Mutex<std::collections::HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>>;

/// A gmail's connections, as its owner sees them.
#[derive(Clone, Debug)]
pub struct Connections {
	pub imap: Option<Imap>,
	/// With why the rotation passes it over now, if it does.
	pub maps_key: Option<(MapsKey, Option<String>)>,
	/// What a request on the shared keys costs, per [`Sku`]; `None`, not sold.
	pub prices: Vec<(Sku, Option<i64>)>,
	pub browser: Option<crate::browsers::Browser>,
}

/// An inbox's credentials, as typed.
pub struct NewImap {
	pub host: String,
	pub username: String,
	pub password: Zeroizing<String>,
}

impl Panel {
	/// The owner's gmail `id`'s connections.
	pub async fn connections(&self, owner: Uuid, id: i64) -> eyre::Result<Result<Connections, Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's connections")?;
		if !is_open(&mut conn).await? {
			return Ok(Err(Refused::Closed));
		}
		if gmbs_db::gmail_owner(&mut conn, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		let imap = db::imap(&mut conn, id).await?.map(|(i, _)| i);
		let maps_key = match db::maps_key(&mut conn, id).await? {
			Some((k, sealed)) => {
				let key = self.unseal(&gmail_maps_key_aad(id), &sealed)?;
				Some((k, self.google.sat_out(&key).map(|s| s.to_string())))
			}
			None => None,
		};
		let mut prices = Vec::new();
		for sku in <Sku as strum::VariantArray>::VARIANTS {
			prices.push((*sku, db::price(&mut conn, *sku).await?));
		}
		let browser = browsers_db::browser(&mut conn, id).await?.map(|(b, _)| b);
		Ok(Ok(Connections { imap, maps_key, prices, browser }))
	}

	/// The gmail's inbox, stored once it logs in; a refusal stores nothing.
	pub async fn set_imap(&self, owner: Uuid, id: i64, new: NewImap, now: Timestamp) -> eyre::Result<Result<Connections, Refused>> {
		let host = new.host.trim().to_lowercase();
		if host.is_empty() || new.username.trim().is_empty() {
			return Ok(Err(Refused::Invalid("an inbox has a host and a username".to_owned())));
		}
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		if let Err(why) = imap::login(&host, new.username.trim(), &new.password).await? {
			return Ok(Err(Refused::Invalid(why)));
		}
		let sealed = self.seal(&gmail_imap_aad(id), &new.password)?;
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		db::set_imap(&mut tx, id, &host, new.username.trim(), &sealed, now).await?;
		tx.commit().await.wrap_err("committing a gmail's IMAP")?;
		self.connections(owner, id).await
	}

	/// The gmail's own Maps key, stored once Google answers for it; a refusal stores nothing.
	pub async fn set_maps_key(&self, owner: Uuid, id: i64, key: Zeroizing<String>, now: Timestamp) -> eyre::Result<Result<Connections, Refused>> {
		let key = Zeroizing::new(key.trim().to_owned());
		if key.len() < 8 || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
			return Ok(Err(Refused::Invalid("a Maps key is letters, digits, - and _".to_owned())));
		}
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		match self.google.probe(&key).await {
			Ok(()) => {}
			Err(sa_places::Error::OwnKey { said, .. }) => return Ok(Err(Refused::Invalid(format!("Google refuses this key: {said}")))),
			Err(e) => return Err(eyre::eyre!(e).wrap_err("probing a Maps key")),
		}
		let sealed = self.seal(&gmail_maps_key_aad(id), &key)?;
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		db::set_maps_key(&mut tx, id, &key[key.len() - 4..], &sealed, now).await?;
		tx.commit().await.wrap_err("committing a gmail's Maps key")?;
		self.connections(owner, id).await
	}

	/// The gmail's `which` asked again, and what it said recorded.
	pub async fn check(&self, owner: Uuid, id: i64, which: Connection, now: Timestamp) -> eyre::Result<Result<Connections, Refused>> {
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's connection")?;
		let error = match which {
			Connection::Imap => {
				let Some((imap, sealed)) = db::imap(&mut conn, id).await? else {
					return Ok(Err(Refused::NotFound));
				};
				drop(conn);
				let password = self.unseal(&gmail_imap_aad(id), &sealed)?;
				imap::login(&imap.host, &imap.username, &password).await?.err()
			}
			Connection::MapsKey => {
				let Some((_, sealed)) = db::maps_key(&mut conn, id).await? else {
					return Ok(Err(Refused::NotFound));
				};
				drop(conn);
				let key = self.unseal(&gmail_maps_key_aad(id), &sealed)?;
				match self.google.probe(&key).await {
					Ok(()) => None,
					Err(sa_places::Error::OwnKey { said, .. }) => Some(said),
					Err(e) => return Err(eyre::eyre!(e).wrap_err("probing a Maps key")),
				}
			}
		};
		let mut tx = self.store.begin_write().await?;
		db::checked(&mut tx, id, which, &Checked { at: now, error }).await?;
		tx.commit().await.wrap_err("committing a check")?;
		self.connections(owner, id).await
	}

	pub async fn disconnect(&self, owner: Uuid, id: i64, which: Connection) -> eyre::Result<Result<(), Refused>> {
		let mut tx = self.store.begin_write().await?;
		if !is_open(&mut tx).await? {
			return Ok(Err(Refused::Closed));
		}
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) || !db::delete(&mut tx, id, which).await? {
			return Ok(Err(Refused::NotFound));
		}
		tx.commit().await.wrap_err("committing a disconnection")?;
		Ok(Ok(()))
	}

	/// What a request on the shared keys costs, per [`Sku`]; `None`, not sold.
	pub async fn prices(&self) -> eyre::Result<Vec<(Sku, Option<i64>)>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the prices")?;
		let mut out = Vec::new();
		for sku in <Sku as strum::VariantArray>::VARIANTS {
			out.push((*sku, db::price(&mut conn, *sku).await?));
		}
		Ok(out)
	}

	/// An admin (`by`, their email) prices every [`Sku`], 0 or more each.
	pub async fn set_prices(&self, prices: &[(Sku, i64)], by: &str, now: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if prices.iter().any(|(_, n)| *n < 0) || <Sku as strum::VariantArray>::VARIANTS.iter().any(|s| !prices.iter().any(|(p, _)| p == s)) {
			return Ok(Err(Refused::Invalid("every request kind is priced, at 0 tokens or more".to_owned())));
		}
		let mut tx = self.store.begin_write().await?;
		for (sku, n) in prices {
			db::set_price(&mut tx, *sku, *n, by, now).await?;
		}
		tx.commit().await.wrap_err("committing the prices")?;
		tracing::info!(by, ?prices, "Places prices set");
		Ok(Ok(()))
	}

	/// Who a request about the owner's gmail `first` (else none of theirs in particular) is made
	/// for: their keys, then the rotation `purse` pays for, the house when `None`.
	pub(crate) async fn payer<'a>(&self, owner: Uuid, first: Option<i64>, purse: Option<&'a dyn Purse>) -> eyre::Result<Payer<'a>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a person's keys")?;
		let mut own: Vec<Zeroizing<String>> = Vec::new();
		for (gmail_id, sealed) in db::own_keys(&mut conn, owner, first).await? {
			let key = self.unseal(&gmail_maps_key_aad(gmail_id), &sealed)?;
			if !own.contains(&key) {
				own.push(key);
			}
		}
		Ok(Payer { own, purse })
	}

	/// The tokens of `user` paying for buys on the rotation.
	pub(crate) fn purse(&self, user: Uuid) -> Tokens {
		Tokens { panel: self.clone(), user }
	}

	pub(crate) async fn gmail_of(&self, owner: Uuid, id: i64) -> eyre::Result<Result<(), Refused>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail")?;
		if !is_open(&mut conn).await? {
			return Ok(Err(Refused::Closed));
		}
		Ok(match gmbs_db::gmail_owner(&mut conn, id).await? == Some(owner) {
			true => Ok(()),
			false => Err(Refused::NotFound),
		})
	}

	pub(crate) fn seal(&self, aad: &[u8], secret: &str) -> eyre::Result<Sealed> {
		Ok(Sealed {
			blob: self.key.seal(aad, secret.as_bytes())?,
			data_key_fp: self.key.fingerprint().to_vec(),
		})
	}

	pub(crate) fn unseal(&self, aad: &[u8], sealed: &Sealed) -> eyre::Result<Zeroizing<String>> {
		eyre::ensure!(sealed.data_key_fp == self.key.fingerprint(), "a gmail's secret was sealed under another PANEL_DATA_KEY");
		let open = self.key.open(aad, &sealed.blob)?;
		Ok(Zeroizing::new(String::from_utf8(open.to_vec()).wrap_err("a gmail's secret is not UTF-8")?))
	}
}

/// A person's tokens, as the purse of their buys on the shared keys.
pub(crate) struct Tokens {
	panel: Panel,
	user: Uuid,
}

struct Held {
	panel: Panel,
	user: Uuid,
	price: i64,
	/// One buy on the rotation at a time per person, from the balance read to the charge written:
	/// two cannot both pass on the one balance.
	turn: tokio::sync::OwnedMutexGuard<()>,
}

impl Purse for Tokens {
	fn hold(&self, sku: Sku) -> Pending<'_, Box<dyn Hold>> {
		Box::pin(async move {
			let turn = self.panel.purses.lock().expect("never poisoned: no panic while held").entry(self.user).or_default().clone();
			let turn = turn.lock_owned().await;
			let unpaid = |e: eyre::Report| sa_places::Error::Purse(format!("{e:#}"));
			let mut tx = self.panel.store.begin_write().await.map_err(unpaid)?;
			if !is_open(&mut tx).await.map_err(unpaid)? {
				return Err(sa_places::Error::Unpaid(Refused::Closed.to_string()));
			}
			let Some(price) = db::price(&mut tx, sku).await.map_err(unpaid)? else {
				return Err(sa_places::Error::Unpaid(format!(
					"Places {sku} on the shared keys is not priced yet: add a Maps key to one of your gmails, or ask an admin to price it"
				)));
			};
			let balance = ledger::renewed(&mut tx, self.user, Timestamp::now(), &self.panel.renewal).await.map_err(unpaid)?;
			tx.commit().await.map_err(|e| unpaid(eyre::eyre!(e)))?;
			if balance < price {
				return Err(sa_places::Error::Unpaid(format!(
					"Places {sku} on the shared keys costs {price} tokens, and you hold {balance}: add a Maps key to one of your gmails, or ask for tokens"
				)));
			}
			Ok(Box::new(Held {
				panel: self.panel.clone(),
				user: self.user,
				price,
				turn,
			}) as Box<dyn Hold>)
		})
	}
}

impl Hold for Held {
	fn paid(self: Box<Self>, bought: Bought) -> Pending<'static, ()> {
		let Self { panel, user, price, turn } = *self;
		Box::pin(async move {
			if price == 0 {
				drop(turn);
				return Ok(());
			}
			let store = |e: eyre::Report| sa_places::Error::Purse(format!("charging a Places buy: {e:#}"));
			let entry = Entry {
				at: Timestamp::now(),
				delta: -price,
				kind: Kind::Charge,
				charged: Some(Charged {
					service: PLACES.to_owned(),
					key: bought.key.to_string(),
					label: Some(bought.label),
				}),
				by: None,
				note: None,
			};
			let mut tx = panel.store.begin_write().await.map_err(store)?;
			ledger::append(&mut tx, user, &entry).await.map_err(store)?;
			tx.commit().await.map_err(|e| store(eyre::eyre!(e)))?;
			drop(turn);
			tracing::info!(user_id = %user, tokens = price, sku = %bought.sku, "Places buy charged");
			panel.snapshot_due.notify_one();
			Ok(())
		})
	}
}
