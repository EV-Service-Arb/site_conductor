//! Holds every enabled API on every billed project of an account by a lever set before it spends, or leaves it to the
//! account's `unguarded`; anything else it disables. Each slice's real money is held to its `allow`, a day's and a
//! month's. What an account is held to is the definition the conductor last published beside its export.
//! docs/ARCHITECTURE.md, The guard; docs/spec/google.md.
//!
//!     guard tick [--dry-run]   read every account, set its levers, record what it saw in `<export dataset>.guard`
//!     guard read               print what a tick reads, as JSON
//!     guard plan <read.json>   what a tick on that reading would set
//!
//! An account failing is reported, and the others held all the same.
//!
//! Env: GUARD_EXPORTS, every account's export table as a JSON list; GUARD_PROJECT, the project it runs in;
//! GOOGLE_OAUTH_ACCESS_TOKEN, else the metadata server's.

use std::{
	collections::{BTreeMap, BTreeSet},
	env, fs,
};

mod list;

use eyre::{Result, WrapErr, bail, ensure, eyre};
use jiff::{Timestamp, ToSpan, tz::TimeZone};
use list::{BigQuery, call, export, num, text};
use sa_core::{
	google::{Allow, BALANCES, Definition, Hold},
	meters,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Services Google takes spend caps on: their billing service id, and the API each is asked through.
const CAPPABLE: [(&str, &str); 4] = [
	("AEFD-7695-64FA", "generativelanguage.googleapis.com"), // Gemini API
	("C7E2-9256-1C43", "aiplatform.googleapis.com"),         // Vertex AI
	("152E-C115-5142", "run.googleapis.com"),                // Cloud Run
	("29E7-DA93-CA13", "cloudfunctions.googleapis.com"),     // Cloud Run functions
];
/// Google's own APIs that bill nothing.
const FREE: [&str; 11] = [
	"apikeys.googleapis.com",
	"billingbudgets.googleapis.com",
	"cloudapis.googleapis.com",
	"cloudbilling.googleapis.com",
	"cloudquotas.googleapis.com",
	"cloudresourcemanager.googleapis.com",
	"iam.googleapis.com",
	"iamcredentials.googleapis.com",
	"servicemanagement.googleapis.com",
	"serviceusage.googleapis.com",
	"youtube.googleapis.com",
];
/// What the guard runs and reads through on its own project; an export is read through `EXPORT` on the export's.
const OWN: [&str; 5] = [
	"artifactregistry.googleapis.com",
	"bigquery.googleapis.com",
	"cloudscheduler.googleapis.com",
	"monitoring.googleapis.com",
	"run.googleapis.com",
];
const EXPORT: &str = "bigquery.googleapis.com";
const MARGIN: f64 = 0.10; // real money past a slice's allowance that unlinks billing; below it is the export's rounding
const DEMAND_DAYS: i64 = 30;
const PACIFIC: &str = "America/Los_Angeles"; // Google's billing month and quota day

fn main() -> Result<()> {
	rustls::crypto::ring::default_provider().install_default().expect("the first thing main does");
	let args: Vec<String> = env::args().skip(1).collect();
	let exports = || -> Result<Vec<String>> { serde_json::from_str(&env::var("GUARD_EXPORTS").wrap_err("GUARD_EXPORTS")?).wrap_err("GUARD_EXPORTS") };
	let mut failed = Vec::new();
	match &args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
		["plan", file] => {
			let read: Read = serde_json::from_str(&fs::read_to_string(file)?)?;
			for (seen, planned) in read.accounts.iter().zip(plan(&read)) {
				match planned {
					Ok((rows, acts)) => print(&seen.account, &rows, &acts),
					Err(e) => failed.push(e),
				}
			}
		}
		["read"] => {
			let read = read(&Google::try_new()?, &exports()?, &mut failed)?;
			println!("{}", serde_json::to_string_pretty(&read)?);
		}
		["tick", rest @ ..] => {
			let dry = match rest {
				[] => false,
				["--dry-run"] => true,
				_ => bail!("usage: guard tick [--dry-run]"),
			};
			let google = Google::try_new()?;
			let read = read(&google, &exports()?, &mut failed)?;
			for (seen, planned) in read.accounts.iter().zip(plan(&read)) {
				let (rows, acts) = match planned {
					Ok(planned) => planned,
					Err(e) => {
						failed.push(e);
						continue;
					}
				};
				print(&seen.account, &rows, &acts);
				if !dry && let Err(e) = hold(&google, &read, seen, &rows, &acts) {
					failed.push(e.wrap_err(seen.account.clone()));
				}
			}
		}
		_ => bail!("usage: guard tick [--dry-run] | read | plan <read.json>"),
	}
	match failed.as_slice() {
		[] => Ok(()),
		_ => bail!("{}", failed.iter().map(|e| format!("{e:#}")).collect::<Vec<_>>().join("\n")),
	}
}

/// `acts` set at Google, then `rows` recorded beside the account's export.
fn hold(google: &Google, read: &Read, seen: &Seen, rows: &[Row], acts: &[Act]) -> Result<()> {
	for act in acts {
		google.act(act).wrap_err_with(|| format!("{act:?}"))?;
	}
	let (project, dataset, _) = export(&seen.definition.held.table)?;
	let list = seen.definition.digest();
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
	ensure!(got["insertErrors"].is_null(), "recording the tick: {got}");
	Ok(())
}

#[derive(Deserialize, Serialize)]
struct Read {
	now: Timestamp,
	own: String,
	services: BTreeMap<String, String>, // cappable id -> display name
	exports: Vec<String>,               // every account's, read or not
	accounts: Vec<Seen>,
}

#[derive(Deserialize, Serialize)]
struct Seen {
	account: String,
	definition: Definition, // as the conductor last published it
	billing: String,
	usd: Option<f64>,                       // the account's currency per USD, off the export
	projects: BTreeMap<String, u64>,        // linked and billed, id -> number
	enabled: BTreeMap<String, Vec<String>>, // project -> its APIs on
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
	sku_id: String,
	month: f64,           // after every credit and discount
	today: f64,           // the same, since the Pacific day began
	free_to: Option<f64>, // where its first priced tier starts, once reached this month
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
	Disable {
		project: String,
		api: String,
	},
	Unlink {
		project: String,
	},
}

/// How an enabled API is held.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Class {
	/// Named in the account's `unguarded`: left alone.
	Unguarded,
	/// Bills nothing.
	Free,
	/// Under a spend cap on this billing service.
	Capped(&'static str),
	/// `sa_core::meters` prices it: its daily quotas pace its slice's free units and what the allowance buys.
	Metered,
	/// Daily quotas but no price: held at none.
	Quotad,
	/// Nothing holds it: disabled.
	Unheld,
}

/// `api`'s class where its quotas do not decide it; `None`, they do.
fn class(api: &str, hold: &Hold) -> Option<Class> {
	if hold.unguarded.iter().any(|u| u == api) {
		Some(Class::Unguarded)
	} else if FREE.contains(&api) {
		Some(Class::Free)
	} else {
		CAPPABLE.iter().find(|(_, a)| *a == api).map(|(id, _)| Class::Capped(id))
	}
}

#[derive(Serialize)]
struct Row {
	account: String,
	category: Option<String>, // the slice, or the service no slice draws; none for an API no slice holds
	project: Option<String>,
	lever: String,
	allow_day: Option<f64>,
	allow_month: Option<f64>,
	real_day: Option<f64>,
	real_month: Option<f64>,
	setting: Option<f64>,
	state: Option<String>,
	today: Option<i64>,
	action: Option<String>,
}

impl Row {
	fn lever(account: &str, category: Option<String>, project: &str, lever: String) -> Self {
		Self {
			account: account.to_owned(),
			category,
			project: Some(project.to_owned()),
			lever,
			allow_day: None,
			allow_month: None,
			real_day: None,
			real_month: None,
			setting: None,
			state: None,
			today: None,
			action: None,
		}
	}
}

/// Every lever's setting to stderr, what would change to stdout.
fn print(account: &str, rows: &[Row], acts: &[Act]) {
	let n = |x: Option<f64>| x.map_or("-".to_owned(), |x| format!("{:.2}", x + 0.0)); // + 0.0: no "-0.00"
	for r in rows {
		eprintln!(
			"{account}\t{}\t{}\t{}\tallow {}/{}\treal {}/{}\tset {}\tis {}",
			r.category.as_deref().unwrap_or("-"),
			r.project.as_deref().unwrap_or("-"),
			r.lever,
			n(r.allow_day),
			n(r.allow_month),
			n(r.real_day),
			n(r.real_month),
			n(r.setting),
			r.state.as_deref().unwrap_or("-")
		);
	}
	for a in acts {
		println!("{account}\t{a:?}");
	}
}

/// Each account's levers' settings, and what of them differs from what was read.
fn plan(read: &Read) -> Vec<Result<(Vec<Row>, Vec<Act>)>> {
	read.accounts
		.iter()
		.map(|seen| {
			let pacific = TimeZone::get(PACIFIC)?;
			let today = read.now.to_zoned(pacific).date();
			// what the guard runs and reads through; capped, zeroed or disabled it would hold nothing
			let mut inputs: BTreeSet<(&str, &str)> = OWN.iter().map(|api| (read.own.as_str(), *api)).collect();
			for table in &read.exports {
				inputs.insert((export(table)?.0, EXPORT));
			}
			let definition = &seen.definition;
			let account = &definition.held;
			let slices = &sa_core::google::drawn(&definition.slices).map_err(|e| eyre!("{}: its slices: {}", seen.account, e.0))?;
			account.hold.check(slices).map_err(|e| eyre!("{}: {}", seen.account, e.0))?;
			// a category no slice is, a service no slice draws, is allowed no real money
			let allow = |c: &str| {
				if slices.iter().any(|s| s.name == c) {
					account.hold.allow[c]
				} else {
					Allow { day: 0.0, month: 0.0 }
				}
			};
			let configured: BTreeSet<&str> = account.credits.iter().filter_map(|c| c.id.as_deref()).collect();
			let unknown: Vec<&String> = seen.drawn.keys().filter(|id| !configured.contains(id.as_str())).collect();
			ensure!(
				unknown.is_empty(),
				"{}: the export draws on credits {unknown:?} with no `credits` entry in nix/google.nix",
				seen.account
			);
			let enabled = |p: &str, api: &str| seen.enabled.get(p).is_some_and(|on| on.iter().any(|a| a == api));
			let exposed: Vec<String> = inputs
				.iter()
				.filter(|(p, api)| seen.projects.contains_key(*p) && enabled(p, api) && !class(api, &account.hold).is_some_and(|c| c == Class::Unguarded || c == Class::Free))
				.map(|(p, api)| format!("{api} on {p}"))
				.collect();
			ensure!(
				exposed.is_empty(),
				"{}: the guard runs or reads through {}, which it must never cap, zero or disable: name each in the account's `unguarded`",
				seen.account,
				exposed.join(", ")
			);
			// r[impl google.guarded]
			let mut classes: Vec<(&str, &str, Class)> = Vec::new();
			for (p, apis) in &seen.enabled {
				ensure!(seen.projects.contains_key(p), "{}: {p}'s APIs read, but it is not billed here", seen.account);
				for api in apis {
					let quoted = seen.quotas.iter().any(|q| &q.project == p && &q.api == api);
					let c = class(api, &account.hold).unwrap_or(match () {
						() if quoted && meters::pace(api).is_some() => Class::Metered,
						() if quoted => Class::Quotad,
						() => Class::Unheld,
					});
					classes.push((p, api, c));
				}
			}
			for q in &seen.quotas {
				ensure!(
					classes.iter().any(|(p, api, c)| *p == q.project && *api == q.api && matches!(c, Class::Metered | Class::Quotad)),
					"{}: a quota read on {} {}, which its daily quotas do not hold",
					seen.account,
					q.project,
					q.api
				);
			}
			// the meters must carry each metered API's least-free SKU, or its pace lets one past its free units
			for r in seen.real.iter().filter(|r| meters::ALL.iter().all(|m| m.id != r.sku_id)) {
				let Some(free_to) = r.free_to else {
					continue; // no priced tier reached: nothing tells its free units
				};
				for m in meters::ALL.iter().filter(|m| seen.real.iter().any(|o| o.service == r.service && o.sku_id == m.id)) {
					let (free, _) = meters::pace(m.api).expect("a meter's own API");
					ensure!(
						free_to >= free as f64,
						"{}: {} on {} is free to {free_to}, below the {free} {} is paced to: sa_core::meters must carry it",
						seen.account,
						r.sku,
						r.service,
						m.api
					);
				}
			}
			// a credit gone a day early: Google's day ends in Pacific time, and a credit gone a day early only caps sooner
			let tomorrow = today.checked_add(1.day())?;
			let credits: f64 = account
				.credits
				.iter()
				.map(|c| c.left_on(c.id.as_ref().map_or(0.0, |id| seen.drawn.get(id).copied().unwrap_or(0.0)), tomorrow))
				.sum();

			// real money by category and project: (this month, today)
			let mut real: BTreeMap<String, BTreeMap<&str, (f64, f64)>> = BTreeMap::new();
			for r in &seen.real {
				let category = sa_core::google::slice_of(slices, &r.service, &r.sku);
				let at = real.entry(category).or_default().entry(r.project.as_str()).or_default();
				at.0 += r.month;
				at.1 += r.today;
			}
			for s in slices {
				real.entry(s.name.clone()).or_default();
			}
			let spent = |c: &str| real.get(c).map_or((0.0, 0.0), |by| by.values().fold((0.0, 0.0), |s, x| (s.0 + x.0, s.1 + x.1)));
			// what a slice may still spend today: its day's allowance, within what is left of its month's
			let left = |c: &str| {
				let (a, (month, day)) = (allow(c), spent(c));
				(a.day - day).min(a.month - month).max(0.0)
			};

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

			let mut disables = Vec::new(); // last: one Google refuses (another API on depends on it) leaves every lever set
			for (p, api, c) in &classes {
				match c {
					Class::Unguarded | Class::Free => rows.push(Row::lever(&seen.account, None, p, format!("{}: {api}", if *c == Class::Free { "free" } else { "unguarded" }))),
					Class::Unheld => {
						let act = Act::Disable {
							project: (*p).to_owned(),
							api: (*api).to_owned(),
						};
						rows.push(Row {
							action: Some(format!("{act:?}")),
							..Row::lever(&seen.account, None, p, format!("disable: {api}"))
						});
						disables.push(act);
					}
					Class::Capped(_) | Class::Metered | Class::Quotad => {}
				}
			}

			// caps: one per billed project and capped service on it
			let pairs: Vec<(&str, &str)> = classes
				.iter()
				.filter_map(|(p, _, c)| match c {
					Class::Capped(s) => Some((*p, *s)),
					_ => None,
				})
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
			let name = |s: &str| read.services.get(s).ok_or_else(|| eyre!("the billing catalog lists no {s}"));
			// what a slice may still spend spreads evenly over every cap it draws through
			let mut draws_through: BTreeMap<String, f64> = BTreeMap::new();
			for (_, s) in &pairs {
				for c in categories(name(s)?) {
					*draws_through.entry(c).or_default() += 1.0;
				}
			}
			for (p, s) in &pairs {
				let name = name(s)?;
				let cats = categories(name);
				let allowed: f64 = cats.iter().map(|c| left(c) / draws_through[c]).sum();
				let target = cents(gross(p, s).map_or(0.0, |g| g.month) + if demand.contains(&(*p, *s)) { share } else { 0.0 } + allowed);
				let existing = cap(p, s)?;
				let act = match existing {
					None => Some(Act::Create {
						billing: seen.billing.clone(),
						project: (*p).to_owned(),
						number: seen.projects[*p],
						service: (*s).to_owned(),
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
						setting: Some(target),
						state: existing.map(|c| c.state.clone()),
						action: action.clone(),
						..Row::lever(&seen.account, Some(c), p, format!("cap: {name}"))
					});
				}
			}

			// daily quotas: a metered API's pace its meters' least free units over 31 days, and what the slice may still
			// spend today at their dearest list price; an unpriced one's, none
			let metered: BTreeSet<(&str, &str)> = classes.iter().filter(|(_, _, c)| *c == Class::Metered).map(|(p, a, _)| (*p, *a)).collect();
			for q in &seen.quotas {
				let value = match meters::pace(&q.api) {
					Some((free, price)) if metered.contains(&(q.project.as_str(), q.api.as_str())) => {
						let bought = match left(meters::SLICE) / metered.len() as f64 {
							0.0 => 0.0,
							money =>
								money
									/ (price as f64 / 1e6
										* seen
											.usd
											.ok_or_else(|| eyre!("{}: no exchange rate in the export, to price {}'s allowance", seen.account, meters::SLICE))?),
						};
						let projects = metered.iter().filter(|(_, a)| *a == q.api).count() as f64; // the free units are the account's
						let k = seen.quotas.iter().filter(|o| o.project == q.project && o.api == q.api).count() as f64; // each daily quota of the API may be spent in full
						((free as f64 / 31.0 / projects + bought) / k).floor() as i64
					}
					_ => 0,
				};
				let act = (q.value != value).then(|| Act::Quota {
					project: q.project.clone(),
					api: q.api.clone(),
					id: q.id.clone(),
					value,
				});
				rows.push(Row {
					setting: Some(value as f64),
					state: Some(q.value.to_string()),
					today: Some(q.today),
					action: act.as_ref().map(|a| format!("{a:?}")),
					..Row::lever(
						&seen.account,
						metered.contains(&(q.project.as_str(), q.api.as_str())).then(|| meters::SLICE.to_owned()),
						&q.project,
						format!("quota: {} {}", q.api, q.id),
					)
				});
				acts.extend(act);
			}

			// the backstop: a slice past its month's allowance, or its day's, takes billing off every project it spent real money in
			let mut unlink = BTreeSet::new();
			for (category, by_project) in &real {
				let (a, (month, day)) = (allow(category), spent(category));
				let over = month > a.month + MARGIN || day > a.day + MARGIN;
				if over {
					unlink.extend(
						by_project
							.iter()
							.filter(|(_, m)| m.0 > 0.0 || m.1 > 0.0)
							.map(|(p, _)| *p)
							.filter(|p| seen.projects.contains_key(*p)),
					);
				}
				rows.push(Row {
					account: seen.account.clone(),
					category: Some(category.clone()),
					project: None,
					lever: "real money".to_owned(),
					allow_day: Some(a.day),
					allow_month: Some(a.month),
					real_day: Some(day),
					real_month: Some(month),
					setting: None,
					state: Some(if over { "over" } else { "within" }.to_owned()),
					today: None,
					action: over.then(|| format!("unlink {}", by_project.keys().copied().collect::<Vec<_>>().join(", "))),
				});
			}
			acts.extend(unlink.into_iter().map(|p| Act::Unlink { project: p.to_owned() }));
			acts.extend(disables);
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
			Act::Disable { project, api } => {
				call(
					self.client
						.post(format!("https://serviceusage.googleapis.com/v1/projects/{project}/services/{api}:disable"))
						.bearer_auth(&self.token)
						.json(&json!({"disableDependentServices": false, "checkIfServiceHasUsage": "SKIP"})),
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

/// Every account of `exports` that reads whole; one that does not is in `failed`.
fn read(google: &Google, exports: &[String], failed: &mut Vec<eyre::Report>) -> Result<Read> {
	let now = Timestamp::now();
	let pacific = TimeZone::get(PACIFIC)?;
	let month = now.to_zoned(pacific.clone()).strftime("%Y%m").to_string();
	let day_began = now.to_zoned(pacific).start_of_day()?.timestamp();
	let services: BTreeMap<String, String> = google
		.list("https://cloudbilling.googleapis.com/v1/services?pageSize=5000", "services")?
		.iter()
		.filter(|s| s["serviceId"].as_str().is_some_and(|id| CAPPABLE.iter().any(|(c, _)| *c == id)))
		.map(|s| Ok((text(&s["serviceId"])?.to_owned(), text(&s["displayName"])?.to_owned())))
		.collect::<Result<_>>()?;
	ensure!(services.len() == CAPPABLE.len(), "the billing catalog lists {services:?} of the cappable services {CAPPABLE:?}");

	let mut accounts = Vec::new();
	for table in exports {
		match seen(google, table, now, &month, day_began) {
			Ok(s) => accounts.push(s),
			Err(e) => failed.push(e.wrap_err(table.clone())),
		}
	}
	Ok(Read {
		now,
		own: env::var("GUARD_PROJECT").wrap_err("GUARD_PROJECT")?,
		services,
		exports: exports.to_vec(),
		accounts,
	})
}

/// What a tick reads of the account exported to `table`.
fn seen(google: &Google, table: &str, now: Timestamp, month: &str, day_began: Timestamp) -> Result<Seen> {
	{
		let (project, dataset, billing) = export(table)?;
		let bq = BigQuery::with_token(google.token.clone(), project);
		let published = bq.query(&format!("SELECT definition, digest FROM `{project}.{dataset}.definition` ORDER BY at DESC LIMIT 1"))?;
		let [row] = published.as_slice() else {
			bail!("the conductor has published no definition beside it: set its hold in the admin's meters card");
		};
		let definition: Definition = serde_json::from_str(text(&row[0])?).wrap_err("its definition, as published")?;
		ensure!(
			definition.digest() == text(&row[1])?,
			"its definition does not hash to the digest published with it: the conductor's sa_core::google is not the guard's"
		);
		ensure!(definition.held.table == table, "its definition is {}'s", definition.held.table);
		let account = &definition.held;

		let mut projects = BTreeMap::new();
		for p in google.list(&format!("https://cloudbilling.googleapis.com/v1/billingAccounts/{billing}/projects"), "projectBillingInfo")? {
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
		let day = day_began.as_second();
		let net = "cost + (SELECT IFNULL(SUM(c.amount), 0) FROM UNNEST(credits) c)";
		let real = bq
			.query(&format!(
				"SELECT project.id, service.description, sku.description, sku.id, SUM(IF(invoice.month = '{month}', {net}, 0)),
				 SUM(IF(usage_start_time >= TIMESTAMP_SECONDS({day}), {net}, 0)), MIN(IF(invoice.month = '{month}' AND price.effective_price > 0, price.tier_start_amount, NULL))
				 FROM `{table}` WHERE project.id IS NOT NULL AND (invoice.month = '{month}' OR usage_start_time >= TIMESTAMP_SECONDS({day})) GROUP BY 1, 2, 3, 4"
			))?
			.iter()
			.map(|r| {
				Ok(Real {
					project: text(&r[0])?.to_owned(),
					service: text(&r[1])?.to_owned(),
					sku: text(&r[2])?.to_owned(),
					sku_id: text(&r[3])?.to_owned(),
					month: num(&r[4])?,
					today: num(&r[5])?,
					free_to: if r[6].is_null() { None } else { Some(num(&r[6])?) },
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

		let mut enabled = BTreeMap::new();
		let mut quotas = Vec::new();
		for p in projects.keys() {
			let apis: Vec<String> = google
				.list(
					&format!("https://serviceusage.googleapis.com/v1/projects/{p}/services?filter=state:ENABLED&pageSize=200"),
					"services",
				)?
				.iter()
				.map(|s| Ok(text(&s["config"]["name"])?.to_owned()))
				.collect::<Result<_>>()?;
			let open: Vec<&String> = apis.iter().filter(|api| class(api, &account.hold).is_none()).collect(); // held by their daily quotas, if any
			if !open.is_empty() {
				let series = google.list(
					&format!(
						"https://monitoring.googleapis.com/v3/projects/{p}/timeSeries?filter={}&interval.startTime={day_began}&interval.endTime={now}&aggregation.alignmentPeriod=86400s&aggregation.perSeriesAligner=ALIGN_SUM&aggregation.crossSeriesReducer=REDUCE_SUM&aggregation.groupByFields=resource.labels.service",
						urlencode("metric.type=\"serviceruntime.googleapis.com/api/request_count\"")
					),
					"timeSeries",
				)?;
				let mut today: BTreeMap<&str, i64> = BTreeMap::new();
				for s in &series {
					let n = s["points"].as_array().into_iter().flatten().map(|pt| num(&pt["value"]["int64Value"])).sum::<Result<f64>>()? as i64;
					*today.entry(text(&s["resource"]["labels"]["service"])?).or_default() += n;
				}
				for api in open {
					for q in google.list(
						&format!("https://cloudquotas.googleapis.com/v1/projects/{p}/locations/global/services/{api}/quotaInfos"),
						"quotaInfos",
					)? {
						if q["refreshInterval"] != "day" || q["containerType"] != "PROJECT" {
							continue;
						}
						let id = text(&q["quotaId"])?;
						if quotas.iter().any(|o: &Quota| &o.project == p && &o.api == api && o.id == id) {
							continue; // one id over several metrics: a preference sets it once
						}
						let Some(flat) = q["dimensionsInfos"]
							.as_array()
							.into_iter()
							.flatten()
							.find(|d| d["dimensions"].as_object().is_none_or(|o| o.is_empty()))
						else {
							continue; // per region or the like: no project-wide value to hold
						};
						quotas.push(Quota {
							project: p.clone(),
							api: api.clone(),
							id: id.to_owned(),
							value: flat["details"]["value"].as_str().map_or(Ok(-1), str::parse)?, // no value is unlimited
							today: today.get(api.as_str()).copied().unwrap_or(0),                 // no series: no request today
						});
					}
				}
			}
			enabled.insert(p.clone(), apis);
		}

		Ok(Seen {
			account: account.name.clone(),
			definition: definition.clone(),
			billing,
			usd,
			projects,
			enabled,
			gross,
			real,
			drawn,
			caps,
			quotas,
		})
	}
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
