//! The queries cache: what was paid for a Places query, kept by the conductor for everyone
//! (`sa_core::queries_cache`), and bought by it for a caller elsewhere ([`Remote`]). One answer per
//! query, so none is bought twice wherever it is asked; refusals are never kept.

use std::{future::Future, pin::Pin, time::Duration};

use jiff::Timestamp;
use sa_core::queries_cache::{BuyBody, BuyRefused, Claim, ClaimBody, Key, Put, Scheme, Stored, under};

use crate::Error;

pub type Pending<'a, T> = Pin<Box<dyn Future<Output = Result<T, Error>> + Send + 'a>>;

/// The cache as `sa_places` asks it.
pub trait QueriesCache: Send + Sync + std::fmt::Debug {
	fn get(&self, scheme: Scheme, key: Key) -> Pending<'_, Option<Stored>>;
	fn claim(&self, scheme: Scheme, key: Key, fresh_since: Option<Timestamp>) -> Pending<'_, Claim>;
	fn put(&self, scheme: Scheme, key: Key, put: Put) -> Pending<'_, ()>;
	/// The claim given up: the buy failed, and nothing is kept.
	fn release(&self, scheme: Scheme, key: Key) -> Pending<'_, ()>;
}

/// The conductor's queries cache over HTTP: its open verbs, and its buy for review_archive
/// (`POST <cache>/places/{key}/buy`), the person its assertion names paying (docs/ARCHITECTURE.md,
/// Queries cache).
#[derive(Debug)]
pub struct Remote {
	http: reqwest::Client,
	/// `<origin>/api/queries_cache`.
	url: String,
}
impl Remote {
	/// The cache at `url`, `<origin>/api/queries_cache`.
	pub fn try_new(url: &str) -> Result<Self, Error> {
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		// a buy waits on another's claim, then on Google
		let http = reqwest::Client::builder().connect_timeout(Duration::from_secs(5)).timeout(Duration::from_secs(60)).build()?;
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

	fn refused(&self, under: &str, (status, text): (reqwest::StatusCode, String)) -> Error {
		Error::QueriesCache(format!("{}{under}: {status}: {text}", self.url))
	}

	async fn buy(&self, req: &Request<'_>, fresh_since: Option<Timestamp>, who: Vouched<'_>) -> Result<serde_json::Value, Error> {
		assert_eq!(req.scheme, Scheme::Places, "only sa_places' own requests are bought for another");
		let path = format!("{}/buy", under(req.scheme, req.key()));
		let body = BuyBody {
			request: req.preimage(),
			fresh_since,
			gbp: who.gbp,
		};
		let resp = self
			.http
			.post(format!("{}{path}", self.url))
			.header(sa_auth::HEADER, who.assertion)
			.json(&body)
			.send()
			.await
			.map_err(|e| Error::QueriesCache(format!("{} did not answer: {e}", self.url)))?;
		let status = resp.status();
		let text = resp.text().await.map_err(|e| Error::QueriesCache(format!("{} answered {status}, cut off: {e}", self.url)))?;
		if status == reqwest::StatusCode::OK {
			return Ok(serde_json::from_str::<serde_json::Value>(&text)?["answer"].take());
		}
		Err(match serde_json::from_str::<BuyRefused>(&text) {
			Ok(BuyRefused::Invalid { why }) => Error::Invalid(why),
			Ok(BuyRefused::Places { asked, status, body }) => Error::Api { asked, status, body },
			Ok(BuyRefused::Unpaid { why }) => Error::Unpaid(why),
			Ok(BuyRefused::Unbought) => Error::Unbought,
			Ok(refused @ (BuyRefused::Unauthenticated { .. } | BuyRefused::Unavailable { .. })) => Error::QueriesCache(format!("{}{path}: {refused}", self.url)),
			Err(_) => Error::QueriesCache(format!("{}{path}: {status}: {text}", self.url)), // not the verb's refusal: what came is all there is to tell
		})
	}
}

/// Who the conductor buys a request for: the person the conductor's assertion on the request being
/// served names, as their GBP `gbp`'s gmail if it is about one.
#[derive(Clone, Copy, Debug)]
pub struct Vouched<'a> {
	/// The `x-sa-assertion` the conductor signed for the request being served.
	pub assertion: &'a str,
	pub gbp: Option<i64>,
}

impl<'p> crate::buy::Buy<'p> for Remote {
	type For = Vouched<'p>;

	fn keyless(&self, _: &Vouched<'p>) -> bool {
		false // the conductor holds the keys
	}

	fn ask<'a>(&'a self, _: &'a reqwest::Client, req: &'a Request<'a>, fresh_since: Option<Timestamp>, who: &'a Vouched<'p>) -> Pending<'a, serde_json::Value>
	where
		'p: 'a, {
		Box::pin(self.buy(req, fresh_since, *who))
	}
}

/// A query as the cache tells it from another: the field mask changes the answer's shape.
pub struct Request<'a> {
	pub method: &'a str,
	pub url: &'a str,
	pub mask: &'a str,
	pub body: &'a str,
	/// Whose key it is: [`Scheme::Gop`] for a search of a study's.
	pub scheme: Scheme,
	/// How Google bills it; not part of the key, the mask decides it.
	pub sku: sa_core::prices::PlacesRequest,
	/// What it asks about, as a person reads it: a place id, a search's text.
	pub subject: &'a str,
}

impl Request<'_> {
	/// What the key is the digest of, kept with the answer.
	pub fn preimage(&self) -> String {
		match self.scheme {
			Scheme::Places => format!("{}\n{}\n{}\n{}", self.method, self.url, self.mask, self.body),
			// keyed by Google's URL, whichever this one is: a stub's stands in for it
			Scheme::Gop => sa_core::queries_cache::gop(&format!("{}:searchText", crate::GOOGLE), self.body),
		}
	}

	pub fn key(&self) -> Key {
		Key::of(&self.preimage())
	}
}

impl QueriesCache for Remote {
	fn get(&self, scheme: Scheme, key: Key) -> Pending<'_, Option<Stored>> {
		Box::pin(async move {
			let path = under(scheme, key);
			match self.send(reqwest::Method::GET, &path, Vec::new()).await? {
				(reqwest::StatusCode::OK, text) => Ok(Some(serde_json::from_str(&text)?)),
				(reqwest::StatusCode::NOT_FOUND, _) => Ok(None),
				answer => Err(self.refused(&path, answer)),
			}
		})
	}

	fn claim(&self, scheme: Scheme, key: Key, fresh_since: Option<Timestamp>) -> Pending<'_, Claim> {
		Box::pin(async move {
			let path = format!("{}/claim", under(scheme, key));
			let body = serde_json::to_vec(&ClaimBody { fresh_since })?;
			match self.send(reqwest::Method::POST, &path, body).await? {
				(reqwest::StatusCode::OK, text) => Ok(serde_json::from_str(&text)?),
				answer => Err(self.refused(&path, answer)),
			}
		})
	}

	fn put(&self, scheme: Scheme, key: Key, put: Put) -> Pending<'_, ()> {
		Box::pin(async move {
			let path = under(scheme, key);
			match self.send(reqwest::Method::PUT, &path, serde_json::to_vec(&put)?).await? {
				(reqwest::StatusCode::NO_CONTENT, _) => Ok(()),
				answer => Err(self.refused(&path, answer)),
			}
		})
	}

	fn release(&self, scheme: Scheme, key: Key) -> Pending<'_, ()> {
		Box::pin(async move {
			let path = format!("{}/claim", under(scheme, key));
			match self.send(reqwest::Method::DELETE, &path, Vec::new()).await? {
				(reqwest::StatusCode::NO_CONTENT, _) => Ok(()),
				answer => Err(self.refused(&path, answer)),
			}
		})
	}
}
