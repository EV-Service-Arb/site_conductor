//! Holds every slice's real money (what credits do not pay) to its `allow` in nix/google.nix, none where unnamed,
//! with the levers Google gives: spend caps, daily quotas, and unlinking billing. docs/ARCHITECTURE.md, The guard.
//!
//!     guard tick [--dry-run]   read every account, set its levers, record what it saw in `<export dataset>.guard`
//!     guard read               print what a tick reads, as JSON
//!     guard plan <read.json>   what a tick on that reading would set
//!
//! Env: GUARD_CONFIG, nix/google.nix as JSON; GUARD_PROJECT, the project it runs in; GOOGLE_OAUTH_ACCESS_TOKEN, else the metadata server's.

use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
};

mod list;

use eyre::{Result, WrapErr, bail, ensure, eyre};
use jiff::{Timestamp, ToSpan, tz::TimeZone};
use list::{BigQuery, Config, call, num, text};
use sa_core::{
	google::{BALANCES, Drawn},
	meters,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Services Google takes spend caps on, by billing service id.
const CAPPABLE: [&str; 4] = ["AEFD-7695-64FA", "C7E2-9256-1C43", "152E-C115-5142", "29E7-DA93-CA13"];
const RUN: &str = "152E-C115-5142"; // the guard's own runtime, in its own project, is never capped: a cap firing on it would stop the guard
const MARGIN: f64 = 0.10; // real money past a slice's allowance that unlinks billing; below it is the export's rounding
const DEMAND_DAYS: i64 = 30;
const PACIFIC: &str = "America/Los_Angeles"; // Google's billing month and quota day

fn main() -> Result<()> {
	rustls::crypto::ring::default_provider().install_default().expect("the first thing main does");
	let args: Vec<String> = env::args().skip(1).collect();
	let config: Config = serde_json::from_str(&env::var("GUARD_CONFIG").wrap_err("GUARD_CONFIG")?).wrap_err("GUARD_CONFIG")?;
	let slices = config.slices()?;
	match &args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
		["plan", file] => {
			let read: Read = serde_json::from_str(&fs::read_to_string(file)?)?;
			for (account, (rows, acts)) in read.accounts.iter().zip(plan(&config, &slices, &read)?) {
				print(&account.account, &rows, &acts);
			}
		}
		["read"] => println!("{}", serde_json::to_string_pretty(&read(&Google::try_new()?, &config, &slices)?)?),
		["tick", rest @ ..] => {
			let dry = match rest {
				[] => false,
				["--dry-run"] => true,
				_ => bail!("usage: guard tick [--dry-run]"),
			};
			let google = Google::try_new()?;
			let read = read(&google, &config, &slices)?;
			let planned = plan(&config, &slices, &read)?;
			let list = config.digest();
			for (seen, (rows, acts)) in read.accounts.iter().zip(planned) {
				print(&seen.account, &rows, &acts);
				if dry {
					continue;
				}
				for act in &acts {
					google.act(act).wrap_err_with(|| format!("{}: {act:?}", seen.account))?;
				}
				let account = config.accounts.iter().find(|a| a.name == seen.account).expect("read from config");
				let (project, dataset, _) = account.export()?;
				let rows: Vec<Value> = rows
					.iter()
					.map(|r| {
						let mut row = serde_json::to_value(r)?;
						row["tick"] = json!(read.now.as_second());
						row["list"] = json!(list);
						Ok(json!({"json": row}))
					})
					.collect::<Result<_>>()?;
				let url = format!("https://bigquery.googleapis.com/bigquery/v2/projects/{project}/datasets/{dataset}/tables/guard/insertAll");
				let got = call(google.client.post(url).bearer_auth(&google.token).json(&json!({"rows": rows})))?;
				ensure!(got["insertErrors"].is_null(), "{}: recording the tick: {got}", seen.account);
			}
		}
		_ => bail!("usage: guard tick [--dry-run] | read | plan <read.json>"),
	}
	Ok(())
}

#[derive(Deserialize, Serialize)]
struct Read {
	now: Timestamp,
	own: String,
	services: BTreeMap<String, String>, // cappable id -> display name
	accounts: Vec<Seen>,
}

#[derive(Deserialize, Serialize)]
struct Seen {
	account: String,
	billing: String,
	usd: Option<f64>,                // the account's currency per USD, off the export
	projects: BTreeMap<String, u64>, // linked and billed, id -> number
	gross: Vec<Gross>,
	real: Vec<Real>,
	drawn: BTreeMap<String, f64>, // credit id -> drawn since its reading, negative
	caps: Vec<Cap>,
	quotas: Vec<Quota>,
}

#[derive(Deserialize, Serialize)]
struct Gross {
	project: String,
	service: String,
	month: f64,  // at list price, credits not taken off: what a cap counts
	recent: f64, // the same over the last `DEMAND_DAYS`
}

#[derive(Deserialize, Serialize)]
struct Real {
	project: String,
	service: String,
	sku: String,
	month: f64, // after every credit and discount
}

#[derive(Deserialize, Serialize)]
struct Cap {
	name: String,
	project: String,
	service: String,
	amount: f64,
	state: String,
}

#[derive(Deserialize, Serialize)]
struct Quota {
	project: String,
	api: String,
	id: String,
	value: i64,
	today: i64, // requests since the quota day began, off Cloud Monitoring
}

#[derive(Debug, PartialEq)]
enum Act {
	Create {
		billing: String,
		project: String,
		number: u64,
		service: String,
		amount: f64,
	},
	Lift {
		name: String,
	},
	Set {
		name: String,
		amount: f64,
	},
	Quota {
		project: String,
		api: String,
		id: String,
		value: i64,
	},
	Unlink {
		project: String,
	},
}

#[derive(Serialize)]
struct Row {
	account: String,
	category: String,
	project: Option<String>,
	lever: String,
	allow: Option<f64>,
	real: Option<f64>,
	setting: Option<f64>,
	state: Option<String>,
	today: Option<i64>,
	action: Option<String>,
}

/// Every lever's setting to stderr, what would change to stdout.
fn print(account: &str, rows: &[Row], acts: &[Act]) {
	let n = |x: Option<f64>| x.map_or("-".to_owned(), |x| format!("{:.2}", x + 0.0)); // + 0.0: no "-0.00"
	for r in rows {
		eprintln!(
			"{account}\t{}\t{}\t{}\tallow {}\treal {}\tset {}\tis {}",
			r.category,
			r.project.as_deref().unwrap_or("-"),
			r.lever,
			n(r.allow),
			n(r.real),
			n(r.setting),
			r.state.as_deref().unwrap_or("-")
		);
	}
	for a in acts {
		println!("{account}\t{a:?}");
	}
}

/// Every lever's setting, and what of it differs from what was read.
fn plan(config: &Config, slices: &[Drawn], read: &Read) -> Result<Vec<(Vec<Row>, Vec<Act>)>> {
	let pacific = TimeZone::get(PACIFIC)?;
	let today = read.now.to_zoned(pacific).date();
	read.accounts
		.iter()
		.map(|seen| {
			let account = config
				.accounts
				.iter()
				.find(|a| a.name == seen.account)
				.ok_or_else(|| eyre!("{} read but not in config", seen.account))?;
			let allow = |c: &str| account.allow.get(c).copied().unwrap_or(0.0); // no entry is no real money
			let configured: BTreeSet<&str> = account.credits.iter().filter_map(|c| c.id.as_deref()).collect();
			let unknown: Vec<&String> = seen.drawn.keys().filter(|id| !configured.contains(id.as_str())).collect();
			ensure!(
				unknown.is_empty(),
				"{}: the export draws on credits {unknown:?} with no `credits` entry in nix/google.nix",
				seen.account
			);
			// a day early: Google's day ends in Pacific time, and a credit gone a day early only caps sooner
			let tomorrow = today.checked_add(1.day())?;
			let credits: f64 = account
				.credits
				.iter()
				.map(|c| c.left_on(c.id.as_ref().map_or(0.0, |id| seen.drawn.get(id).copied().unwrap_or(0.0)), tomorrow))
				.sum();

			let mut rows = Vec::new();
			let mut acts = Vec::new();

			// A service's usage goes to the first slice drawing it; what no sku-less slice takes keeps the service's own name.
			let categories = |service: &str| -> Vec<String> {
				let mut out = Vec::new();
				for s in slices.iter().filter(|s| s.service.is_match(service)) {
					out.push(s.name.clone());
					if s.sku.is_none() {
						return out;
					}
				}
				out.push(service.to_owned());
				out
			};

			// caps: one per linked project and cappable service, but the guard's own runtime
			let pairs: Vec<(&String, &String)> = seen
				.projects
				.keys()
				.flat_map(|p| read.services.keys().map(move |s| (p, s)))
				.filter(|(p, s)| !(**p == read.own && *s == RUN))
				.collect();
			let cap = |p: &str, s: &str| -> Result<Option<&Cap>> {
				let mut found = seen.caps.iter().filter(|c| c.project == p && c.service == s);
				let first = found.next();
				ensure!(found.next().is_none(), "{}: two spend caps on {p} {s}", seen.account);
				Ok(first)
			};
			let gross = |p: &str, s: &str| seen.gross.iter().find(|g| g.project == p && g.service == s);
			let mut demand = Vec::new();
			for (p, s) in &pairs {
				if gross(p, s).is_some_and(|g| g.recent > 0.0) || cap(p, s)?.is_some_and(|c| c.state == "ENFORCED") {
					demand.push((*p, *s));
				}
			}
			// credits go to the caps of services in use, evenly, so together they hold no more than what is left
			let share = if demand.is_empty() { 0.0 } else { credits / demand.len() as f64 };
			// a slice's allowance spreads evenly over every cap it draws through
			let draws_through = |c: &str| pairs.iter().filter(|(_, s)| categories(&read.services[*s]).iter().any(|x| x == c)).count() as f64;
			for (p, s) in &pairs {
				let name = &read.services[*s];
				let cats = categories(name);
				let allowed: f64 = cats.iter().map(|c| allow(c) / draws_through(c)).sum();
				let target = cents(gross(p, s).map_or(0.0, |g| g.month) + if demand.contains(&(*p, *s)) { share } else { 0.0 } + allowed);
				let existing = cap(p, s)?;
				let act = match existing {
					None => Some(Act::Create {
						billing: seen.billing.clone(),
						project: (*p).clone(),
						number: seen.projects[*p],
						service: (*s).clone(),
						amount: target,
					}),
					Some(c) if c.state == "ENFORCED" && target > c.amount + 0.005 => {
						acts.push(Act::Lift { name: c.name.clone() });
						Some(Act::Set {
							name: c.name.clone(),
							amount: target,
						})
					}
					Some(c) if c.state == "ENFORCED" => None, // still over what it may spend
					Some(c) if (c.amount - target).abs() >= 0.005 || c.state != "CONFIGURED" => Some(Act::Set {
						name: c.name.clone(),
						amount: target,
					}),
					Some(_) => None,
				};
				let action = act.as_ref().map(|a| format!("{a:?}"));
				acts.extend(act);
				for c in cats {
					rows.push(Row {
						account: seen.account.clone(),
						category: c,
						project: Some((*p).clone()),
						lever: format!("cap: {name}"),
						allow: None,
						real: None,
						setting: Some(target),
						state: existing.map(|c| c.state.clone()),
						today: None,
						action: action.clone(),
					});
				}
			}

			// daily quotas on the meters' APIs, which Google takes no spend cap on: what the meters that only count give free a
			// month and what the slice's allowance buys at their dearest list price, over 31 days; an API nothing of ours calls, none
			if let Some(maps) = slices.iter().find(|s| s.name == meters::SLICE) {
				let paced = |api: &str| maps.closed.iter().any(|c| c == api) || meters::pace(api).is_some();
				let quotas: Vec<&Quota> = seen.quotas.iter().filter(|q| paced(&q.api)).collect();
				let open: BTreeSet<(&str, &str)> = quotas.iter().filter(|q| meters::pace(&q.api).is_some()).map(|q| (q.project.as_str(), q.api.as_str())).collect();
				for q in &quotas {
					let value = match meters::pace(&q.api) {
						_ if maps.closed.iter().any(|c| *c == q.api) => 0,
						None => unreachable!("only the closed and the metered are paced"),
						Some((free, price)) => {
							let bought = match allow(&maps.name) / open.len() as f64 {
								0.0 => 0.0,
								money =>
									money
										/ (price as f64 / 1e6
											* seen
												.usd
												.ok_or_else(|| eyre!("{}: no exchange rate in the export, to price {}'s allowance", seen.account, maps.name))?),
							};
							let k = quotas.iter().filter(|o| o.project == q.project && o.api == q.api).count() as f64; // each daily quota of the API may be spent in full
							((free as f64 + bought) / 31.0 / k).floor() as i64
						}
					};
					let act = (q.value != value).then(|| Act::Quota {
						project: q.project.clone(),
						api: q.api.clone(),
						id: q.id.clone(),
						value,
					});
					rows.push(Row {
						account: seen.account.clone(),
						category: maps.name.clone(),
						project: Some(q.project.clone()),
						lever: format!("quota: {} {}", q.api, q.id),
						allow: None,
						real: None,
						setting: Some(value as f64),
						state: Some(q.value.to_string()),
						today: Some(q.today),
						action: act.as_ref().map(|a| format!("{a:?}")),
					});
					acts.extend(act);
				}
			}

			// the backstop: any slice past its allowance takes billing off every project it spent real money in
			let mut real: BTreeMap<String, BTreeMap<&str, f64>> = BTreeMap::new();
			for r in &seen.real {
				let category = slices.iter().find(|s| s.draws(&r.service, &r.sku)).map_or(r.service.clone(), |s| s.name.clone());
				*real.entry(category).or_default().entry(r.project.as_str()).or_default() += r.month;
			}
			for s in slices {
				real.entry(s.name.clone()).or_default();
			}
			let mut unlink = BTreeSet::new();
			for (category, by_project) in &real {
				let spent: f64 = by_project.values().sum();
				let over = spent > allow(category) + MARGIN;
				if over {
					unlink.extend(by_project.iter().filter(|(_, m)| **m > 0.0).map(|(p, _)| *p).filter(|p| seen.projects.contains_key(*p)));
				}
				rows.push(Row {
					account: seen.account.clone(),
					category: category.clone(),
					project: None,
					lever: "real money".to_owned(),
					allow: Some(allow(category)),
					real: Some(spent),
					setting: None,
					state: Some(if over { "over" } else { "within" }.to_owned()),
					today: None,
					action: over.then(|| format!("unlink {}", by_project.keys().copied().collect::<Vec<_>>().join(", "))),
				});
			}
			acts.extend(unlink.into_iter().map(|p| Act::Unlink { project: p.to_owned() }));
			Ok((rows, acts))
		})
		.collect()
}

fn cents(x: f64) -> f64 {
	(x * 100.0).floor() / 100.0
}

struct Google {
	client: reqwest::blocking::Client,
	token: String,
}

impl Google {
	fn try_new() -> Result<Self> {
		let client = reqwest::blocking::Client::new();
		let token = match env::var("GOOGLE_OAUTH_ACCESS_TOKEN") {
			Ok(t) => t,
			Err(_) => {
				let got = call(
					client
						.get("http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token")
						.header("Metadata-Flavor", "Google"),
				)
				.wrap_err("no GOOGLE_OAUTH_ACCESS_TOKEN, and no metadata server to take one from")?;
				text(&got["access_token"])?.to_owned()
			}
		};
		Ok(Self { client, token })
	}

	fn get(&self, url: &str) -> Result<Value> {
		call(self.client.get(url).bearer_auth(&self.token)).wrap_err_with(|| url.to_owned())
	}

	/// Every item under `key` of a paged list.
	fn list(&self, url: &str, key: &str) -> Result<Vec<Value>> {
		let mut out = Vec::new();
		let mut token = String::new();
		loop {
			let sep = if url.contains('?') { '&' } else { '?' };
			let page = self.get(&format!("{url}{sep}pageToken={token}"))?;
			out.extend(page[key].as_array().cloned().unwrap_or_default()); // an empty list comes without its key
			match page["nextPageToken"].as_str() {
				Some(t) if !t.is_empty() => token = t.to_owned(),
				_ => return Ok(out),
			}
		}
	}

	fn act(&self, act: &Act) -> Result<()> {
		let budgets = "https://billingbudgets.googleapis.com/v1";
		let amount = |a: f64| json!({"specifiedAmount": {"units": (a.trunc() as i64).to_string(), "nanos": ((a.fract() * 1e9).round() as i64)}});
		match act {
			Act::Create {
				billing,
				number,
				service,
				amount: a,
				..
			} => {
				call(self.client.post(format!("{budgets}/billingAccounts/{billing}/budgets")).bearer_auth(&self.token).json(&json!({
					"displayName": format!("guard {number} {service}"),
					"budgetFilter": {"projects": [format!("projects/{number}")], "services": [format!("services/{service}")], "creditTypesTreatment": "EXCLUDE_ALL_CREDITS", "calendarPeriod": "MONTH"},
					"amount": amount(*a),
					"thresholdRules": [{"thresholdPercent": 0.5}, {"thresholdPercent": 0.8}, {"thresholdPercent": 1.0}],
					"notificationsRule": {"enableProjectLevelRecipients": true},
					"spendCap": {"inputState": "CONFIGURED"},
				})))?;
			}
			Act::Lift { name } => {
				call(
					self.client
						.patch(format!("{budgets}/{name}?updateMask=spendCap.inputState"))
						.bearer_auth(&self.token)
						.json(&json!({"spendCap": {"inputState": "AWAITING_NEXT_PERIOD"}})),
				)?;
			}
			Act::Set { name, amount: a } => {
				call(
					self.client
						.patch(format!("{budgets}/{name}?updateMask=amount.specifiedAmount,spendCap.inputState"))
						.bearer_auth(&self.token)
						.json(&json!({"amount": amount(*a), "spendCap": {"inputState": "CONFIGURED"}})),
				)?;
			}
			Act::Quota { project, api, id, value } => {
				let pref = format!("guard-{}", format!("{api}-{id}").to_lowercase().replace(|c: char| !c.is_ascii_alphanumeric(), "-"));
				call(
					self.client
						.patch(format!("https://cloudquotas.googleapis.com/v1/projects/{project}/locations/global/quotaPreferences/{pref}"))
						.query(&[
							("allowMissing", "true"),
							("ignoreSafetyChecks", "QUOTA_DECREASE_BELOW_USAGE"),
							("ignoreSafetyChecks", "QUOTA_DECREASE_PERCENTAGE_TOO_HIGH"),
						])
						.bearer_auth(&self.token)
						.json(&json!({"service": api, "quotaId": id, "quotaConfig": {"preferredValue": value.to_string()}, "dimensions": {}})),
				)?;
			}
			Act::Unlink { project } => {
				call(
					self.client
						.put(format!("https://cloudbilling.googleapis.com/v1/projects/{project}/billingInfo"))
						.bearer_auth(&self.token)
						.json(&json!({"billingAccountName": ""})),
				)?;
			}
		}
		Ok(())
	}
}

fn read(google: &Google, config: &Config, slices: &[Drawn]) -> Result<Read> {
	let now = Timestamp::now();
	let pacific = TimeZone::get(PACIFIC)?;
	let month = now.to_zoned(pacific.clone()).strftime("%Y%m").to_string();
	let day_began = now.to_zoned(pacific).start_of_day()?.timestamp();
	let services: BTreeMap<String, String> = google
		.list("https://cloudbilling.googleapis.com/v1/services?pageSize=5000", "services")?
		.iter()
		.filter(|s| s["serviceId"].as_str().is_some_and(|id| CAPPABLE.contains(&id)))
		.map(|s| Ok((text(&s["serviceId"])?.to_owned(), text(&s["displayName"])?.to_owned())))
		.collect::<Result<_>>()?;
	ensure!(services.len() == CAPPABLE.len(), "the billing catalog lists {services:?} of the cappable services {CAPPABLE:?}");

	let mut accounts = Vec::new();
	for account in &config.accounts {
		let (project, dataset, billing) = account.export()?;
		let table = format!("{project}.{dataset}.gcp_billing_export_v1_{}", billing.replace('-', "_"));
		let bq = BigQuery::with_token(google.token.clone(), project);

		let mut projects = BTreeMap::new();
		for p in google.list(&format!("https://cloudbilling.googleapis.com/v1/billingAccounts/{billing}/projects"), "projectBillingInfos")? {
			if p["billingEnabled"] == true {
				let id = text(&p["projectId"])?;
				let number = text(&google.get(&format!("https://cloudresourcemanager.googleapis.com/v3/projects/{id}"))?["name"])?
					.trim_start_matches("projects/")
					.parse()?;
				projects.insert(id.to_owned(), number);
			}
		}
		let by_number: BTreeMap<u64, String> = projects.iter().map(|(id, n)| (*n, id.clone())).collect();

		let recent = now.checked_sub((DEMAND_DAYS * 24).hours())?.as_second();
		let gross = bq
			.query(&format!(
				"SELECT project.id, service.id, SUM(IF(invoice.month = '{month}', cost, 0)), SUM(IF(usage_start_time >= TIMESTAMP_SECONDS({recent}), cost, 0))
				 FROM `{table}` WHERE project.id IS NOT NULL AND (invoice.month = '{month}' OR usage_start_time >= TIMESTAMP_SECONDS({recent})) GROUP BY 1, 2"
			))?
			.iter()
			.map(|r| {
				Ok(Gross {
					project: text(&r[0])?.to_owned(),
					service: text(&r[1])?.to_owned(),
					month: num(&r[2])?,
					recent: num(&r[3])?,
				})
			})
			.collect::<Result<_>>()?;
		let real = bq
			.query(&format!(
				"SELECT project.id, service.description, sku.description, SUM(cost + (SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c))
				 FROM `{table}` WHERE invoice.month = '{month}' AND project.id IS NOT NULL GROUP BY 1, 2, 3"
			))?
			.iter()
			.map(|r| {
				Ok(Real {
					project: text(&r[0])?.to_owned(),
					service: text(&r[1])?.to_owned(),
					sku: text(&r[2])?.to_owned(),
					month: num(&r[3])?,
				})
			})
			.collect::<Result<_>>()?;
		let mut drawn = BTreeMap::new();
		for r in bq.query(&format!(
			"SELECT c.id, UNIX_SECONDS(TIMESTAMP_TRUNC(export_time, HOUR)), c.amount FROM `{table}`, UNNEST(credits) c WHERE c.type IN {BALANCES}"
		))? {
			let id = text(&r[0])?.to_owned();
			let read = account.credits.iter().find(|c| c.id.as_deref() == Some(&id)).map(|c| c.read);
			let exported = Timestamp::from_second(num(&r[1])? as i64)?;
			let entry = drawn.entry(id).or_insert(0.0);
			if read.is_none_or(|t| exported >= t) {
				*entry += num(&r[2])?;
			}
		}
		let usd = bq.query(&format!("SELECT ANY_VALUE(currency_conversion_rate HAVING MAX usage_start_time) FROM `{table}`"))?[0][0]
			.as_str()
			.map(str::parse)
			.transpose()?;

		let mut caps = Vec::new();
		for b in google.list(&format!("https://billingbudgets.googleapis.com/v1/billingAccounts/{billing}/budgets"), "budgets")? {
			if b["spendCap"].is_null() {
				continue;
			}
			let filter = &b["budgetFilter"];
			let one = |k: &str| -> Result<&str> {
				let v = filter[k].as_array().ok_or_else(|| eyre!("a spend cap with no {k}: {b}"))?;
				ensure!(v.len() == 1, "a spend cap on {} {k}: {b}", v.len());
				text(&v[0])
			};
			let number: u64 = one("projects")?.trim_start_matches("projects/").parse()?;
			let Some(project) = by_number.get(&number) else {
				continue; // a cap on a project no longer billed here can spend nothing
			};
			let amount = &b["amount"]["specifiedAmount"];
			caps.push(Cap {
				name: text(&b["name"])?.to_owned(),
				project: project.clone(),
				service: one("services")?.trim_start_matches("services/").to_owned(),
				amount: amount["units"].as_str().unwrap_or("0").parse::<f64>()? + amount["nanos"].as_f64().unwrap_or(0.0) / 1e9, // proto3 JSON drops zero fields
				state: text(&b["spendCap"]["outputState"])?.to_owned(),
			});
		}

		let mut quotas = Vec::new();
		let mut apis: BTreeSet<&str> = meters::ALL.iter().map(|m| m.api).collect();
		apis.extend(slices.iter().filter(|s| s.name == meters::SLICE).flat_map(|s| s.closed.iter().map(String::as_str)));
		for api in apis {
			for p in projects.keys() {
				if google.get(&format!("https://serviceusage.googleapis.com/v1/projects/{p}/services/{api}"))?["state"] != "ENABLED" {
					continue; // an API off cannot be used
				}
				let series = google.list(
					&format!(
						"https://monitoring.googleapis.com/v3/projects/{p}/timeSeries?filter={}&interval.startTime={day_began}&interval.endTime={now}&aggregation.alignmentPeriod=86400s&aggregation.perSeriesAligner=ALIGN_SUM&aggregation.crossSeriesReducer=REDUCE_SUM",
						urlencode(&format!("metric.type=\"serviceruntime.googleapis.com/api/request_count\" AND resource.labels.service=\"{api}\""))
					),
					"timeSeries",
				)?;
				let today = series
					.iter()
					.flat_map(|s| s["points"].as_array().cloned().unwrap_or_default())
					.map(|pt| num(&pt["value"]["int64Value"]))
					.sum::<Result<f64>>()? as i64;
				for q in google.list(
					&format!("https://cloudquotas.googleapis.com/v1/projects/{p}/locations/global/services/{api}/quotaInfos"),
					"quotaInfos",
				)? {
					if q["refreshInterval"] != "day" || q["containerType"] != "PROJECT" {
						continue;
					}
					let flat = q["dimensionsInfos"]
						.as_array()
						.into_iter()
						.flatten()
						.find(|d| d["dimensions"].as_object().is_none_or(|o| o.is_empty()));
					let value = flat.ok_or_else(|| eyre!("{p} {api}: a daily quota with no project-wide value: {q}"))?["details"]["value"]
						.as_str()
						.map_or(Ok(-1), str::parse)?; // no value is unlimited
					quotas.push(Quota {
						project: p.clone(),
						api: api.to_owned(),
						id: text(&q["quotaId"])?.to_owned(),
						value,
						today,
					});
				}
			}
		}

		accounts.push(Seen {
			account: account.name.clone(),
			billing,
			usd,
			projects,
			gross,
			real,
			drawn,
			caps,
			quotas,
		});
	}
	Ok(Read {
		now,
		own: env::var("GUARD_PROJECT").wrap_err("GUARD_PROJECT")?,
		services,
		accounts,
	})
}

fn urlencode(s: &str) -> String {
	s.bytes()
		.map(|b| {
			if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
				(b as char).to_string()
			} else {
				format!("%{b:02X}")
			}
		})
		.collect()
}
