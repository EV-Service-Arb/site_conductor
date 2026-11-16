//! Google's meters off a billing account's Cloud Billing export in BigQuery, read by a service
//! account: a JWT it signs (RS256) for a token, then one query.

use std::{sync::Mutex, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::{WrapErr, bail, eyre};
use jiff::{SignedDuration, Timestamp};
use ring::{rand::SystemRandom, signature::RsaKeyPair};
use sa_core::meters::{Meter, Vendor};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Reader, Reading};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/bigquery.readonly";
/// How many times a query not done yet is waited on, a minute each.
const WAITS: usize = 5;

pub struct Google {
	http: reqwest::Client,
	/// The job runs in the export's project.
	project: String,
	/// `project.dataset.table`.
	table: String,
	email: String,
	key: RsaKeyPair,
	token: Mutex<Option<(String, Timestamp)>>,
}
impl Google {
	/// `export` is the table, `project.dataset.table`; `key` the JSON key of a service account
	/// that may query it.
	pub fn try_new(export: &str, key: &str) -> eyre::Result<Self> {
		let parts: Vec<&str> = export.split('.').collect();
		eyre::ensure!(
			parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')),
			"GOOGLE_BILLING_EXPORT is `project.dataset.table`, not {export:?}"
		);
		let key: ServiceKey = serde_json::from_str(key).wrap_err("GOOGLE_BILLING_KEY is no service account's JSON key")?;
		let pem: String = key.private_key.lines().filter(|l| !l.starts_with("-----")).collect();
		let der = base64::engine::general_purpose::STANDARD.decode(pem).wrap_err("GOOGLE_BILLING_KEY's private_key is no PEM")?;
		let rsa = RsaKeyPair::from_pkcs8(&der).map_err(|e| eyre!("GOOGLE_BILLING_KEY's private_key is no RSA PKCS#8 key: {e}"))?;
		Ok(Self {
			http: reqwest::Client::builder().timeout(Duration::from_secs(90)).build().wrap_err("an HTTP client for BigQuery")?,
			project: parts[0].to_owned(),
			table: export.to_owned(),
			email: key.client_email,
			key: rsa,
			token: Mutex::new(None),
		})
	}

	async fn token(&self, now: Timestamp) -> eyre::Result<String> {
		if let Some((token, until)) = &*self.token.lock().expect("never poisoned: nothing panics holding it")
			&& *until > now
		{
			return Ok(token.clone());
		}
		let b64 = |v: &Value| URL_SAFE_NO_PAD.encode(v.to_string());
		let iat = now.as_second();
		let claims = json!({ "iss": self.email, "scope": SCOPE, "aud": TOKEN_URL, "iat": iat, "exp": iat + 3600 });
		let signed = format!("{}.{}", b64(&json!({ "alg": "RS256", "typ": "JWT" })), b64(&claims));
		let mut sig = vec![0; self.key.public().modulus_len()];
		self.key
			.sign(&ring::signature::RSA_PKCS1_SHA256, &SystemRandom::new(), signed.as_bytes(), &mut sig)
			.map_err(|e| eyre!("signing the BigQuery JWT: {e}"))?;
		let jwt = format!("{signed}.{}", URL_SAFE_NO_PAD.encode(sig));
		let got = call(
			self.http
				.post(TOKEN_URL)
				.form(&[("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"), ("assertion", &jwt)]),
		)
		.await?;
		let token = got["access_token"].as_str().ok_or_else(|| eyre!("no access_token in Google's answer"))?.to_owned();
		let ttl = got["expires_in"].as_i64().ok_or_else(|| eyre!("no expires_in in Google's answer"))?;
		// a minute's margin, so a token is never sent as it lapses
		*self.token.lock().expect("never poisoned: nothing panics holding it") = Some((token.clone(), now + SignedDuration::from_secs(ttl - 60)));
		Ok(token)
	}

	/// Every row of `sql` under `params`, each its cells as BigQuery's REST gives them.
	async fn query(&self, sql: &str, params: Value, now: Timestamp) -> eyre::Result<Vec<Vec<Value>>> {
		let token = self.token(now).await?;
		let base = format!("https://bigquery.googleapis.com/bigquery/v2/projects/{}/queries", self.project);
		let body = json!({ "query": sql, "useLegacySql": false, "parameterMode": "NAMED", "queryParameters": params, "timeoutMs": 60000 });
		let mut page = call(self.http.post(&base).bearer_auth(&token).json(&body)).await?;
		for _ in 0..WAITS {
			if page["jobComplete"] == true {
				eyre::ensure!(page["pageToken"].is_null(), "BigQuery paged a reading of a few rows");
				return page["rows"]
					.as_array()
					.map_or(&[][..], Vec::as_slice)
					.iter()
					.map(|row| {
						let cells = row["f"].as_array().ok_or_else(|| eyre!("a row with no cells: {row}"))?;
						Ok(cells.iter().map(|c| c["v"].clone()).collect())
					})
					.collect();
			}
			let job = page["jobReference"]["jobId"].as_str().ok_or_else(|| eyre!("no jobId in {page}"))?.to_owned();
			let location = page["jobReference"]["location"].as_str().ok_or_else(|| eyre!("no location in {page}"))?.to_owned();
			page = call(
				self.http
					.get(format!("{base}/{job}"))
					.bearer_auth(&token)
					.query(&[("location", location.as_str()), ("timeoutMs", "60000")]),
			)
			.await?;
		}
		Err(eyre!("BigQuery did not finish the meters' query in {WAITS} minutes"))
	}
}

#[derive(Deserialize)]
struct ServiceKey {
	client_email: String,
	private_key: String,
}

impl Reader for Google {
	fn vendor(&self) -> Vendor {
		Vendor::Google
	}

	async fn read(&self, meters: &[&'static Meter], now: Timestamp) -> eyre::Result<Vec<Reading>> {
		let start = Vendor::Google.period(now).0;
		let table = &self.table;
		// credits are not taken off: the free units are a count, not money; `through` is the
		// export's own progress, so a quiet SKU does not hold it back
		let sql = format!(
			"WITH used AS (SELECT sku.id AS sku, SUM(usage.amount) AS units, SUM(cost) AS cost FROM `{table}` \
			 WHERE sku.id IN UNNEST(@skus) AND usage_start_time >= TIMESTAMP_MICROS(@start) GROUP BY sku) \
			 SELECT sku, COALESCE(units, 0), COALESCE(cost, 0), \
			 (SELECT ANY_VALUE(currency) FROM `{table}`), (SELECT UNIX_MICROS(MAX(usage_end_time)) FROM `{table}`) \
			 FROM UNNEST(@skus) AS sku LEFT JOIN used USING (sku)"
		);
		let skus: Vec<Value> = meters.iter().map(|m| json!({ "value": m.id })).collect();
		let params = json!([
			{ "name": "skus", "parameterType": { "type": "ARRAY", "arrayType": { "type": "STRING" } }, "parameterValue": { "arrayValues": skus } },
			{ "name": "start", "parameterType": { "type": "INT64" }, "parameterValue": { "value": start.as_microsecond().to_string() } },
		]);
		let rows = self.query(&sql, params, now).await?;
		let text = |v: &Value, what: &str| v.as_str().map(str::to_owned).ok_or_else(|| eyre!("the export gave {what} as {v}"));
		let number = |v: &Value, what: &str| -> eyre::Result<f64> { text(v, what)?.parse().wrap_err_with(|| format!("the export's {what}")) };
		rows.iter()
			.map(|row| {
				let [sku, units, cost, currency, through] = row.as_slice() else {
					bail!("a row of {} cells, not 5", row.len());
				};
				let sku = text(sku, "a SKU")?;
				let meter = *meters.iter().find(|m| m.id == sku).ok_or_else(|| eyre!("the export answered SKU {sku}, not asked"))?;
				let units = number(units, "units")?;
				eyre::ensure!(units.fract() == 0.0 && units >= 0.0, "{meter} counts {units}, no whole count");
				let through: i64 = text(through, "through")?.parse().wrap_err("the export's last usage end")?;
				Ok(Reading {
					meter,
					period_start: start,
					units: units as i64,
					cost_micros: (number(cost, "cost")? * 1e6).round() as i64,
					currency: text(currency, "the currency (an empty export?)")?,
					through: Timestamp::from_microsecond(through).wrap_err("the export's last usage end")?,
				})
			})
			.collect()
	}
}

async fn call(req: reqwest::RequestBuilder) -> eyre::Result<Value> {
	let resp = req.send().await.wrap_err("asking Google")?;
	let status = resp.status();
	let body = resp.text().await.wrap_err("reading Google's answer")?;
	eyre::ensure!(status.is_success(), "Google answered {status}: {body}");
	serde_json::from_str(&body).wrap_err_with(|| format!("Google's answer is no JSON: {body}"))
}
