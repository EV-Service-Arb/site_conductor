//! The queries cache: what was paid for a Places query, kept by the conductor for everyone, in its
//! own process or over `/api/queries_cache` (`sa_core::queries_cache`). One answer per query, so
//! none is bought twice wherever it is asked; refusals are never kept.

use std::{future::Future, pin::Pin, time::Duration};

use jiff::Timestamp;
use sa_core::queries_cache::{Claim, ClaimBody, Key, Put, Scheme, Stored, under};
use sha2::{Digest, Sha256};

use crate::Error;

pub type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

/// The cache as `sa_places` asks it, under [`Scheme::Places`].
pub trait QueriesCache: Send + Sync + std::fmt::Debug {
	fn get(&self, key: Key) -> Pending<'_, Option<Stored>>;
	fn claim(&self, key: Key, fresh_since: Option<Timestamp>) -> Pending<'_, Claim>;
	fn put(&self, key: Key, put: Put) -> Pending<'_, ()>;
	/// The claim given up: the buy failed, and nothing is kept.
	fn release(&self, key: Key) -> Pending<'_, ()>;
}

/// The conductor's queries cache over HTTP.
#[derive(Debug)]
pub struct Remote {
	http: reqwest::Client,
	/// `<origin>/api/queries_cache`.
	url: String,
}
impl Remote {
	/// The cache at `url`, `<origin>/api/queries_cache`.
	pub fn try_new(url: &str) -> Result<Self, Error> {
		let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(30)).build()?;
		url::Url::parse(url).map_err(|e| Error::Invalid(format!("the queries cache's URL {url:?}: {e}")))?;
		Ok(Self {
			http,
			url: url.trim_end_matches('/').to_owned(),
		})
	}

	/// `under` the cache's URL: its status and body.
	async fn send(&self, method: reqwest::Method, under: &str, body: Vec<u8>) -> Result<(reqwest::StatusCode, String), Error> {
		let resp = self
			.http
			.request(method, format!("{}{under}", self.url))
			.header(reqwest::header::CONTENT_TYPE, "application/json")
			.body(body)
			.send()
			.await
			.map_err(|e| Error::QueriesCache(format!("{} did not answer: {e}", self.url)))?;
		let status = resp.status();
		let text = resp.text().await.map_err(|e| Error::QueriesCache(format!("{} answered {status}, cut off: {e}", self.url)))?;
		Ok((status, text))
	}

	fn refused(&self, under: &str, status: reqwest::StatusCode, text: &str) -> Error {
		Error::QueriesCache(format!("{}{under}: {status}: {text}", self.url))
	}
}

/// A query as the cache tells it from another: the field mask changes the answer's shape.
pub(crate) struct Request<'a> {
	pub method: &'a str,
	pub url: &'a str,
	pub mask: &'a str,
	pub body: &'a str,
	/// How Google bills it; not part of the key, the mask decides it.
	pub sku: crate::Sku,
	/// What it asks about, as a person reads it: a place id, a search's text.
	pub subject: &'a str,
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

impl QueriesCache for Remote {
	fn get(&self, key: Key) -> Pending<'_, Option<Stored>> {
		Box::pin(async move {
			let path = under(Scheme::Places, key);
			match self.send(reqwest::Method::GET, &path, Vec::new()).await? {
				(reqwest::StatusCode::OK, text) => Ok(Some(serde_json::from_str(&text)?)),
				(reqwest::StatusCode::NOT_FOUND, _) => Ok(None),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}

	fn claim(&self, key: Key, fresh_since: Option<Timestamp>) -> Pending<'_, Claim> {
		Box::pin(async move {
			let path = format!("{}/claim", under(Scheme::Places, key));
			let body = serde_json::to_vec(&ClaimBody { fresh_since })?;
			match self.send(reqwest::Method::POST, &path, body).await? {
				(reqwest::StatusCode::OK, text) => Ok(serde_json::from_str(&text)?),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}

	fn put(&self, key: Key, put: Put) -> Pending<'_, ()> {
		Box::pin(async move {
			let path = under(Scheme::Places, key);
			match self.send(reqwest::Method::PUT, &path, serde_json::to_vec(&put)?).await? {
				(reqwest::StatusCode::NO_CONTENT, _) => Ok(()),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}

	fn release(&self, key: Key) -> Pending<'_, ()> {
		Box::pin(async move {
			let path = format!("{}/claim", under(Scheme::Places, key));
			match self.send(reqwest::Method::DELETE, &path, Vec::new()).await? {
				(reqwest::StatusCode::NO_CONTENT, _) => Ok(()),
				(status, text) => Err(self.refused(&path, status, &text)),
			}
		})
	}
}
