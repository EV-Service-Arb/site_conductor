//! Whom an answer is asked of, through `ask_llm`: its asker's own models if they connected one, else
//! the panel's, never one for the other; what each answer used; and what a person is told when none
//! answers.

use std::{
	collections::HashMap,
	sync::{Arc, Mutex},
};

use ask_llm::{Attempt, Client, Failure, Recoverable, Response, Source, Step, Unrecoverable, Usage, config::AppConfig, providers::claude::ClaudeModel};
use ev_lib::api::Refusal;
use jiff::{SignedDuration, Timestamp};
use sa_core::{
	prices::{ModelTokens, Provider, Spender},
	usage::Bill,
};
use sa_panel::{Panel, own_models::Own, usage::Tab};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
	api::{ApiResult, Code},
	claude::Connects,
};

/// How long a source whose account failed is not asked by mail, which asks every minute: each failure is
/// an event, and alerting keeps one open 15 min past its last.
const REST: SignedDuration = SignedDuration::from_mins(15);

/// Whose account failed, ours (`None`) or a person's, until it answers.
type Resting = HashMap<(Option<Uuid>, Provider), Rest>;

/// Where [`Model`] reaches the providers.
pub struct Origins {
	/// `https://api.anthropic.com`.
	pub claude: String,
	/// `https://platform.claude.com`: where a code is exchanged.
	pub claude_sign_in: String,
	/// `https://api.openai.com`.
	pub openai: String,
}

/// The models, and the panel's own credentials with their providers, in the order they are asked.
#[derive(Clone)]
pub struct Model {
	pub(crate) connects: Connects,
	claude: String,
	openai: String,
	ours: Arc<Vec<(Provider, Zeroizing<String>)>>,
	resting: Arc<Mutex<Resting>>,
}
impl Model {
	pub fn new(Origins { claude, claude_sign_in, openai }: Origins, ours: Vec<(Provider, Zeroizing<String>)>) -> Self {
		Self {
			connects: Connects::new(claude_sign_in),
			claude,
			openai,
			ours: Arc::new(ours),
			resting: Arc::default(),
		}
	}

	/// The panel answers those who connected nothing of their own.
	pub fn has_ours(&self) -> bool {
		!self.ours.is_empty()
	}

	/// `user`'s own models, else the panel's.
	pub(crate) async fn chain<'a>(&'a self, panel: &'a Panel, user: Uuid) -> eyre::Result<Chain<'a>> {
		let own = panel.own_models(user).await?;
		let (bill, asked) = match own.is_empty() {
			false => (Bill::Theirs, own),
			true => (
				Bill::Ours,
				self.ours
					.iter()
					.map(|(provider, secret)| Own {
						provider: *provider,
						secret: secret.clone(),
						refused: false,
					})
					.collect(),
			),
		};
		Ok(Chain {
			model: self,
			panel,
			user,
			bill,
			asked,
		})
	}

	/// Whether `provider` answers on a person's `secret`; 400 it does not take it, 503 it does not answer now.
	pub(crate) async fn probe(&self, provider: Provider, secret: &str) -> ApiResult<()> {
		let client = Client::new(AppConfig::default())
			.unlogged()
			.fallback_chain([self.source(provider, secret, Bill::Theirs)])
			.max_tokens(1);
		let Err(e) = client.ask("hi").await else { return Ok(()) };
		let fell = tried(e)?;
		let failure = &fell.last().expect("one source is asked").failure;
		let why = Why::of(failure);
		match why {
			Why::Request(_) => Ok(()), // it answered, nothing of use
			Why::Refused => Err(Refusal::new(Code::BadRequest, format!("{provider} refuses it."))),
			Why::Credit => Err(Refusal::new(Code::BadRequest, format!("Its {provider} account is out of credit."))),
			Why::NoModel => Err(Refusal::new(Code::BadRequest, format!("Its {provider} account has no access to the model the panel asks."))),
			_ => Err(Refusal::new(Code::Unavailable, said(provider, Bill::Theirs, &why))),
		}
	}

	fn source(&self, provider: Provider, secret: &str, bill: Bill) -> Source {
		// a model put in another's place is another's price: its tool's, the admin's
		let source = match provider {
			Provider::Claude => Source::from(ClaudeModel::Opus5_5).at(&self.claude),
			Provider::OpenAi => Source::new(ask_llm::Provider::OpenAi, "gpt-5.6-terra").at(&self.openai),
		}
		.token(secret);
		match bill {
			Bill::Ours => source,
			Bill::Theirs => source.theirs(),
		}
	}

	fn resting(&self) -> std::sync::MutexGuard<'_, Resting> {
		self.resting.lock().expect("never poisoned: no panic while held")
	}
}

struct Rest {
	until: Timestamp,
	why: Why,
	/// It was told of, since it last answered, that ours is at its limit.
	limit_told: bool,
}

/// Who one answer is asked of, in order: the first step is the first's who gives it, and the rest of the answer theirs.
pub(crate) struct Chain<'a> {
	model: &'a Model,
	panel: &'a Panel,
	user: Uuid,
	pub(crate) bill: Bill,
	asked: Vec<Own>,
}
impl Chain<'_> {
	pub(crate) fn is_empty(&self) -> bool {
		self.asked.is_empty()
	}

	/// Those not refused and not resting, for what asks by itself; `Err`, why none is.
	pub(crate) fn awake(mut self) -> Result<Self, String> {
		if self.is_empty() {
			return Err("No model to read mail with: connect your own Claude or OpenAI.".to_owned());
		}
		let (now, whose, bill) = (Timestamp::now(), self.whose(), self.bill);
		let mut why = Vec::new();
		let resting = self.model.resting();
		self.asked.retain(|own| {
			let held = match resting.get(&(whose, own.provider)) {
				_ if own.refused => Some(said(own.provider, bill, &Why::Refused)),
				Some(r) if now < r.until => Some(said(own.provider, bill, &r.why)),
				_ => None,
			};
			let awake = held.is_none();
			why.extend(held);
			awake
		});
		drop(resting);
		match self.is_empty() {
			true => Err(self.told(why, true)),
			false => Ok(self),
		}
	}

	pub(crate) fn client(&self) -> Client {
		Client::new(AppConfig::default())
			.unlogged()
			.fallback_chain(self.asked.iter().map(|own| self.model.source(own.provider, &own.secret, self.bill)))
	}

	/// Who gave an answer's first `step`.
	pub(crate) fn provider(&self, step: &Step) -> Provider {
		self.asked[step.failed.len()].provider
	}

	/// `of` answered `by`: its tokens counted, on ours charged. Why no further round runs on ours.
	pub(crate) async fn used(&self, by: Spender<ModelTokens>, of: Provider, answer: &Response, tab: Option<&mut Tab>) -> eyre::Result<Option<String>> {
		self.model.resting().remove(&(self.whose(), of));
		self.panel.model_used(by, &answer.model, of.used(counts(answer.usage)), tab, Timestamp::now()).await
	}

	/// What of it failed: what it was billed counted, a person's refused credential marked, an account that
	/// failed let rest, and what its asker is told.
	pub(crate) async fn failed(&self, by: Spender<ModelTokens>, fell: &[Attempt], mut tab: Option<&mut Tab>) -> eyre::Result<Failed> {
		let now = Timestamp::now();
		let mut failed = Failed {
			said: String::new(),
			connect: false,
			request: false,
		};
		let mut why_not = Vec::new();
		for a in fell {
			let of = match a.provider {
				ask_llm::Provider::Claude => Provider::Claude,
				ask_llm::Provider::OpenAi => Provider::OpenAi,
				other => eyre::bail!("{other} was asked, and is none of ours"),
			};
			if let Some(usage) = billed(&a.failure) {
				self.panel.model_used(by, a.model, of.used(counts(usage)), tab.as_deref_mut(), now).await?;
			}
			let why = Why::of(&a.failure);
			if matches!((&why, self.bill), (Why::Refused, Bill::Theirs)) {
				self.panel.own_model_refused(self.user, of, format!("{of} no longer takes it: connect it again."), now).await?;
			}
			if matches!(why, Why::Unexpected) {
				crate::report(&eyre::eyre!("{of} `{}`: {}", a.model, a.failure), "a model failed unexpectedly");
			}
			failed.connect |= why.is_account();
			failed.request = matches!(why, Why::Request(_));
			why_not.push(said(of, self.bill, &why));
			if why.rests() {
				self.rest(of, why, now);
			}
		}
		failed.said = self.told(why_not, failed.connect);
		Ok(failed)
	}

	/// A limit of ours is the one account failure `ask_llm` does not tell as an error: told here, once until it answers.
	fn rest(&self, of: Provider, why: Why, now: Timestamp) {
		let limit = match (&why, self.bill) {
			(Why::Limit(resets), Bill::Ours) => Some(*resets),
			_ => None,
		};
		let mut resting = self.model.resting();
		let told = resting.remove(&(self.whose(), of)).is_some_and(|r| r.limit_told);
		let rest = Rest {
			until: now + REST,
			why,
			limit_told: told || limit.is_some(),
		};
		resting.insert((self.whose(), of), rest);
		drop(resting);
		if let Some(resets) = limit
			&& !told
		{
			tracing::error!(provider = %of, resets = resets.map(tracing::field::display), "the panel's model is at its limit");
		}
	}

	fn whose(&self) -> Option<Uuid> {
		match self.bill {
			Bill::Ours => None,
			Bill::Theirs => Some(self.user),
		}
	}

	fn told(&self, mut sentences: Vec<String>, connect: bool) -> String {
		if connect && self.bill == Bill::Ours {
			sentences.push("Connect your own Claude or OpenAI and it costs nothing here.".to_owned());
		}
		sentences.join(" ")
	}
}

/// None answered.
pub(crate) struct Failed {
	/// What its asker is told.
	pub(crate) said: String,
	/// Connecting their own, or again, is what helps.
	pub(crate) connect: bool,
	/// Of what was asked, not of who was.
	pub(crate) request: bool,
}

/// Why one did not answer.
enum Why {
	/// Until then, if told.
	Limit(Option<Timestamp>),
	Credit,
	Refused,
	NoModel,
	Region,
	Busy,
	/// What was asked: no other time, and perhaps no other model, answers it.
	Request(&'static str),
	Unexpected,
}
impl Why {
	fn of(failure: &Failure) -> Self {
		match failure {
			Failure::Recoverable(Recoverable::LimitReached { resets, .. }) => Self::Limit(resets.and_then(|t| Timestamp::try_from(t).ok())), // past what a date holds is not told
			Failure::Recoverable(_) => Self::Busy,
			Failure::Unrecoverable(u) => match u {
				Unrecoverable::Auth { .. } => Self::Refused,
				Unrecoverable::Quota { .. } => Self::Credit,
				Unrecoverable::ModelUnavailable { .. } => Self::NoModel,
				Unrecoverable::GeoBlocked { .. } => Self::Region,
				Unrecoverable::ContextLength { .. } => Self::Request("this conversation is longer than it reads: start a new one"),
				Unrecoverable::Other { status: 400, .. } => Self::Request("it does not take this"),
				Unrecoverable::Refused { .. } => Self::Request("it declined to answer this"),
				Unrecoverable::Empty { .. } => Self::Request("it wrote nothing"),
				Unrecoverable::Unusable { .. } => Self::Request("what it wrote does not read"),
				_ => Self::Unexpected,
			},
		}
	}

	fn is_account(&self) -> bool {
		matches!(self, Self::Limit(_) | Self::Credit | Self::Refused | Self::NoModel)
	}

	/// Asked again at once, it would fail the same and be told of again.
	fn rests(&self) -> bool {
		!matches!(self, Self::Busy | Self::Request(_))
	}
}

/// One sentence, to whom it was asked for: the provider's own words are in the logs.
fn said(of: Provider, bill: Bill, why: &Why) -> String {
	let until = |resets: &Option<Timestamp>| resets.map(|t| format!(" until {}", t.strftime("%b %-d, %H:%M UTC"))).unwrap_or_default(); // not told when
	match (bill, why) {
		(Bill::Ours, Why::Limit(resets)) => format!("The panel's {of} credits have run out{}.", until(resets)),
		(Bill::Theirs, Why::Limit(resets)) => format!("Your {of} account is at its limit{}.", until(resets)),
		(Bill::Ours, Why::Credit) => format!("The panel's {of} credits have run out."),
		(Bill::Theirs, Why::Credit) => format!("Your {of} account is out of credit: add some there, or disconnect it to ask on the panel's."),
		(Bill::Ours, Why::Refused) => format!("The panel's {of} connection is refused, and we have been told."),
		(Bill::Theirs, Why::Refused) => format!("Your {of} connection was refused: reconnect it, or disconnect it to ask on the panel's."),
		(Bill::Ours, Why::NoModel) => format!("{of} no longer serves the model the panel asks, and we have been told."),
		(Bill::Theirs, Why::NoModel) => format!("Your {of} account has no access to the model asked of it: grant it there, or disconnect it to ask on the panel's."),
		(Bill::Ours, Why::Region) => format!("{of} does not serve where the panel runs, and we have been told."),
		(Bill::Theirs, Why::Region) => format!("{of} does not serve where the panel runs: disconnect it to ask on the panel's."),
		(_, Why::Busy) => format!("{of} is not answering now: try again in a minute."),
		(_, Why::Request(why)) => format!("{of} did not answer: {why}."),
		(_, Why::Unexpected) => format!("{of} failed in a way we did not expect, and we have been told."),
	}
}

/// What a failure was billed all the same.
pub(crate) fn billed(failure: &Failure) -> Option<Usage> {
	match failure {
		Failure::Unrecoverable(Unrecoverable::Empty { usage, .. } | Unrecoverable::Unusable { usage, .. }) => Some(*usage),
		_ => None,
	}
}

/// Input, cache write, cache read, output.
pub(crate) fn counts(usage: Usage) -> [i64; 4] {
	[usage.input, usage.cache_write, usage.cached_input, usage.output].map(i64::from)
}

/// Who was asked and why each did not answer; `Err`, nobody was.
pub(crate) fn tried(e: ask_llm::Error) -> eyre::Result<Vec<Attempt>> {
	match e {
		ask_llm::Error::Recoverable(e) | ask_llm::Error::Unrecoverable(e) => Ok(e.attempts),
		ask_llm::Error::Interrupted(i) => Ok(vec![i.attempt]),
		e => Err(eyre::eyre!(e)),
	}
}
