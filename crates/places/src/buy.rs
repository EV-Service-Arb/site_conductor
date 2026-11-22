//! Who buys what the queries cache lacks: [`Here`], the conductor itself, on the payer's keys and
//! the rotation; or the conductor for a caller elsewhere ([`crate::Remote`]).

use std::{sync::Arc, time::Duration};

use sa_core::{
	queries_cache::{Claim, Put, Scheme},
	usage::{Bill, Billed},
};
use serde_json::Value;

use crate::{Bought, Error, Hold, Keys, Payer, Pending, QueriesCache, Rank, Spent, keys, queries_cache::Request, said};

/// A [`crate::Google`]'s buyer: who a request is made for, and how its answer is had.
pub trait Buy<'p>: Send + Sync + std::fmt::Debug {
	/// Who a request is made for.
	type For: Sync + 'p;

	/// No key could buy for `who`: a place id resolves unasked.
	fn keyless(&self, who: &Self::For) -> bool;

	/// The answer to `req`: the one kept, unless fetched before `fresh_since`, else bought for `who`.
	fn ask<'a>(&'a self, http: &'a reqwest::Client, req: &'a Request<'a>, fresh_since: Option<jiff::Timestamp>, who: &'a Self::For) -> Pending<'a, Value>
	where
		'p: 'a;
}

/// The conductor's own buyer: its queries cache, then the payer's keys, then the rotation's.
#[derive(Debug)]
pub struct Here {
	pub(crate) keys: Keys,
	rank: Arc<dyn Rank>,
	cache: Arc<dyn QueriesCache>,
	/// Buy what the queries cache lacks; off, only cached answers are served.
	spend: bool,
}

impl Here {
	pub(crate) fn new(keys: Keys, rank: Arc<dyn Rank>, cache: Arc<dyn QueriesCache>) -> Self {
		Self { keys, rank, cache, spend: false }
	}

	pub(crate) fn spend(self, yes: bool) -> Self {
		Self { spend: yes, ..self }
	}

	/// `req` asked of Google with the first key not sitting out: the payer's own, then the
	/// rotation's by its accounts' rank, held in their purse before the first of those is asked;
	/// the hold, to be paid once the answer is kept, and the account that paid (`None`: the payer's
	/// own key).
	async fn buy(&self, http: &reqwest::Client, req: &Request<'_>, payer: &Payer<'_>) -> Result<(Value, Option<Box<dyn Hold>>, Option<String>), Error> {
		let mut tried = Vec::new();
		let mut hold: Option<Box<dyn Hold>> = None;
		let mut rank = None;
		loop {
			let (api_key, account) = match self.keys.own(&payer.own, &tried) {
				Some(key) => (key, None),
				None => {
					let rank = match &rank {
						Some(r) => r,
						None => rank.insert(self.rank.rank(req.sku, &self.keys.accounts()).await?),
					};
					let (account, key) = self.keys.rotation(&payer.own, &tried, rank).map_err(Error::Exhausted)?;
					(key, Some(account))
				}
			};
			if account.is_some()
				&& hold.is_none()
				&& let Some(purse) = payer.purse
			{
				hold = Some(purse.hold(req.sku).await?);
			}
			let r = match req.method {
				"POST" => http.post(req.url).header(reqwest::header::CONTENT_TYPE, "application/json").body(req.body.to_owned()),
				"GET" => http.get(req.url),
				other => unreachable!("Places is asked by GET and POST only, not {other}"),
			};
			let r = r.header("X-Goog-Api-Key", api_key.as_str());
			let r = match req.mask {
				"" => r, // a photo's media takes no field mask
				mask => r.header("X-Goog-FieldMask", mask),
			};
			let resp = r.send().await?;
			let status = resp.status();
			tracing::info!(method = req.method, url = req.url, %status, ?account, "Places asked");
			let text = resp.text().await?;
			if status.is_success() {
				let answer = serde_json::from_str(&text).map_err(|e| Error::Api {
					asked: format!("{} {}", req.method, req.url),
					status: 200,
					body: format!("not JSON ({e}), paid for and not kept: {text}"),
				})?;
				return Ok((answer, hold, account));
			}
			match (Spent::of(&text), account) {
				(Some(why), _) => {
					self.keys.sit_out(&api_key, why);
					tried.push(api_key);
				}
				(None, None) => {
					return Err(Error::OwnKey {
						tail: keys::tail(&api_key).to_owned(),
						said: said(&text),
					});
				}
				(None, Some(_)) => {
					return Err(Error::Api {
						asked: format!("{} {}", req.method, req.url),
						status: status.as_u16(),
						body: text,
					});
				}
			}
		}
	}

	/// Each place a bought text search answered, kept as that place's details: a place a search
	/// returned is not asked about again.
	async fn keep_hits(&self, req: &Request<'_>, answer: &Value) -> Result<(), Error> {
		let base = req.url.strip_suffix(":searchText").expect("a Places search is asked of <base>:searchText");
		for raw in answer["places"].as_array().map_or(&[][..], Vec::as_slice) {
			let id = raw["id"].as_str().ok_or_else(|| Error::Api {
				asked: format!("{} {}", req.method, req.url),
				status: 200,
				body: format!("a hit without an id: {raw}"),
			})?;
			let url = format!("{base}/{id}");
			let details = Request::details(&url);
			self.cache
				.put(
					Scheme::Places,
					details.key(),
					Put {
						answer: raw.clone(),
						request: details.preimage(),
						bought: None,
					},
				)
				.await?;
		}
		Ok(())
	}

	async fn ask_here(&self, http: &reqwest::Client, req: &Request<'_>, fresh_since: Option<jiff::Timestamp>, payer: &Payer<'_>) -> Result<Value, Error> {
		let key = req.key();
		if let Some(kept) = self.cache.get(req.scheme, key).await?
			&& fresh_since.is_none_or(|since| kept.fetched_at >= since)
		{
			return Ok(kept.answer);
		}
		if payer.own.is_empty() && self.keys.is_empty() {
			return Err(Error::Invalid("asking Google Places needs an API key, and none is set".into()));
		}
		if !self.spend {
			return Err(Error::Unbought);
		}
		loop {
			match self.cache.claim(req.scheme, key, fresh_since).await? {
				Claim::Stored(kept) => return Ok(kept.answer),
				Claim::Held { .. } => tokio::time::sleep(Duration::from_millis(500)).await,
				Claim::Granted { .. } => break,
			}
		}
		let bought = self.buy(http, req, payer).await;
		if bought.is_err()
			&& let Err(e) = self.cache.release(req.scheme, key).await
		{
			// the buy's error is the one to tell; the claim lapses with its lease anyway
			tracing::warn!(error = %e, "releasing a claim after a failed buy");
		}
		let (answer, hold, account) = bought?;
		let bill = match account {
			None => Bill::Theirs,
			Some(_) => Bill::Ours,
		};
		self.cache
			.put(
				req.scheme,
				key,
				Put {
					answer: answer.clone(),
					request: req.preimage(),
					bought: Some(Billed {
						item: req.sku.into(),
						bill,
						account,
					}),
				},
			)
			.await?;
		if req.scheme == Scheme::Places && req.sku == sa_core::prices::PlacesRequest::Search {
			self.keep_hits(req, &answer).await?;
		}
		if let Some(hold) = hold {
			let label = match req.sku {
				sa_core::prices::PlacesRequest::Details => format!("Places details · {}", req.subject),
				sa_core::prices::PlacesRequest::Search => format!("Places search · {:?}", req.subject),
				sa_core::prices::PlacesRequest::Autocomplete => format!("Places autocomplete · {:?}", req.subject),
				sa_core::prices::PlacesRequest::Geocode => format!("Geocoding · {}", req.subject),
				sa_core::prices::PlacesRequest::Profile => format!("Places profile · {}", req.subject),
				sa_core::prices::PlacesRequest::Photo => format!("Places photo · {}", req.subject),
			};
			hold.paid(Bought { key, sku: req.sku, label }).await?;
		}
		Ok(answer)
	}
}

impl<'p> Buy<'p> for Here {
	type For = Payer<'p>;

	fn keyless(&self, payer: &Payer<'p>) -> bool {
		payer.own.is_empty() && self.keys.is_empty()
	}

	fn ask<'a>(&'a self, http: &'a reqwest::Client, req: &'a Request<'a>, fresh_since: Option<jiff::Timestamp>, payer: &'a Payer<'p>) -> Pending<'a, Value>
	where
		'p: 'a, {
		Box::pin(self.ask_here(http, req, fresh_since, payer))
	}
}
