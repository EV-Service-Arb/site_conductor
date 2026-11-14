//! The in-page assistant's loop: Claude asked with the caller's question, given the operations as
//! `search` and `call` (`crate::tools`, through the `/api/v1` gate on the caller's own session) and
//! the page as `open` (a view) and `run` (a binding of its keymap). What spends or destroys is
//! proposed, never done: the user confirms it from the answer's actions. The turns are written
//! through the chats' versioned store as the loop goes, each write told to the caller's screens.

use std::{collections::BTreeMap, time::Duration};

use axum::{
	Extension, Json,
	extract::State,
	http::{HeaderMap, StatusCode},
};
use ev_lib::api::{
	JsonSchema,
	schemars::{generate::SchemaSettings, schema_for},
};
use jiff::Timestamp;
use sa_panel::{
	assistant::Chat,
	live::{Audience, Change},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
	api::{ApiError, ApiResult, Body, Id, Ids},
	assistant::{Action, ActionState, AnswerState, Attachment, ChatDto, Mention, Step, Target, Turn},
	http::ApiState,
	signin::Caller,
	tools::{self, Search},
};

const MODEL: &str = "claude-opus-5-5";
/// Model turns one question may take before the loop stops and says so.
const ROUNDS: usize = 24;

const SYSTEM: &str = "You are the Service-Arb panel's assistant. The panel runs Google Business Profiles (GBPs; a GMB in \
code) for trade businesses: the managing gmails, each GBP's state (draft, verifying, verified, suspended), its site and \
leads, its reviews, tokens that pay for Google requests. You act for the person asking, with exactly their permissions.\n\
\n\
Tools: `search` finds the panel's operations by what they do (use it before `call`; an empty query lists them all); \
`call` runs one by its id. An operation that destroys or spends is not run: it becomes a proposal the person confirms, \
so say what it will do. `open` offers a view (a URL of this site) as a link; `run` offers a binding of the page's \
keymap. The <page> block says what is open, the views there are, and the bindings bound now.\n\
\n\
Answer briefly, in the person's language. Formatting: **bold**, `- ` items, `@[label](location:<gbp id>)` to name a GBP.";

/// The Messages API, with the conductor's key (`ANTHROPIC_API_KEY`).
#[derive(Clone)]
pub struct Model {
	http: reqwest::Client,
	/// `https://api.anthropic.com`.
	base: String,
	key: String,
}

impl Model {
	pub fn new(base: String, key: String) -> Self {
		let http = reqwest::Client::builder().timeout(Duration::from_secs(300)).build().expect("the TLS backend initialises at boot");
		Self { http, base, key }
	}

	async fn answer(&self, system: &str, tools: &Value, messages: &[Value]) -> ApiResult<Value> {
		let res = self
			.http
			.post(format!("{}/v1/messages", self.base))
			.header("x-api-key", &self.key)
			.header("anthropic-version", "2026-10-21")
			.header("anthropic-beta", "server-side-fallback-2026-07-01")
			.json(&json!({
				"model": MODEL,
				"max_tokens": 16000,
				"fallbacks": "default",
				"output_config": { "effort": "medium" },
				"system": system,
				"tools": tools,
				"messages": messages,
			}))
			.send()
			.await
			.map_err(|e| ApiError::Unavailable(format!("the assistant's model did not answer: {e}")))?;
		let status = res.status();
		let body: Value = res.json().await.map_err(|e| ApiError::Unavailable(format!("the assistant's model answered no JSON: {e}")))?;
		if !status.is_success() {
			return Err(ApiError::Unavailable(format!("the assistant's model refused with {status}: {body}")));
		}
		Ok(body)
	}
}

/// A binding of the page's keymap, as the page has it bound now.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bound {
	id: String,
	label: String,
	/// Its keys, as the keycaps read.
	sequence: String,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Question {
	/// The turns kept before it: a question asked again from an earlier one drops what followed.
	from: usize,
	text: String,
	mentions: Vec<Mention>,
	files: Vec<Attachment>,
	/// The page the question is asked on: what is open.
	url: String,
	/// The views a person can be shown, by name: the page's route table.
	views: BTreeMap<String, String>,
	bindings: Vec<Bound>,
}

/// Asks the assistant in a chat: the question and the answer join its turns, the answer written as
/// it grows; answered with the chat once the answer is done.
///
/// 503 without the assistant's model, or when it does not answer.
pub(crate) async fn ask(
	State(state): State<ApiState>,
	Extension(caller): Extension<Caller>,
	headers: HeaderMap,
	Ids(Id { id: chat }): Ids<Id<Uuid>>,
	Body(q): Body<Question>,
) -> ApiResult<Json<ChatDto>> {
	let model = state.model.clone().ok_or_else(|| ApiError::Unavailable("the assistant is not connected".into()))?;
	let reader = (&caller).into();
	let read: Chat<Turn> = state.panel.assistant_chat(reader, chat).await??;
	let started = Timestamp::now();
	if q.from > read.turns.len() {
		return Err(ApiError::BadRequest(format!("from {} is past the chat's {} turns", q.from, read.turns.len())));
	}
	let mut turns = read.turns;
	turns.truncate(q.from);
	let mut history = conversation(&turns);
	let asked = Turn::User {
		id: Uuid::now_v7().to_string(),
		at: started.as_millisecond(),
		text: q.text.clone(),
		mentions: q.mentions.clone(),
		files: q.files.clone(),
	};
	let mut answer = Answer {
		id: Uuid::now_v7().to_string(),
		at: started.as_millisecond(),
		steps: Vec::new(),
		text: String::new(),
		actions: Vec::new(),
	};
	let from = q.from;
	turns.push(asked);
	let mut written = Written {
		state: &state,
		caller: &caller,
		chat,
		base: read.summary.version,
		from,
	};
	written.write(&turns, &answer, AnswerState::Working, None).await?;

	history.push(json!({ "role": "user", "content": question(&q) }));
	let tools = tool_definitions();
	let permissions = caller.permissions.clone();
	let mut rounds = 0;
	let end = loop {
		rounds += 1;
		if rounds > ROUNDS {
			answer.text.push_str("\n\nI stopped here: this took more steps than one answer may.");
			break AnswerState::Stopped;
		}
		let reply = model.answer(SYSTEM, &tools, &history).await?;
		let content = reply["content"].as_array().cloned().ok_or_else(|| eyre::eyre!("the model answered no content: {reply}"))?;
		let mut results = Vec::new();
		for block in &content {
			match block["type"].as_str() {
				Some("text") => answer.text = block["text"].as_str().ok_or_else(|| eyre::eyre!("a text block without text: {block}"))?.to_owned(),
				Some("tool_use") => {
					let (result, step) = use_tool(&state, &headers, &permissions, &q, &mut answer, &block["name"], &block["input"]).await;
					answer.steps.push(Step { label: step, done: true });
					results.push(json!({ "type": "tool_result", "tool_use_id": block["id"], "content": result.to_string() }));
				}
				_ => {}
			}
		}
		history.push(json!({ "role": "assistant", "content": content }));
		match reply["stop_reason"].as_str() {
			Some("tool_use") => {
				history.push(json!({ "role": "user", "content": results }));
				written.write(&turns, &answer, AnswerState::Working, None).await?;
			}
			Some("refusal") => {
				answer.text = "I can't help with that.".to_owned();
				break AnswerState::Done;
			}
			Some("end_turn" | "max_tokens" | "pause_turn" | "stop_sequence") => break AnswerState::Done,
			other => return Err(eyre::eyre!("the model stopped for {other:?}").into()),
		}
	};
	let done = written.write(&turns, &answer, end, Some(started)).await?;
	Ok(Json(done))
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Act {
	/// The answer's turn id.
	turn: String,
	/// The action's id in it.
	action: String,
}

/// Does an answer's proposed operation, as the caller and through the same gate as their own
/// request; the action is marked done in the chat.
///
/// 409 the action is not a proposed operation; the operation's own refusal answers as it refused.
pub(crate) async fn act(
	State(state): State<ApiState>,
	Extension(caller): Extension<Caller>,
	headers: HeaderMap,
	Ids(Id { id: chat }): Ids<Id<Uuid>>,
	Body(a): Body<Act>,
) -> ApiResult<Json<ChatDto>> {
	let read: Chat<Turn> = state.panel.assistant_chat((&caller).into(), chat).await??;
	let at = read
		.turns
		.iter()
		.position(|t| matches!(t, Turn::Assistant { id, .. } if *id == a.turn))
		.ok_or(ApiError::NotFound)?;
	let mut turns = read.turns;
	let Turn::Assistant { actions, .. } = &mut turns[at] else {
		unreachable!("found as an answer")
	};
	let action = actions.iter_mut().find(|x| x.id == a.action).ok_or(ApiError::NotFound)?;
	let (ActionState::Proposed, Target::Call(call)) = (&action.state, &action.target) else {
		return Err(ApiError::Conflict("only a proposed operation is done here: a view or a binding is the page's".into()));
	};
	let answer = state.tools.call(&headers, call.clone()).await.map_err(ApiError::BadRequest)?;
	let status = StatusCode::from_u16(answer.status).map_err(|e| eyre::eyre!("the app answered {}: {e}", answer.status))?;
	if !status.is_success() {
		let why = answer.body["error"].as_str().map_or_else(|| answer.body.to_string(), str::to_owned);
		return Err(match status {
			StatusCode::BAD_REQUEST => ApiError::BadRequest(why),
			StatusCode::FORBIDDEN => ApiError::Forbidden,
			StatusCode::NOT_FOUND => ApiError::NotFound,
			StatusCode::CONFLICT => ApiError::Conflict(why),
			StatusCode::PAYLOAD_TOO_LARGE => ApiError::TooLarge(why),
			StatusCode::SERVICE_UNAVAILABLE => ApiError::Unavailable(why),
			s => eyre::eyre!("{} answered {s}: {why}", call.operation).into(),
		});
	}
	action.state = ActionState::Done;
	let tail = turns.split_off(at);
	let done = state.panel.assistant_turns((&caller).into(), chat, read.summary.version, at, tail, Timestamp::now()).await??;
	Ok(Json(done.into()))
}

/// The answer as it grows.
struct Answer {
	id: String,
	at: i64,
	steps: Vec<Step>,
	text: String,
	actions: Vec<Action>,
}

/// Where the loop's writes go: the chat at the version last written.
struct Written<'a> {
	state: &'a ApiState,
	caller: &'a Caller,
	chat: Uuid,
	base: i64,
	from: usize,
}

impl Written<'_> {
	async fn write(&mut self, asked: &[Turn], answer: &Answer, state: AnswerState, started: Option<Timestamp>) -> ApiResult<ChatDto> {
		let now = Timestamp::now();
		let turn = Turn::Assistant {
			id: answer.id.clone(),
			at: answer.at,
			ended: started.map(|_| now.as_millisecond()),
			worked: started.map(|s| now.as_millisecond() - s.as_millisecond()),
			steps: answer.steps.clone(),
			text: answer.text.clone(),
			sources: Vec::new(),
			actions: answer.actions.clone(),
			state,
			feedback: None,
		};
		let turns: Vec<Turn> = asked[self.from..].iter().cloned().chain([turn]).collect();
		let chat = self.state.panel.assistant_turns(self.caller.into(), self.chat, self.base, self.from, turns, now).await??;
		self.base = chat.summary.version;
		// The asker's screens follow the answer as it grows; the route tells its end.
		if started.is_none() {
			self.state.panel.bus().changed(Change {
				topic: "assistant".into(),
				brand: None,
				id: Some(self.chat.to_string()),
				at: now,
				audience: Audience::User(self.caller.user_id),
			});
		}
		Ok(chat.into())
	}
}

/// The chat so far, as the model reads it: what was asked and what was answered.
fn conversation(turns: &[Turn]) -> Vec<Value> {
	turns
		.iter()
		.filter_map(|t| match t {
			Turn::User { text, .. } => Some(json!({ "role": "user", "content": text })),
			Turn::Assistant { text, .. } if !text.is_empty() => Some(json!({ "role": "assistant", "content": text })),
			Turn::Assistant { .. } => None,
		})
		.collect()
}

fn question(q: &Question) -> String {
	let mentions: Vec<String> = q.mentions.iter().map(|m| format!("@[{}](location:{})", m.label, m.id)).collect();
	let bindings: Vec<String> = q.bindings.iter().map(|b| format!("- `{}` ({}): {}", b.id, b.sequence, b.label)).collect();
	let views: Vec<String> = q.views.iter().map(|(name, path)| format!("- {name}: {path}")).collect();
	format!(
		"{}\n\n<page>\nOpen: {}\nAbout: {}\nViews:\n{}\nBindings:\n{}\n</page>",
		q.text,
		q.url,
		if mentions.is_empty() { "nothing named".to_owned() } else { mentions.join(", ") },
		views.join("\n"),
		bindings.join("\n")
	)
}

fn tool_definitions() -> Value {
	let schema = |s: ev_lib::api::schemars::Schema| serde_json::to_value(s).expect("a schema serializes");
	let call = SchemaSettings::draft07().into_generator().into_root_schema_for::<tools::Call>();
	json!([
		{ "name": "search", "description": "Finds the panel's operations matching what you need: their ids, parameters, body schemas and whether they destroy or spend.", "input_schema": schema(schema_for!(Search)) },
		{ "name": "call", "description": "Runs one operation by its id, as the person asking. One that destroys or spends becomes a proposal they confirm.", "input_schema": schema(call) },
		{ "name": "open", "description": "Offers a view as a link: a URL of this site, from the page's views.", "input_schema": { "type": "object", "properties": { "url": { "type": "string" }, "label": { "type": "string" } }, "required": ["url", "label"] } },
		{ "name": "run", "description": "Offers a binding of the page's keymap, by its id, as a button.", "input_schema": { "type": "object", "properties": { "binding": { "type": "string" }, "label": { "type": "string" } }, "required": ["binding", "label"] } },
	])
}

#[derive(Deserialize)]
struct Open {
	url: String,
	label: String,
}

#[derive(Deserialize)]
struct Run {
	binding: String,
	label: String,
}

/// Runs one tool the model asked for: what it answers the model, and the step the person sees.
async fn use_tool(state: &ApiState, headers: &HeaderMap, permissions: &sa_auth::PermissionSet, q: &Question, answer: &mut Answer, name: &Value, input: &Value) -> (Value, String) {
	let action = |label: &str, target: Target| Action {
		id: Uuid::now_v7().to_string(),
		label: label.to_owned(),
		detail: String::new(),
		target,
		done: String::new(),
		state: ActionState::Proposed,
	};
	match name.as_str() {
		Some("search") => match serde_json::from_value::<Search>(input.clone()) {
			Ok(s) => {
				let found = state.tools.search(&s.query, |p| p.is_none_or(|p| permissions.iter().any(|h| h == p)));
				(found, format!("Searched: {}", s.query))
			}
			Err(e) => (json!({ "error": e.to_string() }), "Searched".to_owned()),
		},
		Some("call") => {
			let c: tools::Call = match serde_json::from_value(input.clone()) {
				Ok(c) => c,
				Err(e) => return (json!({ "error": e.to_string() }), "Called".to_owned()),
			};
			let Some(hints) = state.tools.hints(&c.operation) else {
				return (json!({ "error": format!("no operation {:?}: search for one", c.operation) }), format!("Called {}", c.operation));
			};
			if hints.proposed() {
				let label = c.operation.replace('_', " ");
				answer.actions.push(action(&label, Target::Call(c)));
				return (json!({ "proposed": "the person confirms it from the answer; say what it will do" }), format!("Proposed {label}"));
			}
			let step = format!("Called {}", c.operation);
			match state.tools.call(headers, c).await {
				Ok(a) if StatusCode::from_u16(a.status).is_ok_and(|s| s.is_success()) => (a.body, step),
				Ok(a) => (json!({ "status": a.status, "error": a.body }), step),
				Err(e) => (json!({ "error": e }), step),
			}
		}
		Some("open") => match serde_json::from_value::<Open>(input.clone()) {
			Ok(o) if o.url.starts_with('/') && !o.url.starts_with("//") => {
				let step = format!("Linked {}", o.url);
				answer.actions.push(action(&o.label, Target::Href(o.url)));
				(json!({ "offered": "a link" }), step)
			}
			Ok(_) => (json!({ "error": "a URL of this site starts with one /" }), "Linked".to_owned()),
			Err(e) => (json!({ "error": e.to_string() }), "Linked".to_owned()),
		},
		Some("run") => match serde_json::from_value::<Run>(input.clone()) {
			Ok(r) if q.bindings.iter().any(|b| b.id == r.binding) => {
				let step = format!("Offered {}", r.binding);
				answer.actions.push(action(&r.label, Target::Binding(r.binding)));
				(json!({ "offered": "a button" }), step)
			}
			Ok(r) => (json!({ "error": format!("no binding {:?} is bound now", r.binding) }), "Offered a key".to_owned()),
			Err(e) => (json!({ "error": e.to_string() }), "Offered a key".to_owned()),
		},
		other => (json!({ "error": format!("no tool {other:?}") }), "Asked for an unknown tool".to_owned()),
	}
}
