//! The library (`sa_panel::library`): category versions released as listings with numbered
//! versions, the standard library a curator ranks, and who used what. Published studies are
//! placement's (`placement.rs`).
//!
//! ```text
//! GET  /library?trade&country&q&kind&sort    the listings the caller may see
//! GET  /library/{listing}                    its versions with their bodies, its studies
//! POST /library                              a new listing of one of the caller's versions
//! POST /library/{listing}/versions           its next version
//! POST /library/{listing}/withdraw
//! PUT  /library/{listing}/standard           curate
//! ```

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, Created, Ids, JsonSchema, Params};
use jiff::Timestamp;
use sa_auth::Library;
use sa_core::gbp::Country;
use sa_panel::{
	Panel,
	gmbs::GoogleCategory,
	library::{Detail, Kind, Listing, NewListing, Published, Release, Sort},
};
use serde::{Deserialize, Serialize};

use crate::{
	api::{ApiResult, Id},
	categories::{ReleasedDto, VersionDto},
	gmbs::GoogleCategoryDto,
	http::{ApiRoute, USER},
	signin::Caller,
};

/// A listing: a released category of a trade in a country, numbered versions of it.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Listing")]
pub struct ListingDto {
	pub id: i64,
	pub trade: GoogleCategoryDto,
	pub country: Country,
	/// The author's name.
	pub author: String,
	/// The caller's own.
	pub mine: bool,
	pub title: String,
	pub about: String,
	pub forked_from: Option<i64>,
	/// In the standard library, at this place.
	pub standard_rank: Option<i64>,
	/// RFC 3339.
	pub created_at: String,
	/// RFC 3339: hidden from whoever never used it.
	pub withdrawn_at: Option<String>,
	pub latest: ReleasedDto,
	/// Categories whose lineage it is.
	pub used_by: i64,
}
impl ListingDto {
	pub(crate) fn of(l: Listing, caller: &Caller) -> eyre::Result<Self> {
		Ok(Self {
			// a gcid a refresh of Google's list dropped: its rows need migrating to what Google replaced it with
			trade: GoogleCategory::find(l.country, &l.trade)?.into(),
			country: l.country,
			mine: l.author == caller.user_id,
			author: l.author_name,
			id: l.id,
			title: l.title,
			about: l.about,
			forked_from: l.forked_from,
			standard_rank: l.standard_rank,
			created_at: l.created_at.to_string(),
			withdrawn_at: l.withdrawn_at.map(|t| t.to_string()),
			latest: l.latest.into(),
			used_by: l.used_by,
		})
	}
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Library")]
pub struct LibraryDto {
	pub listings: Vec<ListingDto>,
}

/// A listing's version and the category version it releases.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "ListingVersion")]
pub struct ListingVersionDto {
	pub release: ReleasedDto,
	pub body: VersionDto,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "ListingDetail")]
pub struct DetailDto {
	pub listing: ListingDto,
	/// Oldest first.
	pub versions: Vec<ListingVersionDto>,
	pub studies: Vec<PublishedStudyDto>,
}

/// A study published on a released category version.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "PublishedStudy")]
pub struct PublishedStudyDto {
	/// Its definition's id: `GET /placement/studies/{sha}`.
	pub sha: String,
	pub title: String,
	/// The author's name.
	pub author: String,
	/// RFC 3339.
	pub at: String,
	/// RFC 3339.
	pub withdrawn_at: Option<String>,
	/// Its area's slug, as it was frozen.
	pub area: String,
	pub listing: i64,
	pub listing_title: String,
	pub trade: GoogleCategoryDto,
	pub country: Country,
	/// The release its settings are.
	pub release: ReleasedDto,
}
impl PublishedStudyDto {
	pub(crate) fn of(p: Published) -> eyre::Result<Self> {
		Ok(Self {
			// a gcid a refresh of Google's list dropped: its rows need migrating to what Google replaced it with
			trade: GoogleCategory::find(p.country, &p.trade)?.into(),
			area: p.definition.area.slug().to_owned(),
			sha: p.sha,
			title: p.title,
			author: p.author_name,
			at: p.at.to_string(),
			withdrawn_at: p.withdrawn_at.map(|t| t.to_string()),
			listing: p.listing,
			listing_title: p.listing_title,
			country: p.country,
			release: p.on.into(),
		})
	}
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/library", USER, library),
		ApiRoute::new(M::POST, "/library", USER, publish_listing),
		ApiRoute::new(M::GET, "/library/{id}", USER, listing),
		ApiRoute::new(M::POST, "/library/{id}/versions", USER, publish_listing_version),
		ApiRoute::new(M::POST, "/library/{id}/withdraw", USER, withdraw_listing),
		ApiRoute::new(M::PUT, "/library/{id}/standard", USER.needs(Library::Curate), rank_listing),
	]
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct LibraryIn {
	/// A gcid.
	trade: Option<String>,
	country: Option<Country>,
	/// In the title, the about or the trade's names, whatever its case.
	q: Option<String>,
	kind: Option<Kind>,
	sort: Sort,
}

/// A new listing's first version.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NewListing")]
pub(crate) struct NewListingIn {
	/// One the caller's moves went to.
	version: i64,
	/// 1 to 100 characters.
	title: String,
	about: String,
	notes: String,
	/// The listing it was forked from.
	forked_from: Option<i64>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "NextListingVersion")]
pub(crate) struct NextIn {
	/// One the caller's moves went to, of the listing's trade and country.
	version: i64,
	notes: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RankIn {
	/// `null`: out of the standard library.
	rank: Option<i64>,
}

/// The listings the caller may see: not withdrawn, their own, or ones they used.
async fn library(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Params(at): Params<LibraryIn>) -> ApiResult<Json<LibraryDto>> {
	let listings = panel.library(caller.user_id, at.trade.as_deref(), at.country, at.q.as_deref(), at.kind, at.sort).await?;
	Ok(Json(LibraryDto {
		listings: listings.into_iter().map(|l| ListingDto::of(l, &caller)).collect::<eyre::Result<_>>()?,
	}))
}

/// A listing, its versions with what each says, and the studies published on it (404 one the
/// caller may not see).
async fn listing(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<Json<DetailDto>> {
	let Detail { listing, versions, studies } = panel.listing(caller.user_id, id).await??;
	Ok(Json(DetailDto {
		listing: ListingDto::of(listing, &caller)?,
		versions: versions
			.into_iter()
			.map(|(r, v)| {
				Ok(ListingVersionDto {
					release: r.into(),
					body: VersionDto::of(v)?,
				})
			})
			.collect::<eyre::Result<_>>()?,
		studies: studies.into_iter().map(PublishedStudyDto::of).collect::<eyre::Result<_>>()?,
	}))
}

/// Releases one of the caller's versions as a new listing: readable by everyone, for good.
///
/// 400 a title empty or past 100 characters; 404 a version not the caller's, a fork of a listing
/// the caller may not see.
async fn publish_listing(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<NewListingIn>) -> ApiResult<Created<ListingDto>> {
	let release = Release::New(NewListing {
		title: b.title,
		about: b.about,
		notes: b.notes,
		forked_from: b.forked_from,
	});
	let listing = panel.publish(caller.user_id, b.version, release, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, listing = listing.id, version = b.version, "listing published");
	Ok(Created(ListingDto::of(listing, &caller)?))
}

/// Releases one of the caller's versions as the next version of a listing of theirs.
///
/// 400 a version of another trade or country; 404 a listing not theirs or withdrawn; 409 the
/// listing's latest already.
async fn publish_listing_version(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>, Body(b): Body<NextIn>) -> ApiResult<Created<ListingDto>> {
	let listing = panel
		.publish(caller.user_id, b.version, Release::Next { listing: id, notes: b.notes }, Timestamp::now())
		.await??;
	tracing::info!(user_id = %caller.user_id, listing = id, n = listing.latest.n, "listing version published");
	Ok(Created(ListingDto::of(listing, &caller)?))
}

/// Withdraws a listing of the caller's: hidden from whoever never used it, readable by whoever did.
async fn withdraw_listing(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>) -> ApiResult<NoContent> {
	panel.withdraw(caller.user_id, id, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, listing = id, "listing withdrawn");
	Ok(NoContent)
}

/// Puts a listing in the standard library at a rank, or takes it out.
async fn rank_listing(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<i64>>, Body(b): Body<RankIn>) -> ApiResult<NoContent> {
	panel.rank(id, b.rank).await??;
	tracing::info!(user_id = %caller.user_id, listing = id, rank = ?b.rank, "listing ranked");
	Ok(NoContent)
}
