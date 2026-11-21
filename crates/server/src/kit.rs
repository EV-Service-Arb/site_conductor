//! The review kit (`sa_panel::kit`): the owner's routes under `/api/v1`, and the landings' under
//! `/api/kit/v1`, signed as ingest is and answered here.
//!
//! ```text
//! GET    /api/kit/v1/offer?brand&slug[&qr=1]                  KitOffer | 404 the kit off
//! GET    /api/kit/v1/photos/{p}.jpg?view=                     image/jpeg, offered to that view
//! POST   /api/kit/v1/events {view, copied: d} | {view, saved: [p]}   204; 409 went there already
//! ```
//!
//! A landing signs as ingest's sources do (a GET over `GET <path?query>`); the key's brands must
//! hold the place's. (brand, slug) ↔ GBP is sites': `GET /api/conductor/places` (`feeds::Places`), kept 60 s.

use std::{
	sync::{Arc, Mutex},
	time::{Duration, Instant},
};

use axum::{
	Extension, Json, Router,
	body::{Bytes, to_bytes},
	error_handling::HandleErrorLayer,
	extract::{FromRequest, OriginalUri, Path, Query, Request, State, rejection::QueryRejection},
	http::{HeaderMap, HeaderValue, Method, StatusCode, header},
	response::{IntoResponse, NoContent, Response},
	routing::{get, post},
};
use ev_lib::api::{
	Body, Created, GenContext, Ids, JsonSchema, Malformed, OperationInput, OperationOutput, Params, Refusal,
	openapi::{MediaType, Operation, ReferenceOr, RequestBody, Response as Answer, SchemaObject, StatusCode as Status},
	schemars::json_schema,
};
use jiff::Timestamp;
use sa_auth::SourceKind;
use sa_core::{
	feeds::{self, Place, Places},
	gbp::{Country, State as GbpState},
	ids::BrandId,
	prices::{PlacesRequest, Spends},
};
use sa_panel::{
	Grant, Panel, SignedBatch,
	categories::Key,
	kit::{Event, Funnel, Kit, Offer, PHOTO_MAX, Pool, PutKit, Ranked, Scope, Target},
	store::kit::Happening,
};
use sa_places::LatLng;
use serde::{Deserialize, Serialize};
use tower::{BoxError, ServiceBuilder};
use tower_http::timeout::{RequestBodyTimeoutLayer, TimeoutLayer};
use uuid::Uuid;

use crate::{
	api::{ApiResult, Code, Id},
	areas::SlugIn,
	brands::BrandDto,
	categories::CategoryIn,
	forward::Services,
	gmbs::GoogleCategoryDto,
	http::{ApiRoute, ApiState, Limits, USER},
	ingest,
	signin::Caller,
};

/// The landings' routes sit under it.
pub const LANDING: &str = "/api/kit/v1";

/// How long sites' places are kept.
const PAGES_FOR: Duration = Duration::from_secs(60);

/// The landing's path a GBP's page is at, under its site key's origin.
const PAGE: &str = "/avis/";

/// The places as last read, and when.
type Kept = (Instant, Arc<Vec<Place>>);

/// What a [`Picture`] is answered as.
pub(crate) trait Format {
	const MIME: &'static str;
	const CACHE: &'static str;
	const DESCRIPTION: &'static str;
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitDraft")]
pub(crate) struct DraftDto {
	pub id: i64,
	pub text: String,
	/// Areas it is offered in; none: everywhere.
	pub places: Vec<String>,
	/// The category's keywords its text carries.
	pub keywords: Vec<String>,
	/// Areas its text names that `places` lacks: it is offered outside them.
	pub names: Vec<String>,
	/// Copies, every GBP.
	pub uses: i64,
	pub created_at: String,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitPhoto")]
pub(crate) struct PhotoDto {
	pub id: i64,
	/// What it shows, for the reviewer; `null`: not labelled yet.
	pub shows: Option<String>,
	pub width: i64,
	pub height: i64,
	/// Saved, every GBP.
	pub uses: i64,
	pub created_at: String,
}

/// A category's drafts.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitPool")]
pub(crate) struct PoolDto {
	pub drafts: Vec<DraftDto>,
	/// The category's keywords no draft carries.
	pub unmentioned: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "NewKitDraft")]
#[serde(deny_unknown_fields)]
pub(crate) struct NewDraftDto {
	pub text: String,
	/// Area slugs; none: everywhere.
	pub places: Vec<String>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "EditKitDraft")]
#[serde(deny_unknown_fields)]
pub(crate) struct EditDraftDto {
	pub text: Option<String>,
	pub places: Option<Vec<String>>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitRankedPhoto")]
pub(crate) struct RankedPhotoDto {
	pub id: i64,
	pub shows: Option<String>,
	pub width: i64,
	pub height: i64,
	pub uses: i64,
}

/// A draft as the offer weighs it for one GBP.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitRanked")]
pub(crate) struct RankedDto {
	pub id: i64,
	pub text: String,
	pub uses: i64,
	/// Copied on this GBP.
	pub went_here: bool,
	/// Held by a reviewer's page open now.
	pub held: bool,
	/// Among what a reviewer is offered now.
	pub offered: bool,
	/// Its brand's photos it is offered with.
	pub photos: Vec<RankedPhotoDto>,
}

/// What was collected since the period's start.
#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitTally")]
pub(crate) struct TallyDto {
	/// QRs shown.
	pub shown: i64,
	/// Pages opened from the QR.
	pub scanned: i64,
	/// Pages opened, from anywhere.
	pub opened: i64,
	pub copied: i64,
	pub photos_saved: i64,
	/// Reviews from a copy on Google.
	pub on_google: i64,
	/// Of those, gone off it.
	pub gone: i64,
	/// Reviews from no copy.
	pub other_reviews: i64,
}

impl From<Funnel> for TallyDto {
	fn from(f: Funnel) -> Self {
		Self {
			shown: f.shown,
			scanned: f.scanned,
			opened: f.opened,
			copied: f.copied,
			photos_saved: f.photos_saved,
			on_google: f.on_google,
			gone: f.gone,
			other_reviews: f.other_reviews,
		}
	}
}

/// A GBP's kit.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "GmbKit")]
pub(crate) struct GmbKitDto {
	/// `null`: off.
	pub target: Option<Target>,
	/// What its QR encodes; `null` while off.
	pub url: Option<String>,
	/// Its page on its landing: `null` without one, and then only `google` is open to it.
	pub page: Option<String>,
	/// Whose photos its drafts carry: `null`, none.
	pub brand: Option<BrandDto>,
	/// Its category's drafts in its places, in the offer's order.
	pub drafts: Vec<RankedDto>,
	pub tally: TallyDto,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "SetKit")]
#[serde(deny_unknown_fields)]
pub(crate) struct SetKitDto {
	pub target: Option<Target>,
}

/// One of the caller's GBPs in or next to an area.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitRow")]
pub(crate) struct KitRowDto {
	pub slug: String,
	pub name: String,
	pub area: Option<String>,
	pub members: i64,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Kits")]
pub(crate) struct KitsDto {
	pub kits: Vec<KitRowDto>,
}

#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "PutKit")]
#[serde(deny_unknown_fields)]
pub(crate) struct PutKitDto {
	pub name: String,
	/// An area slug, a reference only: membership is `gmbs`.
	pub area: Option<String>,
	pub gmbs: Vec<i64>,
}

/// Why a member collects nothing, or only through Google's form.
#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "KitBlock")]
pub(crate) enum KitBlock {
	/// Not on Google yet.
	Draft,
	/// Google is verifying it: it waits.
	Verifying,
	Suspended,
	/// Its kit is off.
	Off,
	/// No landing page of its: Google's form only.
	NoPage,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitCategory")]
pub(crate) struct CategoryDto {
	/// A gcid.
	pub trade: String,
	pub country: Country,
}

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitLeft")]
pub(crate) struct LeftDto {
	pub drafts: i64,
	pub photos: i64,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitMember")]
pub(crate) struct MemberDto {
	pub id: i64,
	pub name: String,
	pub state: GbpState,
	pub address: Option<String>,
	pub location: Option<LatLng>,
	pub category: Option<CategoryDto>,
	pub target: Option<Target>,
	/// Its landing page; `null` without one.
	pub page: Option<String>,
	/// What its QR encodes; `null` while off.
	pub url: Option<String>,
	/// Since the period's start or its joining, the later.
	pub tally: TallyDto,
	/// What the offer still has for it.
	pub left: LeftDto,
	pub blocked: Option<KitBlock>,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitPoolSummary")]
pub(crate) struct PoolSummaryDto {
	pub trade: GoogleCategoryDto,
	pub country: Country,
	pub drafts: i64,
	pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "KitHappening")]
pub(crate) enum HappeningDto {
	Shown,
	Scanned,
	Opened,
	Copied,
	Saved,
	/// A review from a copy.
	Posted,
	/// A review from no copy.
	Reviewed,
	/// A review from a copy, gone.
	Gone,
}

#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "KitEvent")]
pub(crate) struct EventOutDto {
	pub at: String,
	pub gmb: i64,
	pub kind: HappeningDto,
	/// The draft, the photo, or the review's rating.
	pub detail: Option<i64>,
}

/// A named selection of the caller's GBPs, over a period.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Kit")]
pub(crate) struct KitDto {
	pub slug: String,
	pub name: String,
	pub area: Option<String>,
	/// The members' summed: a GBP in two kits counts in both.
	pub tally: TallyDto,
	/// In the order they joined.
	pub gmbs: Vec<MemberDto>,
	pub pools: Vec<PoolSummaryDto>,
	/// Newest first, at most 20.
	pub latest: Vec<EventOutDto>,
}

impl From<Happening> for HappeningDto {
	fn from(h: Happening) -> Self {
		match h {
			Happening::Shown => Self::Shown,
			Happening::Scanned => Self::Scanned,
			Happening::Opened => Self::Opened,
			Happening::Copied => Self::Copied,
			Happening::Saved => Self::Saved,
			Happening::Posted => Self::Posted,
			Happening::Reviewed => Self::Reviewed,
			Happening::Gone => Self::Gone,
		}
	}
}

/// The period's start: unix seconds (the client's local midnight for "today").
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Since {
	since: i64,
}

impl Since {
	fn at(&self) -> ApiResult<Timestamp> {
		Timestamp::from_second(self.since).map_err(|e| Refusal::new(Code::BadRequest, format!("since: {e}")))
	}
}

/// What a reviewer's page shows (`/api/kit/v1/offer`).
#[derive(Clone, Debug, Serialize)]
pub(crate) struct OfferDto {
	pub target: Target,
	pub place_id: String,
	pub name: String,
	pub lang: String,
	/// Google's form for the place (Places' writeAReviewUri).
	pub review_uri: String,
	pub view: Uuid,
	pub drafts: Vec<OfferedDto>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OfferedDto {
	pub id: i64,
	pub text: String,
	pub photos: Vec<OfferedPhotoDto>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct OfferedPhotoDto {
	pub id: i64,
	/// What it shows: its alt and caption.
	pub shows: Option<String>,
	pub width: i64,
	pub height: i64,
}

/// Which GBP each landing page is: sites' places, kept [`PAGES_FOR`].
#[derive(Clone)]
pub(crate) struct Pages {
	services: Services,
	kept: Arc<Mutex<Option<Kept>>>,
}
impl Pages {
	pub(crate) fn new(services: Services) -> Self {
		Self { services, kept: Arc::default() }
	}

	pub(crate) async fn all(&self) -> Result<Arc<Vec<Place>>, Refusal<Code>> {
		if let Some((at, places)) = self.kept.lock().expect("never poisoned: no panic while held").as_ref()
			&& at.elapsed() < PAGES_FOR
		{
			return Ok(places.clone());
		}
		let fetched: Places = self.services.conductor_get(feeds::PLACES).await.map_err(|e| {
			tracing::warn!(error = %e, "kit: sites' places");
			Refusal::new(Code::Unavailable, "sites did not say which page is which place")
		})?;
		let places = Arc::new(fetched.places);
		*self.kept.lock().expect("never poisoned: no panic while held") = Some((Instant::now(), places.clone()));
		Ok(places)
	}

	async fn of_gmb(&self, gmb: i64) -> Result<Option<Place>, Refusal<Code>> {
		Ok(self.all().await?.iter().find(|p| p.gmb_id == gmb).cloned())
	}

	async fn gmb_at(&self, brand: &BrandId, slug: &str) -> Result<Option<i64>, Refusal<Code>> {
		Ok(self.all().await?.iter().find(|p| p.brand == brand.as_str() && p.slug == slug).map(|p| p.gmb_id))
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EventDto {
	view: Uuid,
	copied: Option<i64>,
	saved: Option<Vec<i64>>,
}

impl From<Pool> for PoolDto {
	fn from(Pool { drafts, unmentioned }: Pool) -> Self {
		Self {
			unmentioned,
			drafts: drafts
				.into_iter()
				.map(|d| DraftDto {
					id: d.id,
					text: d.text,
					places: d.places,
					keywords: d.keywords,
					names: d.names,
					uses: d.uses,
					created_at: d.created_at.to_string(),
				})
				.collect(),
		}
	}
}

fn ranked(drafts: Vec<Ranked>, target: Option<Target>) -> Vec<RankedDto> {
	let shown = match target {
		Some(Target::Drafts) => sa_panel::kit::OFFERED,
		Some(Target::OneTap) => 1,
		Some(Target::Google) | None => 0,
	};
	let mut open = 0;
	drafts
		.into_iter()
		.map(|d| {
			let offered = !d.went_here && !d.held && open < shown;
			open += usize::from(!d.went_here && !d.held);
			RankedDto {
				id: d.id,
				text: d.text,
				uses: d.uses,
				went_here: d.went_here,
				held: d.held,
				offered,
				photos: d
					.photos
					.into_iter()
					.map(|p| RankedPhotoDto {
						id: p.id,
						shows: p.shows,
						width: p.width,
						height: p.height,
						uses: p.uses,
					})
					.collect(),
			}
		})
		.collect()
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	vec![
		ApiRoute::new(M::GET, "/categories/{trade}/{country}/kit", USER, kit_pool),
		ApiRoute::new(M::POST, "/categories/{trade}/{country}/kit/drafts", USER, add_kit_draft),
		ApiRoute::new(M::PATCH, "/categories/{trade}/{country}/kit/drafts/{draft}", USER, edit_kit_draft),
		ApiRoute::new(M::DELETE, "/categories/{trade}/{country}/kit/drafts/{draft}", USER, remove_kit_draft),
		ApiRoute::new(M::GET, "/gmbs/{id}/kit", USER, gmb_kit),
		ApiRoute::new(M::PUT, "/gmbs/{id}/kit", USER, set_gmb_kit),
		ApiRoute::new(M::POST, "/gmbs/{id}/kit/shown", USER, kit_shown),
		ApiRoute::new(M::GET, "/kits", USER, kits),
		ApiRoute::new(M::GET, "/kits/{slug}", USER, kit),
		ApiRoute::new(M::PUT, "/kits/{slug}", USER, put_kit),
		ApiRoute::new(M::DELETE, "/kits/{slug}", USER, delete_kit),
	]
}

#[derive(Deserialize, JsonSchema)]
struct DraftIn {
	/// A gcid.
	trade: String,
	country: Country,
	draft: i64,
}

/// `<id>.jpg` → id.
pub(crate) fn jpg(file: &str) -> ApiResult<i64> {
	file.strip_suffix(".jpg").and_then(|id| id.parse().ok()).ok_or_else(|| Refusal::new(Code::NotFound, "not found"))
}

/// A photo, never changed under its id.
pub(crate) struct Jpeg;

impl Format for Jpeg {
	const CACHE: &'static str = "private, max-age=31536000, immutable";
	const DESCRIPTION: &'static str = "A JPEG.";
	const MIME: &'static str = "image/jpeg";
}

/// An image's bytes, in its format.
pub(crate) struct Picture<F>(pub(crate) Vec<u8>, pub(crate) F);

impl<F: Format> IntoResponse for Picture<F> {
	fn into_response(self) -> Response {
		([(header::CONTENT_TYPE, F::MIME), (header::CACHE_CONTROL, F::CACHE)], self.0).into_response()
	}
}

impl<F: Format> OperationOutput for Picture<F> {
	type Inner = Self;

	fn operation_response(_: &mut GenContext, _: &mut Operation) -> Option<Answer> {
		Some(Answer {
			description: F::DESCRIPTION.to_owned(),
			content: [(F::MIME.into(), MediaType::default())].into_iter().collect(),
			..Answer::default()
		})
	}

	fn inferred_responses(ctx: &mut GenContext, op: &mut Operation) -> Vec<(Option<Status>, Answer)> {
		Self::operation_response(ctx, op).map(|r| (Some(Status::Code(200)), r)).into_iter().collect()
	}
}

/// An uploaded photo: JPEG, PNG or WebP, ≤ [`PHOTO_MAX`].
pub(crate) struct Image(pub(crate) Bytes);

impl<S: Send + Sync> FromRequest<S> for Image {
	type Rejection = Refusal<Malformed>;

	async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
		Bytes::from_request(req, state).await.map(Self).map_err(|e| match e.status() {
			StatusCode::PAYLOAD_TOO_LARGE => Refusal::new(Malformed::TooLarge, format!("a photo is {} MiB at most", PHOTO_MAX >> 20)),
			_ => Refusal::new(Malformed::BadRequest, e.body_text()),
		})
	}
}

impl OperationInput for Image {
	fn operation_input(_: &mut GenContext, op: &mut Operation) {
		op.request_body = Some(ReferenceOr::Item(RequestBody {
			content: ["image/jpeg", "image/png", "image/webp"]
				.into_iter()
				.map(|t| {
					let photo = SchemaObject {
						json_schema: json_schema!({ "type": "string", "format": "binary", "maxLength": PHOTO_MAX }),
						example: None,
						external_docs: None,
					};
					(
						t.into(),
						MediaType {
							schema: Some(photo),
							..MediaType::default()
						},
					)
				})
				.collect(),
			required: true,
			..RequestBody::default()
		}));
	}

	fn inferred_early_responses(ctx: &mut GenContext, op: &mut Operation) -> Vec<(Option<Status>, Answer)> {
		Refusal::<Malformed>::inferred_responses(ctx, op)
	}
}

/// The category's drafts.
async fn kit_pool(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(at): Ids<CategoryIn>) -> ApiResult<Json<PoolDto>> {
	Ok(Json(panel.kit_pool(at.key(&caller)?).await??.into()))
}

/// Adds a draft to the category; answers the category's drafts.
async fn add_kit_draft(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(at): Ids<CategoryIn>, Body(b): Body<NewDraftDto>) -> ApiResult<Created<PoolDto>> {
	let key = at.key(&caller)?;
	let draft = panel.add_draft(key, &b.text, &b.places, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, trade = key.trade.id, country = %key.country, draft, "kit draft added");
	Ok(Created(panel.kit_pool(key).await??.into()))
}

/// Edits a draft's text or its places; answers the category's drafts.
///
/// 409 one already copied: a use is of the text it had.
async fn edit_kit_draft(
	State(panel): State<Panel>,
	Extension(caller): Extension<Caller>,
	Ids(DraftIn { trade, country, draft }): Ids<DraftIn>,
	Body(b): Body<EditDraftDto>,
) -> ApiResult<Json<PoolDto>> {
	let key = Key::try_new(caller.user_id, &trade, country)?;
	panel.edit_draft(key, draft, b.text.as_deref(), b.places.as_deref()).await??;
	Ok(Json(panel.kit_pool(key).await??.into()))
}

/// Removes a draft: out of every offer, its uses kept.
async fn remove_kit_draft(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(DraftIn { trade, country, draft }): Ids<DraftIn>) -> ApiResult<NoContent> {
	panel.remove_draft(Key::try_new(caller.user_id, &trade, country)?, draft, Timestamp::now()).await??;
	Ok(NoContent)
}

/// The landing page of the GBP, `<origin>/avis/<slug>`: the origin of the active site key of its
/// place's site. 409 when its brand is on another site: its photos would show on the wrong one.
async fn page_of(s: &ApiState, gmb: i64, site: Option<&BrandId>) -> ApiResult<Option<String>> {
	let pages = s.pages.as_ref().ok_or_else(|| Refusal::new(Code::Unavailable, "sites not connected: no landing page is known"))?;
	let Some(place) = pages.of_gmb(gmb).await? else {
		return Ok(None);
	};
	if let Some(site) = site
		&& site.as_str() != place.brand
	{
		return Err(Refusal::new(Code::Conflict, format!("its brand is on {site}, its page on {}: move one", place.brand)));
	}
	let mut origins: Vec<String> = s
		.sign_in
		.panel
		.store()
		.sources()
		.await?
		.into_iter()
		.filter(|k| k.revoked_at.is_none() && k.grant.kind == SourceKind::Site && k.grant.brands.iter().any(|b| b.as_str() == place.brand))
		.filter_map(|k| k.origin.map(|o| o.to_string()))
		.collect();
	origins.sort();
	origins.dedup();
	match <[String; 1]>::try_from(origins) {
		Ok([origin]) => Ok(Some(format!("{origin}{PAGE}{}", place.slug))),
		Err(none) if none.is_empty() => Ok(None),
		Err(many) => Err(Refusal::new(
			Code::Conflict,
			format!("{}'s site keys have {} origins: {}", place.brand, many.len(), many.join(", ")),
		)),
	}
}

/// What the QR of a GBP whose page is `page` encodes.
/// From a kit, `&kit=<id>` tags the views it opens with it; Google's own link carries nothing.
fn qr_url(page: Option<&String>, target: Option<Target>, review_uri: Option<String>, kit: Option<i64>) -> Option<String> {
	match (page, target) {
		(_, None) => None,
		(Some(page), Some(_)) => Some(match kit {
			Some(k) => format!("{page}?qr&kit={k}"),
			None => format!("{page}?qr"),
		}),
		(None, Some(_)) => review_uri,
	}
}

async fn kit_dto(s: &ApiState, caller: &Caller, gmb: i64, since: Timestamp) -> ApiResult<GmbKitDto> {
	let Kit {
		target,
		review_uri,
		brand,
		drafts,
		funnel,
	} = s.sign_in.panel.kit(caller.user_id, gmb, since, Timestamp::now()).await??;
	let page = page_of(s, gmb, brand.as_ref().map(|b| &b.site)).await?;
	Ok(GmbKitDto {
		target,
		url: qr_url(page.as_ref(), target, review_uri, None),
		page,
		brand: brand.map(Into::into),
		drafts: ranked(drafts, target),
		tally: funnel.into(),
	})
}

/// The GBP's kit: where its QR leads, its category's drafts as its offer weighs them, and what it
/// collected since `since`.
async fn gmb_kit(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Params(q): Params<Since>) -> ApiResult<Json<GmbKitDto>> {
	Ok(Json(kit_dto(&s, &caller, gmb, q.at()?).await?))
}

/// Points the GBP's QR at `target`, `null` turning the kit off. On, its review link is asked of
/// Places once (a Place Details, paid as a lookup).
///
/// 409 no place on Google yet, or `drafts` / `one_tap` without a landing page of its.
async fn set_gmb_kit(
	State(s): State<ApiState>,
	Spends(caller, by): Spends<PlacesRequest, Caller>,
	Ids(Id { id: gmb }): Ids<Id<i64>>,
	Body(SetKitDto { target }): Body<SetKitDto>,
) -> ApiResult<Json<GmbKitDto>> {
	let site = s.sign_in.panel.kit(caller.user_id, gmb, Timestamp::now(), Timestamp::now()).await??.brand.map(|b| b.site);
	if matches!(target, Some(Target::Drafts | Target::OneTap)) && page_of(&s, gmb, site.as_ref()).await?.is_none() {
		return Err(Refusal::new(Code::Conflict, "no landing page of its: only Google's form"));
	}
	s.sign_in.panel.set_kit_target(by, gmb, target, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, gmb_id = gmb, ?target, "kit target set");
	Ok(Json(kit_dto(&s, &caller, gmb, Timestamp::now()).await?))
}

/// The operator showed the GBP's QR: the tally's `shown`, tagged with the kit whose page showed it.
///
/// 404 a kit the GBP is not in.
async fn kit_shown(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: gmb }): Ids<Id<i64>>, Params(q): Params<ShownIn>) -> ApiResult<NoContent> {
	panel.kit_shown(caller.user_id, gmb, q.kit.as_deref(), Timestamp::now()).await??;
	Ok(NoContent)
}

/// The caller's kits.
async fn kits(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<KitsDto>> {
	Ok(Json(KitsDto {
		kits: panel
			.kits(caller.user_id)
			.await?
			.into_iter()
			.map(|k| KitRowDto {
				slug: k.slug,
				name: k.name,
				area: k.area,
				members: k.members,
			})
			.collect(),
	}))
}

/// The caller's kit over the period from `since`: what its GBPs collected, each GBP's QR and what
/// is left to offer it, its categories' pools and the latest of what happened.
async fn kit(State(s): State<ApiState>, Extension(caller): Extension<Caller>, Ids(SlugIn { slug }): Ids<SlugIn>, Params(q): Params<Since>) -> ApiResult<Json<KitDto>> {
	let Scope {
		id,
		slug,
		name,
		area,
		members,
		pools,
		latest,
	} = s.sign_in.panel.kit_scope(caller.user_id, &slug, q.at()?, Timestamp::now()).await??;
	let mut tally = Funnel::default();
	let mut gmbs = Vec::with_capacity(members.len());
	for m in members {
		tally += m.tally;
		let page = page_of(&s, m.gmb.id, m.brand.as_ref().map(|b| &b.site)).await?;
		let blocked = match (m.gmb.state, m.target, &page) {
			(GbpState::Draft, ..) => Some(KitBlock::Draft),
			(GbpState::Verifying, ..) => Some(KitBlock::Verifying),
			(GbpState::Suspended, ..) => Some(KitBlock::Suspended),
			(GbpState::Verified, None, _) => Some(KitBlock::Off),
			(GbpState::Verified, Some(_), None) => Some(KitBlock::NoPage),
			(GbpState::Verified, Some(_), Some(_)) => None,
		};
		gmbs.push(MemberDto {
			id: m.gmb.id,
			url: qr_url(page.as_ref(), m.target, m.review_uri, Some(id)),
			page,
			blocked,
			name: m.gmb.name,
			state: m.gmb.state,
			address: m.gmb.address,
			location: m.gmb.location.map(|p| p.at),
			category: m.gmb.trade.clone().zip(m.gmb.country).map(|(trade, c)| CategoryDto { trade, country: c.code }),
			target: m.target,
			tally: m.tally.into(),
			left: LeftDto {
				drafts: m.left.drafts,
				photos: m.left.photos,
			},
		});
	}
	Ok(Json(KitDto {
		slug,
		name,
		area,
		tally: tally.into(),
		gmbs,
		pools: pools
			.into_iter()
			.map(|p| PoolSummaryDto {
				trade: p.trade.into(),
				country: p.country,
				drafts: p.drafts,
				warnings: p.warnings,
			})
			.collect(),
		latest: latest
			.into_iter()
			.map(|e| EventOutDto {
				at: e.at.to_string(),
				gmb: e.gmb,
				kind: e.kind.into(),
				detail: e.detail,
			})
			.collect(),
	}))
}

/// Makes the caller's kit `slug`, or sets it over what it was: a GBP that stays keeps when it
/// joined. `new` is no kit's.
///
/// 400 a bad slug or name, or no such area; 409 a GBP not the caller's.
async fn put_kit(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(SlugIn { slug }): Ids<SlugIn>, Body(b): Body<PutKitDto>) -> ApiResult<NoContent> {
	let kit = PutKit {
		name: b.name,
		area: b.area,
		gmbs: b.gmbs,
	};
	panel.put_kit(caller.user_id, &slug, kit, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, slug, "kit set");
	Ok(NoContent)
}

/// Removes the caller's kit: its slug is free again; nothing it collected goes, its events keep its
/// tag.
async fn delete_kit(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(SlugIn { slug }): Ids<SlugIn>) -> ApiResult<NoContent> {
	panel.remove_kit(caller.user_id, &slug, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, slug, "kit deleted");
	Ok(NoContent)
}

/// What the landings' routes are served on.
#[derive(Clone)]
pub(crate) struct Landing {
	panel: Panel,
	pages: Pages,
}

/// The landings' routes, bounded as ingest's.
pub(crate) fn landing_routes(panel: Panel, pages: Pages, limits: &Limits) -> Router {
	let layers = ServiceBuilder::new()
		.layer(HandleErrorLayer::new(|_: BoxError| async { ingest::error(StatusCode::SERVICE_UNAVAILABLE, "busy, try again") }))
		.load_shed()
		.concurrency_limit(limits.max_concurrent)
		.layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, limits.request_timeout))
		.layer(RequestBodyTimeoutLayer::new(limits.body_timeout));
	Router::new()
		.route(&format!("{LANDING}/offer"), get(offer))
		.route(&format!("{LANDING}/photos/{{file}}"), get(offered_photo))
		.route(&format!("{LANDING}/events"), post(event))
		.layer(layers)
		.with_state(Landing { panel, pages })
}

/// The landing's key, its MAC over `body` (a GET's: `GET <path?query>`).
async fn landing_key(l: &Landing, headers: &HeaderMap, body: &[u8]) -> Result<Grant, Box<Response>> {
	let (key_id, timestamp, signature) = ingest::signed(headers).ok_or_else(|| Box::new(ingest::unsigned()))?;
	let batch = SignedBatch { key_id, timestamp, signature, body };
	l.panel.authenticate(batch, Timestamp::now()).await.map_err(|e| Box::new(ingest::refused(e)))
}

/// The GBP `view` showed, its brand one of `key`'s.
async fn view_of(l: &Landing, key: &Grant, view: Uuid) -> Result<(), Box<Response>> {
	let mut conn = l
		.panel
		.store()
		.pool()
		.acquire()
		.await
		.map_err(|e| Box::new(Refusal::<Code>::from(eyre::Report::from(e)).into_response()))?;
	let gmb = sa_panel::store::kit::view_gmb(&mut conn, view)
		.await
		.map_err(|e| Box::new(Refusal::<Code>::from(e).into_response()))?
		.ok_or_else(|| Box::new(Refusal::new(Code::NotFound, "not found").into_response()))?;
	let place = l
		.pages
		.of_gmb(gmb)
		.await
		.map_err(|e| Box::new(e.into_response()))?
		.ok_or_else(|| Box::new(Refusal::new(Code::NotFound, "not found").into_response()))?;
	if !key.brands.iter().any(|b| b.as_str() == place.brand) {
		return Err(Box::new(Refusal::new(Code::NotFound, "not found").into_response()));
	}
	Ok(())
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShownIn {
	/// The kit whose page shows it.
	kit: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OfferQuery {
	brand: BrandId,
	slug: String,
	qr: Option<u8>,
	/// The kit whose QR it came from.
	kit: Option<i64>,
}

async fn offer(State(l): State<Landing>, OriginalUri(uri): OriginalUri, headers: HeaderMap, q: Result<Query<OfferQuery>, QueryRejection>) -> Response {
	let signed = format!("GET {}", uri.path_and_query().map_or_else(|| uri.path(), |pq| pq.as_str()));
	let key = match landing_key(&l, &headers, signed.as_bytes()).await {
		Ok(k) => k,
		Err(res) => return *res,
	};
	let Ok(Query(q)) = q else {
		return Refusal::new(Code::BadRequest, "brand, slug, qr=1 if it came from the QR and kit=<id> if from a kit's").into_response();
	};
	if !key.brands.iter().any(|b| b.as_str() == q.brand.as_str()) {
		return Refusal::new(Code::NotFound, "not found").into_response();
	}
	let gmb = match l.pages.gmb_at(&q.brand, &q.slug).await {
		Ok(Some(g)) => g,
		Ok(None) => return Refusal::new(Code::NotFound, "not found").into_response(),
		Err(e) => return e.into_response(),
	};
	match l.panel.kit_offer(gmb, q.qr == Some(1), q.kit, Timestamp::now()).await {
		Ok(Ok(o)) => {
			let mut res = Json(OfferDto::from(o)).into_response();
			res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
			res
		}
		Ok(Err(r)) => Refusal::<Code>::from(r).into_response(),
		Err(e) => Refusal::<Code>::from(e).into_response(),
	}
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewQuery {
	view: Uuid,
}

async fn offered_photo(State(l): State<Landing>, OriginalUri(uri): OriginalUri, headers: HeaderMap, Path(file): Path<String>, q: Result<Query<ViewQuery>, QueryRejection>) -> Response {
	let signed = format!("GET {}", uri.path_and_query().map_or_else(|| uri.path(), |pq| pq.as_str()));
	let key = match landing_key(&l, &headers, signed.as_bytes()).await {
		Ok(k) => k,
		Err(res) => return *res,
	};
	let (Ok(Query(q)), Ok(photo)) = (q, jpg(&file)) else {
		return Refusal::new(Code::NotFound, "not found").into_response();
	};
	if let Err(res) = view_of(&l, &key, q.view).await {
		return *res;
	}
	match l.panel.offered_photo(q.view, photo).await {
		Ok(Ok(bytes)) => Picture(bytes, Jpeg).into_response(),
		Ok(Err(r)) => Refusal::<Code>::from(r).into_response(),
		Err(e) => Refusal::<Code>::from(e).into_response(),
	}
}

async fn event(State(l): State<Landing>, headers: HeaderMap, body: axum::body::Body) -> Response {
	let body = match to_bytes(body, 64 * 1024).await {
		Ok(b) => b,
		Err(e) => return ingest::body_error(e, 64 * 1024),
	};
	let key = match landing_key(&l, &headers, &body).await {
		Ok(k) => k,
		Err(res) => return *res,
	};
	let e: EventDto = match serde_json::from_slice(&body) {
		Ok(e) => e,
		Err(e) => return Refusal::new(Code::BadRequest, e.to_string()).into_response(),
	};
	let event = match (e.copied, e.saved) {
		(Some(draft), None) => Event::Copied(draft),
		(None, Some(photos)) => Event::Saved(photos),
		_ => return Refusal::new(Code::BadRequest, "one of copied or saved").into_response(),
	};
	if let Err(res) = view_of(&l, &key, e.view).await {
		return *res;
	}
	match l.panel.kit_event(e.view, event, Timestamp::now()).await {
		Ok(Ok(())) => StatusCode::NO_CONTENT.into_response(),
		Ok(Err(r)) => Refusal::<Code>::from(r).into_response(),
		Err(e) => Refusal::<Code>::from(e).into_response(),
	}
}

impl From<Offer> for OfferDto {
	fn from(o: Offer) -> Self {
		Self {
			target: o.target,
			place_id: o.place_id,
			name: o.name,
			lang: o.lang.to_owned(),
			review_uri: o.review_uri,
			view: o.view,
			drafts: o
				.drafts
				.into_iter()
				.map(|d| OfferedDto {
					id: d.id,
					text: d.text,
					photos: d
						.photos
						.into_iter()
						.map(|p| OfferedPhotoDto {
							id: p.id,
							shows: p.shows,
							width: p.width,
							height: p.height,
						})
						.collect(),
				})
				.collect(),
		}
	}
}
