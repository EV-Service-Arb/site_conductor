//! nix/google.nix as the guard reads it, its secrets aside, and BigQuery, which it reads the billing exports through.

use std::collections::BTreeMap;

use eyre::{Result, ensure, eyre};
use jiff::civil::Date;
use regex::Regex;
use sa_core::google::{Allow, Credit, Drawn, Held, Slice};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
pub struct Config {
	pub accounts: Vec<Account>,
	pub slices: Vec<Slice>,
}
impl Config {
	pub fn slices(&self) -> Result<Vec<Drawn>> {
		sa_core::google::drawn(&self.slices, self.accounts.iter().map(|a| (a.name.as_str(), &a.allow))).map_err(|e| eyre!("nix/google.nix: {}", e.0))
	}

	/// What the conductor holds too, if it runs this definition.
	pub fn digest(&self) -> String {
		let held: Vec<Held> = self
			.accounts
			.iter()
			.map(|a| Held {
				name: a.name.clone(),
				table: a.export.table.clone(),
				since: a.export.since,
				credits: a.credits.clone(),
				allow: a.allow.clone(),
				unguarded: a.unguarded.clone(),
			})
			.collect();
		sa_core::google::digest(&held, &self.slices)
	}
}

#[derive(Deserialize)]
pub struct Account {
	pub name: String,
	pub export: Export,
	pub credits: Vec<Credit>,
	pub allow: BTreeMap<String, Allow>,
	pub unguarded: Vec<String>,
}
impl Account {
	/// `<project>.<dataset>.gcp_billing_export_v1_<billing account>`, split.
	pub fn export(&self) -> Result<(&str, &str, String)> {
		let table = &self.export.table;
		let caps = Regex::new(r"^([\w-]+)\.(\w+)\.gcp_billing_export_v1_(\w{6})_(\w{6})_(\w{6})$")?
			.captures(table)
			.ok_or_else(|| eyre!("`table` is not <project>.<dataset>.gcp_billing_export_v1_<billing account>: {table}"))?;
		let part = |i| caps.get(i).expect("matched above").as_str();
		Ok((part(1), part(2), format!("{}-{}-{}", part(3), part(4), part(5))))
	}
}

#[derive(Deserialize)]
pub struct Export {
	pub table: String, // <project>.<dataset>.gcp_billing_export_v1_<billing account>
	pub since: Date,
}

pub struct BigQuery {
	client: reqwest::blocking::Client,
	token: String,
	project: String,
}

impl BigQuery {
	pub fn with_token(token: String, project: &str) -> Self {
		Self {
			client: reqwest::blocking::Client::new(),
			token,
			project: project.to_owned(),
		}
	}

	/// Every row, each a list of its cells as BigQuery gives them: a string, or null.
	pub fn query(&self, sql: &str) -> Result<Vec<Vec<Value>>> {
		let base = format!("https://bigquery.googleapis.com/bigquery/v2/projects/{}/queries", self.project);
		let mut page = call(
			self.client
				.post(&base)
				.bearer_auth(&self.token)
				.json(&json!({"query": sql, "useLegacySql": false, "timeoutMs": 60000})),
		)?;
		let job = page["jobReference"]["jobId"].as_str().ok_or_else(|| eyre!("no jobId in {page}"))?.to_owned();
		let location = page["jobReference"]["location"].as_str().ok_or_else(|| eyre!("no location in {page}"))?.to_owned();
		let mut rows = Vec::new();
		loop {
			if page["jobComplete"] == true {
				for row in page["rows"].as_array().map_or(&[][..], Vec::as_slice) {
					let cells = row["f"].as_array().ok_or_else(|| eyre!("a row with no cells: {row}"))?;
					rows.push(cells.iter().map(|c| c["v"].clone()).collect());
				}
				if page["pageToken"].is_null() {
					return Ok(rows);
				}
			}
			let mut ask = vec![("location", location.clone()), ("timeoutMs", "60000".to_owned())];
			if let Some(t) = page["pageToken"].as_str() {
				ask.push(("pageToken", t.to_owned()));
			}
			page = call(self.client.get(format!("{base}/{job}")).bearer_auth(&self.token).query(&ask))?;
		}
	}
}

pub fn call(req: reqwest::blocking::RequestBuilder) -> Result<Value> {
	let resp = req.send()?;
	let status = resp.status();
	let body: Value = resp.json()?;
	ensure!(
		status.is_success(),
		"{status}: {}",
		body["error"]["message"].as_str().map_or_else(|| body.to_string(), str::to_owned)
	);
	Ok(body)
}

pub fn text(v: &Value) -> Result<&str> {
	v.as_str().ok_or_else(|| eyre!("expected a value, got {v}"))
}

pub fn num(v: &Value) -> Result<f64> {
	Ok(text(v)?.parse()?)
}
