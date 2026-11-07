//! The assistant's chats (`sa_panel::assistant`): the conductor's own state, shared with people
//! by their owner. Whoever may not read a chat is answered 404, as if it did not exist.
//!
//! ```text
//! GET    /api/v1/assistant/chats                 Chats: owned and joined, with the caller's marks
//! POST   /api/v1/assistant/chats {id, title, turns}            → 201 Chat; 409 the id is taken
//! GET    /api/v1/assistant/chats/{id}            Chat
//! PUT    /api/v1/assistant/chats/{id}/turns {base, from, turns} → 200 Chat; 409 `base` is not the version
//! PUT    /api/v1/assistant/chats/{id}/title {title}            → 200 Chat
//! PUT    /api/v1/assistant/chats/{id}/pinned {on}              → 204, the caller's own
//! PUT    /api/v1/assistant/chats/{id}/archived {on}            → 204, the caller's own
//! DELETE /api/v1/assistant/chats/{id}            → 204; the owner only
//! DELETE /api/v1/assistant/archived              → 204: the caller's archived chats deleted, left if shared with them
//! GET    /api/v1/assistant/chats/{id}/access     Access
//! POST   /api/v1/assistant/chats/{id}/members {users}          → 200 Access
//! DELETE /api/v1/assistant/chats/{id}/members/{user}           → 204; the owner anyone, a member themselves
//! PUT    /api/v1/assistant/chats/{id}/general {general}        → 200 Access; the owner only
//! GET    /api/v1/assistant/people?q=             People
//! ```
//!
//! A viewer (general access, a signed-in holder of any `sa` permission) is 403 on every write, a
//! member on the owner's. A turns write past `from` turns is 400; a create or turns body past
//! 4 MiB, or a chat's turns past [`sa_panel::assistant::MAX_TURNS_BYTES`], 413.

use axum::{
	Extension, Json,
	extract::{
		DefaultBodyLimit, Path, Query, State,
		rejection::{JsonRejection, QueryRejection},
	},
	handler::Handler,
	http::{Method, StatusCode},
	response::{IntoResponse, Response},
};
use jiff::Timestamp;
use sa_panel::{
	Panel,
	assistant::{Access, Chat, General, Mark, Person, Reader, Role, Summary},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::{
	api::{ApiError, ApiResult, body},
	http::ApiRoute,
	signin::{Caller, Freshness},
};

/// A create or turns write: images ride in as data URLs.
const MAX_BODY: usize = 4 << 20;

/// Someone a chat names: its owner, a member, one to invite.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Person")]
pub struct PersonDto {
	pub id: Uuid,
	pub name: String,
	pub email: String,
}

/// What the caller may do with a chat.
#[derive(Clone, Copy, Debug, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "Role")]
pub enum RoleDto {
	/// Everything.
	Owner,
	/// Ask, edit turns, rename, invite; leave.
	Member,
	/// Read, by general access.
	Viewer,
}

/// Who beyond the owner and members may read a chat by its link.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "GeneralAccess")]
pub enum GeneralAccessDto {
	None,
	/// Everyone in Service-Arb: a signed-in holder of any `sa` permission.
	View,
}

/// A chat as a history row.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "ChatSummary")]
pub struct ChatSummaryDto {
	pub id: Uuid,
	/// `null` until named by its first question, or renamed.
	pub title: Option<String>,
	pub owner: PersonDto,
	pub role: RoleDto,
	/// It has a member, or general access.
	pub shared: bool,
	/// RFC 3339; the caller's own.
	pub pinned_at: Option<String>,
	/// RFC 3339; the caller's own.
	pub archived_at: Option<String>,
	/// RFC 3339.
	pub updated_at: String,
}

/// `GET /assistant/chats`.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Chats")]
pub struct ChatsDto {
	pub chats: Vec<ChatSummaryDto>,
}

/// A chat, whole.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Chat")]
pub struct ChatDto {
	#[serde(flatten)]
	#[ts(flatten)]
	pub summary: ChatSummaryDto,
	pub general: GeneralAccessDto,
	/// 1 when created; moves with every turns write: what the next one names as its `base`.
	#[ts(type = "number")]
	pub version: i64,
	pub turns: Vec<Turn>,
}

/// `GET /assistant/chats/{id}/access`, and what its writes answer.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "Access")]
pub struct AccessDto {
	pub owner: PersonDto,
	/// Each can interact.
	pub members: Vec<PersonDto>,
	pub general: GeneralAccessDto,
}

/// `GET /assistant/people?q=`: whom the caller may invite.
#[derive(Clone, Debug, Serialize, TS)]
#[ts(rename = "People")]
pub struct PeopleDto {
	pub people: Vec<PersonDto>,
}

/// A chat's turn, as its asker's browser wrote it. Times are milliseconds since the epoch.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum Turn {
	User {
		id: String,
		#[ts(type = "number")]
		at: i64,
		text: String,
		mentions: Vec<Mention>,
		files: Vec<Attachment>,
	},
	Assistant {
		id: String,
		#[ts(type = "number")]
		at: i64,
		/// When the answer stopped growing; `null` while it runs.
		#[ts(type = "number | null")]
		ended: Option<i64>,
		/// How long its steps took, once they are over.
		#[ts(type = "number | null")]
		worked: Option<i64>,
		steps: Vec<Step>,
		/// `**bold**`, `- ` items, `@[label](location:id)` and `[n]` for `sources[n-1]`.
		text: String,
		sources: Vec<Source>,
		actions: Vec<Action>,
		state: AnswerState,
		feedback: Option<Feedback>,
	},
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AnswerState {
	Working,
	Writing,
	Done,
	Stopped,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
	Up,
	Down,
}

/// Something a turn is about: a GMB today; other kinds (a lead, a site) join with their own picker.
#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Mention {
	pub kind: MentionKind,
	#[ts(type = "number")]
	pub id: i64,
	pub label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MentionKind {
	Location,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
	pub name: String,
	/// An image's preview, downscaled; `null` for any other file.
	pub preview: Option<Preview>,
	#[ts(type = "number")]
	pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Preview {
	/// A `data:image/…` URL.
	pub url: String,
	pub width: u32,
	pub height: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Step {
	pub label: String,
	pub done: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Source {
	pub label: String,
	/// `null`: read, but nowhere in the panel to open.
	pub href: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Action {
	pub id: String,
	pub label: String,
	pub detail: String,
	/// Where it goes, for one that opens a page instead of doing something.
	pub href: Option<String>,
	/// Said once it is done: "Appealed 4 reviews".
	pub done: String,
	pub state: ActionState,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
	Proposed,
	Done,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Freshness::Cached;
	use Method as M;
	let big = DefaultBodyLimit::max(MAX_BODY);
	vec![
		ApiRoute::new(M::GET, "/assistant/chats", None, Cached, chats),
		ApiRoute::new(M::POST, "/assistant/chats", None, Cached, create.layer(big)),
		ApiRoute::new(M::GET, "/assistant/chats/{id}", None, Cached, chat),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/turns", None, Cached, write_turns.layer(big)),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/title", None, Cached, rename),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/pinned", None, Cached, pin),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/archived", None, Cached, archive),
		ApiRoute::new(M::DELETE, "/assistant/chats/{id}", None, Cached, delete),
		ApiRoute::new(M::DELETE, "/assistant/archived", None, Cached, clear_archived),
		ApiRoute::new(M::GET, "/assistant/chats/{id}/access", None, Cached, access),
		ApiRoute::new(M::POST, "/assistant/chats/{id}/members", None, Cached, invite),
		ApiRoute::new(M::DELETE, "/assistant/chats/{id}/members/{user}", None, Cached, remove_member),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/general", None, Cached, set_general),
		ApiRoute::new(M::GET, "/assistant/people", None, Cached, people),
	]
}

impl From<Person> for PersonDto {
	fn from(p: Person) -> Self {
		Self {
			id: p.id,
			name: p.name,
			email: p.email,
		}
	}
}

impl From<Role> for RoleDto {
	fn from(r: Role) -> Self {
		match r {
			Role::Owner => Self::Owner,
			Role::Member => Self::Member,
			Role::Viewer => Self::Viewer,
		}
	}
}

impl From<General> for GeneralAccessDto {
	fn from(g: General) -> Self {
		match g {
			General::None => Self::None,
			General::View => Self::View,
		}
	}
}

impl From<GeneralAccessDto> for General {
	fn from(g: GeneralAccessDto) -> Self {
		match g {
			GeneralAccessDto::None => Self::None,
			GeneralAccessDto::View => Self::View,
		}
	}
}

impl From<Summary> for ChatSummaryDto {
	fn from(s: Summary) -> Self {
		Self {
			id: s.id,
			title: s.title,
			owner: s.owner.into(),
			role: s.role.into(),
			shared: s.shared,
			pinned_at: s.pinned_at.map(|t| t.to_string()),
			archived_at: s.archived_at.map(|t| t.to_string()),
			updated_at: s.updated_at.to_string(),
		}
	}
}

impl From<Chat<Turn>> for ChatDto {
	fn from(c: Chat<Turn>) -> Self {
		Self {
			general: c.summary.general.into(),
			version: c.summary.version,
			summary: c.summary.into(),
			turns: c.turns,
		}
	}
}

impl From<Access> for AccessDto {
	fn from(a: Access) -> Self {
		Self {
			owner: a.owner.into(),
			members: a.members.into_iter().map(PersonDto::from).collect(),
			general: a.general.into(),
		}
	}
}

impl From<&Caller> for Reader {
	fn from(c: &Caller) -> Self {
		Self {
			user: c.user_id,
			in_sa: c.permissions.iter().next().is_some(),
		}
	}
}

/// A path's id: one that is no UUID is no chat of anyone's.
fn id(raw: &str) -> ApiResult<Uuid> {
	raw.parse().map_err(|_| ApiError::NotFound)
}

async fn chats(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<ChatsDto>> {
	let chats = panel.assistant_chats(caller.user_id).await?;
	Ok(Json(ChatsDto {
		chats: chats.into_iter().map(ChatSummaryDto::from).collect(),
	}))
}

async fn chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<Json<ChatDto>> {
	let chat = panel.assistant_chat::<Turn>((&caller).into(), id(&raw)?).await??;
	Ok(Json(chat.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewChat {
	id: Uuid,
	title: Option<String>,
	turns: Vec<Turn>,
}

async fn create(State(panel): State<Panel>, Extension(caller): Extension<Caller>, b: Result<Json<NewChat>, JsonRejection>) -> ApiResult<Response> {
	let NewChat { id, title, turns } = body(b)?;
	let chat = panel.assistant_create(caller.user_id, id, title.as_deref(), turns, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, chat = %id, "chat created");
	Ok((StatusCode::CREATED, Json(ChatDto::from(chat))).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TurnsWrite {
	base: i64,
	from: usize,
	turns: Vec<Turn>,
}

async fn write_turns(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<TurnsWrite>, JsonRejection>) -> ApiResult<Json<ChatDto>> {
	let chat = id(&raw)?;
	let TurnsWrite { base, from, turns } = body(b)?;
	let written = panel.assistant_turns((&caller).into(), chat, base, from, turns, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, base, from, "chat's turns written");
	Ok(Json(written.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Title {
	title: Option<String>,
}

async fn rename(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<Title>, JsonRejection>) -> ApiResult<Json<ChatDto>> {
	let chat = id(&raw)?;
	let renamed = panel.assistant_rename::<Turn>((&caller).into(), chat, body(b)?.title.as_deref()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, "chat renamed");
	Ok(Json(renamed.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct On {
	on: bool,
}

async fn mark(panel: Panel, caller: Caller, raw: String, mark: Mark, b: Result<Json<On>, JsonRejection>) -> ApiResult<StatusCode> {
	let chat = id(&raw)?;
	let on = body(b)?.on;
	panel.assistant_mark((&caller).into(), chat, mark, on, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, ?mark, on, "chat marked");
	Ok(StatusCode::NO_CONTENT)
}

async fn pin(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<On>, JsonRejection>) -> ApiResult<StatusCode> {
	mark(panel, caller, raw, Mark::Pinned, b).await
}

async fn archive(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<On>, JsonRejection>) -> ApiResult<StatusCode> {
	mark(panel, caller, raw, Mark::Archived, b).await
}

async fn delete(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<StatusCode> {
	let chat = id(&raw)?;
	panel.assistant_delete((&caller).into(), chat).await??;
	tracing::info!(user_id = %caller.user_id, %chat, "chat deleted");
	Ok(StatusCode::NO_CONTENT)
}

async fn clear_archived(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<StatusCode> {
	panel.assistant_clear_archived(caller.user_id).await?;
	tracing::info!(user_id = %caller.user_id, "archived chats cleared");
	Ok(StatusCode::NO_CONTENT)
}

async fn access(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>) -> ApiResult<Json<AccessDto>> {
	let access = panel.assistant_access((&caller).into(), id(&raw)?).await??;
	Ok(Json(access.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invite {
	users: Vec<Uuid>,
}

async fn invite(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<Invite>, JsonRejection>) -> ApiResult<Json<AccessDto>> {
	let chat = id(&raw)?;
	let users = body(b)?.users;
	let access = panel.assistant_invite((&caller).into(), chat, &users, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, ?users, "chat's members invited");
	Ok(Json(access.into()))
}

async fn remove_member(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path((raw, user)): Path<(String, String)>) -> ApiResult<StatusCode> {
	let (chat, member) = (id(&raw)?, id(&user)?);
	panel.assistant_remove_member((&caller).into(), chat, member).await??;
	tracing::info!(user_id = %caller.user_id, %chat, %member, "chat's member removed");
	Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetGeneral {
	general: GeneralAccessDto,
}

async fn set_general(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Path(raw): Path<String>, b: Result<Json<SetGeneral>, JsonRejection>) -> ApiResult<Json<AccessDto>> {
	let chat = id(&raw)?;
	let general = body(b)?.general;
	let access = panel.assistant_set_general((&caller).into(), chat, general.into()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, ?general, "chat's general access set");
	Ok(Json(access.into()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PeopleQuery {
	q: String,
}

async fn people(State(panel): State<Panel>, Extension(caller): Extension<Caller>, q: Result<Query<PeopleQuery>, QueryRejection>) -> ApiResult<Json<PeopleDto>> {
	let Query(q) = q.map_err(|e| ApiError::BadRequest(e.body_text()))?;
	let people = panel.assistant_people(caller.user_id, &q.q).await?;
	Ok(Json(PeopleDto {
		people: people.into_iter().map(PersonDto::from).collect(),
	}))
}
