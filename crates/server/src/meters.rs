//! The vendors' meters (`sa_panel::meters`): in `serve` each read at start and daily, and their use
//! for the Admin page and Grafana.
//!
//! ```text
//! GET /api/v1/admin/meters                     Meters: each meter's use this period, all accounts and each
//! GET /api/v1/admin/meters/readings?from=&to=  MeterReadings: every reading synced, Unix ms
//! ```

use std::{sync::Mutex, time::Duration};

use axum::{Json, extract::State, http::Method};
use ev_lib::api::JsonSchema;
use eyre::WrapErr;
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	meters::{ALL, Meter, Stop, Vendor},
	prices::Item,
};
use sa_panel::{
	Panel,
	meters::{Gauge, Reader, Synced},
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
	api::{ApiError, ApiResult, Params},
	http::{ApiRoute, Gate, Section},
};

/// How often a reading is due.
const SYNC_EVERY: SignedDuration = SignedDuration::from_hours(24);
/// How soon a failed read is tried again.
const RETRY: Duration = Duration::from_secs(15 * 60);
/// The widest window `/admin/meters/readings` reads.
const SPAN: SignedDuration = SignedDuration::from_hours(24 * 400);

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
	let admin = Gate::section(Section::Tokens);
	vec![
		ApiRoute::new(M::GET, "/admin/meters", admin, meters),
		ApiRoute::new(M::GET, "/admin/meters/readings", admin, meter_readings),
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
async fn meters(State(panel): State<Panel>) -> ApiResult<Json<MetersDto>> {
	let now = Timestamp::now();
	let prices = panel.prices().await?;
	let mut accounts = Vec::with_capacity(panel.accounts().len());
	for a in panel.accounts().iter() {
		let spend = panel.spend(&a.name, Vendor::Google, now).await?;
		accounts.push(AccountDto {
			name: a.name.clone(),
			places: a.places.is_some(),
			browser: a.browser.is_some(),
			allow: a.allow,
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
			let costs: Vec<Option<i64>> = items
				.iter()
				.map(|i| prices.iter().find(|p| p.item == *i).expect("the catalog prices every item").cost.as_ref().map(|c| c.usd_micros))
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
				[Some(c), rest @ ..] if rest.iter().all(|r| *r == Some(*c)) => Some((total.delta * c) as f64 / 1e6),
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
async fn meter_readings(State(panel): State<Panel>, Params(q): Params<Window>) -> ApiResult<Json<ReadingsDto>> {
	let at = |ms: i64| Timestamp::from_millisecond(ms).map_err(|e| ApiError::BadRequest(format!("{ms}: {e}")));
	let (from, to) = (at(q.from)?, at(q.to)?);
	if from >= to || to.duration_since(from) > SPAN {
		return Err(ApiError::BadRequest(format!("from is before to, at most {} days", SPAN.as_hours() / 24)));
	}
	Ok(Json(ReadingsDto {
		readings: panel.meter_readings(from, to).await?.into_iter().map(ReadingDto::from).collect(),
	}))
}
