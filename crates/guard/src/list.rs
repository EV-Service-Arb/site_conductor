//! BigQuery, which the guard reads the billing exports and the definitions beside them through.

use eyre::{Result, ensure, eyre};
use regex::Regex;
use serde_json::{Value, json};

/// `<project>.<dataset>.gcp_billing_export_v1_<billing account>`, split.
pub fn export(table: &str) -> Result<(&str, &str, String)> {
	let caps = Regex::new(r"^([\w-]+)\.(\w+)\.gcp_billing_export_v1_(\w{6})_(\w{6})_(\w{6})$")?
		.captures(table)
		.ok_or_else(|| eyre!("`table` is not <project>.<dataset>.gcp_billing_export_v1_<billing account>: {table}"))?;
	let part = |i| caps.get(i).expect("matched above").as_str();
	Ok((part(1), part(2), format!("{}-{}-{}", part(3), part(4), part(5))))
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
