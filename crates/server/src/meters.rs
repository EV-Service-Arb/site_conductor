//! The vendors' meters (`sa_panel::meters`) and each account's bill: in `serve` each read at start
//! and hourly, and their use for the Admin page and Grafana.
//!
//! ```text
//! GET /api/v1/admin/meters                     Meters: each meter's use this period, all accounts and each;
//!                                              each account's credits and allowances, as the guard holds them
//! GET /api/v1/admin/meters/readings?from=&to=  MeterReadings: every reading synced, Unix ms
//! GET /api/v1/admin/meters/bill?from=&to=      Bill: each account's days in the window, by slice and SKU,
//!                                              and this month's tiers
//! ```

use std::{sync::Mutex, time::Duration};

use axum::{Json, extract::State, http::Method};
use ev_lib::api::{JsonSchema, Params, Refusal};
use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_auth::Tokens;
use sa_core::{
	meters::{ALL, Meter, Stop, Vendor},
	prices::Item,
};
use sa_panel::{
	Panel,
	meters::{Bill, Gauge, Reader, Synced},
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
	api::{ApiResult, Code},
	http::{ApiRoute, USER},
};

/// How often a reading is due: each scans under a megabyte of an export partitioned by day.
const SYNC_EVERY: SignedDuration = SignedDuration::from_hours(1);
/// How soon a failed read is tried again.
const RETRY: Duration = Duration::from_secs(15 * 60);
/// The widest window `/admin/meters/readings` reads.
const SPAN: SignedDuration = SignedDuration::from_hours(24 * 400);

/// How many days of draws set a credit's pace.
const BURN_DAYS: i64 = 14;

type Standing = (Vec<CreditDto>, Vec<String>, Vec<AllowanceDto>, Option<GuardDto>);

/// Reads each account's meters now, then every SYNC_EVERY, a failure again after RETRY, until
/// `shutdown`, each on its own: one failing leaves the others be, and its last good reading in
/// force meanwhile.
pub async fn run(panel: Panel, readers: Vec<impl Reader + Sync>, shutdown: watch::Receiver<bool>) {
	let lasts: Vec<Mutex<Option<Timestamp>>> = readers.iter().map(|_| Mutex::new(None)).collect();
	let panel = &panel;
	futures::future::join_all(readers.iter().zip(&lasts).map(|(reader, last)| {
		crate::every(RETRY, shutdown.clone(), "reading an account's meters", move || async move {
			let now = Timestamp::now();
			if last
				.lock()
				.expect("never poisoned: nothing panics holding it")
				.is_some_and(|at| now.duration_since(at) < SYNC_EVERY)
			{
				return Ok(());
			}
			let n = panel.sync_meters(reader, now).await.wrap_err_with(|| format!("{}'s meters", reader.account()))?;
			tracing::info!(meters = n, vendor = ?reader.vendor(), account = reader.account(), "meters read");
			*last.lock().expect("never poisoned: nothing panics holding it") = Some(now);
			Ok(())
		})
	}))
	.await;
}

/// The vendor's count of a meter, as last read.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterReading")]
pub struct ReadingDto {
	/// The billing account's name.
	pub account: String,
	/// Its id, `<vendor>.<name>`.
	#[schemars(with = "String")]
	pub meter: &'static Meter,
	/// RFC 3339: the period it counts from.
	pub period_start: String,
	pub units: i64,
	/// Gross, before credits, in `currency`.
	pub cost: f64,
	/// After credits, in `currency`.
	pub net: f64,
	pub currency: String,
	/// RFC 3339: how far the vendor's record reaches.
	pub through: String,
	/// RFC 3339.
	pub synced_at: String,
}

/// A meter's use this period on one account: `used` is the vendor's `baseline` plus our `delta`
/// on its keys after `since`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterAccountUse")]
pub struct AccountUseDto {
	pub account: String,
	/// The account's latest reading, of whichever period; `null` before a first one.
	pub reading: Option<ReadingDto>,
	pub baseline: i64,
	/// RFC 3339: ours are counted after it.
	pub since: String,
	pub delta: i64,
	pub used: i64,
	/// Of the account's free units.
	pub left: i64,
}

/// A meter's use this period, summed over the accounts.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterTotal")]
pub struct TotalDto {
	pub baseline: i64,
	pub delta: i64,
	pub used: i64,
	/// Every account's free units.
	pub free: i64,
	pub left: i64,
}

/// A meter's use this period: all accounts, and each.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterUse")]
pub struct MeterDto {
	/// Its id, `<vendor>.<name>`.
	#[schemars(with = "String")]
	pub meter: &'static Meter,
	pub vendor: Vendor,
	pub name: &'static str,
	/// Free units a period, on each account.
	pub free: i64,
	pub stop: Stop,
	pub items: Vec<Item>,
	/// RFC 3339, `[start, end)`.
	pub period_start: String,
	pub period_end: String,
	pub total: TotalDto,
	/// The total `delta` at the admin's cost of a unit; `null` while its items' costs are unknown or differ.
	pub delta_usd: Option<f64>,
	/// In the config's order.
	pub accounts: Vec<AccountUseDto>,
}

/// A billing account's spend with Google this period, as its latest readings tell.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AccountSpend")]
pub struct SpendDto {
	/// Gross, in `currency`.
	pub cost: f64,
	/// After credits: what `allow` is held against.
	pub net: f64,
	pub currency: String,
	/// RFC 3339: the earliest its readings reach.
	pub through: String,
}

/// A Google billing account, as configured.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterAccount")]
pub struct AccountDto {
	pub name: String,
	/// Whether it has a Places key, and a browser key.
	pub places: bool,
	pub browser: bool,
	/// Money a month past its free units and credits, in its currency.
	pub allow: f64,
	/// `null` before a reading of this period.
	pub spend: Option<SpendDto>,
	/// Each configured credit's balance.
	pub credits: Vec<CreditDto>,
	/// Credits the export draws on that the config names not.
	pub unnamed_credits: Vec<String>,
	/// Each slice's real money against its allowance, and its levers, at the guard's last tick.
	pub allowances: Vec<AllowanceDto>,
	/// The guard's last tick; `null` before it ticked, or before the bill was read.
	pub guard: Option<GuardDto>,
	/// RFC 3339: when the bill was last read; `null`, never.
	pub bill_read: Option<String>,
}

/// A credit's balance, in its account's currency.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterCredit")]
pub struct CreditDto {
	pub name: String,
	/// The console's reading less what the export drew since.
	pub left: f64,
	/// RFC 3339: the later of the console's reading and the bill's.
	pub as_of: String,
	/// At the last 14 days' pace; `null` before any draw.
	pub per_day: Option<f64>,
	/// The day it is spent at that pace; `null`, never.
	pub runs_out: Option<String>,
	pub ends: String,
	/// To whichever comes first.
	pub days_left: f64,
	/// `spent`, `ends` or `ended`.
	pub binds: String,
}

/// A slice's allowance as the guard held it.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterAllowance")]
pub struct AllowanceDto {
	pub slice: String,
	/// Real money a month, past credits and free tiers.
	pub allowed: f64,
	/// What it spent of that this month.
	pub real: f64,
	pub headroom: f64,
	/// `within` or `over`.
	pub state: String,
	pub levers: Vec<LeverDto>,
}

/// One lever the guard set.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterLever")]
pub struct LeverDto {
	pub project: Option<String>,
	pub lever: String,
	pub setting: Option<f64>,
	/// What it was before.
	pub state: Option<String>,
	pub today: Option<i64>,
	/// What the tick changed.
	pub action: Option<String>,
}

/// The guard's last tick on an account.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterGuard")]
pub struct GuardDto {
	/// RFC 3339.
	pub tick: String,
	/// Whether it held the definition this conductor runs (`sa_core::google::digest`).
	pub holds_this_list: bool,
}

/// A day's use of one SKU on an account, in its currency.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "BillDay")]
pub struct DayDto {
	pub account: String,
	/// RFC 3339, the day's midnight UTC.
	pub day: String,
	pub slice: String,
	pub service: String,
	pub sku: String,
	pub currency: String,
	/// At list price.
	pub used: f64,
	/// What credits' balances paid of it.
	pub credit: f64,
	/// What discounts took off.
	pub discount: f64,
	/// What was billed.
	pub paid: f64,
}

/// An SKU's units this invoice month on an account.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "BillTier")]
pub struct TierDto {
	pub account: String,
	pub service: String,
	pub sku: String,
	pub unit: Option<String>,
	pub units: f64,
	/// Where its first priced tier starts; `null`, none reached.
	pub free_to: Option<f64>,
	pub paid_units: f64,
	/// At list price, in the account's currency.
	pub list: f64,
	/// What free tiers and discounts took off.
	pub free: f64,
}

/// `GET /admin/meters/bill`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Bill")]
pub struct BillDto {
	/// In the window, oldest first.
	pub days: Vec<DayDto>,
	/// Each account's latest invoice month.
	pub tiers: Vec<TierDto>,
}

/// `GET /admin/meters`: every meter, in the catalog's order.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Meters")]
pub struct MetersDto {
	/// In the config's order.
	pub accounts: Vec<AccountDto>,
	pub meters: Vec<MeterDto>,
}

/// `GET /admin/meters/readings`: oldest first.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterReadings")]
pub struct ReadingsDto {
	pub readings: Vec<ReadingDto>,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let admin = USER.needs(Tokens::Grant);
	vec![
		ApiRoute::new(M::GET, "/admin/meters", admin, meters),
		ApiRoute::new(M::GET, "/admin/meters/readings", admin, meter_readings),
		ApiRoute::new(M::GET, "/admin/meters/bill", admin, bill),
	]
}

impl From<Synced> for ReadingDto {
	fn from(s: Synced) -> Self {
		let r = s.reading;
		Self {
			account: r.account,
			meter: r.meter,
			period_start: r.period_start.to_string(),
			units: r.units,
			cost: r.cost_micros as f64 / 1e6,
			net: r.net_micros as f64 / 1e6,
			currency: r.currency,
			through: r.through.to_string(),
			synced_at: s.synced_at.to_string(),
		}
	}
}

/// Every vendor meter this period, summed over the accounts and on each: the vendor's own count
/// at its last reading, ours since, what is left of the free units and when buying stops; and each
/// account's spend against its allowance.
pub(crate) async fn meters(State(panel): State<Panel>) -> ApiResult<Json<MetersDto>> {
	let now = Timestamp::now();
	let prices = panel.prices().await?;
	let digest = panel.accounts().digest();
	let mut accounts = Vec::with_capacity(panel.accounts().len());
	for (a, bill) in panel.bills().await? {
		let spend = panel.spend(&a.name, Vendor::Google, now).await?;
		let (credits, unnamed_credits, allowances, guard) = match &bill {
			None => (Vec::new(), Vec::new(), Vec::new(), None),
			Some((bill, read_at)) => standing(a, bill, *read_at, &digest, now)?,
		};
		accounts.push(AccountDto {
			credits,
			unnamed_credits,
			allowances,
			guard,
			bill_read: bill.as_ref().map(|(_, at)| at.to_string()),
			name: a.name.clone(),
			places: a.provides.places.is_some(),
			browser: a.provides.browser.is_some(),
			allow: sa_panel::meters::allowance(a),
			spend: spend.map(|s| SpendDto {
				cost: s.cost_micros as f64 / 1e6,
				net: s.net_micros as f64 / 1e6,
				currency: s.currency,
				through: s.through.to_string(),
			}),
		});
	}
	let meters = ALL
		.iter()
		.zip(panel.meters(now).await?)
		.map(|(meter, gauges): (&'static Meter, Vec<Gauge>)| {
			let items: Vec<Item> = meter.items().collect();
			let costs: Vec<i64> = items
				.iter()
				.map(|i| prices.iter().find(|p| p.item == *i).expect("the catalog prices every item").cost.usd_micros)
				.collect();
			let sum = |f: fn(&Gauge) -> i64| gauges.iter().map(f).sum::<i64>();
			let total = TotalDto {
				baseline: sum(|g| g.baseline),
				delta: sum(|g| g.delta),
				used: sum(Gauge::used),
				free: meter.free * gauges.len() as i64,
				left: sum(Gauge::left),
			};
			let delta_usd = match costs.as_slice() {
				[c, rest @ ..] if rest.iter().all(|r| r == c) => Some((total.delta * c) as f64 / 1e6),
				_ => None,
			};
			let (start, end) = meter.vendor.period(now);
			MeterDto {
				meter,
				vendor: meter.vendor,
				name: meter.name,
				free: meter.free,
				stop: meter.stop,
				items,
				period_start: start.to_string(),
				period_end: end.to_string(),
				total,
				delta_usd,
				accounts: gauges
					.into_iter()
					.map(|g| AccountUseDto {
						used: g.used(),
						left: g.left(),
						reading: g.synced.clone().map(ReadingDto::from),
						baseline: g.baseline,
						since: g.since.to_string(),
						delta: g.delta,
						account: g.account,
					})
					.collect(),
			}
		})
		.collect();
	Ok(Json(MetersDto { accounts, meters }))
}

/// Grafana's `${__from}` and `${__to}`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Window {
	/// Unix milliseconds.
	from: i64,
	/// Unix milliseconds, at most 400 days after `from`.
	to: i64,
}

/// The vendors' readings of their meters in a window, oldest first: what the sa-meters dashboard
/// reads.
pub(crate) async fn meter_readings(State(panel): State<Panel>, Params(q): Params<Window>) -> ApiResult<Json<ReadingsDto>> {
	let at = |ms: i64| Timestamp::from_millisecond(ms).map_err(|e| Refusal::new(Code::BadRequest, format!("{ms}: {e}")));
	let (from, to) = (at(q.from)?, at(q.to)?);
	if from >= to || to.duration_since(from) > SPAN {
		return Err(Refusal::new(Code::BadRequest, format!("from is before to, at most {} days", SPAN.as_hours().div_euclid(24))));
	}
	Ok(Json(ReadingsDto {
		readings: panel.meter_readings(from, to).await?.into_iter().map(ReadingDto::from).collect(),
	}))
}

/// `account`'s credits off `bill`, read at `read_at`, and its allowances as the guard last held them.
fn standing(account: &sa_places::Account, bill: &Bill, read_at: Timestamp, digest: &str, now: Timestamp) -> eyre::Result<Standing> {
	let today = now.to_zoned(jiff::tz::TimeZone::UTC).date();
	let window_from = now - SignedDuration::from_hours(BURN_DAYS * 24);
	let named: Vec<&str> = account.credits.iter().filter_map(|c| c.id.as_deref()).collect();
	let mut unnamed: Vec<String> = bill.draws.iter().map(|d| d.credit.clone()).filter(|id| !named.contains(&id.as_str())).collect();
	unnamed.dedup();
	let mut credits = Vec::with_capacity(account.credits.len());
	for c in &account.credits {
		let draws: Vec<&sa_panel::meters::Draw> = bill.draws.iter().filter(|d| Some(d.credit.as_str()) == c.id.as_deref()).collect(); // none until the export first names it
		let since_read = draws.iter().filter(|d| d.hour >= c.read).map(|d| d.amount_micros).sum::<i64>() as f64 / 1e6;
		let burnt = -(draws.iter().filter(|d| d.hour >= window_from).map(|d| d.amount_micros).sum::<i64>() as f64) / 1e6;
		let left = c.left_on(since_read, today);
		let covered = draws
			.iter()
			.map(|d| d.hour)
			.min()
			.map(|first| (now.duration_since(first.max(window_from)).as_secs_f64() / 86_400.0).max(1.0));
		let per_day = covered.map(|days| burnt / days);
		let to_spent = per_day.filter(|&d| d > 0.0).map(|d| left / d);
		let to_end = (c.ends - today).get_days() as f64;
		let (days_left, binds) = match to_spent {
			_ if to_end <= 0.0 => (0.0, "ended"),
			Some(s) if s < to_end => (s, "spent"),
			_ => (to_end, "ends"),
		};
		credits.push(CreditDto {
			name: c.name.clone(),
			left,
			as_of: read_at.max(c.read).to_string(),
			per_day,
			runs_out: to_spent.map(|d| today.checked_add(jiff::Span::new().days(d.floor() as i64))).transpose()?.map(|d| d.to_string()),
			ends: c.ends.to_string(),
			days_left,
			binds: binds.to_owned(),
		});
	}
	let Some(levers) = &bill.guard else {
		return Ok((credits, unnamed, Vec::new(), None));
	};
	let allowances = levers
		.iter()
		.filter(|l| l.lever == "real money")
		.map(|m| {
			let (allowed, real) = (m.allow.expect("the guard writes a slice's allow with its real money"), m.real.expect("and its real money"));
			AllowanceDto {
				slice: m.slice.clone(),
				allowed,
				real,
				headroom: allowed - real,
				state: m.state.clone().expect("and whether it is within"),
				levers: levers
					.iter()
					.filter(|l| l.slice == m.slice && l.lever != "real money")
					.map(|l| LeverDto {
						project: l.project.clone(),
						lever: l.lever.clone(),
						setting: l.setting,
						state: l.state.clone(),
						today: l.today,
						action: l.action.clone(),
					})
					.collect(),
			}
		})
		.collect();
	let guard = levers.first().map(|l| GuardDto {
		tick: l.tick.to_string(),
		holds_this_list: l.list.as_deref() == Some(digest),
	});
	Ok((credits, unnamed, allowances, guard))
}

/// Each account's days in a window, by slice and SKU, and its latest month's tiers: what the
/// sa-meters dashboard draws the bill from.
pub(crate) async fn bill(State(panel): State<Panel>, Params(q): Params<Window>) -> ApiResult<Json<BillDto>> {
	let at = |ms: i64| Timestamp::from_millisecond(ms).map_err(|e| Refusal::new(Code::BadRequest, format!("{ms}: {e}")));
	let (from, to) = (at(q.from)?, at(q.to)?);
	if from >= to || to.duration_since(from) > SPAN {
		return Err(Refusal::new(Code::BadRequest, format!("from is before to, at most {} days", SPAN.as_hours().div_euclid(24))));
	}
	let slices = panel.accounts().slices().to_vec();
	let (mut days, mut tiers) = (Vec::new(), Vec::new());
	for (a, bill) in panel.bills().await? {
		let Some((bill, _)) = bill else { continue };
		let money = |micros: i64| micros as f64 / 1e6;
		for d in &bill.days {
			let midnight = d.day.to_zoned(jiff::tz::TimeZone::UTC).expect("every day has a UTC midnight").timestamp();
			if midnight < from || midnight >= to {
				continue;
			}
			days.push(DayDto {
				account: a.name.clone(),
				day: midnight.to_string(),
				slice: sa_core::google::slice_of(&slices, &d.service, &d.sku),
				service: d.service.clone(),
				sku: d.sku.clone(),
				currency: d.currency.clone(),
				used: money(d.cost_micros),
				credit: -money(d.credit_micros),
				discount: -money(d.discount_micros),
				paid: money(d.cost_micros + d.credit_micros + d.discount_micros),
			});
		}
		let latest = bill.tiers.iter().map(|t| t.month.as_str()).max();
		tiers.extend(bill.tiers.iter().filter(|t| Some(t.month.as_str()) == latest).map(|t| TierDto {
			account: a.name.clone(),
			service: t.service.clone(),
			sku: t.sku.clone(),
			unit: t.unit.clone(),
			units: t.units,
			free_to: t.free_to,
			paid_units: t.paid_units,
			list: money(t.cost_micros),
			free: -money(t.free_micros),
		}));
	}
	days.sort_by(|a, b| a.day.cmp(&b.day));
	Ok(Json(BillDto { days, tiers }))
}
