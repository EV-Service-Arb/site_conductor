//! The caller's categories (`sa_panel::categories`): their settings for a trade, one of Google's
//! categories, in a country, as versions a head moves over.
//!
//! ```text
//! GET    /categories                           the caller's: head, lineage, upstream
//! PUT    /categories/{trade}/{country}         Apply: a new version, the head onto it (edit)
//! POST   /categories/{trade}/{country}/head    adopt, update (a listing version) or revert
//! GET    /categories/{trade}/{country}/history the moves
//! DELETE /categories/{trade}/{country}
//! GET    /category-versions/{id}               one the caller stands on, or a released one
//! ```

use axum::{Extension, Json, extract::State, http::Method, response::NoContent};
use ev_lib::api::{Body, Ids, JsonSchema};
use gmaps_optimal_placement_core::{Demand, Excluded, Keyword};
use jiff::Timestamp;
use sa_core::gbp::Country;
use sa_panel::{
	Panel,
	categories::{Additional, Body as Settings, Key, Mine, Move, Version, Why},
	gmbs::GoogleCategory,
	library::Released,
};
use serde::{Deserialize, Serialize};

use crate::{
	api::ApiResult,
	gmbs::GoogleCategoryDto,
	http::{ApiRoute, USER},
	signin::Caller,
};

/// One version of a trade in a country, named for reading.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "CategoryVersion")]
pub struct VersionDto {
	pub id: i64,
	pub trade: GoogleCategoryDto,
	pub country: Country,
	/// The country's language, its keywords' and Google's form's: `fr`.
	pub lang: String,
	/// Lowercase, in the country's language, none naming a city: the terms every GBP of the
	/// category is scanned on, and each competitor of its studies scored on as weighed; the trade's
	/// name among them at weight 1.
	pub keywords: Vec<Keyword>,
	/// At most 9, in the order they are entered into Google's form: a competitor tier each.
	pub additional: Vec<AdditionalDto>,
	pub excluded: Excluded,
	pub demand: Option<Demand>,
	/// RFC 3339.
	pub at: String,
}
impl VersionDto {
	pub(crate) fn of(v: Version) -> eyre::Result<Self> {
		// a gcid a refresh of Google's list dropped: its rows need migrating to what Google replaced it with
		let google = |id: &str| GoogleCategory::find(v.country, id).map(GoogleCategoryDto::from);
		Ok(Self {
			id: v.id,
			trade: google(&v.trade)?,
			country: v.country,
			lang: v.country.lang().to_owned(),
			additional: v
				.body
				.additional
				.iter()
				.map(|a| {
					Ok(AdditionalDto {
						google: google(&a.id)?,
						weight: a.weight,
					})
				})
				.collect::<eyre::Result<_>>()?,
			keywords: v.body.keywords,
			excluded: v.body.excluded,
			demand: v.body.demand,
			at: v.at.to_string(),
		})
	}
}

/// An additional Google category and what one competitor of its tier weighs against one of the
/// trade's.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "AdditionalCategory")]
pub struct AdditionalDto {
	pub google: GoogleCategoryDto,
	pub weight: f64,
}

/// A category of the caller's: their settings for a trade in a country.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Category")]
pub struct CategoryDto {
	pub trade: GoogleCategoryDto,
	pub country: Country,
	pub head: VersionDto,
	/// Where it was last adopted or updated from; `null`: its own.
	pub lineage: Option<LineageDto>,
}
impl CategoryDto {
	fn of(m: Mine) -> eyre::Result<Self> {
		Ok(Self {
			trade: m.trade.into(),
			country: m.country,
			head: VersionDto::of(m.head)?,
			lineage: m.lineage.map(|l| LineageDto {
				listing: l.listing.id,
				title: l.listing.title,
				withdrawn: l.listing.withdrawn_at.is_some(),
				from: l.from.into(),
				edited: l.edited,
				upstream: l.upstream.map(ReleasedDto::from),
			}),
		})
	}
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Lineage")]
pub struct LineageDto {
	pub listing: i64,
	pub title: String,
	/// What it took.
	pub from: ReleasedDto,
	/// Its head is not what it took.
	pub edited: bool,
	pub withdrawn: bool,
	/// The listing's latest, newer than what it took; never taken but by the caller.
	pub upstream: Option<ReleasedDto>,
}

/// A listing's numbered version.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Release")]
pub struct ReleasedDto {
	/// The listing version's id.
	pub id: i64,
	pub listing: i64,
	pub n: i64,
	/// The category version it releases.
	pub version: i64,
	pub notes: String,
	/// RFC 3339.
	pub published_at: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Categories")]
pub struct CategoriesDto {
	pub categories: Vec<CategoryDto>,
}

/// A move of a category's head.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "CategoryMove")]
pub struct MoveDto {
	pub id: i64,
	pub why: Why,
	/// `null`: the category's creation.
	pub from: Option<i64>,
	pub to: i64,
	/// An adopt's or an update's.
	pub listing_version: Option<i64>,
	/// RFC 3339.
	pub at: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "CategoryHistory")]
pub struct HistoryDto {
	/// Oldest first, since it was set up.
	pub moves: Vec<MoveDto>,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/categories", USER, categories),
		ApiRoute::new(M::PUT, "/categories/{trade}/{country}", USER, apply_category),
		ApiRoute::new(M::DELETE, "/categories/{trade}/{country}", USER, delete_category),
		ApiRoute::new(M::POST, "/categories/{trade}/{country}/head", USER, move_category_head),
		ApiRoute::new(M::GET, "/categories/{trade}/{country}/history", USER, category_history),
		ApiRoute::new(M::GET, "/category-versions/{id}", USER, category_version),
	]
}

impl From<Released> for ReleasedDto {
	fn from(r: Released) -> Self {
		Self {
			id: r.id,
			listing: r.listing,
			n: r.n,
			version: r.version,
			notes: r.notes,
			published_at: r.published_at.to_string(),
		}
	}
}

impl From<Move> for MoveDto {
	fn from(m: Move) -> Self {
		Self {
			id: m.id,
			why: m.why,
			from: m.from,
			to: m.to,
			listing_version: m.listing_version,
			at: m.at.to_string(),
		}
	}
}

/// A path's category: a gcid and a country.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct CategoryIn {
	/// `gcid:plumber`.
	pub(crate) trade: String,
	pub(crate) country: Country,
}
impl CategoryIn {
	pub(crate) fn key(&self, caller: &Caller) -> ApiResult<Key> {
		Ok(Key::try_new(caller.user_id, &self.trade, self.country)?)
	}
}

#[derive(Deserialize, JsonSchema)]
pub(crate) struct VersionIn {
	id: i64,
}

/// What a version says.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CategorySettings")]
pub(crate) struct SettingsIn {
	/// In the country's language, each once normalized; the trade's name among them at weight 1.
	keywords: Vec<Keyword>,
	/// At most 9, none the trade.
	additional: Vec<AdditionalIn>,
	excluded: Excluded,
	demand: Option<Demand>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AdditionalIn {
	/// A gcid Google lists in the country.
	id: String,
	/// Positive.
	weight: f64,
}

/// Where a head moves.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeadIn {
	version: i64,
	/// The listing version releasing `version`: an adopt, or an update from the lineage's listing.
	/// `null`: a revert to one of the category's own.
	listing_version: Option<i64>,
}

/// The caller's categories, by trade and country.
async fn categories(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<CategoriesDto>> {
	let mine = panel.categories(caller.user_id).await?;
	Ok(Json(CategoriesDto {
		categories: mine.into_iter().map(CategoryDto::of).collect::<eyre::Result<_>>()?,
	}))
}

/// Applies settings to the caller's category of a trade in a country, setting it up if it was not:
/// a new version, the head moved onto it. Settings equal to the head's move nothing.
///
/// 400 a gcid Google does not list in the country, a keyword twice once normalized, keywords
/// without the trade's name at weight 1, a weight not positive, a gcid twice, past 9 additional
/// ones, an excluded word twice or a type that is none, a demand with an empty expression or a
/// column twice.
async fn apply_category(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(at): Ids<CategoryIn>, Body(b): Body<SettingsIn>) -> ApiResult<Json<CategoryDto>> {
	let key = at.key(&caller)?;
	let settings = Settings {
		keywords: b.keywords,
		additional: b.additional.into_iter().map(|a| Additional { id: a.id, weight: a.weight }).collect(),
		excluded: b.excluded,
		demand: b.demand,
	};
	let set = panel.apply(key, settings, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, trade = key.trade.id, country = %key.country, head = set.head.id, "category applied");
	Ok(Json(CategoryDto::of(set)?))
}

/// Moves the caller's category's head: onto a listing's version (an adopt, setting it up if it was
/// not; an update when the listing is its lineage's and the version newer), or back to one of its
/// own (a revert). Onto its head moves nothing.
///
/// 400 a listing version not releasing `version`, a listing of another trade or country, a version
/// not of its own history; 404 a listing the caller may not see, a category not set up to revert.
async fn move_category_head(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(at): Ids<CategoryIn>, Body(b): Body<HeadIn>) -> ApiResult<Json<CategoryDto>> {
	let key = at.key(&caller)?;
	let moved = panel.move_head(key, b.version, b.listing_version, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, trade = key.trade.id, country = %key.country, head = moved.head.id, "category's head moved");
	Ok(Json(CategoryDto::of(moved)?))
}

/// The caller's category's moves since it was set up, oldest first.
async fn category_history(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(at): Ids<CategoryIn>) -> ApiResult<Json<HistoryDto>> {
	let moves = panel.history(at.key(&caller)?).await??;
	Ok(Json(HistoryDto {
		moves: moves.into_iter().map(MoveDto::from).collect(),
	}))
}

/// Takes the caller's category away; its history stays.
///
/// 409 while a live GBP, a review draft or a brand holds it.
async fn delete_category(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(at): Ids<CategoryIn>) -> ApiResult<NoContent> {
	let key = at.key(&caller)?;
	panel.delete_category(key).await??;
	tracing::info!(user_id = %caller.user_id, trade = key.trade.id, country = %key.country, "category deleted");
	Ok(NoContent)
}

/// A category version: one the caller's moves went to, or one a listing released (404 otherwise).
async fn category_version(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(VersionIn { id }): Ids<VersionIn>) -> ApiResult<Json<VersionDto>> {
	Ok(Json(VersionDto::of(panel.version(caller.user_id, id).await??)?))
}
