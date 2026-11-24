//! Google's mail about Business Profiles (docs/ARCHITECTURE.md, Google's mail): who sends it, what rules
//! alone make of a message, what it names, which case it joins, and a case's state.

use std::{collections::BTreeSet, sync::LazyLock};

use jiff::{SignedDuration, Timestamp};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Google is asked again after this long without an answer, if it gave no other term.
const RE_ASK: SignedDuration = SignedDuration::from_hours(48);
/// A video or an appeal is decided on within it, if Google gave no other term.
const IN_REVIEW: SignedDuration = SignedDuration::from_hours(24 * 7);
/// Ours in a case since Google last gave something new; past it a person decides.
const MAX_ASKS: usize = 6;
/// What goes by itself waits this long after ours before it.
const APART: SignedDuration = SignedDuration::from_hours(12);
/// An inbox not read for this long may hold Google's answer.
const STALE: SignedDuration = SignedDuration::from_hours(1);
/// A draft that goes by itself waits this long.
pub const HOLD: SignedDuration = SignedDuration::from_mins(15);
/// A case whose last message is older needs nothing.
const QUIET_AFTER: SignedDuration = SignedDuration::from_hours(24 * 21);
/// Older mail is kept, not read by the model.
pub const READ_BACK: SignedDuration = SignedDuration::from_hours(24 * 90);
/// A reading failed this often is a person's.
pub const MAX_READS: i64 = 3;
/// A send not over by then was cut off.
const SENDING_FOR: SignedDuration = SignedDuration::from_mins(5);
/// A shorter name, place id or address line is in every other message.
const NAMES_FROM: usize = 4;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "MailTopic")]
pub enum Topic {
	Verification,
	Suspension,
	Reviews,
	Activity,
	Access,
	Profile,
	Other,
}

/// What a message of Google's says.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, schemars::JsonSchema, strum::EnumMessage, strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "MailKind")]
pub enum Kind {
	/// The recorded verification video did not pass.
	VideoRejected,
	/// The recorded verification video is being reviewed.
	VideoInReview,
	/// The profile is verified.
	Verified,
	/// The profile has to be verified again.
	ReverificationRequired,
	/// Support points back at the recorded video or the on-screen prompts.
	SelfServe,
	/// The live verification call is enabled.
	CallOffered,
	/// A live verification call was held and did not verify.
	CallFailed,
	/// No way to verify is left on the profile.
	OptionsExhausted,
	Suspended,
	/// The account, not one profile, is restricted.
	AccountRestricted,
	AppealReceived,
	AppealApproved,
	AppealDenied,
	/// New reviews are paused: a spike of spam.
	ReviewsPaused,
	/// Fake or incentivised reviews were found, the profile's reviews restricted.
	ReviewsRestricted,
	RestrictionLifted,
	/// A review was removed.
	ReviewRemoved,
	/// Support will not bring the reviews back.
	ReinstatementRefused,
	/// Support brought all or part of the reviews back.
	ReinstatementDone,
	/// Support sees the reviews live already.
	ReviewsLiveAlready,
	/// Support has no record of the reviews.
	ReviewsNoData,
	/// Is the business still open: inactivity, removed or revoked unless answered.
	StillActive,
	/// Someone invited this account to manage a profile.
	Invited,
	/// This account was given access to a profile.
	AccessGranted,
	/// This account can no longer manage a profile.
	AccessLost,
	/// Someone else was given access to a profile.
	OthersAccess,
	/// Someone asks for a profile's ownership.
	OwnershipRequested,
	/// Someone asks to manage a profile.
	ManagementRequested,
	/// Someone suggested an edit: does this look right to you?
	EditSuggested,
	/// An edit of ours was not accepted.
	EditRejected,
	/// Google changed the profile.
	ChangedByGoogle,
	/// A photo, post or other content was removed.
	ContentRemoved,
	PolicyWarning,
	/// The profile is marked as another's duplicate.
	Duplicate,
	/// A request was received, nothing more.
	Acknowledged,
	/// Handed to a specialist or another team, who answers.
	Routed,
	/// Asks for documents, photos, changes or answers before going on: which profile, which reviews.
	Asks,
	/// The case is closed: it accepts no reply, or a survey follows it.
	Closed,
	/// About a profile, what it says cannot be told.
	Unclear,
	/// Performance, promotions, surveys, new reviews, posts.
	Noise,
	/// Not about a Business Profile.
	NotBusiness,
	/// Not from Google.
	Impersonation,
}
impl Kind {
	/// The topic it is always of; `None`, the message's own.
	pub fn topic(self) -> Option<Topic> {
		Some(match self {
			Self::VideoRejected | Self::VideoInReview | Self::Verified | Self::ReverificationRequired | Self::SelfServe | Self::CallOffered | Self::CallFailed | Self::OptionsExhausted =>
				Topic::Verification,
			Self::Suspended | Self::AccountRestricted | Self::AppealReceived | Self::AppealApproved | Self::AppealDenied => Topic::Suspension,
			Self::ReviewsPaused
			| Self::ReviewsRestricted
			| Self::RestrictionLifted
			| Self::ReviewRemoved
			| Self::ReinstatementRefused
			| Self::ReinstatementDone
			| Self::ReviewsLiveAlready
			| Self::ReviewsNoData => Topic::Reviews,
			Self::StillActive => Topic::Activity,
			Self::Invited | Self::AccessGranted | Self::AccessLost | Self::OthersAccess | Self::OwnershipRequested | Self::ManagementRequested => Topic::Access,
			Self::EditSuggested | Self::EditRejected | Self::ChangedByGoogle | Self::ContentRemoved | Self::PolicyWarning | Self::Duplicate => Topic::Profile,
			Self::Noise | Self::NotBusiness | Self::Impersonation => Topic::Other,
			Self::Acknowledged | Self::Routed | Self::Asks | Self::Closed | Self::Unclear => return None,
		})
	}

	/// Whose turn it leaves.
	fn turn(self) -> Turn {
		match self {
			Self::SelfServe | Self::OptionsExhausted | Self::ReinstatementRefused | Self::ReviewsNoData | Self::Asks | Self::Unclear => Turn::Reply,
			Self::Acknowledged | Self::Routed | Self::VideoInReview | Self::AppealReceived => Turn::Google,
			Self::Verified | Self::AppealApproved | Self::RestrictionLifted | Self::ReinstatementDone | Self::Closed => Turn::Nobody { resolved: true },
			Self::ReviewRemoved
			| Self::Invited
			| Self::AccessGranted
			| Self::EditRejected
			| Self::ChangedByGoogle
			| Self::ContentRemoved
			| Self::Noise
			| Self::NotBusiness
			| Self::Impersonation => Turn::Nobody { resolved: false },
			Self::VideoRejected
			| Self::ReverificationRequired
			| Self::CallOffered
			| Self::CallFailed
			| Self::Suspended
			| Self::AccountRestricted
			| Self::AppealDenied
			| Self::ReviewsPaused
			| Self::ReviewsRestricted
			| Self::ReviewsLiveAlready
			| Self::StillActive
			| Self::AccessLost
			| Self::OthersAccess
			| Self::OwnershipRequested
			| Self::ManagementRequested
			| Self::EditSuggested
			| Self::PolicyWarning
			| Self::Duplicate => Turn::Operator,
		}
	}

	/// Whether it settles a case of `topic`, said in it.
	fn resolves(self, topic: Topic) -> bool {
		self.turn() == Turn::Nobody { resolved: true } && self.topic().is_none_or(|t| t == topic)
	}

	/// Whether what it says of a GBP stands no more once `later` is said of it.
	fn superseded_by(self, later: Self) -> bool {
		let by: &[Self] = match self {
			Self::VideoRejected | Self::VideoInReview | Self::ReverificationRequired => &[Self::VideoRejected, Self::VideoInReview, Self::CallOffered, Self::Verified],
			Self::CallOffered | Self::CallFailed => &[Self::CallOffered, Self::CallFailed, Self::Verified],
			Self::Suspended | Self::AppealReceived | Self::AppealDenied => &[Self::AppealReceived, Self::AppealApproved, Self::AppealDenied],
			Self::AccountRestricted => &[Self::AppealReceived, Self::AppealApproved],
			Self::ReviewsPaused | Self::ReviewsRestricted => &[Self::ReviewsPaused, Self::ReviewsRestricted, Self::RestrictionLifted],
			Self::ReviewsLiveAlready => &[Self::ReinstatementDone],
			Self::StillActive => &[Self::StillActive],
			Self::AccessLost => &[Self::AccessGranted],
			_ => &[], // two edits suggested, two requests, two warnings are two
		};
		by.contains(&later)
	}
}

/// What a message of Google's was read as.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "MailReading")]
pub struct Reading {
	pub topic: Topic,
	pub kind: Kind,
	/// The GBP it is about, as its reader saw it; one it names outright goes first.
	pub gmb: Option<i64>,
	/// One line, English.
	pub summary: String,
	/// What Google asks for, each thing once.
	pub asks: Vec<String>,
	/// Why, in Google's words.
	pub reasons: Vec<String>,
	/// Only when the mail states one.
	pub deadline: Option<Timestamp>,
	/// Only when the mail states it.
	pub answer_within_hours: Option<u32>,
}

/// Which of Google's addresses sends a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sender {
	/// Support, human or templated: the one that takes a reply.
	Support,
	/// The automated notices.
	Notice,
	/// Sometimes about a profile, never taking a reply.
	Noreply,
	/// Not a Business Profile's.
	Other,
}
impl Sender {
	pub fn of(address: &str) -> Self {
		match address.trim().to_lowercase().as_str() {
			"googlebusinessprofile-support@google.com" => Self::Support,
			"businessprofile-noreply@google.com" => Self::Notice,
			"googlebusinessprofile-noreply@google.com" | "noreply@google.com" | "noreply-maps-issues@google.com" | "help-communities@google.com" => Self::Noreply,
			a if a.starts_with("googlemybusiness-") && a.ends_with("@google.com") => Self::Noreply,
			_ => Self::Other,
		}
	}
}

/// Where a reply may go, or why not: the counterpart's Reply-To, else its From, support's, of a message that
/// is Google's.
pub fn recipient<'a>(authentic: bool, from: Option<&'a str>, reply_to: Option<&'a str>) -> Result<&'a str, String> {
	if !authentic {
		return Err("the message it answers is not proved Google's".to_owned());
	}
	let to = reply_to.or(from).ok_or("the message it answers names no sender")?;
	match Sender::of(to) {
		Sender::Support => Ok(to),
		Sender::Notice | Sender::Noreply | Sender::Other => Err(format!("{to} is not Google's support")),
	}
}

/// Whether a message is Google's: its one From is @google.com exactly, and the first
/// `Authentication-Results` Gmail itself wrote (`results`, topmost first) passes DMARC for it.
pub fn authentic(from: &[&str], results: &[&str]) -> bool {
	let [from] = from else { return false };
	if domain(from).as_deref() != Some("google.com") {
		return false;
	}
	let gmails = results.iter().find_map(|r| {
		let bare = bare(r)?;
		(bare.split(';').next()?.split_whitespace().next()? == "mx.google.com").then_some(bare)
	});
	let Some(gmails) = gmails else { return false };
	let mut dmarc = gmails.split(';').skip(1).map(str::trim).filter(|s| s.starts_with("dmarc="));
	let (Some(one), None) = (dmarc.next(), dmarc.next()) else { return false };
	let mut words = one.split_whitespace();
	words.next() == Some("dmarc=pass") && words.any(|w| w == "header.from=google.com")
}

/// What rules alone make of a message received.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Ruled {
	/// Not a Business Profile's: nothing of it is kept but its line in the log.
	Not,
	Reads(Kind),
	/// The model reads it.
	Model,
}

/// `from`: its address; `shown`: its sender as a mail client shows it.
pub fn ruled(from: Option<&str>, shown: &str, authentic: bool, subject: &str) -> Ruled {
	let Some(from) = from else { return Ruled::Not };
	match domain(from).as_deref() {
		Some("google.com") => match Sender::of(from) {
			Sender::Other => Ruled::Not,
			_ if !authentic => Ruled::Reads(Kind::Impersonation),
			Sender::Notice => fixed(subject).map_or(Ruled::Model, Ruled::Reads),
			Sender::Support | Sender::Noreply => Ruled::Model,
		},
		Some(d) if d.ends_with(".google.com") => Ruled::Not,
		_ if shown.to_lowercase().contains("google") || from.to_lowercase().contains("google") => Ruled::Reads(Kind::Impersonation),
		_ => Ruled::Not,
	}
}

/// Whether a message from `sender` takes a reply.
pub fn repliable(sender: Sender, text: &str) -> bool {
	static REFUSES: LazyLock<Regex> = LazyLock::new(|| {
		let refuses = [
			r"(can't|cannot|can not|doesn't|does not|don't|do not|unable to) (accept|receive|allow|take) (any )?(replies|responses)",
			// "if you do not reply" asks for one
			r"(^|[.!?:*] )(do not|don't) (reply|respond)\b",
			r"please,? (do not|don't) (reply|respond)\b",
			r"(isn't|is not|aren't|are not|not) (being )?(actively )?monitored\b",
			r"(n'accepte|ne peut|ne permet|ne reçoit|ne prend) (pas|aucune|plus)( de)?( recevoir| accepter)? ?(les |de |des |aucune )?réponses?",
			r"(veuillez|merci de|prière de) ne pas (y )?répondre|(^|[.!?:*] )ne (pas répondre|répondez pas)",
			r"n'est pas (surveillée?|consultée?|relevée?)",
		];
		Regex::new(&refuses.join("|")).expect("a constant")
	});
	sender == Sender::Support && !REFUSES.is_match(&squeezed(text).replace('’', "'"))
}

/// The case numbers a subject carries.
pub fn numbers(subject: &str) -> BTreeSet<String> {
	static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d-\d{13}\b").expect("a constant"));
	NUMBER.find_iter(subject).map(|m| m.as_str().to_owned()).collect()
}

/// The one profile a message links, by Google's id in the link: its verification's and its page's are two.
pub fn listing(text: &str) -> Option<String> {
	static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"business\.google\.com/(?:verify/l/(\d+)|n/(\d+)/profile)").expect("a constant"));
	let ids: BTreeSet<&str> = LINK.captures_iter(text).map(|c| c.get(1).or(c.get(2)).expect("one alternative matched").as_str()).collect();
	match ids.into_iter().collect::<Vec<_>>().as_slice() {
		[one] => Some((*one).to_owned()),
		_ => None,
	}
}

/// A message's own words: quoted history, forwards and tracking links dropped.
pub fn trimmed(raw: &str) -> String {
	static FORWARD: LazyLock<Regex> =
		LazyLock::new(|| Regex::new(r"^(Forwarded Conversation|-{3,} ?(Forwarded message|Original Message|Message transféré|Message d'origine) ?-{3,})$").expect("a constant"));
	static TRACKED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s?<https?://c\.gle/[^>]*>|(business\.google\.com/[^\s?>]*)\?[^\s>]*").expect("a constant"));
	static BLANKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").expect("a constant"));
	let lines: Vec<&str> = raw.lines().map(str::trim_end).collect();
	let mut own = Vec::new();
	let mut at = 0;
	while at < lines.len() {
		if FORWARD.is_match(lines[at].trim()) {
			break;
		}
		if let Some(n) = attribution(&lines[at..]) {
			at += n;
			match lines[at..].iter().find(|l| !l.is_empty()) {
				Some(l) if l.starts_with('>') => continue,
				_ => break, // quoted without marks, to the end
			}
		}
		if !lines[at].starts_with('>') {
			own.push(lines[at]);
		}
		at += 1;
	}
	let own = own.join("\n");
	BLANKS.replace_all(&TRACKED.replace_all(&own, "$1"), "\n\n").trim().to_owned()
}

/// A GBP as a message may name it.
#[derive(Clone, Debug)]
pub struct Named {
	pub id: i64,
	pub name: Option<String>,
	pub place_id: Option<String>,
	pub address: Option<String>,
}

/// The GBPs `text` names, by name or place id, else by their address's first line. A longer name takes
/// its words first: one GBP's name may begin another's.
pub fn named(text: &str, gbps: &[Named]) -> BTreeSet<i64> {
	let text = squeezed(text);
	let by = |marks: Vec<(i64, String)>| {
		let mut marks: Vec<(i64, String)> = marks.into_iter().map(|(id, m)| (id, squeezed(&m))).filter(|(_, m)| m.chars().count() >= NAMES_FROM).collect();
		marks.sort_by_key(|(_, m)| std::cmp::Reverse(m.len()));
		let mut left = text.clone();
		let mut found = BTreeSet::new();
		for (id, mark) in marks {
			let at: Vec<usize> = left.match_indices(&mark).map(|(i, _)| i).filter(|&i| whole(&left, i, mark.len())).collect();
			if at.is_empty() {
				continue;
			}
			found.insert(id);
			for i in at {
				left.replace_range(i..i + mark.len(), &"\0".repeat(mark.len()));
			}
		}
		found
	};
	let names = by(gbps.iter().flat_map(|g| [&g.name, &g.place_id].into_iter().flatten().map(|m| (g.id, m.clone()))).collect());
	if !names.is_empty() {
		return names;
	}
	by(gbps
		.iter()
		.filter_map(|g| Some((g.id, g.address.as_deref()?.split(',').next().expect("split yields one").to_owned())))
		.collect())
}

/// What decides which case a message joins.
#[derive(Clone, Debug)]
pub struct Joining<'a> {
	/// A notice: Gmail threads those of different profiles together by subject.
	pub alone: bool,
	pub thread: &'a str,
	/// The Message-IDs it answers.
	pub refs: &'a [String],
	pub numbers: &'a BTreeSet<String>,
}

/// What a case of the gmail's holds.
#[derive(Clone, Debug, Default)]
pub struct Holds {
	pub case: i64,
	/// Its conversation's: a notice's holds other profiles' too.
	pub threads: BTreeSet<String>,
	pub message_ids: BTreeSet<String>,
	pub numbers: BTreeSet<String>,
}

/// The case a message joins; `None`, a new one.
pub fn join(m: &Joining<'_>, cases: &[Holds]) -> Option<i64> {
	let latest = |holds: &dyn Fn(&Holds) -> bool| cases.iter().filter(|c| holds(c)).map(|c| c.case).max();
	let by_number = || latest(&|c| !c.numbers.is_disjoint(m.numbers));
	if m.alone {
		return by_number();
	}
	latest(&|c| m.refs.iter().any(|r| c.message_ids.contains(r)))
		.or_else(|| latest(&|c| c.threads.contains(m.thread)))
		.or_else(by_number)
}

/// A message of a case, as its state reads it.
#[derive(Clone, Debug)]
pub struct Message {
	pub id: i64,
	pub at: Timestamp,
	pub sent: bool,
	/// Received, from an address taking no reply; sent, to such alone.
	pub noreply: bool,
	pub repliable: bool,
	/// Received, to the account its inbox is read as; sent, by it.
	pub addressed: bool,
	pub reading: Option<Reading>,
	/// Why it is not read, once given up on.
	pub read_error: Option<String>,
}
impl Message {
	/// Whether it counts in its case: not ours where nobody reads it, nor a survey.
	pub fn heard(&self) -> bool {
		!(self.sent && self.noreply) && !self.reading.as_ref().is_some_and(|r| matches!(r.kind, Kind::Noise | Kind::NotBusiness))
	}
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "MailDraftState")]
pub enum DraftState {
	/// Waits for the operator.
	Proposed,
	/// Goes by itself at its `send_at`.
	Held,
	/// Handed to SMTP; one left so may have gone out.
	Sending,
	Sent,
	Dismissed,
	/// Another message came in between.
	Superseded,
	/// Not sent, or not written.
	Failed,
}

/// A reply to a case, as its state reads it.
#[derive(Clone, Debug)]
pub struct Draft {
	pub state: DraftState,
	/// The case's last message when it was written.
	pub answers: i64,
	pub needs: Vec<String>,
	pub written_at: Timestamp,
	/// Held, when it goes; sending, since when.
	pub send_at: Option<Timestamp>,
	pub sent_at: Option<Timestamp>,
	/// Why it was not sent, or nothing was written.
	pub error: Option<String>,
}

/// The GBP a case is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum About {
	Unknown,
	Live,
	Removed,
}

/// What a case's state stands on.
#[derive(Clone, Debug)]
pub struct Case<'a> {
	pub topic: Topic,
	pub about: About,
	/// Oldest first.
	pub messages: &'a [Message],
	/// The latest.
	pub draft: Option<&'a Draft>,
	/// The message a person marked it done at.
	pub done: Option<i64>,
	/// Google's readings on its GBP in its other cases, and the GBP's moves as such.
	pub elsewhere: &'a [(Timestamp, Kind)],
	/// Another case, open, asks Google to verify its GBP.
	pub asked_elsewhere: bool,
	/// Why its mail is not read or its reply not written: no Claude, no balance.
	pub stuck: Option<&'a str>,
	/// When its inbox was last read, since the conductor started.
	pub read: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, strum::EnumDiscriminants, schemars::JsonSchema)]
#[serde(tag = "is", rename_all = "snake_case")]
#[schemars(rename = "MailState")]
#[strum_discriminants(
	doc = "A case's state, by its name.",
	name(Standing),
	derive(Deserialize, schemars::JsonSchema),
	serde(rename_all = "snake_case"),
	schemars(rename = "MailStanding")
)]
pub enum State {
	NeedsYou {
		why: Why,
	},
	/// A draft waits for the operator.
	ReplyReady,
	/// A draft goes by itself.
	Sending {
		at: Timestamp,
	},
	/// Google waits for an answer, or has been silent too long; no draft yet.
	ReplyDue,
	/// Google's turn.
	Waiting {
		until: Timestamp,
	},
	Done {
		/// A person marked it so.
		by_hand: bool,
	},
	/// Nothing to do.
	Quiet,
}

/// Why a case needs the operator.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "code", rename_all = "snake_case")]
#[schemars(rename = "MailWhy")]
pub enum Why {
	/// What Google said is the operator's to act on; `asks`, with the reply its draft has ready.
	Kind { kind: Kind },
	/// A date Google set runs.
	Deadline { at: Timestamp },
	/// The draft lacks what only the operator has.
	Needs { needs: Vec<String> },
	/// Google answers nothing new, however often asked.
	AskedOften,
	/// Google is silent, and nothing here takes a reply, or its specialist was asked again already.
	Unanswered { since: Timestamp },
	/// Google asks in a mail that takes no reply.
	NoReply,
	/// Not read; `null`: not yet.
	Unread { error: Option<String> },
	/// Neither read nor answered until this is mended.
	Stuck { error: String },
	/// The writer had nothing to say.
	NothingToSay { why: String },
	/// Cut off while sending, or sent and never seen in the inbox: check the thread.
	MayHaveGoneOut,
	/// The reply was not written, or not sent.
	Failed { error: String },
}

pub fn state(c: &Case<'_>, now: Timestamp) -> State {
	let heard = heard(c);
	let Some(last) = heard.last() else { return State::Quiet };
	if c.done.is_some() && c.messages.iter().rev().take_while(|m| Some(m.id) != c.done).all(|m| !m.heard()) {
		return State::Done { by_hand: true };
	}
	let draft = c.draft.filter(|d| d.answers == last.id);
	let needs = |why| State::NeedsYou { why };
	if let Some(Draft {
		state: DraftState::Sending,
		send_at,
		..
	}) = draft
	{
		let at = send_at.expect("a draft is sending since a moment");
		return if now.duration_since(at) < SENDING_FOR {
			State::Sending { at }
		} else {
			needs(Why::MayHaveGoneOut)
		};
	}
	if c.about == About::Removed {
		return State::Quiet;
	}
	let sent_at = draft.filter(|d| d.state == DraftState::Sent).map(|d| d.sent_at.expect("a sent draft went out at a moment"));
	let said = said(&heard);
	let deadline = said.and_then(|r| r.deadline).filter(|at| now < *at);
	if deadline.is_none() && now.duration_since(sent_at.unwrap_or(last.at)) > QUIET_AFTER {
		return State::Quiet;
	}
	if over(c, &heard) {
		return State::Done { by_hand: false };
	}
	// what Google asks is the operator's to give or to confirm
	let asks = last.reading.as_ref().is_some_and(|r| r.kind == Kind::Asks);
	match draft {
		Some(Draft {
			state: DraftState::Held,
			send_at,
			needs: lacks,
			..
		}) if lacks.is_empty() && press(c, now).is_some() => {
			return State::Sending {
				at: send_at.expect("a held draft goes at a moment"),
			};
		}
		Some(Draft {
			state: DraftState::Proposed | DraftState::Held,
			needs: lacks,
			..
		}) =>
			return match (lacks.is_empty(), asks) {
				(true, false) => State::ReplyReady,
				(true, true) => needs(Why::Kind { kind: Kind::Asks }),
				(false, _) => needs(Why::Needs { needs: lacks.clone() }),
			},
		Some(Draft {
			state: DraftState::Failed, error, ..
		}) => {
			return needs(Why::Failed {
				error: error.clone().expect("a failed draft says why"),
			});
		}
		Some(Draft {
			state: DraftState::Dismissed,
			error: Some(why),
			..
		}) => return needs(Why::NothingToSay { why: why.clone() }),
		Some(Draft { state: DraftState::Dismissed, .. }) if asks => return needs(Why::Kind { kind: Kind::Asks }),
		Some(Draft {
			state: DraftState::Dismissed,
			written_at,
			..
		}) if now < later(*written_at, RE_ASK) => return State::Waiting { until: later(*written_at, RE_ASK) },
		_ => {}
	}
	let turn = |kind: Kind| match kind.turn() {
		Turn::Operator if kind == Kind::CallFailed && counterpart(c.messages.iter()).is_some() => Turn::Reply,
		t => t,
	};
	let silence = || {
		let until = term(&heard, sent_at);
		// an inbox not read since may hold the answer
		if now < until || c.read.is_none_or(|read| read < until) {
			return State::Waiting { until };
		}
		match said.map(|r| (r.kind, turn(r.kind))) {
			Some((kind, Turn::Operator)) => needs(Why::Kind { kind }),
			_ if sent_at.is_some() => needs(Why::MayHaveGoneOut),
			Some((Kind::Routed, _)) if last.sent => needs(Why::Unanswered { since: until }),
			_ => reply(c, &heard, Some(until)),
		}
	};
	if last.sent || sent_at.is_some() {
		return match said {
			Some(r) if r.kind.resolves(c.topic) => State::Done { by_hand: false },
			_ => silence(),
		};
	}
	let Some(read) = &last.reading else {
		return needs(match (&last.read_error, c.stuck) {
			(None, Some(error)) => Why::Stuck { error: error.to_owned() },
			(error, _) => Why::Unread { error: error.clone() },
		});
	};
	let or_deadline = |otherwise| deadline.map_or(otherwise, |at| needs(Why::Deadline { at }));
	match turn(read.kind) {
		_ if matches!(read.kind, Kind::VideoRejected | Kind::VideoInReview) && c.asked_elsewhere => State::Quiet,
		Turn::Reply => reply(c, &heard, None),
		Turn::Google => or_deadline(silence()),
		Turn::Nobody { resolved: false } => or_deadline(State::Quiet),
		Turn::Nobody { resolved: true } if read.kind.resolves(c.topic) => State::Done { by_hand: false },
		// resolved, and not what the case is about
		Turn::Nobody { resolved: true } | Turn::Operator => needs(Why::Kind { kind: read.kind }),
	}
}

/// From when a reply lacking nothing may go to Google by itself; `None`, never.
pub fn press(c: &Case<'_>, now: Timestamp) -> Option<Timestamp> {
	let heard = heard(c);
	let r = said(&heard)?;
	let to = counterpart(c.messages.iter())?;
	let ours = heard.iter().rev().find(|m| m.sent);
	let presses = match r.kind {
		Kind::SelfServe | Kind::OptionsExhausted | Kind::ReinstatementRefused | Kind::ReviewsNoData | Kind::CallFailed => true,
		k => k.turn() == Turn::Google && now >= term(&heard, None),
	};
	let goes = presses
		&& r.asks.is_empty()
		&& r.kind.topic().is_none_or(|t| t == c.topic)
		&& c.about == About::Live
		&& !settled(c)
		&& !over(c, &heard)
		&& (to.addressed || heard.iter().any(|m| m.sent && m.addressed))
		&& asked(&heard) < MAX_ASKS
		&& c.read.is_none_or(|read| now.duration_since(read) <= STALE);
	goes.then(|| ours.map_or(to.at, |m| later(m.at, APART)))
}

/// Whom a reply goes to: Google's last message from an address that takes one, unless it refuses it.
pub fn counterpart<'a>(messages: impl DoubleEndedIterator<Item = &'a Message>) -> Option<&'a Message> {
	messages.rev().find(|m| m.heard() && !m.sent && !m.noreply).filter(|m| m.repliable)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "VerificationMethod")]
pub enum Method {
	Video,
	Call,
	Unknown,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "VerificationOutcome")]
pub enum Outcome {
	InReview,
	Rejected,
	Offered,
	Failed,
	Verified,
}

/// What a reading says of a try to verify its GBP.
pub fn attempt(kind: Kind) -> Option<(Method, Outcome)> {
	Some(match kind {
		Kind::VideoInReview => (Method::Video, Outcome::InReview),
		Kind::VideoRejected => (Method::Video, Outcome::Rejected),
		Kind::CallOffered => (Method::Call, Outcome::Offered),
		Kind::CallFailed => (Method::Call, Outcome::Failed),
		Kind::Verified => (Method::Unknown, Outcome::Verified),
		_ => return None,
	})
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Turn {
	Reply,
	Google,
	Operator,
	Nobody { resolved: bool },
}

fn domain(address: &str) -> Option<String> {
	address.trim().rsplit_once('@').map(|(_, d)| d.to_lowercase())
}

/// A header without its comments and quoted strings, which a sender writes; `None`, one never closes.
fn bare(header: &str) -> Option<String> {
	let (mut out, mut depth, mut quoted) = (String::new(), 0_usize, false);
	let mut chars = header.chars();
	while let Some(c) = chars.next() {
		match c {
			'\\' if quoted || depth > 0 => {
				chars.next()?;
			}
			'"' if depth == 0 => quoted = !quoted,
			'(' if !quoted => depth += 1,
			')' if !quoted => depth = depth.checked_sub(1)?,
			_ if !quoted && depth == 0 => out.push(c.to_ascii_lowercase()),
			_ => {}
		}
	}
	(!quoted && depth == 0).then_some(out)
}

/// What a notice's subject says by itself.
fn fixed(subject: &str) -> Option<Kind> {
	static SUBJECTS: LazyLock<[(Regex, Kind); 4]> = LazyLock::new(|| {
		[
			(r"^you are now (an? |the )?(primary )?(manager|owner) of ", Kind::AccessGranted),
			(r" invited you to manage ", Kind::Invited),
			(r"^you can no longer manage ", Kind::AccessLost),
			(r"performance report|^you got \d+ new reviews?| left a review for ", Kind::Noise),
		]
		.map(|(r, k)| (Regex::new(r).expect("a constant"), k))
	});
	let subject = squeezed(subject);
	SUBJECTS.iter().find(|(r, _)| r.is_match(&subject)).map(|(_, k)| *k)
}

/// Lowercase, runs of whitespace one space: Google wraps names over lines.
fn squeezed(s: &str) -> String {
	s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How many of `lines`, from the first, say who wrote what follows: "On … wrote:", "Le … a écrit :",
/// wrapped.
fn attribution(lines: &[&str]) -> Option<usize> {
	static SAID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(On\b.*\bwrote ?:|Le\b.*\ba écrit ?:)$").expect("a constant"));
	static DATED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@|\b20\d\d\b").expect("a constant"));
	if !lines[0].starts_with("On ") && !lines[0].starts_with("Le ") {
		return None;
	}
	(1..=lines.len().min(4)).find(|&n| {
		let joined = lines[..n].join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
		SAID.is_match(&joined) && DATED.is_match(&joined)
	})
}

/// Whether `len` bytes of `text` at `at` are no part of a longer word.
fn whole(text: &str, at: usize, len: usize) -> bool {
	let inside = |c: Option<char>| c.is_some_and(char::is_alphanumeric);
	!inside(text[..at].chars().next_back()) && !inside(text[at + len..].chars().next())
}

fn heard<'a>(c: &Case<'a>) -> Vec<&'a Message> {
	c.messages.iter().filter(|m| m.heard()).collect()
}

/// Google's last word, once read.
fn said<'a>(heard: &[&'a Message]) -> Option<&'a Reading> {
	heard.iter().rev().find(|m| !m.sent)?.reading.as_ref()
}

/// A moment told by a mail, whatever its server or its reader made of it.
fn later(at: Timestamp, by: SignedDuration) -> Timestamp {
	at.saturating_add(by).expect("a duration has no calendar unit")
}

/// Until when Google's answer is waited for; `sent_at`, our last, not in the inbox yet.
fn term(heard: &[&Message], sent_at: Option<Timestamp>) -> Timestamp {
	let promised = heard.iter().rev().find(|m| !m.sent).map(|m| {
		let by = m.reading.as_ref().map_or(RE_ASK, |r| match (r.answer_within_hours, r.kind) {
			(Some(h), _) => SignedDuration::from_hours(i64::from(h)),
			(None, Kind::VideoInReview | Kind::AppealReceived) => IN_REVIEW,
			(None, _) => RE_ASK,
		});
		later(m.at, by)
	});
	// a promise made before we asked again does not answer what we asked since
	let ours = sent_at.or_else(|| heard.iter().rev().find(|m| m.sent).map(|m| m.at));
	promised.into_iter().chain(ours.map(|at| later(at, RE_ASK))).max().expect("what is heard is Google's or ours")
}

/// Ours since Google last gave something new.
fn asked(heard: &[&Message]) -> usize {
	heard
		.iter()
		.rev()
		.take_while(|m| m.sent || !m.reading.as_ref().is_some_and(|r| matches!(r.kind.turn(), Turn::Operator | Turn::Nobody { resolved: true })))
		.filter(|m| m.sent)
		.count()
}

/// Whether its GBP's other mail, or the GBP's moves, leave nothing of it to do.
fn over(c: &Case<'_>, heard: &[&Message]) -> bool {
	let (Some(last), Some(said)) = (heard.last(), said(heard)) else { return false };
	let operators = said.kind.turn() == Turn::Operator;
	let settles = |k: Kind| !operators && matches!(k, Kind::Verified | Kind::AppealApproved | Kind::ReinstatementDone) && k.resolves(c.topic);
	c.elsewhere.iter().any(|&(at, k)| at > last.at && (said.kind.superseded_by(k) || settles(k))) || (!operators && said.kind != Kind::Asks && settled(c))
}

/// Whether its GBP, by all that is said of it, is verified, of a verification; reinstated, of a suspension.
fn settled(c: &Case<'_>) -> bool {
	let own = c.messages.iter().filter_map(|m| Some((m.at, m.reading.as_ref()?.kind)));
	let mut all: Vec<(Timestamp, Kind)> = c.elsewhere.iter().copied().chain(own).collect();
	all.sort_by_key(|(at, _)| *at);
	let (mut verified, mut suspended) = (false, None);
	for (_, kind) in all {
		match kind {
			Kind::Verified => (verified, suspended) = (true, suspended.map(|_| false)),
			Kind::ReverificationRequired | Kind::VideoRejected => verified = false,
			Kind::Suspended => suspended = Some(true),
			Kind::AppealApproved => suspended = Some(false),
			_ => {}
		}
	}
	match c.topic {
		Topic::Verification => verified && suspended != Some(true),
		Topic::Suspension => suspended == Some(false),
		_ => false,
	}
}

/// A reply of ours is due: of Google's word, or its `silence` since.
fn reply(c: &Case<'_>, heard: &[&Message], silence: Option<Timestamp>) -> State {
	let why = match (counterpart(c.messages.iter()), silence) {
		(None, Some(since)) => Why::Unanswered { since },
		(None, None) => Why::NoReply,
		(Some(_), _) if asked(heard) >= MAX_ASKS => Why::AskedOften,
		(Some(_), _) => match c.stuck {
			None => return State::ReplyDue,
			Some(error) => Why::Stuck { error: error.to_owned() },
		},
	};
	State::NeedsYou { why }
}

#[cfg(test)]
mod tests {
	use super::*;

	const GMAIL: &str = "mx.google.com; dkim=pass header.i=@google.com header.s=20230601 header.b=abc; spf=pass (google.com: domain of 3xyz@scoutcamp.bounces.google.com designates 209.85.220.69 as permitted sender) smtp.mailfrom=3xyz@scoutcamp.bounces.google.com; dmarc=pass (p=REJECT sp=REJECT dis=NONE) header.from=google.com; dara=pass header.i=@gmail.com";
	const NOTICE: &str = "businessprofile-noreply@google.com";
	const SUPPORT: &str = "googlebusinessprofile-support@google.com";
	const DONE: State = State::Done { by_hand: false };

	fn t(s: &str) -> Timestamp {
		s.parse().unwrap()
	}

	#[test]
	fn only_gmails_own_first_verdict_makes_a_message_googles() {
		assert!(authentic(&[NOTICE], &[GMAIL]));
		assert!(!authentic(&[NOTICE], &[]), "nobody checked it");
		assert!(!authentic(&[NOTICE, "a@google.com"], &[GMAIL]), "two senders");
		assert!(!authentic(&["businessprofile-noreply@google.com.verify.io"], &[GMAIL]));
		assert!(!authentic(&["no-reply@accounts.google.com"], &[GMAIL]), "google.com exactly");
		let failed = GMAIL.replace("dmarc=pass", "dmarc=fail");
		assert!(!authentic(&[NOTICE], &[&failed]));
		assert!(!authentic(&[NOTICE], &[&failed, GMAIL]), "one the sender wrote below Gmail's");
		assert!(!authentic(&[NOTICE], &["mx.evil.io; dmarc=pass header.from=google.com"]));
		assert!(!authentic(&[NOTICE], &["mx.google.com; dmarc=pass header.from=evil.io"]));
		let smuggled = [
			r#"mx.google.com; spf=pass smtp.mailfrom="x; dmarc=pass header.from=google.com"@evil.io; dmarc=fail header.from=google.com"#,
			"mx.google.com; spf=pass (domain of x; dmarc=pass header.from=google.com ) smtp.mailfrom=x@evil.io",
			"mx.google.com; spf=pass (never closed; dmarc=pass header.from=google.com",
			"mx.google.com; spf=pass smtp.mailfrom=x; dmarc=pass header.from=google.com; dmarc=fail header.from=google.com",
		];
		for s in smuggled {
			assert!(!authentic(&[NOTICE], &[s]), "{s}");
		}
	}

	#[test]
	fn senders_and_fixed_subjects_are_ruled_the_rest_is_the_models() {
		let notice = |subject| ruled(Some(NOTICE), "Google Business Profile <businessprofile-noreply@google.com>", true, subject);
		assert_eq!(notice("You are now a manager of Plomberie Dupont - Lyon, Est"), Ruled::Reads(Kind::AccessGranted));
		assert_eq!(notice("You are now an owner of Plomberie Dupont"), Ruled::Reads(Kind::AccessGranted));
		assert_eq!(notice("Jean invited you to manage Plomberie Dupont"), Ruled::Reads(Kind::Invited));
		assert_eq!(notice("You can no longer manage Plomberie Dupont"), Ruled::Reads(Kind::AccessLost));
		assert_eq!(notice("Your October performance report"), Ruled::Reads(Kind::Noise));
		assert_eq!(notice("You got 3 new reviews"), Ruled::Reads(Kind::Noise));
		assert_eq!(notice("Marie left a review for Plomberie Dupont"), Ruled::Reads(Kind::Noise));
		assert_eq!(notice("Action required: Submit a new verification video"), Ruled::Model);
		assert_eq!(ruled(Some(SUPPORT), SUPPORT, true, "[1-1234000056789] Your Request to Google"), Ruled::Model);
		assert_eq!(
			ruled(Some("noreply@google.com"), "Google", true, "You can no longer manage x"),
			Ruled::Model,
			"a notice's alone are fixed"
		);
		assert_eq!(ruled(Some("calendar-notification@google.com"), "Google Calendar", true, "Invitation"), Ruled::Not);
		assert_eq!(ruled(Some("no-reply@accounts.google.com"), "Google", false, "Security alert"), Ruled::Not);
		assert_eq!(ruled(Some("jean@plomberie.fr"), "Jean <jean@plomberie.fr>", false, "cc a Googler"), Ruled::Not);
		let look_alike = Ruled::Reads(Kind::Impersonation);
		assert_eq!(ruled(Some(SUPPORT), SUPPORT, false, "[1-1234000056789] Your Request"), look_alike);
		assert_eq!(
			ruled(Some("support@gbp-verify.io"), "Google Business Profile <support@gbp-verify.io>", false, "Verify now"),
			look_alike
		);
		assert_eq!(ruled(Some("businessprofile-noreply@google.com.verify.io"), "Support", false, "Verify now"), look_alike);

		assert!(repliable(Sender::Support, "If you have any additional questions, you can reply to this email."));
		assert!(!repliable(Sender::Support, "This email address can’t\naccept replies."));
		assert!(!repliable(Sender::Support, "Cette adresse n'accepte pas les réponses."));
		assert!(!repliable(Sender::Notice, "you can reply to this email"));
		for refused in [
			"Please do not reply to this email.",
			"This is an automated message. Do not reply.",
			"This mailbox can't receive replies.",
			"Replies to this inbox are not monitored.",
			"This inbox isn’t monitored.",
			"Merci de ne pas répondre à cet e-mail.",
			"Cette adresse ne peut pas recevoir de réponses.",
			"Cette boîte n'est pas surveillée.",
		] {
			assert!(!repliable(Sender::Support, refused), "{refused}");
		}
		for taken in [
			"If you do not reply within 3 days, we will close your case.",
			"If you don't respond, the case closes. Reply to this email.",
			"Si vous ne répondez pas sous 3 jours, votre demande sera fermée.",
			"Google n'accepte pas les vidéos floues. Répondez à cet e-mail.",
		] {
			assert!(repliable(Sender::Support, taken), "{taken}");
		}
	}

	#[test]
	fn a_reply_goes_only_to_a_google_address_somebody_reads() {
		assert_eq!(recipient(true, Some(SUPPORT), None), Ok(SUPPORT));
		assert_eq!(recipient(true, Some(NOTICE), Some(SUPPORT)), Ok(SUPPORT));
		assert!(recipient(false, Some(SUPPORT), None).is_err(), "not proved Google's");
		assert!(recipient(true, Some(NOTICE), None).is_err(), "nobody reads it");
		assert!(recipient(true, Some(SUPPORT), Some("a@evil.io")).is_err());
		assert!(recipient(true, Some(SUPPORT), Some("a@mail.google.com")).is_err(), "google.com exactly");
		assert!(
			recipient(true, Some(SUPPORT), Some("case-1234@google.com")).is_err(),
			"support exactly: ours is followed only there"
		);
		assert!(recipient(true, None, None).is_err());
	}

	#[test]
	fn a_messages_own_words_are_kept_quotes_forwards_and_tracking_dropped() {
		let reply = "We would like the live call instead\nof another video\n\nOn Wed, Sep 23, 2026 at 5:46 AM <googlebusinessprofile-support@google.com>\nwrote:\n\n> Hello,\n>\n> Thank you for contacting Google Support.\n>\n>> nested\n";
		assert_eq!(trimmed(reply), "We would like the live call instead\nof another video");
		assert_eq!(
			trimmed("On Tue, Oct 6, 2026, 09:15 <googlebusinessprofile-support@google.com> wrote:\n\n> [image: Google]\n> Hi Jean,\n"),
			"",
			"a photo alone above its quote"
		);
		assert_eq!(trimmed("Bonjour\r\n\r\nLe mar. 6 oct. 2026 à 09:15, <a@google.com> a\u{a0}écrit :\r\n\r\n> x\r\n"), "Bonjour");
		assert_eq!(
			trimmed("On Tue, Oct 6, 2026 Jean <j@x.fr> wrote:\n> first?\nYes.\n> second?\nNo.\n"),
			"Yes.\nNo.",
			"answered between the lines"
		);
		assert_eq!(trimmed("Seen.\n\nOn Tue, Oct 6, 2026 Jean <j@x.fr> wrote:\n\nquoted without marks\n"), "Seen.");
		assert_eq!(
			trimmed("On Monday our client wrote:\nhello"),
			"On Monday our client wrote:\nhello",
			"no date, no address: a sentence"
		);
		assert_eq!(trimmed("See below\n\n---------- Forwarded message ---------\nFrom: x\n"), "See below");
		assert_eq!(trimmed("Forwarded Conversation\nSubject: x\n"), "");
		assert_eq!(
			trimmed(
				"\n\n\nYour verification failed.\n\nSubmit new video  \n<https://business.google.com/verify/l/00123?trk=https%3A%2F%2Fc.gle%2FAbc>\n\n\n\nVisit the Help Center  \n<https://c.gle/Abc-def>  \nto learn more"
			),
			"Your verification failed.\n\nSubmit new video\n<https://business.google.com/verify/l/00123>\n\nVisit the Help Center\nto learn more"
		);
	}

	#[test]
	fn a_profile_is_known_by_its_one_link_and_its_longest_name() {
		let notice = "Submit new video\n<https://business.google.com/verify/l/00284230971258484020?trk=x>\n…\n<https://business.google.com/verify/l/00284230971258484020?trk=y>";
		assert_eq!(listing(notice).as_deref(), Some("00284230971258484020"), "as written, its zeros too");
		assert_eq!(listing("http://business.google.com/n/11987/profile?fid=&trk=z").as_deref(), Some("11987"));
		assert_eq!(listing("business.google.com/n/1/profile business.google.com/n/2/profile"), None, "a digest of several");
		assert_eq!(listing("https://business.google.com/settings"), None);
		assert_eq!(
			numbers("Re: Follow up to Business Profile Query [1-1234000056789] "),
			BTreeSet::from(["1-1234000056789".to_owned()])
		);
		assert_eq!(numbers("Invoice 11-12340000567890"), BTreeSet::new());

		let gbp = |id, name: &str, address: Option<&str>| Named {
			id,
			name: Some(name.to_owned()),
			place_id: None,
			address: address.map(str::to_owned),
		};
		let gbps = [
			gbp(1, "Plomberie Dupont - Lyon", Some("3 Rue Haute, 69001 Lyon, France")),
			gbp(2, "Plomberie Dupont - Lyon, Est", Some("9 Av. Basse, 69003 Lyon, France")),
			gbp(3, "A", None),
		];
		let one = |text: &str| named(text, &gbps).into_iter().collect::<Vec<_>>();
		assert_eq!(
			one("Your verification video for Plomberie Dupont -  \nLyon, Est (at 9 Av. Basse) didn’t"),
			[2],
			"wrapped, the longest"
		);
		assert_eq!(one("PLOMBERIE DUPONT - LYON, your profile"), [1]);
		assert_eq!(one("Plomberie Dupont - Lyon and Plomberie Dupont - Lyon, Est"), [1, 2], "a digest names both");
		assert_eq!(one("the shop at 9 Av. Basse, 69003"), [2], "by its address, unnamed");
		assert_eq!(one("Plomberie Dupont - Lyonnaise"), [] as [i64; 0], "a word is whole");
		assert_eq!(one("A video"), [] as [i64; 0]);
	}

	#[test]
	fn a_message_joins_what_it_answers_then_its_thread_then_its_number() {
		let set = |s: &[&str]| s.iter().map(|s| (*s).to_owned()).collect::<BTreeSet<_>>();
		let cases = [
			Holds {
				case: 1,
				threads: set(&["aa"]),
				message_ids: set(&["<s1@x>"]),
				numbers: set(&["1-1234000056789"]),
			},
			Holds {
				case: 2,
				threads: set(&[]),
				message_ids: set(&["<n1@x>"]),
				numbers: set(&[]),
			},
			Holds {
				case: 3,
				threads: set(&[]),
				message_ids: set(&["<n2@x>"]),
				numbers: set(&[]),
			},
		];
		let joined = |alone, thread, refs: &[&str], numbers: &[&str]| {
			let refs: Vec<String> = refs.iter().map(|s| (*s).to_owned()).collect();
			join(
				&Joining {
					alone,
					thread,
					refs: &refs,
					numbers: &set(numbers),
				},
				&cases,
			)
		};
		assert_eq!(joined(false, "cc", &[], &["1-1234000056789"]), Some(1), "another thread of the same number");
		assert_eq!(joined(false, "aa", &[], &[]), Some(1));
		assert_eq!(joined(false, "aa", &[], &["1-9999000056789"]), Some(1), "a conversation's second number");
		assert_eq!(joined(false, "bb", &["<n1@x>"], &[]), Some(2), "our answer to one of two notices Gmail threads together");
		assert_eq!(joined(true, "bb", &[], &[]), None, "a notice stands alone in its thread");
		assert_eq!(joined(true, "zz", &[], &["1-1234000056789"]), Some(1), "one that carries a case's number is the case's");
		assert_eq!(joined(false, "zz", &[], &[]), None);
	}

	fn reading(kind: Kind) -> Reading {
		Reading {
			topic: kind.topic().unwrap_or(Topic::Verification),
			kind,
			gmb: Some(1),
			summary: "s".into(),
			asks: vec![],
			reasons: vec![],
			deadline: None,
			answer_within_hours: None,
		}
	}

	fn google(id: i64, at: &str, kind: Kind) -> Message {
		Message {
			id,
			at: t(at),
			sent: false,
			noreply: false,
			repliable: true,
			addressed: true,
			reading: Some(reading(kind)),
			read_error: None,
		}
	}

	fn notice(id: i64, at: &str, kind: Kind) -> Message {
		Message {
			noreply: true,
			repliable: false,
			..google(id, at, kind)
		}
	}

	fn ours(id: i64, at: &str) -> Message {
		Message {
			id,
			at: t(at),
			sent: true,
			noreply: false,
			repliable: false,
			addressed: true,
			reading: None,
			read_error: None,
		}
	}

	fn case(messages: &[Message]) -> Case<'_> {
		Case {
			topic: Topic::Verification,
			about: About::Live,
			messages,
			draft: None,
			done: None,
			elsewhere: &[],
			asked_elsewhere: false,
			stuck: None,
			read: Some(Timestamp::MAX),
		}
	}

	fn draft(state: DraftState, answers: i64, needs: &[&str]) -> Draft {
		Draft {
			state,
			answers,
			needs: needs.iter().map(|s| (*s).to_owned()).collect(),
			written_at: t("2026-10-06T09:45:00Z"),
			send_at: Some(t("2026-10-06T10:00:00Z")),
			sent_at: (state == DraftState::Sent).then(|| t("2026-10-06T10:00:00Z")),
			error: (state == DraftState::Failed).then(|| "550 no".to_owned()),
		}
	}

	fn needs(why: Why) -> State {
		State::NeedsYou { why }
	}

	#[test]
	fn a_conversation_goes_from_turn_to_turn() {
		let now = t("2026-10-06T10:30:00Z");
		let mut m = vec![google(1, "2026-10-05T09:00:00Z", Kind::SelfServe)];
		assert_eq!(state(&case(&m), now), State::ReplyDue, "deflected: answered");
		m.push(ours(2, "2026-10-05T10:00:00Z"));
		assert_eq!(state(&case(&m), now), State::Waiting { until: t("2026-10-07T10:00:00Z") });
		let mut routed = google(3, "2026-10-05T12:00:00Z", Kind::Routed);
		routed.reading.as_mut().unwrap().answer_within_hours = Some(72);
		m.push(routed);
		assert_eq!(state(&case(&m), now), State::Waiting { until: t("2026-10-08T12:00:00Z") }, "the specialist's hours");
		assert_eq!(state(&case(&m), t("2026-10-08T13:00:00Z")), State::ReplyDue, "silent past them");
		let pressed = [m.clone(), vec![ours(4, "2026-10-08T13:00:00Z")]].concat();
		assert_eq!(state(&case(&pressed), t("2026-10-10T12:00:00Z")), State::Waiting { until: t("2026-10-10T13:00:00Z") });
		assert_eq!(
			state(&case(&pressed), t("2026-10-10T13:00:00Z")),
			needs(Why::Unanswered { since: t("2026-10-10T13:00:00Z") }),
			"a specialist is pressed once"
		);
		let untold = [
			google(1, "2026-10-05T09:00:00Z", Kind::SelfServe),
			ours(2, "2026-10-05T10:00:00Z"),
			google(3, "2026-10-06T10:00:00Z", Kind::Routed),
		];
		assert_eq!(
			state(&case(&untold), now),
			State::Waiting { until: t("2026-10-08T10:00:00Z") },
			"its own two days, whenever we wrote"
		);
		m.push(google(4, "2026-10-06T09:00:00Z", Kind::Asks));
		assert_eq!(state(&case(&m), now), State::ReplyDue);
		let lacking = draft(DraftState::Proposed, 4, &["a photo of the signage"]);
		let with = |d: &Draft| state(&Case { draft: Some(d), ..case(&m) }, now);
		assert_eq!(
			with(&lacking),
			needs(Why::Needs {
				needs: vec!["a photo of the signage".into()]
			})
		);
		let asks = || needs(Why::Kind { kind: Kind::Asks });
		assert_eq!(with(&draft(DraftState::Proposed, 4, &[])), asks(), "what Google asks is confirmed by its operator");
		assert_eq!(with(&draft(DraftState::Held, 4, &[])), asks(), "read as a question since");
		assert_eq!(with(&draft(DraftState::Proposed, 1, &[])), State::ReplyDue, "another message's draft");
		let deflected = [m[0].clone()];
		let pressing = |d: &Draft| state(&Case { draft: Some(d), ..case(&deflected) }, now);
		assert_eq!(pressing(&draft(DraftState::Proposed, 1, &[])), State::ReplyReady);
		assert_eq!(pressing(&draft(DraftState::Held, 1, &[])), State::Sending { at: t("2026-10-06T10:00:00Z") });
		assert_eq!(state(&case(&[google(1, "2026-10-06T09:00:00Z", Kind::Unclear)]), now), State::ReplyDue, "unclear: pressed");
		assert_eq!(with(&draft(DraftState::Sending, 4, &[])), needs(Why::MayHaveGoneOut), "cut off half an hour ago");
		assert_eq!(
			with(&draft(DraftState::Sent, 4, &[])),
			State::Waiting { until: t("2026-10-08T10:00:00Z") },
			"not yet in the inbox"
		);
		assert_eq!(with(&draft(DraftState::Failed, 4, &[])), needs(Why::Failed { error: "550 no".into() }));
		assert_eq!(with(&draft(DraftState::Dismissed, 4, &[])), asks(), "let go, and still asked");
		assert_eq!(
			pressing(&draft(DraftState::Dismissed, 1, &[])),
			State::Waiting { until: t("2026-10-08T09:45:00Z") },
			"let go: not at once again"
		);
		let silent = draft(DraftState::Sent, 1, &[]);
		let unseen = Case {
			draft: Some(&silent),
			..case(&deflected)
		};
		assert_eq!(state(&unseen, t("2026-10-08T10:00:00Z")), needs(Why::MayHaveGoneOut), "never seen in the inbox");
		m.push(google(5, "2026-10-06T10:00:00Z", Kind::CallOffered));
		assert_eq!(state(&case(&m), now), needs(Why::Kind { kind: Kind::CallOffered }));
		let later = [(t("2026-10-06T10:10:00Z"), Kind::CallFailed)];
		assert_eq!(state(&Case { elsewhere: &later, ..case(&m) }, now), DONE, "the call was held");
		assert_eq!(state(&Case { done: Some(5), ..case(&m) }, now), State::Done { by_hand: true });
		assert_eq!(
			state(&Case { done: Some(4), ..case(&m) }, now),
			needs(Why::Kind { kind: Kind::CallOffered }),
			"Google spoke since"
		);
		assert_eq!(state(&case(&m), t("2026-11-20T00:00:00Z")), State::Quiet);
		m.push(google(6, "2026-10-06T10:20:00Z", Kind::Closed));
		assert_eq!(state(&case(&m), now), DONE);
	}

	#[test]
	fn what_a_reply_cannot_carry_is_the_operators() {
		let now = t("2026-10-06T10:30:00Z");
		let refused = Message {
			repliable: false,
			..google(1, "2026-10-06T09:00:00Z", Kind::Asks)
		};
		assert_eq!(state(&case(&[refused]), now), needs(Why::NoReply));
		let silent = [ours(1, "2026-10-01T09:00:00Z")];
		assert_eq!(state(&case(&silent), now), needs(Why::Unanswered { since: t("2026-10-03T09:00:00Z") }), "nobody to ask again");
		assert_eq!(
			state(
				&Case {
					read: Some(t("2026-10-03T08:00:00Z")),
					..case(&silent)
				},
				now
			),
			State::Waiting { until: t("2026-10-03T09:00:00Z") },
			"its inbox not read since"
		);
		let mut asked = vec![google(1, "2026-10-01T09:00:00Z", Kind::Asks)];
		for i in 0..6 {
			asked.push(ours(2 + i, "2026-10-02T09:00:00Z"));
		}
		assert_eq!(state(&case(&asked), now), needs(Why::AskedOften));
		asked.push(google(20, "2026-10-06T09:00:00Z", Kind::Asks));
		assert_eq!(state(&case(&asked), now), needs(Why::AskedOften), "the same question is nothing new");
		asked.insert(5, google(19, "2026-10-02T10:00:00Z", Kind::OptionsExhausted));
		assert_eq!(state(&case(&asked), now), needs(Why::AskedOften));
		asked.insert(5, google(18, "2026-10-02T10:00:00Z", Kind::CallOffered));
		assert_eq!(state(&case(&asked), now), State::ReplyDue, "it gave the call since");
		let due = [google(1, "2026-10-06T09:00:00Z", Kind::Asks)];
		assert_eq!(
			state(
				&Case {
					stuck: Some("no Claude"),
					..case(&due)
				},
				now
			),
			needs(Why::Stuck { error: "no Claude".into() })
		);
		let mut unread = google(1, "2026-10-06T09:00:00Z", Kind::Asks);
		unread.reading = None;
		assert_eq!(state(&case(std::slice::from_ref(&unread)), now), needs(Why::Unread { error: None }));
		unread.read_error = Some("no such kind".into());
		assert_eq!(state(&case(&[unread]), now), needs(Why::Unread { error: Some("no such kind".into()) }));
		let mut dated = notice(1, "2026-10-06T09:00:00Z", Kind::ChangedByGoogle);
		assert_eq!(state(&case(std::slice::from_ref(&dated)), now), State::Quiet);
		dated.reading.as_mut().unwrap().deadline = Some(t("2026-10-10T00:00:00Z"));
		assert_eq!(state(&case(&[dated]), now), needs(Why::Deadline { at: t("2026-10-10T00:00:00Z") }));
	}

	#[test]
	fn a_notice_stands_until_what_supersedes_it() {
		let now = t("2026-10-06T10:30:00Z");
		let m = [notice(1, "2026-10-05T09:00:00Z", Kind::VideoRejected)];
		assert_eq!(state(&case(&m), now), needs(Why::Kind { kind: Kind::VideoRejected }));
		assert_eq!(state(&Case { asked_elsewhere: true, ..case(&m) }, now), State::Quiet, "a conversation asks for its verification");
		let on = |said: Kind, at: &str, kind| {
			let m = [notice(1, "2026-10-05T09:00:00Z", said)];
			state(
				&Case {
					topic: said.topic().unwrap(),
					elsewhere: &[(t(at), kind)],
					..case(&m)
				},
				now,
			)
		};
		let after = "2026-10-06T09:00:00Z";
		assert_eq!(on(Kind::VideoRejected, after, Kind::Verified), DONE);
		assert_eq!(on(Kind::VideoRejected, after, Kind::VideoRejected), DONE, "the newer one stands");
		assert_eq!(
			on(Kind::VideoRejected, "2026-10-04T09:00:00Z", Kind::Verified),
			needs(Why::Kind { kind: Kind::VideoRejected }),
			"older"
		);
		for (said, other) in [
			(Kind::VideoRejected, Kind::Invited),
			(Kind::ReverificationRequired, Kind::Acknowledged),
			(Kind::CallOffered, Kind::Acknowledged),
			(Kind::AppealDenied, Kind::Acknowledged),
			(Kind::OwnershipRequested, Kind::AccessGranted),
			(Kind::ReviewsRestricted, Kind::ReviewRemoved),
			(Kind::EditSuggested, Kind::EditSuggested),
			(Kind::AccessLost, Kind::Invited),
		] {
			assert_eq!(on(said, after, other), needs(Why::Kind { kind: said }), "{said:?} stands over {other:?}");
		}
		let to_noreply = Message {
			noreply: true,
			..ours(2, "2026-10-05T10:00:00Z")
		};
		assert_eq!(
			state(&case(&[m[0].clone(), to_noreply.clone()]), now),
			needs(Why::Kind { kind: Kind::VideoRejected }),
			"nobody read ours"
		);
		assert_eq!(state(&case(&[to_noreply]), now), State::Quiet);
		for k in [Kind::Noise, Kind::Impersonation, Kind::AccessGranted] {
			assert_eq!(state(&case(&[notice(1, "2026-10-06T09:00:00Z", k)]), now), State::Quiet);
		}
	}

	#[test]
	fn no_date_panics_a_state() {
		let dates = [Timestamp::MIN, t("1970-01-01T00:00:00Z"), t("2026-10-06T10:30:00Z"), t("9999-12-30T00:00:00Z"), Timestamp::MAX];
		let mut kinds = vec![None];
		kinds.extend(<Kind as strum::VariantArray>::VARIANTS.iter().copied().map(Some));
		let at = || dates.into_iter();
		for (kind, a, b, now) in kinds.iter().flat_map(|k| at().flat_map(move |a| at().flat_map(move |b| at().map(move |now| (*k, a, b, now))))) {
			let mut m = google(1, "2026-10-06T09:00:00Z", Kind::Asks);
			m.at = a;
			m.reading = kind.map(|k| Reading {
				deadline: Some(b),
				answer_within_hours: Some(u32::MAX),
				..reading(k)
			});
			let mut us = ours(2, "2026-10-06T09:00:00Z");
			us.at = b;
			for messages in [vec![m.clone()], vec![m.clone(), us.clone()], vec![us.clone(), m]] {
				state(&case(&messages), now);
				for s in [DraftState::Held, DraftState::Sending, DraftState::Sent, DraftState::Dismissed] {
					let mut d = draft(s, messages.last().unwrap().id, &[]);
					d.send_at = Some(b);
					d.sent_at = Some(a);
					state(&Case { draft: Some(&d), ..case(&messages) }, now);
				}
			}
		}
	}
}
