//! The vendors' meters (`sa_panel::meters`): in `serve` each read at start and daily, and their use
//! for the Admin page and Grafana.
//!
//! ```text
//! GET /api/v1/admin/meters                     Meters: each meter's use this period
//! GET /api/v1/admin/meters/readings?from=&to=  MeterReadings: every reading synced, Unix ms
//! ```

use std::{sync::Mutex, time::Duration};

use axum::{Json, extract::State, http::Method};
use ev_lib::api::JsonSchema;
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	meters::{Meter, Stop, Vendor},
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

/// Reads `reader`'s meters now, then every SYNC_EVERY, a failure again after RETRY, until
/// `shutdown`; the last good reading stays in force meanwhile.
pub async fn run(panel: Panel, reader: impl Reader + Sync, shutdown: watch::Receiver<bool>) {
	let last = Mutex::new(None::<Timestamp>);
	crate::every(RETRY, shutdown, "reading the vendors' meters", || async {
		let now = Timestamp::now();
		if last
			.lock()
			.expect("never poisoned: nothing panics holding it")
			.is_some_and(|at| now.duration_since(at) < SYNC_EVERY)
		{
			return Ok(());
		}
		let n = panel.sync_meters(&reader, now).await?;
		tracing::info!(meters = n, vendor = ?reader.vendor(), "meters read");
		*last.lock().expect("never poisoned: nothing panics holding it") = Some(now);
		Ok(())
	})
	.await;
}

/// The vendor's count of a meter, as last read.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterReading")]
pub struct ReadingDto {
	/// Its id, `<vendor>.<name>`.
	#[schemars(with = "String")]
	pub meter: &'static Meter,
	/// RFC 3339: the period it counts from.
	pub period_start: String,
	pub units: i64,
	/// Gross, before credits, in `currency`.
	pub cost: f64,
	pub currency: String,
	/// RFC 3339: how far the vendor's record reaches.
	pub through: String,
	/// RFC 3339.
	pub synced_at: String,
}

/// A meter's use this period: `used` is the vendor's `baseline` plus our `delta` after `since`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "MeterUse")]
pub struct MeterDto {
	/// Its id, `<vendor>.<name>`.
	#[schemars(with = "String")]
	pub meter: &'static Meter,
	pub vendor: Vendor,
	pub name: &'static str,
	pub free: i64,
	pub stop: Stop,
	pub items: Vec<Item>,
	/// RFC 3339, `[start, end)`.
	pub period_start: String,
	pub period_end: String,
	/// The latest reading, of whichever period; `null` before a first one.
	pub reading: Option<ReadingDto>,
	pub baseline: i64,
	/// RFC 3339: ours are counted after it.
	pub since: String,
	pub delta: i64,
	pub used: i64,
	pub left: i64,
	/// `delta` at the admin's cost of a unit; `null` while its items' costs are unknown or differ.
	pub delta_usd: Option<f64>,
}

/// `GET /admin/meters`: every meter, in the catalog's order.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Meters")]
pub struct MetersDto {
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
			meter: r.meter,
			period_start: r.period_start.to_string(),
			units: r.units,
			cost: r.cost_micros as f64 / 1e6,
			currency: r.currency,
			through: r.through.to_string(),
			synced_at: s.synced_at.to_string(),
		}
	}
}

/// Every vendor meter this period: the vendor's own count at its last reading, ours since, what is
/// left of the free units and when buying stops.
async fn meters(State(panel): State<Panel>) -> ApiResult<Json<MetersDto>> {
	let prices = panel.prices().await?;
	let gauges = panel.meters(Timestamp::now()).await?;
	Ok(Json(MetersDto {
		meters: gauges
			.into_iter()
			.map(|g: Gauge| {
				let items: Vec<Item> = g.meter.items().collect();
				let costs: Vec<Option<i64>> = items
					.iter()
					.map(|i| prices.iter().find(|p| p.item == *i).expect("the catalog prices every item").cost.as_ref().map(|c| c.usd_micros))
					.collect();
				let delta_usd = match costs.as_slice() {
					[Some(c), rest @ ..] if rest.iter().all(|r| *r == Some(*c)) => Some((g.delta * c) as f64 / 1e6),
					_ => None,
				};
				MeterDto {
					meter: g.meter,
					vendor: g.meter.vendor,
					name: g.meter.name,
					free: g.meter.free,
					stop: g.meter.stop,
					items,
					period_start: g.period.0.to_string(),
					period_end: g.period.1.to_string(),
					reading: g.synced.clone().map(ReadingDto::from),
					baseline: g.baseline,
					since: g.since.to_string(),
					delta: g.delta,
					used: g.used(),
					left: g.left(),
					delta_usd,
				}
			})
			.collect(),
	}))
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
