//! People's tokens (`sa_panel::tokens`): the caller's own under `/api/v1/tokens`, everyone's under
//! `/api/v1/admin/tokens` (`sa:admin:tokens:grant`), and what `serve` does with review_archive —
//! follows its charges and pushes it the snapshot, under a Conductor assertion.
//!
//! Before the import from review_archive, every one of them answers 503, and `serve` waits.

use std::{collections::BTreeMap, time::Duration};

use axum::{
	Extension, Json,
	body::{Body as RawBody, to_bytes},
	extract::State,
	http::{HeaderMap, HeaderValue, Method, StatusCode, header},
};
use ev_lib::api::JsonSchema;
use jiff::Timestamp;
use sa_auth::{Principal, Service};
use sa_core::{
	prices::{Item, Sale, Scope},
	tokens::ChargePage,
};
use sa_panel::{
	Panel,
	tokens::{Change, Entry, Flow, FlowKind, Kickback, Kind, Owed, Priced},
};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use uuid::Uuid;

use crate::{
	api::{ApiError, ApiResult, Body, Ids, Params},
	forward::Services,
	http::{ApiRoute, Gate, Section},
	signin::Caller,
};

/// The most entries a page of the ledger holds.
const LEDGER_PAGE: i64 = 200;

/// Charges asked of review_archive at once.
const CHARGES_PAGE: usize = 500;

/// A page is asked for this often while review_archive has no more.
const FOLLOW_EVERY: Duration = Duration::from_secs(10);
/// The snapshot goes out at least this often, renewals included.
const PUSH_EVERY: Duration = Duration::from_secs(60);
/// Before the import, or while review_archive answers 404 (not cut over yet), looked at again
/// this often.
const LOOK_AGAIN: Duration = Duration::from_secs(60);
/// After a failure: doubling from the first to the second.
const BACKOFF: (Duration, Duration) = (Duration::from_secs(1), Duration::from_secs(60));
const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of review_archive's answer read: a page of charges, with room.
const ANSWER: usize = 16 * 1024 * 1024;

/// The widest window `flows` reads.
const FLOWS_SPAN: jiff::SignedDuration = jiff::SignedDuration::from_hours(24 * 400);
/// What the caller is owed until they claim it: a charge kicked back (`service`, `key`,
/// `label`), or an admin's grant (`by`, `note`).
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Kickback")]
pub struct KickbackDto {
	pub id: i64,
	/// RFC 3339.
	pub at: String,
	pub amount: i64,
	pub service: Option<String>,
	pub key: Option<String>,
	pub label: Option<String>,
	/// The admin's email.
	pub by: Option<String>,
	pub note: Option<String>,
}

/// `GET /tokens`, `POST /tokens/claim`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Tokens")]
pub struct TokensDto {
	pub balance: i64,
	/// Renewal: added per day while under `cap`.
	pub daily: i64,
	pub cap: i64,
	pub kickbacks: Vec<KickbackDto>,
	/// Each sold, priced item: its tokens.
	pub prices: BTreeMap<Item, i64>,
	/// An action costing more asks first.
	pub confirm_above: i64,
}

/// `PUT /tokens/settings`.
#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "SpendingSettings")]
#[serde(deny_unknown_fields)]
pub struct SpendingSettingsDto {
	/// 0 or more.
	pub confirm_above: i64,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "LedgerEntry")]
pub struct LedgerEntryDto {
	pub id: i64,
	/// RFC 3339.
	pub at: String,
	pub delta: i64,
	pub kind: Kind,
	pub service: Option<String>,
	pub key: Option<String>,
	pub label: Option<String>,
	/// The admin's email.
	pub by: Option<String>,
	pub note: Option<String>,
}

/// `GET /tokens/ledger`: `next` is the `before` of the next page, `null` at the end.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Ledger")]
pub struct LedgerDto {
	pub entries: Vec<LedgerEntryDto>,
	pub next: Option<i64>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "UsageDay")]
pub struct UsageDayDto {
	/// `YYYY-MM-DD`, UTC.
	pub day: String,
	pub charges: i64,
	pub tokens: i64,
}

/// `GET /tokens/usage`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Usage")]
pub struct UsageDto {
	pub days: Vec<UsageDayDto>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "TokenHolder")]
pub struct TokenHolderDto {
	pub user_id: Uuid,
	pub email: String,
	pub name: String,
	pub balance: i64,
}

/// `GET /admin/tokens`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "TokenHolders")]
pub struct TokenHoldersDto {
	pub users: Vec<TokenHolderDto>,
}

/// `POST /admin/tokens/{user_id}`: set to 0 or more, add 1 or more (a sale, its reference in
/// the note), or grant 1 or more, owed until claimed, saying what for.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
#[schemars(rename = "BalanceChange")]
pub enum BalanceChangeDto {
	Set { set: i64, note: Option<String> },
	Add { add: i64, note: Option<String> },
	Grant { grant: i64, note: String },
}

/// The answer to a balance change.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Balance")]
pub struct BalanceDto {
	pub balance: i64,
}

/// Follows review_archive's charges and pushes it the snapshot until `shutdown` turns true.
/// Nothing here ends `serve`.
pub async fn run(panel: Panel, services: Services, shutdown: watch::Receiver<bool>) {
	let follow = repeat("following review_archive's charges", FOLLOW_EVERY, std::future::pending, shutdown.clone(), || async {
		let Some(after) = panel.charges_after().await? else { return Ok(Pass::Closed) };
		let (status, answer) = call(&services, Method::GET, &format!("/api/conductor/charges?after={after}&limit={CHARGES_PAGE}"), None).await?;
		match status {
			StatusCode::NOT_FOUND => return Ok(Pass::NotCutOver),
			StatusCode::OK => {}
			other => eyre::bail!("review_archive answered its charges with {other}: {}", String::from_utf8_lossy(&answer)),
		}
		let page: ChargePage = serde_json::from_slice(&answer).map_err(|e| eyre::eyre!("review_archive's charges: {e}"))?;
		Ok(Pass::Done(panel.take_charges(page, after, Timestamp::now()).await?))
	});
	let push = repeat(
		"pushing review_archive the snapshot",
		PUSH_EVERY,
		|| panel.snapshot_due(),
		shutdown,
		|| async {
			let Some(snapshot) = panel.snapshot(Timestamp::now()).await? else { return Ok(Pass::Closed) };
			let body = serde_json::to_vec(&snapshot).expect("a snapshot serializes");
			let (status, answer) = call(&services, Method::PUT, "/api/conductor/gmbs", Some(body)).await?;
			match status {
				StatusCode::NOT_FOUND => Ok(Pass::NotCutOver),
				StatusCode::NO_CONTENT => Ok(Pass::Done(0)),
				other => eyre::bail!("review_archive refused the snapshot with {other}: {}", String::from_utf8_lossy(&answer)),
			}
		},
	);
	tokio::join!(follow, push);
}

/// A catalog item, its price (`null`: not sold, or metered) and what a unit costs us (`null`: unknown).
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Price")]
pub struct PriceDto {
	pub item: Item,
	pub scope: Scope,
	pub sale: Sale,
	/// What one is counted in.
	pub unit: &'static str,
	pub tool: &'static str,
	pub tool_title: &'static str,
	pub title: &'static str,
	pub description: &'static str,
	pub price: Option<TokenPriceDto>,
	pub cost: Option<UnitCostDto>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "TokenPrice")]
pub struct TokenPriceDto {
	pub tokens: i64,
	/// The admin's email.
	pub set_by: String,
	/// RFC 3339.
	pub set_at: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "UnitCost")]
pub struct UnitCostDto {
	/// Millionths of a dollar a unit.
	pub usd_micros: i64,
	/// The admin's email.
	pub set_by: String,
	/// RFC 3339.
	pub set_at: String,
}

/// Every item, in the catalog's order.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Prices")]
pub struct PricesDto {
	pub prices: Vec<PriceDto>,
}

/// The items given, each priced at 0 tokens or more or `null` off sale, each costed at 0 µ$ a unit
/// or more or `null` unknown; the others stay.
#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "SetPrices")]
#[serde(deny_unknown_fields)]
pub struct SetPricesDto {
	pub prices: BTreeMap<Item, Option<i64>>,
	pub costs: BTreeMap<Item, Option<i64>>,
}

/// What moved a balance, as Grafana reads it.
#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FlowKindDto {
	Renewal,
	Purchase,
	Set,
	Spend,
	/// A charge kicked back, claimed.
	Refund,
	GrantClaimed,
	/// A grant made, owed until claimed.
	Granted,
}

/// One flow: `tokens` what the balance moved by, a grant's amount for `granted`.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Flow")]
pub struct FlowDto {
	/// RFC 3339.
	pub at: String,
	pub email: String,
	pub name: String,
	pub kind: FlowKindDto,
	pub tokens: i64,
	pub service: Option<String>,
	pub label: Option<String>,
	/// The admin's email.
	pub by: Option<String>,
	pub note: Option<String>,
}

/// Everyone's flows, oldest first.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Flows")]
pub struct FlowsDto {
	pub flows: Vec<FlowDto>,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let user = Gate::USER;
	let admin = Gate::section(Section::Tokens);
	vec![
		ApiRoute::new(M::GET, "/tokens", user, tokens),
		ApiRoute::new(M::GET, "/tokens/ledger", user, ledger),
		ApiRoute::new(M::GET, "/tokens/usage", user, token_usage),
		ApiRoute::new(M::POST, "/tokens/claim", user, claim_tokens),
		ApiRoute::new(M::PUT, "/tokens/settings", user, set_spending),
		ApiRoute::new(M::GET, "/admin/tokens", admin, token_holders),
		// Tokens must not be handed out on a permission revoked a moment ago.
		ApiRoute::new(M::POST, "/admin/tokens/{user_id}", admin.fresh(), change_balance),
		ApiRoute::new(M::GET, "/admin/tokens/prices", admin, token_prices),
		// A price is what every request on the shared keys charges.
		ApiRoute::new(M::PUT, "/admin/tokens/prices", admin.fresh(), set_token_prices),
		ApiRoute::new(M::GET, "/admin/tokens/flows", admin, token_flows),
	]
}

/// A path's user.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct UserIn {
	user_id: Uuid,
}

impl From<Kickback> for KickbackDto {
	fn from(k: Kickback) -> Self {
		let (service, key, label, by, note) = match k.why {
			Owed::Charge(c) => (Some(c.service), Some(c.key), c.label, None, None),
			Owed::Grant { by, note } => (None, None, None, Some(by), Some(note)),
		};
		Self {
			id: k.id,
			at: k.at.to_string(),
			amount: k.amount,
			service,
			key,
			label,
			by,
			note,
		}
	}
}

impl From<(i64, Entry)> for LedgerEntryDto {
	fn from((id, e): (i64, Entry)) -> Self {
		let (service, key, label) = match e.charged {
			Some(c) => (Some(c.service), Some(c.key), c.label),
			None => (None, None, None),
		};
		Self {
			id,
			at: e.at.to_string(),
			delta: e.delta,
			kind: e.kind,
			service,
			key,
			label,
			by: e.by,
			note: e.note,
		}
	}
}

impl From<BalanceChangeDto> for Change {
	fn from(b: BalanceChangeDto) -> Self {
		match b {
			BalanceChangeDto::Set { set, note } => Self::Set { n: set, note },
			BalanceChangeDto::Add { add, note } => Self::Add { n: add, note },
			BalanceChangeDto::Grant { grant, note } => Self::Grant { n: grant, note },
		}
	}
}

async fn tokens_dto(panel: &Panel, user: Uuid, (balance, kickbacks): (i64, Vec<Kickback>)) -> ApiResult<TokensDto> {
	let (prices, confirm_above) = panel.spending(user).await?;
	Ok(TokensDto {
		balance,
		daily: panel.renewal().daily,
		cap: panel.renewal().cap,
		kickbacks: kickbacks.into_iter().map(KickbackDto::from).collect(),
		prices,
		confirm_above,
	})
}

/// The caller's balance, renewed a day's worth when read, the kick-backs not claimed, and what
/// they read before spending.
async fn tokens(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<TokensDto>> {
	let held = panel.tokens(caller.user_id, Timestamp::now()).await??;
	Ok(Json(tokens_dto(&panel, caller.user_id, held).await?))
}

/// Claims every kick-back and grant owed to the caller into their balance.
async fn claim_tokens(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<TokensDto>> {
	let held = panel.claim(caller.user_id, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, balance = held.0, "kick-backs claimed");
	Ok(Json(tokens_dto(&panel, caller.user_id, held).await?))
}

/// Above how many tokens an action asks the caller first.
async fn set_spending(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<SpendingSettingsDto>) -> ApiResult<axum::response::NoContent> {
	panel.set_confirm_above(caller.user_id, b.confirm_above).await??;
	Ok(axum::response::NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LedgerQuery {
	/// The previous page's `next`.
	before: Option<i64>,
	/// 1–200.
	limit: i64,
}

/// The caller's ledger, newest first.
async fn ledger(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Params(q): Params<LedgerQuery>) -> ApiResult<Json<LedgerDto>> {
	if !(1..=LEDGER_PAGE).contains(&q.limit) {
		return Err(ApiError::BadRequest(format!("limit is 1 to {LEDGER_PAGE}")));
	}
	let entries = panel.ledger(caller.user_id, q.before, q.limit).await??;
	let next = (entries.len() as i64 == q.limit).then(|| entries.last().map(|(id, _)| *id)).flatten();
	Ok(Json(LedgerDto {
		entries: entries.into_iter().map(LedgerEntryDto::from).collect(),
		next,
	}))
}

/// What the caller spent per UTC day over the last 30, oldest first.
async fn token_usage(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<UsageDto>> {
	let days = panel.usage(caller.user_id, Timestamp::now()).await??;
	Ok(Json(UsageDto {
		days: days
			.into_iter()
			.map(|d| UsageDayDto {
				day: d.day.to_string(),
				charges: d.charges,
				tokens: d.tokens,
			})
			.collect(),
	}))
}

/// Every user's balance, renewed.
async fn token_holders(State(panel): State<Panel>) -> ApiResult<Json<TokenHoldersDto>> {
	let holders = panel.holders(Timestamp::now()).await??;
	Ok(Json(TokenHoldersDto {
		users: holders
			.into_iter()
			.map(|(u, balance)| TokenHolderDto {
				user_id: u.user_id,
				email: u.email,
				name: u.name,
				balance,
			})
			.collect(),
	}))
}

/// Sets a user's balance, adds to it, or grants tokens owed until they claim them.
async fn change_balance(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Ids(UserIn { user_id: user }): Ids<UserIn>,
	Body(b): Body<BalanceChangeDto>,
) -> ApiResult<Json<BalanceDto>> {
	let balance = panel.change_balance(user, b.into(), &caller.email, Timestamp::now()).await??;
	Ok(Json(BalanceDto { balance }))
}

impl From<Priced> for PriceDto {
	fn from(Priced { item, price, cost }: Priced) -> Self {
		Self {
			item,
			scope: item.scope(),
			sale: item.sale(),
			unit: item.unit(),
			tool: item.tool(),
			tool_title: item.tool_title(),
			title: item.title(),
			description: item.description(),
			price: price.map(|p| TokenPriceDto {
				tokens: p.tokens,
				set_by: p.set_by,
				set_at: p.set_at.to_string(),
			}),
			cost: cost.map(|c| UnitCostDto {
				usd_micros: c.usd_micros,
				set_by: c.set_by,
				set_at: c.set_at.to_string(),
			}),
		}
	}
}

/// Every item of the price catalog (`sa_core::prices`): Places requests, Street View, review_archive's
/// walk steps; each priced in tokens or not sold.
async fn token_prices(State(panel): State<Panel>) -> ApiResult<Json<PricesDto>> {
	Ok(Json(PricesDto {
		prices: panel.prices().await?.into_iter().map(PriceDto::from).collect(),
	}))
}

/// Prices the items given, 0 or more tokens or `null` off sale, and costs them, 0 µ$ a unit or
/// more or `null` unknown; the others stay.
///
/// 400 an item not in the catalog, or a metered one priced.
async fn set_token_prices(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<SetPricesDto>) -> ApiResult<Json<PricesDto>> {
	let (priced, costs) = (b.prices.into_iter().collect::<Vec<_>>(), b.costs.into_iter().collect::<Vec<_>>());
	panel.set_prices(&priced, &costs, &caller.email, Timestamp::now()).await??;
	token_prices(State(panel)).await
}

impl From<Flow> for FlowDto {
	fn from(f: Flow) -> Self {
		let kind = match f.kind {
			FlowKind::Granted => FlowKindDto::Granted,
			FlowKind::Ledger(Kind::Accrual) => FlowKindDto::Renewal,
			FlowKind::Ledger(Kind::Add) => FlowKindDto::Purchase,
			FlowKind::Ledger(Kind::Set) => FlowKindDto::Set,
			FlowKind::Ledger(Kind::Charge) => FlowKindDto::Spend,
			FlowKind::Ledger(Kind::Kickback) if f.by.is_some() => FlowKindDto::GrantClaimed,
			FlowKind::Ledger(Kind::Kickback) => FlowKindDto::Refund,
		};
		Self {
			at: f.at.to_string(),
			email: f.email,
			name: f.name,
			kind,
			tokens: f.delta,
			service: f.service,
			label: f.label,
			by: f.by,
			note: f.note,
		}
	}
}

/// Grafana's `${__from}` and `${__to}`.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FlowsQuery {
	/// Unix milliseconds.
	from: i64,
	/// Unix milliseconds, at most 400 days after `from`.
	to: i64,
}

/// Everyone's ledger and grants in a window, oldest first: what Grafana's sa-tokens dashboard reads.
async fn token_flows(State(panel): State<Panel>, Params(q): Params<FlowsQuery>) -> ApiResult<Json<FlowsDto>> {
	let at = |ms: i64| Timestamp::from_millisecond(ms).map_err(|e| ApiError::BadRequest(format!("{ms}: {e}")));
	let (from, to) = (at(q.from)?, at(q.to)?);
	if from >= to || to.duration_since(from) > FLOWS_SPAN {
		return Err(ApiError::BadRequest(format!("from is before to, at most {} days", FLOWS_SPAN.as_hours() / 24)));
	}
	let flows = panel.flows(from, to).await??;
	Ok(Json(FlowsDto {
		flows: flows.into_iter().map(FlowDto::from).collect(),
	}))
}

// ── review_archive, in `serve` ──────────────────────────────────────────────────────────

/// How a pass with review_archive went.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Pass {
	/// That many charges taken; the snapshot pushed.
	Done(usize),
	/// Tokens are not open: the import has not run.
	Closed,
	/// review_archive answered 404: it is not cut over to the conductor's tokens yet.
	NotCutOver,
}

/// `pass` until `shutdown`: again at once after a full page of charges, else after `every` or
/// as soon as `early` resolves; a closed or absent counterpart is said once and looked at again
/// after LOOK_AGAIN (or `early`); a failure is reported and tried again after a growing wait.
async fn repeat<F, Fut, E, EFut>(what: &'static str, every: Duration, early: E, mut shutdown: watch::Receiver<bool>, pass: F)
where
	F: Fn() -> Fut,
	Fut: Future<Output = eyre::Result<Pass>>,
	E: Fn() -> EFut,
	EFut: Future<Output = ()>, {
	let mut said: Option<Pass> = None;
	let mut backoff = BACKOFF.0;
	while !*shutdown.borrow() {
		let (wait, wakes) = match pass().await {
			Ok(Pass::Done(n)) => {
				said = None;
				backoff = BACKOFF.0;
				if n == CHARGES_PAGE { (Duration::ZERO, false) } else { (every, true) }
			}
			Ok(p) => {
				if said != Some(p) {
					tracing::info!(state = ?p, "{what}: waiting");
					said = Some(p);
				}
				(LOOK_AGAIN, true)
			}
			Err(e) => {
				crate::report(&e, what);
				let wait = backoff;
				backoff = (backoff * 2).min(BACKOFF.1);
				(wait, false)
			}
		};
		let early = async {
			if wakes {
				early().await;
			} else {
				std::future::pending::<()>().await;
			}
		};
		tokio::select! {
			() = tokio::time::sleep(wait) => {}
			() = early => {}
			_ = shutdown.changed() => return,
		}
	}
}

/// `method path` of review_archive as the conductor: its status and body.
async fn call(services: &Services, method: Method, path: &str, json: Option<Vec<u8>>) -> eyre::Result<(StatusCode, axum::body::Bytes)> {
	let mut headers = HeaderMap::new();
	let body = match json {
		Some(json) => {
			headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
			RawBody::from(json)
		}
		None => RawBody::empty(),
	};
	let sent = services.send(Service::ReviewArchive, method, path, headers, Some(Principal::Conductor), body);
	let res = tokio::time::timeout(CALL_TIMEOUT, sent)
		.await
		.map_err(|_| eyre::eyre!("review_archive did not answer {path} in time"))??;
	let status = res.status();
	let answer = to_bytes(res.into_body(), ANSWER)
		.await
		.map_err(|e| eyre::eyre!("reading review_archive's answer to {path}: {e}"))?;
	Ok((status, answer))
}
