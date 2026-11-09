//! Where paid answers are kept: the conductor's store, in its own process or over
//! `/api/answers` (`sa_core::answers`). One per request, so an answer is never bought twice
//! wherever it is asked; refusals are never kept.

use std::{future::Future, pin::Pin, time::Duration};

use jiff::Timestamp;
use sa_core::answers::{self, Claim, ClaimBody, Key, Put, Scheme, Stored};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::Error;

pub type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

/// The store as `sa_places` asks it, under [`Scheme::Places`].
pub trait Answers: Send + Sync + std::fmt::Debug {
	fn get(&self, key: Key) -> Pending<'_, Option<Stored>>;
	fn claim(&self, key: Key, fresh_since: Option<Timestamp>) -> Pending<'_, Claim>;
	fn put(&self, key: Key, put: Put) -> Pending<'_, ()>;
	/// The claim given up: the buy failed, and nothing is kept.
	fn release(&self, key: Key) -> Pending<'_, ()>;
}

/// The conductor's store over HTTP, as an `answers` source key.
pub struct Remote {
	http: reqwest::Client,
	/// `<origin>/api/answers`.
	url: url::Url,
	key_id: String,
	secret: Zeroizing<String>,
}
impl Remote {
	/// The store at `url`, `<origin>/api/answers`.
	pub fn try_new(url: &str, key_id: impl Into<String>, secret: Zeroizing<String>) -> Result<Self, Error> {
		let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build()?;
		let url = url::Url::parse(url.trim_end_matches('/')).map_err(|e| Error::Invalid(format!("the answers store's URL {url:?}: {e}")))?;
		Ok(Self {
			http,
			url,
			key_id: key_id.into(),
			secret,
		})
	}

	/// `under` the store's URL: its status and body.
	async fn send(&self, method: reqwest::Method, under: &str, body: Vec<u8>) -> Result<(reqwest::StatusCode, String), Error> {
		let path = format!("{}{under}", self.url.path().trim_end_matches('/'));
		let ts = Timestamp::now().as_second().to_string();
		let signature = sa_core::signature::sign(self.secret.as_bytes(), &ts, &answers::signed(method.as_str(), &path, &body));
		let resp = self
			.http
			.request(method, format!("{}{under}", self.url.as_str().trim_end_matches('/')))
			.header("x-sa-key-id", &self.key_id)
			.header("x-sa-timestamp", ts)
			.header("x-sa-signature", signature)
			.header(reqwest::header::CONTENT_TYPE, "application/json")
			.body(body)
			.send()
			.await
			.map_err(|e| Error::Store(format!("{} did not answer: {e}", self.url)))?;
		let status = resp.status();
		let text = resp.text().await.map_err(|e| Error::Store(format!("{} answered {status}, cut off: {e}", self.url)))?;
		Ok((status, text))
	}

	fn refused(&self, under: &str, status: reqwest::StatusCode, text: &str) -> Error {
		Error::Store(format!("{}{under}: {status}: {text}", self.url))
	}
}

/// A request as the store tells it from another: the field mask changes the answer's shape.
pub(crate) struct Request<'a> {
	pub method: &'a str,
	pub url: &'a str,
	pub mask: &'a str,
	pub body: &'a str,
}

impl Request<'_> {
	/// What the key is the digest of, kept with the answer.
	pub fn preimage(&self) -> String {
		format!("{}\n{}\n{}\n{}", self.method, self.url, self.mask, self.body)
	}

	pub fn key(&self) -> Key {
		Key(Sha256::digest(self.preimage()).into())
	}
}

impl std::fmt::Debug for Remote {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Remote").field("url", &self.url.as_str()).field("key_id", &self.key_id).finish_non_exhaustive()
	}
}

impl Answers for Remote {
	fn get(&self, key: Key) -> Pending<'_, Option<Stored>> {
		Box::pin(async move {
			let path = answers::under(Scheme::Places, key);
			match self.send(reqwest::Method::GET, &path, Vec::new()).await? {
				(reqwest::StatusCode::OK, text) => Ok(Some(serde_json::from_str(&text)?)),
				(reqwest::StatusCode::NOT_FOUND, _) => Ok(None),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}

	fn claim(&self, key: Key, fresh_since: Option<Timestamp>) -> Pending<'_, Claim> {
		Box::pin(async move {
			let path = format!("{}/claim", answers::under(Scheme::Places, key));
			let body = serde_json::to_vec(&ClaimBody { fresh_since })?;
			match self.send(reqwest::Method::POST, &path, body).await? {
				(reqwest::StatusCode::OK, text) => Ok(serde_json::from_str(&text)?),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}

	fn put(&self, key: Key, put: Put) -> Pending<'_, ()> {
		Box::pin(async move {
			let path = answers::under(Scheme::Places, key);
			match self.send(reqwest::Method::PUT, &path, serde_json::to_vec(&put)?).await? {
				(reqwest::StatusCode::NO_CONTENT, _) => Ok(()),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}

	fn release(&self, key: Key) -> Pending<'_, ()> {
		Box::pin(async move {
			let path = format!("{}/claim", answers::under(Scheme::Places, key));
			match self.send(reqwest::Method::DELETE, &path, Vec::new()).await? {
				(reqwest::StatusCode::NO_CONTENT, _) => Ok(()),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}
}
