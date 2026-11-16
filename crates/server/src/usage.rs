//! What our tools spend on vendors (`sa_panel::usage`): a map built in the browser, everyone's
//! usage for Grafana, and in `serve` each service's usage feed followed (review_archive's walks,
//! the browsers' runner's phones and starts).

use std::{collections::BTreeSet, time::Duration};

use axum::{
	Extension, Json,
	body::{Body, to_bytes},
	extract::State,
	http::{HeaderMap, Method, StatusCode},
	response::NoContent,
};
use ev_lib::api::JsonSchema;
use jiff::Timestamp;
use sa_auth::{Principal, Service};
use sa_core::{
	prices::{Item, Scope},
	usage::{Bill, UsagePage},
};
use sa_panel::{Panel, usage::Day};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::{
	api::{ApiError, ApiResult, Params},
	browser::Connected,
	forward::Services,
	http::{ApiRoute, Gate, Section},
	signin::Caller,
};

/// Each feed is asked this often while it has no more.
const FOLLOW_EVERY: Duration = Duration::from_secs(10);
/// A failing feed is asked again after a wait doubling up to this.
const BACKOFF_CAP: Duration = Duration::from_secs(300);
/// Uses asked of a feed at once.
const PAGE: usize = 500;
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// The widest window `/admin/usage` reads.
const SPAN: jiff::SignedDuration = jiff::SignedDuration::from_hours(24 * 400);

/// A UTC day's units of one item, by whose bill and who told; `usd` each use at the cost in effect
/// when it was done, `null` when none had one, `uncosted` the units done without one.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "SpendDay")]
pub struct UsageDayDto {
	/// `YYYY-MM-DD`.
	pub day: String,
	pub item: Item,
	pub scope: Scope,
	pub tool_title: &'static str,
	pub title: &'static str,
	pub unit: &'static str,
	pub bill: Bill,
	pub source: String,
	pub units: i64,
	/// How many times it was recorded: a phone's run is one time of many minutes.
	pub times: i64,
	pub usd: Option<f64>,
	pub uncosted: i64,
}

/// Everyone's usage by day, oldest first.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Spend")]
pub struct UsageDto {
	pub days: Vec<UsageDayDto>,
}

/// Follows every usage feed there is until `shutdown` turns true. Nothing here ends `serve`.
pub async fn run(panel: Panel, services: Option<Services>, browsers: Option<Connected>, shutdown: watch::Receiver<bool>) {
	let of = |scope| Item::all().filter(|i| i.scope() == scope).collect::<BTreeSet<_>>();
	let feeds: Vec<Feed> = [
		services.map(|s| Feed {
			name: "review_archive",
			items: of(Scope::ReviewArchive),
			from: From::ReviewArchive(s),
		}),
		browsers.map(|b| Feed {
			name: "browsers",
			items: of(Scope::Browsers),
			from: From::Browsers(b),
		}),
	]
	.into_iter()
	.flatten()
	.collect();
	let follow = feeds.iter().map(|feed| {
		let (panel, mut shutdown) = (&panel, shutdown.clone());
		async move {
			// a service restarting fails every pass until it is back: said when it starts and ends
			let mut failing = false;
			let mut wait = FOLLOW_EVERY;
			while !*shutdown.borrow() {
				match feed.follow(panel).await {
					Ok(()) => {
						if failing {
							tracing::info!(feed = feed.name, "its usage feed answers again");
						}
						(failing, wait) = (false, FOLLOW_EVERY);
					}
					Err(e) => {
						if failing {
							tracing::debug!(feed = feed.name, error = %e, "its usage feed still fails");
						} else {
							crate::report(&e.wrap_err(format!("{}'s usage feed, asked again with a growing wait", feed.name)), "following a usage feed");
						}
						(failing, wait) = (true, (wait * 2).min(BACKOFF_CAP));
					}
				}
				tokio::select! {
					() = tokio::time::sleep(wait) => {}
					_ = shutdown.changed() => return,
				}
			}
		}
	});
	futures::future::join_all(follow).await;
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::POST, "/map/loaded", Gate::USER, map_loaded),
		ApiRoute::new(M::GET, "/admin/usage", Gate::section(Section::Tokens), spend),
	]
}

/// Records that the caller's browser built a Google map: a billed load of ours.
async fn map_loaded(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<NoContent> {
	panel.map_loaded(caller.user_id, Timestamp::now()).await?;
	Ok(NoContent)
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

/// What our tools spent on vendors per UTC day, item, bill and source, in units and each use at its
/// unit's cost when it was done: what Grafana's sa-tokens dashboard reads.
async fn spend(State(panel): State<Panel>, Params(q): Params<Window>) -> ApiResult<Json<UsageDto>> {
	let at = |ms: i64| Timestamp::from_millisecond(ms).map_err(|e| ApiError::BadRequest(format!("{ms}: {e}")));
	let (from, to) = (at(q.from)?, at(q.to)?);
	if from >= to || to.duration_since(from) > SPAN {
		return Err(ApiError::BadRequest(format!("from is before to, at most {} days", SPAN.as_hours() / 24)));
	}
	let days = panel.used(from, to).await?;
	Ok(Json(UsageDto {
		days: days
			.into_iter()
			.map(|d: Day| UsageDayDto {
				day: Timestamp::from_second(d.day * 86_400).expect("a stored day").strftime("%Y-%m-%d").to_string(),
				item: d.item,
				scope: d.item.scope(),
				tool_title: d.item.tool_title(),
				title: d.item.title(),
				unit: d.item.unit(),
				bill: d.bill,
				source: d.source,
				units: d.units,
				times: d.times,
				usd: d.usd_micros.map(|u| u as f64 / 1e6),
				uncosted: d.uncosted,
			})
			.collect(),
	}))
}

/// A service whose usage feed is followed, and what it may tell of.
struct Feed {
	name: &'static str,
	items: BTreeSet<Item>,
	from: From,
}
impl Feed {
	/// Every page there is after the cursor, taken.
	async fn follow(&self, panel: &Panel) -> eyre::Result<()> {
		loop {
			let after = panel.usage_after(self.name).await?;
			let Some(page) = self.page(after).await? else { return Ok(()) };
			if panel.take_usage(self.name, &self.items, page, after).await? < PAGE {
				return Ok(());
			}
		}
	}

	/// The page after `after`; `None` while the service serves no feed yet (404).
	async fn page(&self, after: i64) -> eyre::Result<Option<UsagePage>> {
		let (status, answer) = match &self.from {
			From::ReviewArchive(services) => {
				let path = format!("/api/conductor/usage?after={after}&limit={PAGE}");
				let sent = services.send(Service::ReviewArchive, Method::GET, &path, HeaderMap::new(), Some(Principal::Conductor), Body::empty());
				let res = tokio::time::timeout(CALL_TIMEOUT, sent)
					.await
					.map_err(|_| eyre::eyre!("review_archive did not answer {path} in time"))??;
				let status = res.status();
				let answer = to_bytes(res.into_body(), 16 * 1024 * 1024)
					.await
					.map_err(|e| eyre::eyre!("reading review_archive's usage: {e}"))?;
				(status, answer.to_vec())
			}
			From::Browsers(b) => b.usage(after, PAGE).await?,
		};
		match status {
			StatusCode::NOT_FOUND => Ok(None),
			StatusCode::OK => Ok(Some(serde_json::from_slice(&answer).map_err(|e| eyre::eyre!("{}'s usage: {e}", self.name))?)),
			other => eyre::bail!("{} answered its usage with {other}: {}", self.name, String::from_utf8_lossy(&answer)),
		}
	}
}

enum From {
	ReviewArchive(Services),
	Browsers(Connected),
}
