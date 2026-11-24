//! The in-page assistant's loop: Claude asked with the caller's question, given the operations as
//! `search`, `call`, `execute` and `poll` (`crate::tools`, through the `/api/v1` gate on the caller's own
//! session) and the page as `open` (a view) and `run` (a binding of its keymap). What the asker's
//! approval level holds is proposed: they confirm it from the answer's actions, and a cell waiting on
//! it goes on. The turns are written through the chats' versioned store as the loop goes, each write
//! told to the caller's screens.

use axum::{
	Json,
	extract::State,
	http::{HeaderMap, StatusCode},
};
use ev_lib::api::{Body, ErrorCode, Ids, JsonSchema, Refusal};
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	agent::Approval,
	prices::{ModelTokens, Spender, Spends},
	usage::Bill,
};
use sa_panel::assistant::Chat;
use sa_sandbox::{Call, Until};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
	api::{ApiResult, Code, Id},
	assistant::{Action, ActionState, AnswerState, Attachment, ChatDto, Cost, Mention, Step, Target, Turn, Used},
	claude::{MODEL, SUBSCRIPTION},
	execute::{CellState, Key, Told},
	http::ApiState,
	signin::Caller,
	tools::{self, Gate, Search},
};

/// An answer that took this long may end with its asker elsewhere.
const AWAY: SignedDuration = SignedDuration::from_mins(1);

/// Model turns one question may take before the loop stops and says so.
const ROUNDS: usize = 24;

/// What the model is told, at the level the person picked.
fn system(approval: Approval) -> String {
	format!(
		"You are the Service-Arb panel's assistant. The panel runs Google Business Profiles (GBPs; a GMB in \
code) for trade businesses: the managing gmails, each GBP's state (draft, verifying, verified, suspended), its site and \
leads, its reviews, tokens that pay for Google requests. You act for the person asking, with exactly their permissions.\n\
\n\
Tools: `search` finds the panel's operations by what they do (use it before `call`; an empty query lists them all); \
`call` runs one by its id; `execute` runs Starlark over them for what takes several, in a session \
that keeps what its cells bound, and `poll` follows a cell still running. {} A proposal is confirmed by the person from the answer, so say what it will do. `open` offers a view as a link; `run` offers a binding of the page's keymap. The <page> block \
says what is open and the bindings bound now.\n\
\n\
Answer briefly, in the person's language. Formatting: **bold**, `- ` items, `@[label](location:<gbp id>)` to name a GBP.",
		tools::held(approval)
	)
}

/// The model that answered: a fallback's, when it stood in.
pub(crate) fn answered_by(reply: &Value) -> eyre::Result<&str> {
	reply["model"].as_str().ok_or_else(|| eyre::eyre!("the model answered without its name: {reply}"))
}

/// What an answer read and wrote, as its `usage` says.
pub(crate) fn tokens(reply: &Value) -> eyre::Result<[(ModelTokens, i64); 4]> {
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
	/// What the assistant does without asking first.
	approval: Approval,
}

/// Asks the assistant in a chat: the question and the answer join its turns, the answer written as
/// it grows; answered with the chat once the answer is done. It runs on the asker's own Claude if
/// they connected one, else on the panel's, each round charged to them at its prices. A call a cell
/// of the chat's session waits on is declined: the person moved on.
///
/// 503 without either Claude, or when the model does not answer.
pub(crate) async fn ask(
	State(state): State<ApiState>,
	Spends(caller, by): Spends<ModelTokens, Caller>,
	headers: HeaderMap,
	Ids(Id { id: chat }): Ids<Id<Uuid>>,
	Body(q): Body<Question>,
) -> ApiResult<Json<ChatDto>> {
	let read: Chat<Turn> = state.sign_in.panel.assistant_chat((&caller).into(), chat).await??;
	if q.from > read.turns.len() {
		return Err(Refusal::new(Code::BadRequest, format!("from {} is past the chat's {} turns", q.from, read.turns.len())));
	}
	let a = Asking {
		state: &state,
		caller: &caller,
		headers: &headers,
		chat,
		approval: q.approval,
		bindings: &q.bindings,
	};
	state.tools.sandbox().decide(&a.key(), None, false);
	let mut turns = read.turns;
	turns.truncate(q.from);
	let mut history = conversation(&turns);
	history.push(json!({ "role": "user", "content": question(&q) }));
	turns.push(Turn::User {
		id: Uuid::now_v7().to_string(),
		at: Timestamp::now().as_millisecond(),
		text: q.text.clone(),
		mentions: q.mentions.clone(),
		files: q.files.clone(),
	});
	Ok(Json(a.answer(by, turns, q.from, read.summary.version, history).await?))
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
/// request; the action is marked done in the chat. One a cell of the chat's session waits on goes on
/// there, and the answer resumes: a new one, the model told what the code did next.
///
/// 409 the action is not a proposed operation; the operation's own refusal answers with its code, or
/// `bad_gateway` for a service's word the conductor has none of.
pub(crate) async fn act(
	State(state): State<ApiState>,
	Spends(caller, by): Spends<ModelTokens, Caller>,
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
	let call = call.clone();
	let key = key(&caller, chat);
	let resumed = state.tools.sandbox().decide(&key, Some(&a.action), true);
	if resumed.is_none() {
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
	}
	action.state = ActionState::Done;
	let tail = turns.split_off(at);
	let done = state
		.sign_in
		.panel
		.assistant_turns((&caller).into(), chat, read.summary.version, at, tail, Timestamp::now())
		.await??;
	let Some((cell, approval)) = resumed else {
		return Ok(Json(done.into()));
	};
	let told = match state.tools.poll(key, &headers, cell, Until::Ended).await {
		Ok(t) => state.tools.told(&t),
		Err(e) => json!({ "error": e }),
	};
	let turns = done.turns;
	let mut history = conversation(&turns);
	history.push(json!({
		"role": "user",
		"content": format!("<resumed>The person agreed to {}, which cell {cell} waited on; the code went on:\n{told}</resumed>", call.operation),
	}));
	let a = Asking {
		state: &state,
		caller: &caller,
		headers: &headers,
		chat,
		approval,
		bindings: &[],
	};
	let from = turns.len();
	Ok(Json(a.answer(by, turns, from, done.summary.version, history).await?))
}

/// Whose a chat's sandbox session is: the chat's, as this caller asks in it.
fn key(caller: &Caller, chat: Uuid) -> Key {
	Key::Chat {
		chat,
		asker: caller.user_id,
		actor: caller.actor.as_ref().map(|a| a.sub.clone()),
	}
}

/// Who asks, where, and what the assistant may do without asking.
struct Asking<'a> {
	state: &'a ApiState,
	caller: &'a Caller,
	headers: &'a HeaderMap,
	chat: Uuid,
	approval: Approval,
	/// The page's keymap as bound now; none when no page asks.
	bindings: &'a [Bound],
}

impl Asking<'_> {
	fn key(&self) -> Key {
		key(self.caller, self.chat)
	}

	fn may(&self, permission: Option<&str>) -> bool {
		permission.is_none_or(|p| self.caller.permissions.iter().any(|h| h == p))
	}

	/// The answer after `turns[..from]`, `history` the model's read of what came before it, written into
	/// the chat as it grows from `base`.
	async fn answer(&self, by: Spender<ModelTokens>, turns: Vec<Turn>, from: usize, base: i64, mut history: Vec<Value>) -> ApiResult<ChatDto> {
		let state = self.state;
		let model = state.model.clone().ok_or_else(|| Refusal::new(Code::Unavailable, "the assistant is not connected"))?;
		let (token, bill) = match (state.sign_in.panel.claude_token(self.caller.user_id).await?, model.owner()) {
			(Some(own), _) => (own, Bill::Theirs),
			(None, Some(owner)) => (owner.clone(), Bill::Ours),
			(None, None) => return Err(Refusal::new(Code::Unavailable, "the assistant runs on your own Claude here: connect it")),
		};
		let started = Timestamp::now();
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
		let mut written = Written {
			state,
			caller: self.caller,
			chat: self.chat,
			base,
			from,
		};
		let mut tab = match bill {
			Bill::Ours => match state.sign_in.panel.model_tab(by, &answer.id, "The assistant's answer", started).await? {
				Ok(tab) => Some(tab),
				Err(why) => {
					answer.text = format!("I can't answer on the panel's Claude: {why}.");
					return written.write(&turns, &answer, AnswerState::Stopped, Some(started)).await;
				}
			},
			Bill::Theirs => None,
		};
		written.write(&turns, &answer, AnswerState::Working, None).await?;
		let tools: Vec<Value> = state
			.tools
			.definitions(|p| self.may(p), true, self.approval)
			.into_iter()
			.map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.input }))
			.collect();
		let tools = Value::Array(tools);
		let end = 'rounds: {
			for _ in 0..ROUNDS {
				let asked = json!({
					"model": MODEL,
					"max_tokens": 16000,
					"fallbacks": "default",
					"output_config": { "effort": "medium" },
					"system": [{ "type": "text", "text": SUBSCRIPTION }, { "type": "text", "text": system(self.approval) }],
					"tools": tools,
					"messages": history,
				});
				let reply = match model.answer(&token, &asked).await? {
					Ok(reply) => reply,
					Err(said) if bill == Bill::Theirs => {
						state.sign_in.panel.claude_refused(self.caller.user_id, said, Timestamp::now()).await?;
						answer
							.text
							.push_str("\n\nYour Claude connection was refused: reconnect it, or disconnect it to ask on the panel's.");
						break 'rounds AnswerState::Stopped;
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
							let (result, step) = self.use_tool(&mut answer, &block["name"], &block["input"]).await;
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
							break 'rounds AnswerState::Stopped;
						}
						None => {
							history.push(json!({ "role": "user", "content": results }));
							written.write(&turns, &answer, AnswerState::Working, None).await?;
						}
					},
					Some("refusal") => {
						answer.text = "I can't help with that.".to_owned();
						break 'rounds AnswerState::Done;
					}
					Some("end_turn" | "max_tokens" | "pause_turn" | "stop_sequence") => break 'rounds AnswerState::Done,
					other => return Err(eyre::eyre!("the model stopped for {other:?}").into()),
				}
			}
			answer.text.push_str("\n\nI stopped here: this took more steps than one answer may.");
			AnswerState::Stopped
		};
		let done = written.write(&turns, &answer, end, Some(started)).await?;
		// what is proposed at once is read where it was asked
		let proposed = answer.actions.iter().find(|a| matches!((&a.state, &a.target), (ActionState::Proposed, Target::Call(_))));
		if let Some(a) = proposed
			&& Timestamp::now().duration_since(started) >= AWAY
		{
			state
				.sign_in
				.panel
				.alerts_assistant_waits(self.caller.user_id, self.chat, &answer.id, &a.label, Timestamp::now())
				.await?;
		}
		Ok(done)
	}

	/// Runs one tool the model asked for: what it answers the model, and the step the person sees.
	async fn use_tool(&self, answer: &mut Answer, name: &Value, input: &Value) -> (Value, String) {
		let state = self.state;
		match name.as_str() {
			Some("search") => match serde_json::from_value::<Search>(input.clone()) {
				Ok(s) => {
					let found = state.tools.search(&s.query, |p| self.may(p));
					(found, format!("Searched: {}", s.query))
				}
				Err(e) => (json!({ "error": e.to_string() }), "Searched".to_owned()),
			},
			Some("call") => match serde_json::from_value::<Call>(input.clone()) {
				Ok(c) => self.call(c, answer).await,
				Err(e) => (json!({ "error": e.to_string() }), "Called".to_owned()),
			},
			Some("execute") => match serde_json::from_value::<tools::Execute>(input.clone()) {
				Ok(e) => {
					let told = state.tools.execute(self.key(), self.headers, e.code, self.approval, true).await;
					self.told(told, answer, "Ran code".to_owned())
				}
				Err(e) => (json!({ "error": e.to_string() }), "Ran code".to_owned()),
			},
			Some("poll") => match serde_json::from_value::<tools::PollCell>(input.clone()) {
				Ok(p) => {
					let told = state.tools.poll(self.key(), self.headers, p.cell, Until::More).await;
					self.told(told, answer, format!("Followed cell {}", p.cell))
				}
				Err(e) => (json!({ "error": e.to_string() }), "Followed code".to_owned()),
			},
			Some("open") => match serde_json::from_value::<tools::Open>(input.clone()) {
				Ok(o) => match state.tools.open(&o, |p| self.may(p)) {
					Ok(href) => {
						let step = format!("Linked {href}");
						answer.actions.push(action(&o.label, Target::Href(href), ActionState::Proposed));
						(json!({ "offered": "a link" }), step)
					}
					Err(e) => (json!({ "error": e }), format!("Linked {}", o.view)),
				},
				Err(e) => (json!({ "error": e.to_string() }), "Linked".to_owned()),
			},
			Some("run") => match serde_json::from_value::<tools::Run>(input.clone()) {
				Ok(r) if self.bindings.iter().any(|b| b.id == r.binding) => {
					let step = format!("Offered {}", r.binding);
					answer.actions.push(action(&r.label, Target::Binding(r.binding), ActionState::Proposed));
					(json!({ "offered": "a button" }), step)
				}
				Ok(r) => (json!({ "error": format!("no binding {:?} is bound now", r.binding) }), "Offered a key".to_owned()),
				Err(e) => (json!({ "error": e.to_string() }), "Offered a key".to_owned()),
			},
			Some(name) => match (state.tools.own(|p| self.may(p)).into_iter().find(|t| t.name == name), input) {
				(Some(t), Value::Object(input)) => self.call(t.call(input.clone()), answer).await,
				(Some(_), _) => (json!({ "error": "its input is an object" }), format!("Called {name}")),
				(None, _) => (json!({ "error": format!("no tool {name:?}") }), "Asked for an unknown tool".to_owned()),
			},
			None => (json!({ "error": "a tool use without a name" }), "Asked for an unknown tool".to_owned()),
		}
	}

	/// Runs `c` as the person asking, or proposes it for them to confirm when the level holds it; one
	/// that destroys or spends and ran on its own is an action already done.
	async fn call(&self, c: Call, answer: &mut Answer) -> (Value, String) {
		let gate = match self.state.tools.gate(&c, self.approval) {
			Ok(g) => g,
			Err(e) => return (json!({ "error": e }), format!("Called {}", c.operation)),
		};
		let label = c.operation.replace('_', " ");
		if matches!(gate, Gate::Held) {
			answer.actions.push(action(&label, Target::Call(c), ActionState::Proposed));
			return (json!({ "proposed": "the person confirms it from the answer; say what it will do" }), format!("Proposed {label}"));
		}
		if matches!(gate, Gate::OnItsOwn) {
			answer.actions.push(action(&label, Target::Call(c.clone()), ActionState::Done));
		}
		let step = format!("Called {}", c.operation);
		match self.state.tools.call(self.headers, c).await {
			Ok(a) if StatusCode::from_u16(a.status).is_ok_and(|s| s.is_success()) => (a.body, step),
			Ok(a) => (json!({ "status": a.status, "error": a.body }), step),
			Err(e) => (json!({ "error": e }), step),
		}
	}

	/// What a cell tells the model, its calls taken on their own done actions, and the one it waits
	/// on a proposed one.
	fn told(&self, told: Result<Told, String>, answer: &mut Answer, step: String) -> (Value, String) {
		let t = match told {
			Ok(t) => t,
			Err(e) => return (json!({ "error": e }), step),
		};
		for c in &t.ran_on_its_own {
			answer.actions.push(action(&c.operation.replace('_', " "), Target::Call(c.clone()), ActionState::Done));
		}
		let mut result = self.state.tools.told(&t);
		let CellState::Paused(c) = &t.state else {
			return (result, step);
		};
		let label = c.operation.replace('_', " ");
		let proposed = action(&label, Target::Call(c.clone()), ActionState::Proposed);
		self.state.tools.sandbox().proposed_as(&self.key(), proposed.id.clone());
		answer.actions.push(proposed);
		result["note"] = json!("the cell waits for the person to agree from the answer, and goes on once they do; say what it will do");
		(result, format!("{step}; waits on {label}"))
	}
}

fn action(label: &str, target: Target, state: ActionState) -> Action {
	Action {
		id: Uuid::now_v7().to_string(),
		label: label.to_owned(),
		detail: String::new(),
		target,
		done: String::new(),
		state,
	}
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
