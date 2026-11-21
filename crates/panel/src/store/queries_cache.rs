//! The queries cache: what was paid for a query, the latest per query beside every older version,
//! and the claims of whoever is buying one (`sa_core::queries_cache`).

use std::collections::BTreeSet;

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::{
	queries_cache::{Claim, Key, LEASE, Put, Scheme, Stored, Version},
	usage::Billed,
};

use super::{
	Store, from_db, to_db,
	usage::{self, CONDUCTOR, Use},
};
use crate::Refused;

impl Store {
	pub async fn cached_query(&self, scheme: Scheme, key: Key) -> eyre::Result<Option<Stored>> {
		let row: Option<(String, i64)> = sqlx::query_as("SELECT answer, fetched_at FROM queries_cache WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.fetch_optional(self.pool())
			.await
			.wrap_err("reading an answer")?;
		row.map(stored).transpose()
	}

	/// The answer, unless fetched before `fresh_since`; else the key for the caller to buy, unless
	/// another holds it.
	pub async fn claim_query(&self, scheme: Scheme, key: Key, fresh_since: Option<Timestamp>, now: Timestamp) -> eyre::Result<Claim> {
		let mut tx = self.begin_write().await?;
		let row: Option<(String, i64)> = sqlx::query_as("SELECT answer, fetched_at FROM queries_cache WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.fetch_optional(&mut *tx)
			.await
			.wrap_err("reading an answer")?;
		if let Some(kept) = row.map(stored).transpose()?
			&& fresh_since.is_none_or(|since| kept.fetched_at >= since)
		{
			return Ok(Claim::Stored(kept));
		}
		let held: Option<i64> = sqlx::query_scalar("SELECT until FROM queries_cache_claims WHERE scheme = $1 AND key = $2 AND until > $3")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.bind(to_db(now))
			.fetch_optional(&mut *tx)
			.await
			.wrap_err("reading a claim")?;
		if let Some(until) = held {
			return Ok(Claim::Held { until: from_db(until)? });
		}
		let until = now + LEASE;
		sqlx::query("INSERT INTO queries_cache_claims (scheme, key, until) VALUES ($1, $2, $3) ON CONFLICT (scheme, key) DO UPDATE SET until = excluded.until")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.bind(to_db(until))
			.execute(&mut *tx)
			.await
			.wrap_err("claiming an answer")?;
		tx.commit().await.wrap_err("claiming an answer")?;
		Ok(Claim::Granted { until })
	}

	/// The claim on a key given up, whoever held it.
	pub async fn release_query(&self, scheme: Scheme, key: Key) -> eyre::Result<()> {
		sqlx::query("DELETE FROM queries_cache_claims WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.execute(self.pool())
			.await
			.wrap_err("releasing a claim")?;
		Ok(())
	}

	/// A version of an answer arrives: the latest stays in `queries_cache`, every other in the
	/// history, none is dropped; any claim on the key ends. `bought`, it is counted as `source`'s use
	/// in the same write. Refused unless `key` is `request`'s SHA-256.
	pub async fn keep_query(&self, scheme: Scheme, key: Key, answer: &str, request: &str, bought: Option<(Billed, &str)>, fetched_at: Timestamp) -> eyre::Result<Result<(), Refused>> {
		if Key::of(request) != key {
			return Ok(Err(Refused::Invalid("the key is not the SHA-256 of the request".to_owned())));
		}
		// Held to the end: a version reaches the history before the row it leaves changes, and no other keep moves that row meanwhile.
		let mut tx = self.begin_write().await?;
		if let Some((Billed { item, bill, account }, source)) = bought {
			let used = Use {
				at: fetched_at,
				item,
				units: 1,
				bill,
				account,
				source: source.to_owned(),
				user: None,
				key: None,
			};
			usage::record(&mut tx, &used).await?;
		}
		let kept: Option<(String, i64)> = sqlx::query_as("SELECT answer, fetched_at FROM queries_cache WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.fetch_optional(&mut *tx)
			.await
			.wrap_err("reading an answer")?;
		let at = to_db(fetched_at);
		match kept {
			None => {
				sqlx::query("INSERT INTO queries_cache (scheme, key, answer, request, fetched_at) VALUES ($1, $2, $3, $4, $5)")
					.bind(scheme.as_str())
					.bind(&key.0[..])
					.bind(answer)
					.bind(request)
					.bind(at)
					.execute(&mut *tx)
					.await
					.wrap_err("keeping an answer")?;
			}
			Some((_, was)) if was == at => {}
			Some((_, was)) if at < was => self.keep_version(scheme, key, answer, request, at).await?,
			Some((was_answer, was)) => {
				self.keep_version(scheme, key, &was_answer, request, was).await?; // the row's request is this one: its key is the digest of both
				sqlx::query("UPDATE queries_cache SET answer = $3, request = $4, fetched_at = $5 WHERE scheme = $1 AND key = $2")
					.bind(scheme.as_str())
					.bind(&key.0[..])
					.bind(answer)
					.bind(request)
					.bind(at)
					.execute(&mut *tx)
					.await
					.wrap_err("keeping an answer")?;
			}
		}
		sqlx::query("DELETE FROM queries_cache_claims WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.execute(&mut *tx)
			.await
			.wrap_err("ending a claim")?;
		tx.commit().await.wrap_err("keeping an answer")?;
		Ok(Ok(()))
	}

	/// Committed on its own: kept already, it is let be.
	async fn keep_version(&self, scheme: Scheme, key: Key, answer: &str, request: &str, at: i64) -> eyre::Result<()> {
		sqlx::query("INSERT INTO queries_history (scheme, key, fetched_at, answer, request) VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.bind(at)
			.bind(answer)
			.bind(request)
			.execute(&self.history)
			.await
			.wrap_err("keeping a version of an answer")?;
		Ok(())
	}

	/// Every version of every answer held, the latest and the history's.
	pub async fn queries_index(&self) -> eyre::Result<BTreeSet<(Scheme, Key, Timestamp)>> {
		// The latest first: a row a keep moves meanwhile is in the history by the time it is read.
		let latest: Vec<(String, Vec<u8>, i64)> = sqlx::query_as("SELECT scheme, key, fetched_at FROM queries_cache")
			.fetch_all(self.pool())
			.await
			.wrap_err("reading the answers' index")?;
		let history: Vec<(String, Vec<u8>, i64)> = sqlx::query_as("SELECT scheme, key, fetched_at FROM queries_history")
			.fetch_all(&self.history)
			.await
			.wrap_err("reading the history's index")?;
		latest
			.into_iter()
			.chain(history)
			.map(|(scheme, key, at)| Ok((parse_scheme(&scheme)?, from_key(key)?, from_db(at)?)))
			.collect()
	}

	/// The version of `key`'s answer fetched at `fetched_at`, the latest's or the history's.
	pub async fn query_version(&self, scheme: Scheme, key: Key, fetched_at: Timestamp) -> eyre::Result<Option<Version>> {
		// The latest first, as in the index.
		let latest: Option<(String, String)> = sqlx::query_as("SELECT answer, request FROM queries_cache WHERE scheme = $1 AND key = $2 AND fetched_at = $3")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.bind(to_db(fetched_at))
			.fetch_optional(self.pool())
			.await
			.wrap_err("reading an answer")?;
		let row = match latest {
			Some(row) => Some(row),
			None => sqlx::query_as("SELECT answer, request FROM queries_history WHERE scheme = $1 AND key = $2 AND fetched_at = $3")
				.bind(scheme.as_str())
				.bind(&key.0[..])
				.bind(to_db(fetched_at))
				.fetch_optional(&self.history)
				.await
				.wrap_err("reading a version of an answer")?,
		};
		row.map(|(answer, request)| {
			Ok(Version {
				scheme,
				key,
				request,
				answer: serde_json::from_str(&answer).wrap_err("a kept answer is not JSON")?,
				fetched_at,
			})
		})
		.transpose()
	}
}

fn parse_scheme(s: &str) -> eyre::Result<Scheme> {
	s.parse().map_err(|e: sa_core::Invalid| eyre::eyre!(e.0))
}

fn from_key(key: Vec<u8>) -> eyre::Result<Key> {
	Ok(Key(key.try_into().map_err(|_| eyre::eyre!("a stored key is not 32 bytes"))?))
}

fn stored((answer, fetched_at): (String, i64)) -> eyre::Result<Stored> {
	Ok(Stored {
		answer: serde_json::from_str(&answer).wrap_err("a kept answer is not JSON")?,
		fetched_at: from_db(fetched_at)?,
	})
}

impl sa_places::QueriesCache for Store {
	fn get(&self, scheme: Scheme, key: Key) -> sa_places::Pending<'_, Option<Stored>> {
		Box::pin(async move { self.cached_query(scheme, key).await.map_err(store_error) })
	}

	fn claim(&self, scheme: Scheme, key: Key, fresh_since: Option<Timestamp>) -> sa_places::Pending<'_, Claim> {
		Box::pin(async move { self.claim_query(scheme, key, fresh_since, Timestamp::now()).await.map_err(store_error) })
	}

	fn put(&self, scheme: Scheme, key: Key, put: Put) -> sa_places::Pending<'_, ()> {
		Box::pin(async move {
			self.keep_query(scheme, key, &put.answer.to_string(), &put.request, put.bought.map(|b| (b, CONDUCTOR)), Timestamp::now())
				.await
				.map_err(store_error)?
				.map_err(|refused| sa_places::Error::QueriesCache(refused.to_string()))
		})
	}

	fn release(&self, scheme: Scheme, key: Key) -> sa_places::Pending<'_, ()> {
		Box::pin(async move { self.release_query(scheme, key).await.map_err(store_error) })
	}
}

fn store_error(e: eyre::Report) -> sa_places::Error {
	sa_places::Error::QueriesCache(format!("{e:#}"))
}
