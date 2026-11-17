//! What an operator once ran as `panel` commands: the conductor's own admin operations, each behind
//! one of the conductor's admin permissions and a fresh read of it.

use std::collections::BTreeMap;

use axum::{
	Extension, Json,
	extract::{DefaultBodyLimit, State},
	http::Method,
	response::NoContent,
};
use ev_lib::api::{Body, Created, Ids, JsonSchema, Refusal};
use jiff::Timestamp;
use sa_auth::{Sources, Tokens};
use sa_core::{
	ids::SiteOrigin,
	prices::{PlacesRequest, Spends},
	queries_cache::{Key, Scheme},
};
use sa_panel::{Panel, import::Export};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiResult, Code, KeyId},
	gmbs::GmailDto,
	http::{ApiRoute, FRESH, Flags, Gate},
	signin::{Caller, Freshness},
};

/// A review_archive export, or a cache of answers, is megabytes.
const MAX_IMPORT: usize = 256 * 1024 * 1024;

const SLOW_FRESH: Gate = sa_auth::Gate::USER.with(Flags {
	freshness: Freshness::Fresh,
	slow: true,
});

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let big = |h: axum::routing::MethodRouter<_>| h.layer(DefaultBodyLimit::max(MAX_IMPORT));
	vec![
		ApiRoute::new(M::PUT, "/sources/{key_id}/origin", FRESH.needs(Sources::Manage), set_origin),
		ApiRoute::new(M::POST, "/admin/gmails", FRESH.needs(Tokens::Grant), add_gmail_for),
		ApiRoute::new(M::POST, "/admin/gmbs/locate", SLOW_FRESH.needs(Sources::Manage), locate),
		ApiRoute::new(M::POST, "/admin/imports/review-archive", FRESH.needs(Tokens::Grant), import_review_archive).layer(big),
		ApiRoute::new(M::POST, "/admin/imports/queries-cache", SLOW_FRESH.needs(Sources::Manage), import_queries_cache).layer(big),
	]
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Origin {
	/// `https://<domain>[:port]`, no path.
	#[schemars(pattern(sa_core::ids::SITE_ORIGIN_PATTERN))]
	origin: String,
}

/// Gives a site key issued before origins its origin, once; the same origin again is no change.
///
/// 409 when there is no active site key of that id without another origin.
async fn set_origin(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(KeyId { key_id }): Ids<KeyId>, Body(b): Body<Origin>) -> ApiResult<NoContent> {
	panel.set_site_origin(&key_id, &SiteOrigin::parse(&b.origin)?).await??;
	tracing::info!(user_id = %caller.user_id, key_id, "source origin set");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GmailForPerson {
	/// The email the person signs in with: they have signed in to the conductor already.
	email: String,
	gmail: String,
}

/// Adds a managing gmail to a person.
///
/// 400 for an email nobody signed in with; 409 if they have the gmail already.
async fn add_gmail_for(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<GmailForPerson>) -> ApiResult<Created<GmailDto>> {
	let owner = panel.user_by_email(&b.email).await??;
	let added = panel.add_gmail(owner, &b.gmail, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, owner = %owner, gmail_id = added.id, "gmail added for a person");
	Ok(Created(GmailDto::from(added)))
}

#[derive(JsonSchema, Serialize)]
#[schemars(rename = "Located")]
pub(crate) struct LocatedDto {
	/// Places given a point or a country.
	located: usize,
	/// Service-area places not placed yet: for the map to place.
	no_point: Vec<String>,
	/// For a person to set the country of.
	no_country: Vec<String>,
	/// Places Places refused, or put in a country not served.
	refused: Vec<RefusedPlace>,
}

#[derive(JsonSchema, Serialize)]
pub(crate) struct RefusedPlace {
	place_id: String,
	why: String,
}

/// Locates every GBP on Google that has no point or no country of Google's yet, through Places.
///
/// Each place is asked of its gmail's keys first, then the shared ones, which the house pays for.
/// Run after a deploy that brings points or countries, and after a copy of prod. 503 when no Google
/// account has a Places key.
async fn locate(State(panel): State<Panel>, Spends(caller, _): Spends<PlacesRequest, Caller>) -> ApiResult<Json<LocatedDto>> {
	if !panel.accounts().iter().any(|a| a.provides.places.is_some()) {
		return Err(Refusal::new(Code::Unavailable, "locating asks Places: no Google account has a Places key"));
	}
	let done = panel.locate().await?;
	tracing::info!(user_id = %caller.user_id, located = done.located, "GBPs located");
	Ok(Json(LocatedDto {
		located: done.located,
		no_point: done.no_point,
		no_country: done.no_country,
		refused: done.refused.into_iter().map(|(place_id, why)| RefusedPlace { place_id, why }).collect(),
	}))
}

/// Writes review_archive's people, gmails, GMBs, tokens and kick-backs, as its
/// `scripts/export_members.sql` prints them, and opens tokens and GMBs. A fresh install imports an
/// empty export.
///
/// 400 for people without a `sub` (they sign in to the conductor first); 409 once tokens are open
/// or anything was written to them.
async fn import_review_archive(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(export): Body<Export>) -> ApiResult<NoContent> {
	panel.import_review_archive(export).await??;
	tracing::info!(user_id = %caller.user_id, "imported from review_archive");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct QueriesCacheImport {
	/// `places` or `gop`: whose digest the keys are.
	scheme: String,
	/// By the request's SHA-256, 64 hex characters.
	answers: BTreeMap<String, KeptAnswer>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeptAnswer {
	answer: serde_json::Value,
	/// When it was bought: a newer answer kept already stays.
	fetched_at: Timestamp,
}

#[derive(JsonSchema, Serialize)]
pub(crate) struct Imported {
	answers: usize,
}

/// Keeps paid answers a client held as files in the queries cache under `scheme`, each as of its
/// own `fetched_at`; an answer kept already that is newer stays.
///
/// 400 for a scheme or a key that is none.
async fn import_queries_cache(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<QueriesCacheImport>) -> ApiResult<Json<Imported>> {
	let scheme: Scheme = b.scheme.parse()?;
	let answers = b
		.answers
		.into_iter()
		.map(|(key, kept)| Ok((key.parse::<Key>()?, kept)))
		.collect::<Result<Vec<_>, sa_core::Invalid>>()?;
	for (key, kept) in &answers {
		panel.store().keep_query(scheme, *key, &kept.answer.to_string(), None, None, kept.fetched_at).await?;
	}
	tracing::info!(user_id = %caller.user_id, %scheme, answers = answers.len(), "answers cached");
	Ok(Json(Imported { answers: answers.len() }))
}
