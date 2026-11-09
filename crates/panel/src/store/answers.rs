//! Paid answers, the latest per request, and the claims of whoever is buying one
//! (`sa_core::answers`).

use eyre::WrapErr;
use jiff::Timestamp;
use sa_core::answers::{Claim, Key, LEASE, Put, Scheme, Stored};

use super::{Store, from_db, to_db};

impl Store {
	pub async fn answer(&self, scheme: Scheme, key: Key) -> eyre::Result<Option<Stored>> {
		let row: Option<(String, i64)> = sqlx::query_as("SELECT answer, fetched_at FROM answers WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.fetch_optional(self.pool())
			.await
			.wrap_err("reading an answer")?;
		row.map(stored).transpose()
	}

	/// The answer, unless fetched before `fresh_since`; else the key for the caller to buy, unless
	/// another holds it.
	pub async fn claim_answer(&self, scheme: Scheme, key: Key, fresh_since: Option<Timestamp>, now: Timestamp) -> eyre::Result<Claim> {
		let mut tx = self.begin_write().await?;
		let row: Option<(String, i64)> = sqlx::query_as("SELECT answer, fetched_at FROM answers WHERE scheme = $1 AND key = $2")
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
		let held: Option<i64> = sqlx::query_scalar("SELECT until FROM answer_claims WHERE scheme = $1 AND key = $2 AND until > $3")
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
		sqlx::query("INSERT INTO answer_claims (scheme, key, until) VALUES ($1, $2, $3) ON CONFLICT (scheme, key) DO UPDATE SET until = excluded.until")
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
	pub async fn release_answer_claim(&self, scheme: Scheme, key: Key) -> eyre::Result<()> {
		sqlx::query("DELETE FROM answer_claims WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.execute(self.pool())
			.await
			.wrap_err("releasing a claim")?;
		Ok(())
	}

	/// Keeps `answer` (JSON text) unless a newer one is kept already; any claim on the key ends.
	/// `request` is `None` only for an answer that never kept it.
	pub async fn put_answer(&self, scheme: Scheme, key: Key, answer: &str, request: Option<&str>, fetched_at: Timestamp) -> eyre::Result<()> {
		let mut tx = self.begin_write().await?;
		sqlx::query(
			"INSERT INTO answers (scheme, key, answer, request, fetched_at) VALUES ($1, $2, $3, $4, $5) \
			 ON CONFLICT (scheme, key) DO UPDATE SET answer = excluded.answer, request = excluded.request, fetched_at = excluded.fetched_at \
			 WHERE excluded.fetched_at > answers.fetched_at",
		)
		.bind(scheme.as_str())
		.bind(&key.0[..])
		.bind(answer)
		.bind(request)
		.bind(to_db(fetched_at))
		.execute(&mut *tx)
		.await
		.wrap_err("keeping an answer")?;
		sqlx::query("DELETE FROM answer_claims WHERE scheme = $1 AND key = $2")
			.bind(scheme.as_str())
			.bind(&key.0[..])
			.execute(&mut *tx)
			.await
			.wrap_err("ending a claim")?;
		tx.commit().await.wrap_err("keeping an answer")
	}
}

fn stored((answer, fetched_at): (String, i64)) -> eyre::Result<Stored> {
	Ok(Stored {
		answer: serde_json::from_str(&answer).wrap_err("a kept answer is not JSON")?,
		fetched_at: from_db(fetched_at)?,
	})
}

impl sa_places::Answers for Store {
	fn get(&self, key: Key) -> sa_places::Pending<'_, Option<Stored>> {
		Box::pin(async move { self.answer(Scheme::Places, key).await.map_err(store_error) })
	}

	fn claim(&self, key: Key, fresh_since: Option<Timestamp>) -> sa_places::Pending<'_, Claim> {
		Box::pin(async move { self.claim_answer(Scheme::Places, key, fresh_since, Timestamp::now()).await.map_err(store_error) })
	}

	fn put(&self, key: Key, put: Put) -> sa_places::Pending<'_, ()> {
		Box::pin(async move {
			self.put_answer(Scheme::Places, key, &put.answer.to_string(), Some(&put.request), Timestamp::now())
				.await
				.map_err(store_error)
		})
	}

	fn release(&self, key: Key) -> sa_places::Pending<'_, ()> {
		Box::pin(async move { self.release_answer_claim(Scheme::Places, key).await.map_err(store_error) })
	}
}

fn store_error(e: eyre::Report) -> sa_places::Error {
	sa_places::Error::Store(format!("{e:#}"))
}
