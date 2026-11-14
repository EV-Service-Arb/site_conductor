//! The assistant's chats (`sa_panel::assistant`): the conductor's own state, shared with people
//! by their owner. Whoever may not read a chat is answered 404, as if it did not exist.
//!
//! A viewer (general access, a signed-in holder of any `sa` permission) is 403 on every write, a
//! member on the owner's. A turns write past `from` turns is 400; a create or turns body past
//! 4 MiB, or a chat's turns past [`sa_panel::assistant::MAX_TURNS_BYTES`], 413.

use axum::{
	Extension, Json,
	extract::{DefaultBodyLimit, State},
	http::Method,
	response::NoContent,
};
use ev_lib::api::{Created, JsonSchema};
use jiff::Timestamp;
use sa_panel::{
	Panel,
	assistant::{Access, Chat, General, Mark, Person, Reader, Role, Summary},
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
	api::{ApiResult, Body, Id, Ids, Params},
	http::{ApiRoute, Gate},
	signin::Caller,
};

/// A create or turns write: images ride in as data URLs.
const MAX_BODY: usize = 4 << 20;

/// Someone a chat names: its owner, a member, one to invite.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Person")]
pub struct PersonDto {
	pub id: Uuid,
	pub name: String,
	pub email: String,
}

/// What the caller may do with a chat.
#[derive(Clone, Copy, Debug, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "Role")]
pub enum RoleDto {
	/// Everything.
	Owner,
	/// Ask, edit turns, rename, invite; leave.
	Member,
	/// Read, by general access.
	Viewer,
}

/// Who beyond the owner and members may read a chat by its link.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "GeneralAccess")]
pub enum GeneralAccessDto {
	None,
	/// Everyone in Service-Arb: a signed-in holder of any `sa` permission.
	View,
}

/// A chat as a history row.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "ChatSummary")]
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
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Chats")]
pub struct ChatsDto {
	pub chats: Vec<ChatSummaryDto>,
}

/// A chat, whole.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Chat")]
pub struct ChatDto {
	#[serde(flatten)]
	pub summary: ChatSummaryDto,
	pub general: GeneralAccessDto,
	/// 1 when created; moves with every turns write: what the next one names as its `base`.
	pub version: i64,
	pub turns: Vec<Turn>,
}

/// `GET /assistant/chats/{id}/access`, and what its writes answer.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "Access")]
pub struct AccessDto {
	pub owner: PersonDto,
	/// Each can interact.
	pub members: Vec<PersonDto>,
	pub general: GeneralAccessDto,
}

/// `GET /assistant/people?q=`: whom the caller may invite.
#[derive(Clone, Debug, JsonSchema, Serialize)]
#[schemars(rename = "People")]
pub struct PeopleDto {
	pub people: Vec<PersonDto>,
}

/// A chat's turn, as its asker's browser wrote it. Times are milliseconds since the epoch.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(tag = "role", rename_all = "snake_case", deny_unknown_fields)]
pub enum Turn {
	User {
		id: String,
		at: i64,
		text: String,
		mentions: Vec<Mention>,
		files: Vec<Attachment>,
	},
	Assistant {
		id: String,
		at: i64,
		/// When the answer stopped growing; `null` while it runs.
		ended: Option<i64>,
		/// How long its steps took, once they are over.
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

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerState {
	Working,
	Writing,
	Done,
	Stopped,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Feedback {
	Up,
	Down,
}

/// Something a turn is about: a GMB today; other kinds (a lead, a site) join with their own picker.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mention {
	pub kind: MentionKind,
	pub id: i64,
	pub label: String,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MentionKind {
	Location,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
	pub name: String,
	/// An image's preview, downscaled; `null` for any other file.
	pub preview: Option<Preview>,
	pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
	/// A `data:image/…` URL.
	pub url: String,
	pub width: u32,
	pub height: u32,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
	pub label: String,
	pub done: bool,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
	pub label: String,
	/// `null`: read, but nowhere in the panel to open.
	pub href: Option<String>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
	pub id: String,
	pub label: String,
	pub detail: String,
	pub target: Target,
	/// Said once it is done: "Appealed 4 reviews".
	pub done: String,
	pub state: ActionState,
}

/// What an action does: a view, an operation, or a binding of the page's keymap — nothing else.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Target {
	/// A view, by its URL on this origin.
	Href(String),
	/// An operation of `/api/v1/openapi.json`, by its id, with its arguments.
	Call(crate::tools::Call),
	/// A binding of the page's keymap, by its id.
	Binding(String),
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
	Proposed,
	Done,
}

pub(crate) fn routes() -> Vec<ApiRoute> {
	use Method as M;
	let user = Gate::USER;
	let big = |h: axum::routing::MethodRouter<_>| h.layer(DefaultBodyLimit::max(MAX_BODY));
	vec![
		ApiRoute::new(M::GET, "/assistant/chats", user, chats),
		ApiRoute::new(M::POST, "/assistant/chats", user, create_chat).layer(big),
		ApiRoute::new(M::GET, "/assistant/chats/{id}", user, chat),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/turns", user, write_chat_turns).layer(big),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/title", user, rename_chat),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/pinned", user, pin_chat),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/archived", user, archive_chat),
		ApiRoute::new(M::DELETE, "/assistant/chats/{id}", user, delete_chat),
		ApiRoute::new(M::DELETE, "/assistant/archived", user, clear_archived_chats),
		ApiRoute::new(M::GET, "/assistant/chats/{id}/access", user, chat_access),
		ApiRoute::new(M::POST, "/assistant/chats/{id}/members", user, invite_to_chat),
		ApiRoute::new(M::DELETE, "/assistant/chats/{id}/members/{user}", user, remove_chat_member),
		ApiRoute::new(M::PUT, "/assistant/chats/{id}/general", user, set_chat_general),
		ApiRoute::new(M::GET, "/assistant/people", user, chat_people),
		// The answer takes the model's turns and the operations it calls.
		ApiRoute::new(M::POST, "/assistant/chats/{id}/ask", user.slow(), crate::ask::ask),
		ApiRoute::new(M::POST, "/assistant/chats/{id}/act", user, crate::ask::act),
	]
}

/// A path's chat and one of its people.
#[derive(Deserialize, JsonSchema)]
pub(crate) struct Member {
	id: Uuid,
	user: Uuid,
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

/// The caller's chats, owned and joined, with their own marks, newest first.
async fn chats(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<Json<ChatsDto>> {
	let chats = panel.assistant_chats(caller.user_id).await?;
	Ok(Json(ChatsDto {
		chats: chats.into_iter().map(ChatSummaryDto::from).collect(),
	}))
}

/// A chat: its turns, version and the caller's role.
async fn chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<Uuid>>) -> ApiResult<Json<ChatDto>> {
	let chat = panel.assistant_chat::<Turn>((&caller).into(), id).await??;
	Ok(Json(chat.into()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewChat {
	/// Made by the client.
	id: Uuid,
	title: Option<String>,
	turns: Vec<Turn>,
}

/// Creates a chat at version 1. 409 the id is taken; 413 a body past 4 MiB.
async fn create_chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Body(b): Body<NewChat>) -> ApiResult<Created<ChatDto>> {
	let NewChat { id, title, turns } = b;
	let chat = panel.assistant_create(caller.user_id, id, title.as_deref(), turns, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, chat = %id, "chat created");
	Ok(Created(ChatDto::from(chat)))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TurnsWrite {
	/// The version read.
	base: i64,
	/// Turns before it are kept.
	from: usize,
	turns: Vec<Turn>,
}

/// Writes a chat's turns: those before `from` kept, the rest replaced.
///
/// 409 `base` is not the version; 400 `from` past the end; 413 the body past 4 MiB, or the chat's
/// turns past 16 MiB.
async fn write_chat_turns(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: chat }): Ids<Id<Uuid>>, Body(b): Body<TurnsWrite>) -> ApiResult<Json<ChatDto>> {
	let TurnsWrite { base, from, turns } = b;
	let written = panel.assistant_turns((&caller).into(), chat, base, from, turns, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, base, from, "chat's turns written");
	Ok(Json(written.into()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Title {
	/// Empty or null unnames it.
	title: Option<String>,
}

/// Renames a chat.
async fn rename_chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: chat }): Ids<Id<Uuid>>, Body(b): Body<Title>) -> ApiResult<Json<ChatDto>> {
	let renamed = panel.assistant_rename::<Turn>((&caller).into(), chat, b.title.as_deref()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, "chat renamed");
	Ok(Json(renamed.into()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct On {
	on: bool,
}

async fn mark(panel: Panel, caller: Caller, chat: Uuid, mark: Mark, on: bool) -> ApiResult<NoContent> {
	panel.assistant_mark((&caller).into(), chat, mark, on, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, ?mark, on, "chat marked");
	Ok(NoContent)
}

/// Pins or unpins a chat, for the caller alone.
async fn pin_chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<Uuid>>, Body(b): Body<On>) -> ApiResult<NoContent> {
	mark(panel, caller, id, Mark::Pinned, b.on).await
}

/// Archives or unarchives a chat, for the caller alone.
async fn archive_chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<Uuid>>, Body(b): Body<On>) -> ApiResult<NoContent> {
	mark(panel, caller, id, Mark::Archived, b.on).await
}

/// Deletes a chat, its members and marks with it: the owner's only.
async fn delete_chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: chat }): Ids<Id<Uuid>>) -> ApiResult<NoContent> {
	panel.assistant_delete((&caller).into(), chat).await??;
	tracing::info!(user_id = %caller.user_id, %chat, "chat deleted");
	Ok(NoContent)
}

/// Deletes the caller's archived chats; one shared with them they leave.
async fn clear_archived_chats(State(panel): State<Panel>, Extension(caller): Extension<Caller>) -> ApiResult<NoContent> {
	panel.assistant_clear_archived(caller.user_id).await?;
	tracing::info!(user_id = %caller.user_id, "archived chats cleared");
	Ok(NoContent)
}

/// Who has a chat: its owner, members and general access.
async fn chat_access(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id }): Ids<Id<Uuid>>) -> ApiResult<Json<AccessDto>> {
	let access = panel.assistant_access((&caller).into(), id).await??;
	Ok(Json(access.into()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Invite {
	users: Vec<Uuid>,
}

/// Makes people members of a chat. 400 an unknown user.
async fn invite_to_chat(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: chat }): Ids<Id<Uuid>>, Body(b): Body<Invite>) -> ApiResult<Json<AccessDto>> {
	let users = b.users;
	let access = panel.assistant_invite((&caller).into(), chat, &users, Timestamp::now()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, ?users, "chat's members invited");
	Ok(Json(access.into()))
}

/// Removes a member from a chat: the owner anyone, a member themselves.
async fn remove_chat_member(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Member { id: chat, user: member }): Ids<Member>) -> ApiResult<NoContent> {
	panel.assistant_remove_member((&caller).into(), chat, member).await??;
	tracing::info!(user_id = %caller.user_id, %chat, %member, "chat's member removed");
	Ok(NoContent)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SetGeneral {
	general: GeneralAccessDto,
}

/// Sets who beyond its members may read a chat: the owner's.
async fn set_chat_general(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Ids(Id { id: chat }): Ids<Id<Uuid>>, Body(b): Body<SetGeneral>) -> ApiResult<Json<AccessDto>> {
	let general = b.general;
	let access = panel.assistant_set_general((&caller).into(), chat, general.into()).await??;
	tracing::info!(user_id = %caller.user_id, %chat, ?general, "chat's general access set");
	Ok(Json(access.into()))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PeopleQuery {
	/// A name or an email's part.
	q: String,
}

/// Whom the caller shares a chat with, by name or email, and whoever has `q` as their whole email.
async fn chat_people(State(panel): State<Panel>, Extension(caller): Extension<Caller>, Params(q): Params<PeopleQuery>) -> ApiResult<Json<PeopleDto>> {
	let people = panel.assistant_people(caller.user_id, &q.q).await?;
	Ok(Json(PeopleDto {
		people: people.into_iter().map(PersonDto::from).collect(),
	}))
}
