//! What an operator once ran as `panel` commands: the conductor's own admin operations, each behind
//! one of the conductor's admin permissions and a fresh read of it.

use axum::{
	Extension,
	extract::{DefaultBodyLimit, State},
	http::Method,
	response::NoContent,
};
use ev_lib::api::{Body, Created, Ids, JsonSchema};
use jiff::Timestamp;
use sa_auth::{Sources, Tokens};
use sa_core::ids::SiteOrigin;
use sa_panel::{Panel, import::Export};
use serde::Deserialize;

use crate::{
	api::{ApiResult, KeyId},
	gmbs::GmailDto,
	http::{ApiRoute, FRESH},
	signin::Caller,
};

/// A review_archive export is megabytes.
const MAX_IMPORT: usize = 256 * 1024 * 1024;

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let big = |h: axum::routing::MethodRouter<_>| h.layer(DefaultBodyLimit::max(MAX_IMPORT));
	vec![
		ApiRoute::new(M::PUT, "/sources/{key_id}/origin", FRESH.needs(Sources::Manage), set_origin),
		ApiRoute::new(M::POST, "/admin/gmails", FRESH.needs(Tokens::Grant), add_gmail_for),
		ApiRoute::new(M::POST, "/admin/imports/review-archive", FRESH.needs(Tokens::Grant), import_review_archive).layer(big),
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
