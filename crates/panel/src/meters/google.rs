//! Google's meters off a billing account's Cloud Billing export in BigQuery, read by a service
//! account: a JWT it signs (RS256) for a token, then one query.

use std::{sync::Mutex, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use eyre::{WrapErr, bail, eyre};
use jiff::{SignedDuration, Timestamp, civil::Date};
use ring::{rand::SystemRandom, signature::RsaKeyPair};
use sa_core::{
	google::BALANCES,
	meters::{Meter, Vendor},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Bill, Day, Draw, Lever, Reader, Reading, Tier};

const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const SCOPE: &str = "https://www.googleapis.com/auth/bigquery"; // reads the export, and writes its dataset's `definition`
/// How many times a query not done yet is waited on, a minute each.
const WAITS: usize = 5;

pub struct Google {
	account: String,
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
	/// `account`'s export read with its reader.
	pub fn try_new(account: &sa_places::Account) -> eyre::Result<Self> {
		let name = &account.name;
		let e = account.export.as_ref().ok_or_else(|| eyre!("{name} has no export to read"))?;
		let (export, reader) = (&e.table, &e.reader);
		let parts: Vec<&str> = export.split('.').collect();
		eyre::ensure!(
			parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')),
			"{name}'s export is `project.dataset.table`, not {export:?}"
		);
		let key: ServiceKey = serde_json::from_str(reader).wrap_err_with(|| format!("{name}'s reader is no service account's JSON key"))?;
		let pem: String = key.private_key.lines().filter(|l| !l.starts_with("-----")).collect();
		let der = base64::engine::general_purpose::STANDARD
			.decode(pem)
			.wrap_err_with(|| format!("{name}'s reader's private_key is no PEM"))?;
		let rsa = RsaKeyPair::from_pkcs8(&der).map_err(|e| eyre!("{name}'s reader's private_key is no RSA PKCS#8 key: {e}"))?;
		let _ = rustls::crypto::ring::default_provider().install_default(); //IGNORED_ERROR: Err is "a provider is already installed", which is all reqwest needs
		Ok(Self {
			account: name.clone(),
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
		let mut rows = Vec::new();
		let mut waits = 0;
		loop {
			if page["jobComplete"] == true {
				for row in page["rows"].as_array().map_or(&[][..], Vec::as_slice) {
					let cells = row["f"].as_array().ok_or_else(|| eyre!("a row with no cells: {row}"))?;
					rows.push(cells.iter().map(|c| c["v"].clone()).collect());
				}
				if page["pageToken"].is_null() {
					return Ok(rows);
				}
			} else {
				waits += 1;
				eyre::ensure!(waits <= WAITS, "BigQuery did not finish a query of the export in {WAITS} minutes");
			}
			let job = page["jobReference"]["jobId"].as_str().ok_or_else(|| eyre!("no jobId in {page}"))?.to_owned();
			let location = page["jobReference"]["location"].as_str().ok_or_else(|| eyre!("no location in {page}"))?.to_owned();
			let mut ask = vec![("location", location), ("timeoutMs", "60000".to_owned())];
			if let Some(t) = page["pageToken"].as_str() {
				ask.push(("pageToken", t.to_owned()));
			}
			page = call(self.http.get(format!("{base}/{job}")).bearer_auth(&token).query(&ask)).await?;
		}
	}

	/// The export's dataset's `definition` table, `` `project.dataset.definition` ``.
	fn definitions(&self) -> String {
		format!("`{}.definition`", self.table.rsplit_once('.').expect("checked when made: project.dataset.table").0)
	}

	/// The export's dataset's `guard` table exists: the guard has ticked there.
	async fn guarded(&self) -> eyre::Result<bool> {
		let (project, rest) = self.table.split_once('.').expect("checked when made: project.dataset.table");
		let dataset = rest.split_once('.').expect("checked when made: project.dataset.table").0;
		let url = format!("https://bigquery.googleapis.com/bigquery/v2/projects/{project}/datasets/{dataset}/tables/guard");
		let resp = self.http.get(url).bearer_auth(self.token(Timestamp::now()).await?).send().await.wrap_err("asking Google")?;
		match resp.status() {
			reqwest::StatusCode::OK => Ok(true),
			reqwest::StatusCode::NOT_FOUND => Ok(false),
			status => bail!("Google answered {status} of the guard's table: {}", resp.text().await.unwrap_or_default()), // the status is the refusal; its body only details it
		}
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

	fn account(&self) -> &str {
		&self.account
	}

	async fn read(&self, meters: &[&'static Meter], now: Timestamp) -> eyre::Result<Vec<Reading>> {
		let start = Vendor::Google.period(now).0;
		let table = &self.table;
		// the free units are a count, not money; `through` is the export's own progress, so a quiet
		// SKU does not hold it back
		let sql = format!(
			"WITH used AS (SELECT sku.id AS sku, SUM(usage.amount) AS units, SUM(cost) AS cost, \
			 SUM(cost) + SUM(IFNULL((SELECT SUM(c.amount) FROM UNNEST(credits) c), 0)) AS net FROM `{table}` \
			 WHERE sku.id IN UNNEST(@skus) AND usage_start_time >= TIMESTAMP_MICROS(@start) GROUP BY sku) \
			 SELECT sku, COALESCE(units, 0), COALESCE(cost, 0), COALESCE(net, 0), \
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
				let [sku, units, cost, net, currency, through] = row.as_slice() else {
					bail!("a row of {} cells, not 6", row.len());
				};
				let sku = text(sku, "a SKU")?;
				let meter = *meters.iter().find(|m| m.id == sku).ok_or_else(|| eyre!("the export answered SKU {sku}, not asked"))?;
				let units = number(units, "units")?;
				eyre::ensure!(units.fract() == 0.0 && units >= 0.0, "{meter} counts {units}, no whole count");
				let through: i64 = text(through, "through")?.parse().wrap_err("the export's last usage end")?;
				Ok(Reading {
					account: self.account.clone(),
					meter,
					period_start: start,
					units: units as i64,
					cost_micros: (number(cost, "cost")? * 1e6).round() as i64,
					net_micros: (number(net, "net")? * 1e6).round() as i64,
					currency: text(currency, "the currency (an empty export?)")?,
					through: Timestamp::from_microsecond(through).wrap_err("the export's last usage end")?,
				})
			})
			.collect()
	}

	async fn published(&self, now: Timestamp) -> eyre::Result<Option<String>> {
		let rows = self
			.query(&format!("SELECT digest FROM {} ORDER BY `at` DESC LIMIT 1", self.definitions()), json!([]), now)
			.await?;
		rows.first().map(|r| string(&r[0], "a digest")).transpose()
	}

	async fn publish(&self, definition: &sa_core::google::Definition, now: Timestamp) -> eyre::Result<()> {
		let (project, rest) = self.table.split_once('.').expect("checked when made: project.dataset.table");
		let dataset = rest.split_once('.').expect("checked when made: project.dataset.table").0;
		let url = format!("https://bigquery.googleapis.com/bigquery/v2/projects/{project}/datasets/{dataset}/tables/definition/insertAll");
		let row = json!({ "at": now.as_microsecond(), "digest": definition.digest(), "definition": serde_json::to_string(definition).expect("plain data serializes") });
		let got = call(self.http.post(url).bearer_auth(self.token(now).await?).json(&json!({ "rows": [{ "json": row }] }))).await?;
		eyre::ensure!(got["insertErrors"].is_null(), "BigQuery refused the definition: {got}");
		Ok(())
	}

	async fn bill(&self, since: Date) -> eyre::Result<Bill> {
		let table = &self.table;
		let now = Timestamp::now();
		// partitions by export day: a day's usage is exported that day or after, Pacific or UTC
		let since_param = json!([{ "name": "since", "parameterType": { "type": "DATE" }, "parameterValue": { "value": since.to_string() } }]);
		let pruned = "_PARTITIONTIME >= TIMESTAMP_SUB(TIMESTAMP(@since), INTERVAL 1 DAY) AND DATE(usage_start_time) >= @since";
		let days = self
			.query(
				&format!(
					"SELECT FORMAT_DATE('%F', DATE(usage_start_time)), service.description, sku.description, currency, SUM(cost), \
					 SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c WHERE c.type IN {BALANCES})), \
					 SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c WHERE c.type NOT IN {BALANCES})) \
					 FROM `{table}` WHERE {pruned} GROUP BY 1, 2, 3, 4"
				),
				since_param.clone(),
				now,
			)
			.await?
			.iter()
			.map(|r| {
				Ok(Day {
					day: string(&r[0], "a day")?.parse().wrap_err("the export's day")?,
					service: string(&r[1], "a service")?,
					sku: string(&r[2], "a SKU")?,
					currency: string(&r[3], "a currency")?,
					cost_micros: micros(&r[4], "cost")?,
					credit_micros: micros(&r[5], "credits")?,
					discount_micros: micros(&r[6], "discounts")?,
				})
			})
			.collect::<eyre::Result<_>>()?;
		let draws = self
			.query(
				&format!(
					"SELECT c.id, UNIX_SECONDS(TIMESTAMP_TRUNC(export_time, HOUR)), SUM(c.amount) FROM `{table}`, UNNEST(credits) c \
					 WHERE c.type IN {BALANCES} GROUP BY 1, 2"
				),
				json!([]),
				now,
			)
			.await?
			.iter()
			.map(|r| {
				Ok(Draw {
					credit: string(&r[0], "a credit's id")?,
					hour: Timestamp::from_second(string(&r[1], "an hour")?.parse().wrap_err("the export's hour")?)?,
					amount_micros: micros(&r[2], "a draw")?,
				})
			})
			.collect::<eyre::Result<_>>()?;
		let tiers = self
			.query(
				&format!(
					"SELECT invoice.month, service.description, sku.description, ANY_VALUE(usage.pricing_unit), SUM(usage.amount_in_pricing_units), \
					 SUM(IF(price.effective_price > 0, usage.amount_in_pricing_units, 0)), MIN(IF(price.effective_price > 0, price.tier_start_amount, NULL)), \
					 SUM(cost), SUM((SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c WHERE c.type NOT IN {BALANCES})) \
					 FROM `{table}` WHERE {pruned} GROUP BY 1, 2, 3"
				),
				since_param,
				now,
			)
			.await?
			.iter()
			.map(|r| {
				Ok(Tier {
					month: string(&r[0], "a month")?,
					service: string(&r[1], "a service")?,
					sku: string(&r[2], "a SKU")?,
					unit: r[3].as_str().map(str::to_owned),
					units: opt_f64(&r[4], "units")?.unwrap_or(0.0),           // SUM of none: no usage in pricing units
					paid_units: opt_f64(&r[5], "paid units")?.unwrap_or(0.0), // likewise
					free_to: opt_f64(&r[6], "where its priced tier starts")?,
					cost_micros: micros(&r[7], "cost")?,
					free_micros: micros(&r[8], "discounts")?,
				})
			})
			.collect::<eyre::Result<_>>()?;
		let guard = match self.guarded().await? {
			false => None,
			true => {
				let guard = format!("`{}.guard`", table.rsplit_once('.').expect("checked when made: project.dataset.table").0);
				let rows = self
					.query(
						&format!(
							"SELECT tick, category, project, lever, allow_day, allow_month, real_day, real_month, setting, state, today, action, list FROM {guard} \
							 WHERE tick = (SELECT MAX(tick) FROM {guard})"
						),
						json!([]),
						now,
					)
					.await?;
				Some(
					rows.iter()
						.map(|r| {
							Ok(Lever {
								tick: Timestamp::from_second(string(&r[0], "a tick")?.parse().wrap_err("the guard's tick")?)?,
								slice: r[1].as_str().map(str::to_owned),
								project: r[2].as_str().map(str::to_owned),
								lever: string(&r[3], "a lever")?,
								allow_day: opt_f64(&r[4], "a day's allow")?,
								allow_month: opt_f64(&r[5], "a month's allow")?,
								real_day: opt_f64(&r[6], "real money today")?,
								real_month: opt_f64(&r[7], "real money this month")?,
								setting: opt_f64(&r[8], "a setting")?,
								state: r[9].as_str().map(str::to_owned),
								today: r[10].as_str().map(str::parse).transpose().wrap_err("the guard's today")?,
								action: r[11].as_str().map(str::to_owned),
								list: r[12].as_str().map(str::to_owned),
							})
						})
						.collect::<eyre::Result<_>>()?,
				)
			}
		};
		Ok(Bill { days, draws, tiers, guard })
	}
}

/// `v`, BigQuery's decimal string, in millionths.
fn micros(v: &Value, what: &str) -> eyre::Result<i64> {
	let n: f64 = v
		.as_str()
		.ok_or_else(|| eyre!("the export gave {what} as {v}"))?
		.parse()
		.wrap_err_with(|| format!("the export's {what}"))?;
	Ok((n * 1e6).round() as i64)
}

fn opt_f64(v: &Value, what: &str) -> eyre::Result<Option<f64>> {
	v.as_str().map(|s| s.parse().wrap_err_with(|| format!("the export's {what}"))).transpose()
}

fn string(v: &Value, what: &str) -> eyre::Result<String> {
	v.as_str().map(str::to_owned).ok_or_else(|| eyre!("the export gave {what} as {v}"))
}

async fn call(req: reqwest::RequestBuilder) -> eyre::Result<Value> {
	let resp = req.send().await.wrap_err("asking Google")?;
	let status = resp.status();
	let body = resp.text().await.wrap_err("reading Google's answer")?;
	eyre::ensure!(status.is_success(), "Google answered {status}: {body}");
	serde_json::from_str(&body).wrap_err_with(|| format!("Google's answer is no JSON: {body}"))
}
