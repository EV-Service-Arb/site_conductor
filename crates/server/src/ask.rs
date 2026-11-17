//! The in-page assistant's loop: Claude asked with the caller's question, given the operations as
//! `search`, `call` and `execute` (`crate::tools`, through the `/api/v1` gate on the caller's own session) and
//! the page as `open` (a view) and `run` (a binding of its keymap). What spends or destroys is
//! proposed, never done: the user confirms it from the answer's actions. The turns are written
//! through the chats' versioned store as the loop goes, each write told to the caller's screens.

use axum::{
	Extension, Json,
	extract::State,
	http::{HeaderMap, StatusCode},
};
use ev_lib::api::{Body, ErrorCode, Ids, JsonSchema, Refusal};
use jiff::Timestamp;
use sa_core::{
	prices::{ModelTokens, Spends},
	usage::Bill,
};
use sa_panel::assistant::Chat;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
	api::{ApiResult, Code, Id},
	assistant::{Action, ActionState, AnswerState, Attachment, ChatDto, Cost, Mention, Step, Target, Turn, Used},
	claude::{MODEL, SUBSCRIPTION},
	execute::Executed,
	http::ApiState,
	signin::Caller,
	tools::{self, Search},
};

/// Model turns one question may take before the loop stops and says so.
const ROUNDS: usize = 24;

const SYSTEM: &str = "You are the Service-Arb panel's assistant. The panel runs Google Business Profiles (GBPs; a GMB in \
code) for trade businesses: the managing gmails, each GBP's state (draft, verifying, verified, suspended), its site and \
leads, its reviews, tokens that pay for Google requests. You act for the person asking, with exactly their permissions.\n\
\n\
Tools: `search` finds the panel's operations by what they do (use it before `call`; an empty query lists them all); \
`call` runs one by its id; `execute`, where it is given, runs code over them for what takes several. An operation that \
destroys or spends is not run: it becomes a proposal the person confirms, so say what it will do. `open` offers a view as a link; `run` offers a binding of the page's keymap. The <page> block \
says what is open and the bindings bound now.\n\
\n\
Answer briefly, in the person's language. Formatting: **bold**, `- ` items, `@[label](location:<gbp id>)` to name a GBP.";

/// The model that answered: a fallback's, when it stood in.
fn answered_by(reply: &Value) -> eyre::Result<&str> {
	reply["model"].as_str().ok_or_else(|| eyre::eyre!("the model answered without its name: {reply}"))
}

/// What an answer read and wrote, as its `usage` says.
fn tokens(reply: &Value) -> eyre::Result<[(ModelTokens, i64); 4]> {
	let usage = &reply["usage"];
	let n = |field: &str| usage[field].as_i64().ok_or_else(|| eyre::eyre!("the model's usage has no {field}: {usage}"));
	Ok([
		(ModelTokens::Input, n("input_tokens")?),
		(ModelTokens::CacheWrite, n("cache_creation_input_tokens")?),
		(ModelTokens::CacheRead, n("cache_read_input_tokens")?),
		(ModelTokens::Output, n("output_tokens")?),
	])
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
	bindings: Vec<Bound>,
}

/// Asks the assistant in a chat: the question and the answer join its turns, the answer written as
/// it grows; answered with the chat once the answer is done. It runs on the asker's own Claude if
/// they connected one, else on the panel's, each round charged to them at its prices.
///
/// 503 without either Claude, or when the model does not answer.
pub(crate) async fn ask(
	State(state): State<ApiState>,
	Spends(caller, by): Spends<ModelTokens, Caller>,
	headers: HeaderMap,
	Ids(Id { id: chat }): Ids<Id<Uuid>>,
	Body(q): Body<Question>,
) -> ApiResult<Json<ChatDto>> {
	let model = state.model.clone().ok_or_else(|| Refusal::new(Code::Unavailable, "the assistant is not connected"))?;
	let (token, bill) = match (state.sign_in.panel.claude_token(caller.user_id).await?, model.owner()) {
		(Some(own), _) => (own, Bill::Theirs),
		(None, Some(owner)) => (owner.clone(), Bill::Ours),
		(None, None) => return Err(Refusal::new(Code::Unavailable, "the assistant runs on your own Claude here: connect it")),
	};
	let reader = (&caller).into();
	let read: Chat<Turn> = state.sign_in.panel.assistant_chat(reader, chat).await??;
	let started = Timestamp::now();
	if q.from > read.turns.len() {
		return Err(Refusal::new(Code::BadRequest, format!("from {} is past the chat's {} turns", q.from, read.turns.len())));
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
		cost: Cost {
			bill,
			tokens: Used {
				input: 0,
				cache_write: 0,
				cache_read: 0,
				output: 0,
			},
			charged: 0,
		},
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
	let mut tab = match bill {
		Bill::Ours => match state.sign_in.panel.model_tab(by, &answer.id, started).await? {
			Ok(tab) => Some(tab),
			Err(why) => {
				answer.text = format!("I can't answer on the panel's Claude: {why}.");
				let done = written.write(&turns, &answer, AnswerState::Stopped, Some(started)).await?;
				return Ok(Json(done));
			}
		},
		Bill::Theirs => None,
	};
	written.write(&turns, &answer, AnswerState::Working, None).await?;

	history.push(json!({ "role": "user", "content": question(&q) }));
	let permissions = caller.permissions.clone();
	let tools: Vec<Value> = state
		.tools
		.definitions(|p| p.is_none_or(|p| permissions.iter().any(|h| h == p)), true)
		.into_iter()
		.map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input }))
		.collect();
	let tools = Value::Array(tools);
	let mut rounds = 0;
	let end = loop {
		rounds += 1;
		if rounds > ROUNDS {
			answer.text.push_str("\n\nI stopped here: this took more steps than one answer may.");
			break AnswerState::Stopped;
		}
		let asked = json!({
			"model": MODEL,
			"max_tokens": 16000,
			"fallbacks": "default",
			"output_config": { "effort": "medium" },
			"system": [{ "type": "text", "text": SUBSCRIPTION }, { "type": "text", "text": SYSTEM }],
			"tools": tools,
			"messages": history,
		});
		let reply = match model.answer(&token, &asked).await? {
			Ok(reply) => reply,
			Err(said) if bill == Bill::Theirs => {
				state.sign_in.panel.claude_refused(caller.user_id, said, Timestamp::now()).await?;
				answer
					.text
					.push_str("\n\nYour Claude connection was refused: reconnect it, or disconnect it to ask on the panel's.");
				break AnswerState::Stopped;
			}
			Err(said) => return Err(Refusal::new(Code::Unavailable, format!("the panel's Claude refused its token: {said}"))),
		};
		let used = tokens(&reply)?;
		answer.cost.tokens.add(&used);
		let spent = state.sign_in.panel.model_used(by, answered_by(&reply)?, &used, tab.as_mut(), Timestamp::now()).await?;
		answer.cost.charged = tab.as_ref().map_or(0, |t| t.charged);
		let content = reply["content"].as_array().cloned().ok_or_else(|| eyre::eyre!("the model answered no content: {reply}"))?;
		let mut results = Vec::new();
		for block in &content {
			match block["type"].as_str() {
				Some("text") => answer.text = block["text"].as_str().ok_or_else(|| eyre::eyre!("a text block without text: {block}"))?.to_owned(),
				Some("tool_use") => {
					let (result, step) = use_tool(&state, &headers, &permissions, &q, &mut answer, &block["name"], &block["input"]).await;
					answer.steps.push(Step { label: step, done: true });
					// a text answer is the model's as it came; anything else, its JSON
					let content = match result {
						Value::String(text) => text,
						other => other.to_string(),
					};
					results.push(json!({ "type": "tool_result", "tool_use_id": block["id"], "content": content }));
				}
				_ => {}
			}
		}
		history.push(json!({ "role": "assistant", "content": content }));
		match reply["stop_reason"].as_str() {
			Some("tool_use") => match spent {
				Some(why) => {
					answer.text.push_str(&format!("\n\nI stopped here: {why}."));
					break AnswerState::Stopped;
				}
				None => {
					history.push(json!({ "role": "user", "content": results }));
					written.write(&turns, &answer, AnswerState::Working, None).await?;
				}
			},
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
/// 409 the action is not a proposed operation; the operation's own refusal answers with its code, or
/// `bad_gateway` for a service's word the conductor has none of.
pub(crate) async fn act(
	State(state): State<ApiState>,
	Extension(caller): Extension<Caller>,
	headers: HeaderMap,
	Ids(Id { id: chat }): Ids<Id<Uuid>>,
	Body(a): Body<Act>,
) -> ApiResult<Json<ChatDto>> {
	let read: Chat<Turn> = state.sign_in.panel.assistant_chat((&caller).into(), chat).await??;
	let at = read
		.turns
		.iter()
		.position(|t| matches!(t, Turn::Assistant { id, .. } if *id == a.turn))
		.ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
	let mut turns = read.turns;
	let Turn::Assistant { actions, .. } = &mut turns[at] else {
		unreachable!("found as an answer")
	};
	let action = actions.iter_mut().find(|x| x.id == a.action).ok_or_else(|| Refusal::new(Code::NotFound, "not found"))?;
	let (ActionState::Proposed, Target::Call(call)) = (&action.state, &action.target) else {
		return Err(Refusal::new(Code::Conflict, "only a proposed operation is done here: a view or a binding is the page's"));
	};
	let answer = state.tools.call(&headers, call.clone()).await.map_err(|m| Refusal::new(Code::BadRequest, m))?;
	let status = StatusCode::from_u16(answer.status).map_err(|e| eyre::eyre!("the app answered {}: {e}", answer.status))?;
	if !status.is_success() {
		let error = answer.body["error"].as_str().map_or_else(|| answer.body.to_string(), str::to_owned);
		let word = answer.body["code"].as_str();
		return Err(match word.and_then(|w| serde_json::from_value::<Code>(w.into()).ok()).filter(|c| c.status() == status) {
			Some(code) => Refusal::new(code, error),
			// another service's word, which the conductor's codes do not hold
			None => Refusal::new(Code::BadGateway, format!("{} refused ({}): {error}", call.operation, word.unwrap_or(status.as_str()))),
		});
	}
	action.state = ActionState::Done;
	let tail = turns.split_off(at);
	let done = state
		.sign_in
		.panel
		.assistant_turns((&caller).into(), chat, read.summary.version, at, tail, Timestamp::now())
		.await??;
	Ok(Json(done.into()))
}

/// The answer as it grows.
struct Answer {
	id: String,
	at: i64,
	steps: Vec<Step>,
	text: String,
	actions: Vec<Action>,
	cost: Cost,
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
			cost: Some(answer.cost.clone()),
		};
		let turns: Vec<Turn> = asked[self.from..].iter().cloned().chain([turn]).collect();
		let chat = self
			.state
			.sign_in
			.panel
			.assistant_turns(self.caller.into(), self.chat, self.base, self.from, turns, now)
			.await??;
		self.base = chat.summary.version;
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
	format!(
		"{}\n\n<page>\nOpen: {}\nAbout: {}\nBindings:\n{}\n</page>",
		q.text,
		q.url,
		if mentions.is_empty() { "nothing named".to_owned() } else { mentions.join(", ") },
		bindings.join("\n")
	)
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
			call(state, headers, c, answer, action).await
		}
		Some("execute") => match serde_json::from_value::<tools::Execute>(input.clone()) {
			Ok(e) => match state.tools.execute(headers, e.code).await {
				Ok(Executed { ran, proposed }) => {
					let mut result = json!({ "ran": ran });
					let step = match proposed {
						Some(c) => {
							let label = c.operation.replace('_', " ");
							answer.actions.push(action(&label, Target::Call(c)));
							result["proposed"] = json!("the run stopped at this call; the person confirms it from the answer: say what it will do");
							format!("Ran code; proposed {label}")
						}
						None => "Ran code".to_owned(),
					};
					(result, step)
				}
				Err(e) => (json!({ "error": e }), "Ran code".to_owned()),
			},
			Err(e) => (json!({ "error": e.to_string() }), "Ran code".to_owned()),
		},
		Some("open") => match serde_json::from_value::<tools::Open>(input.clone()) {
			Ok(o) => match state.tools.open(&o, |p| p.is_none_or(|p| permissions.iter().any(|h| h == p))) {
				Ok(href) => {
					let step = format!("Linked {href}");
					answer.actions.push(action(&o.label, Target::Href(href)));
					(json!({ "offered": "a link" }), step)
				}
				Err(e) => (json!({ "error": e }), format!("Linked {}", o.view)),
			},
			Err(e) => (json!({ "error": e.to_string() }), "Linked".to_owned()),
		},
		Some("run") => match serde_json::from_value::<tools::Run>(input.clone()) {
			Ok(r) if q.bindings.iter().any(|b| b.id == r.binding) => {
				let step = format!("Offered {}", r.binding);
				answer.actions.push(action(&r.label, Target::Binding(r.binding)));
				(json!({ "offered": "a button" }), step)
			}
			Ok(r) => (json!({ "error": format!("no binding {:?} is bound now", r.binding) }), "Offered a key".to_owned()),
			Err(e) => (json!({ "error": e.to_string() }), "Offered a key".to_owned()),
		},
		Some(name) => match (
			state.tools.own(|p| p.is_none_or(|p| permissions.iter().any(|h| h == p))).into_iter().find(|t| t.name == name),
			input,
		) {
			(Some(t), Value::Object(input)) => call(state, headers, t.call(input.clone()), answer, action).await,
			(Some(_), _) => (json!({ "error": "its input is an object" }), format!("Called {name}")),
			(None, _) => (json!({ "error": format!("no tool {name:?}") }), "Asked for an unknown tool".to_owned()),
		},
		None => (json!({ "error": "a tool use without a name" }), "Asked for an unknown tool".to_owned()),
	}
}

/// Runs `c` as the person asking, or proposes it for them to confirm when it destroys or spends.
async fn call(state: &ApiState, headers: &HeaderMap, c: tools::Call, answer: &mut Answer, action: impl Fn(&str, Target) -> Action) -> (Value, String) {
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
