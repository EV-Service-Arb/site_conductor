//! A gmail's connections: its inbox over IMAP, its own Maps key and its Business Profile client
//! and grant (`business`), each checked before it is
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
use sa_core::{
	prices::{Item, PlacesRequest, Rate, Spender},
	queries_cache::{BuyBody, BuyRefused},
};
use sa_places::{Bought, Hold, Payer, Pending, Purse};
use uuid::Uuid;
use zeroize::Zeroizing;

pub use crate::store::connections::{Business, Checked, Connection, Imap, MapsKey};
use crate::{
	Panel, Refused,
	browsers::Browsers,
	business, imap,
	inbox::Folder,
	own_alerts::Lost,
	seal::{BUSINESS_STATE_AAD, gmail_business_grant_aad, gmail_business_secret_aad, gmail_imap_aad, gmail_maps_key_aad},
	store::{
		browsers as browsers_db,
		connections::{self as db, Inbox, Sealed},
		gmbs as gmbs_db, inbox as inbox_db, mail as mail_db,
		tokens::{self as ledger, Charged, Entry, Kind},
		users as users_db,
	},
	tokens::is_open,
};

/// The ledger's name for what the shared Places keys charge.
const PLACES: &str = "places";

/// How long a Business Profile consent may take, from its URL handed out to Google's redirect.
const CONSENT_WITHIN: jiff::SignedDuration = jiff::SignedDuration::from_mins(15);

/// A connection that answered is asked again after this.
const ASK_EVERY: jiff::SignedDuration = jiff::SignedDuration::from_hours(24);

/// One that did not.
const ASK_FAILING: jiff::SignedDuration = jiff::SignedDuration::from_mins(15);

/// One lock per person whose buys reach the rotation; see [`Held`].
pub(crate) type Purses = Arc<Mutex<std::collections::HashMap<Uuid, Arc<tokio::sync::Mutex<()>>>>>;

/// A gmail's connections, as its owner sees them.
#[derive(Clone, Debug)]
pub struct Connections {
	pub imap: Option<Imap>,
	/// With why the rotation passes it over now, if it does.
	pub maps_key: Option<(MapsKey, Option<String>)>,
	/// What a request on the shared keys costs, per [`Item::Places`].
	pub prices: Vec<crate::tokens::Priced>,
	pub browser: Option<crate::browsers::Browser>,
	pub business: Option<Business>,
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
		let prices = self.prices().await?.into_iter().filter(|p| matches!(p.item, Item::Places(_))).collect();
		let browser = browsers_db::browser(&mut conn, id).await?.map(|(b, _)| b);
		let business = db::business(&mut conn, id).await?.map(|(b, ..)| b);
		Ok(Ok(Connections {
			imap,
			maps_key,
			prices,
			browser,
			business,
		}))
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
		let inbox = Inbox {
			host: &host,
			username: new.username.trim(),
		};
		if let Err(why) = imap::login(inbox, &new.password).await? {
			return Ok(Err(Refused::Invalid(why)));
		}
		let sealed = self.seal(&gmail_imap_aad(id), &new.password)?;
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		if db::inbox_elsewhere(&mut tx, id, inbox.username).await? {
			return Ok(Err(Refused::Conflict("that inbox is another gmail's already: an inbox is one gmail's".to_owned())));
		}
		// its cases are the account's before: none is answered from another unasked
		if inbox_db::cursor(&mut tx, id, Folder::All).await?.is_some_and(|(before, _)| before != inbox.username) {
			mail_db::let_go(&mut tx, id).await?;
		}
		db::set_imap(&mut tx, id, inbox, &sealed, now).await?;
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

	/// The gmail's Business Profile client, stored once Google knows it; its grant to the one before
	/// is dropped.
	pub async fn set_business(&self, owner: Uuid, id: i64, client_id: &str, secret: Zeroizing<String>, redirect_uri: &str, now: Timestamp) -> eyre::Result<Result<Connections, Refused>> {
		let client_id = client_id.trim();
		let secret = Zeroizing::new(secret.trim().to_owned());
		if !client_id.ends_with(".apps.googleusercontent.com") || client_id.contains(char::is_whitespace) {
			return Ok(Err(Refused::Invalid("a client ID ends in .apps.googleusercontent.com".to_owned())));
		}
		if secret.is_empty() {
			return Ok(Err(Refused::Invalid("a client has a secret".to_owned())));
		}
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let client = business::Client { id: client_id, secret: &secret };
		if let Err(said) = business::probe(&client, redirect_uri).await? {
			return Ok(Err(Refused::Invalid(format!("Google refuses this client: {said}"))));
		}
		let sealed = self.seal(&gmail_business_secret_aad(id), &secret)?;
		let mut tx = self.store.begin_write().await?;
		if gmbs_db::gmail_owner(&mut tx, id).await? != Some(owner) {
			return Ok(Err(Refused::NotFound));
		}
		db::set_business(&mut tx, id, client_id, &sealed, now).await?;
		tx.commit().await.wrap_err("committing a gmail's Business Profile client")?;
		self.connections(owner, id).await
	}

	/// Where the gmail grants its client `business.manage`: Google's consent, coming back to
	/// `redirect_uri` with a state only [`Self::grant_business`] opens, for this owner, a while.
	pub async fn business_consent(&self, owner: Uuid, id: i64, redirect_uri: &str, now: Timestamp) -> eyre::Result<Result<String, Refused>> {
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a consent")?;
		let Some((b, ..)) = db::business(&mut conn, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let gmail = db::address(&mut conn, id).await?;
		let until = (now + CONSENT_WITHIN).as_second();
		let state = [owner.as_bytes().as_slice(), &id.to_be_bytes(), &until.to_be_bytes()].concat();
		let state = base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, self.key.seal(BUSINESS_STATE_AAD, &state)?);
		Ok(Ok(business::consent(&b.client_id, redirect_uri, &gmail, &state)))
	}

	/// Google's redirect after a consent: the code exchanged for the gmail's grant, kept only when
	/// the gmail itself gave it, then the APIs asked. Answers the gmail it was for.
	pub async fn grant_business(&self, owner: Uuid, state: &str, code: &str, redirect_uri: &str, now: Timestamp) -> eyre::Result<Result<i64, Refused>> {
		let stale = || {
			Ok(Err(Refused::Invalid(
				"this consent is stale or not yours: start it again from the gmail's Connections".to_owned(),
			)))
		};
		let Ok(sealed) = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, state) else {
			return stale();
		};
		let Ok(open) = self.key.open(BUSINESS_STATE_AAD, &sealed) else {
			return stale();
		};
		assert_eq!(open.len(), 32, "sealed as 16 + 8 + 8 bytes");
		let (user, rest) = open.split_at(16);
		let (id, until) = rest.split_at(8);
		let id = i64::from_be_bytes(id.try_into().expect("sealed as 16 + 8 + 8 bytes"));
		let until = i64::from_be_bytes(until.try_into().expect("sealed as 16 + 8 + 8 bytes"));
		if Uuid::from_slice(user).expect("sealed as 16 + 8 + 8 bytes") != owner || now.as_second() > until {
			return stale();
		}
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a grant")?;
		let Some((b, secret, _)) = db::business(&mut conn, id).await? else {
			return Ok(Err(Refused::NotFound));
		};
		let gmail = db::address(&mut conn, id).await?;
		drop(conn);
		let secret = self.unseal(&gmail_business_secret_aad(id), &secret)?;
		let client = business::Client { id: &b.client_id, secret: &secret };
		let granted = match business::exchange(&client, code, redirect_uri).await? {
			Ok(g) => g,
			Err(said) => return Ok(Err(Refused::Invalid(format!("Google refuses the grant: {said}")))),
		};
		if !granted.email.eq_ignore_ascii_case(&gmail) {
			return Ok(Err(Refused::Invalid(format!(
				"{} granted, not {gmail}: sign in to Google as {gmail} and start again",
				granted.email
			))));
		}
		let error = business::check(&client, &granted.refresh).await?;
		let sealed = self.seal(&gmail_business_grant_aad(id), &granted.refresh)?;
		let mut tx = self.store.begin_write().await?;
		if !db::set_business_grant(&mut tx, id, &b.client_id, &sealed, now).await? {
			return Ok(Err(Refused::Conflict("the gmail's client changed during the consent: start it again".to_owned())));
		}
		db::checked(&mut tx, id, Connection::Business, &Checked { at: now, error }).await?;
		tx.commit().await.wrap_err("committing a gmail's Business Profile grant")?;
		Ok(Ok(id))
	}

	/// The gmail's `which` asked again, and what it said recorded.
	pub async fn check(&self, owner: Uuid, id: i64, which: Connection, now: Timestamp) -> eyre::Result<Result<Connections, Refused>> {
		if let Err(r) = self.gmail_of(owner, id).await? {
			return Ok(Err(r));
		}
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for a gmail's connection")?;
		let error = match which {
			Connection::Business => {
				let Some((b, secret, Some(grant))) = db::business(&mut conn, id).await? else {
					return Ok(Err(Refused::NotFound));
				};
				drop(conn);
				let secret = self.unseal(&gmail_business_secret_aad(id), &secret)?;
				let refresh = self.unseal(&gmail_business_grant_aad(id), &grant)?;
				business::check(&business::Client { id: &b.client_id, secret: &secret }, &refresh).await?
			}
			Connection::Imap => {
				let Some((imap, sealed)) = db::imap(&mut conn, id).await? else {
					return Ok(Err(Refused::NotFound));
				};
				drop(conn);
				let password = self.unseal(&gmail_imap_aad(id), &sealed)?;
				let inbox = Inbox {
					host: &imap.host,
					username: &imap.username,
				};
				imap::login(inbox, &password).await?.err()
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

	/// The grants, keys and proxies to ask again: those that answered not asked since `ASK_EVERY`, those
	/// that did not since `ASK_FAILING`.
	pub async fn connections_unasked(&self, now: Timestamp) -> eyre::Result<Vec<(Uuid, i64, Lost)>> {
		let mut conn = self.store.pool().acquire().await.wrap_err("a connection for the connections to ask")?;
		db::unasked(&mut conn, now - ASK_EVERY, now - ASK_FAILING).await
	}

	/// One of them asked, and what it said recorded: why it does not answer, if it does not. A proxy is
	/// asked through `b`; without it, or once the connection is gone, there is nothing to ask.
	pub async fn ask_again(&self, b: Option<&Browsers>, owner: Uuid, gmail: i64, lost: Lost, now: Timestamp) -> eyre::Result<Option<String>> {
		let asked = match lost {
			Lost::Inbox => self.check(owner, gmail, Connection::Imap, now).await?,
			Lost::Business => self.check(owner, gmail, Connection::Business, now).await?,
			Lost::MapsKey => self.check(owner, gmail, Connection::MapsKey, now).await?,
			Lost::Proxy => {
				let Some(b) = b else { return Ok(None) };
				match self.check_browser(b, owner, gmail, now).await? {
					Ok(()) => self.connections(owner, gmail).await?,
					Err(r) => Err(r),
				}
			}
		};
		let Ok(c) = asked else { return Ok(None) }; // removed meanwhile, or tokens are closed
		let checked = match lost {
			Lost::Inbox => c.imap.map(|i| i.checked),
			Lost::Business => c.business.and_then(|b| b.checked),
			Lost::MapsKey => c.maps_key.map(|(k, _)| k.checked),
			Lost::Proxy => c.browser.map(|b| b.checked),
		};
		Ok(checked.and_then(|c| c.error))
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

	/// `body.request` (a Places request of sa_places', as its key's preimage) kept, else bought for
	/// `user`, as their GBP `body.gbp`'s gmail when it names one: what another service asks for the
	/// person whose request it serves.
	// r[impl google.buy-as-user]
	pub async fn buy_for(&self, user: Uuid, body: &BuyBody) -> eyre::Result<Result<serde_json::Value, BuyRefused>> {
		if !users_db::exists(&mut *self.store.pool().acquire().await.wrap_err("a connection for a person")?, user).await? {
			return Ok(Err(BuyRefused::Unauthenticated {
				why: format!("{user} is nobody this conductor has signed in"),
			}));
		}
		let gmail = match body.gbp {
			None => None,
			Some(id) => match gmbs_db::gmb_of(&mut *self.store.pool().acquire().await.wrap_err("a connection for a GBP")?, Some(user), id).await? {
				Some(gbp) => gbp.gmail_id,
				None =>
					return Ok(Err(BuyRefused::Invalid {
						why: format!("no GBP {id} is theirs"),
					})),
			},
		};
		let purse = self.purse(Spender::new(user));
		let payer = self.payer(user, gmail, Some(&purse)).await?;
		Ok(match self.google.bought(&body.request, body.fresh_since, &payer).await {
			Ok(answer) => Ok(answer),
			Err(sa_places::Error::Invalid(why)) => Err(BuyRefused::Invalid { why }),
			Err(e @ sa_places::Error::OwnKey { .. }) => Err(BuyRefused::Invalid {
				why: format!("{e}; replace it on its gmail's Connections"),
			}),
			Err(sa_places::Error::Unpaid(why)) => Err(BuyRefused::Unpaid { why }),
			Err(sa_places::Error::Unbought) => Err(BuyRefused::Unbought),
			Err(sa_places::Error::Api { asked, status, body }) => Err(BuyRefused::Places { asked, status, body }),
			Err(e @ (sa_places::Error::Exhausted(_) | sa_places::Error::Http(_))) => Err(BuyRefused::Unavailable { why: e.to_string() }),
			Err(e) => return Err(eyre::eyre!(e).wrap_err("buying a Places answer for another service")),
		})
	}

	/// The tokens of `by` paying for buys on the rotation.
	pub(crate) fn purse(&self, by: Spender<PlacesRequest>) -> Tokens {
		Tokens {
			panel: self.clone(),
			user: by.user(),
		}
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
	rate: Rate,
	/// One buy on the rotation at a time per person, from the balance read to the charge written:
	/// two cannot both pass on the one balance.
	turn: tokio::sync::OwnedMutexGuard<()>,
}

impl Purse for Tokens {
	fn hold(&self, sku: PlacesRequest) -> Pending<'_, Box<dyn Hold>> {
		Box::pin(async move {
			let turn = self.panel.purses.lock().expect("never poisoned: no panic while held").entry(self.user).or_default().clone();
			let turn = turn.lock_owned().await;
			let unpaid = |e: eyre::Report| sa_places::Error::Purse(format!("{e:#}"));
			let mut tx = self.panel.store.begin_write().await.map_err(unpaid)?;
			if !is_open(&mut tx).await.map_err(unpaid)? {
				return Err(sa_places::Error::Unpaid(Refused::Closed.to_string()));
			}
			let rate = ledger::rate(&mut tx, sku.into()).await.map_err(unpaid)?;
			let balance = ledger::renewed(&mut tx, self.user, Timestamp::now(), &self.panel.renewal).await.map_err(unpaid)?;
			tx.commit().await.map_err(|e| unpaid(eyre::eyre!(e)))?;
			let price = rate.price();
			if balance < price {
				return Err(sa_places::Error::Unpaid(format!(
					"Places {} on the shared keys costs {price} tokens, and you hold {balance}: add a Maps key to one of your gmails, or ask for tokens",
					Item::from(sku).title()
				)));
			}
			Ok(Box::new(Held {
				panel: self.panel.clone(),
				user: self.user,
				rate,
				turn,
			}) as Box<dyn Hold>)
		})
	}
}

impl Hold for Held {
	fn paid(self: Box<Self>, bought: Bought) -> Pending<'static, ()> {
		let Self { panel, user, rate, turn } = *self;
		let price = rate.price();
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
				cost: Some(rate.cost_of(bought.sku.into(), 1)),
				by: None,
				note: None,
			};
			let mut tx = panel.store.begin_write().await.map_err(store)?;
			ledger::append(&mut tx, user, &entry).await.map_err(store)?;
			tx.commit().await.map_err(|e| store(eyre::eyre!(e)))?;
			drop(turn);
			tracing::info!(user_id = %user, tokens = price, item = %Item::from(bought.sku), "Places buy charged");
			panel.snapshot_due.notify_one();
			Ok(())
		})
	}
}
