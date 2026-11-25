//! Google's mail about Business Profiles (docs/ARCHITECTURE.md, Google's mail): who sends it, what rules
//! alone make of a message, what it names, which case it joins, and a case's state.

use std::{collections::BTreeSet, sync::LazyLock};

use jiff::{SignedDuration, Timestamp};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Ours in a case since Google last gave something new; past it a person decides.
pub const MAX_ASKS: usize = 6;
/// A draft that goes by itself waits this long.
pub const HOLD: SignedDuration = SignedDuration::from_mins(15);
/// Older mail is kept, not read by the model.
pub const READ_BACK: SignedDuration = SignedDuration::from_hours(24 * 90);
/// A reading failed this often is a person's.
pub const MAX_READS: i64 = 3;
/// Google is asked again after this long without an answer, if it gave no other term.
pub const RE_ASK: SignedDuration = SignedDuration::from_hours(48);
/// A video or an appeal is decided on within it, if Google gave no other term.
const IN_REVIEW: SignedDuration = SignedDuration::from_hours(24 * 7);
/// What goes by itself waits this long after ours before it.
const APART: SignedDuration = SignedDuration::from_hours(12);
/// An inbox not read for this long may hold Google's answer.
const STALE: SignedDuration = SignedDuration::from_hours(1);
/// A case whose last message is older needs nothing.
const QUIET_AFTER: SignedDuration = SignedDuration::from_hours(24 * 21);
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
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, strum::EnumMessage, strum::EnumString, strum::IntoStaticStr, schemars::JsonSchema, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
#[schemars(rename = "MailKind")]
pub enum Kind {
	/// The recorded verification video did not pass.
	VideoRejected,
	/// The recorded verification video is being reviewed.
	VideoInReview,
	/// A code by phone, text, email or postcard did not verify.
	CodeFailed,
	/// The profile is verified.
	Verified,
	/// The profile has to be verified again.
	ReverificationRequired,
	/// Support points back at the recorded video or the on-screen prompts, and takes nothing by reply.
	SelfServe,
	/// The live verification call is enabled.
	CallOffered,
	/// A live verification call was held and did not verify.
	CallFailed,
	/// No way to verify is left on the profile.
	OptionsExhausted,
	/// The profile is suspended or disabled: off Maps, or out of our control.
	Suspended,
	/// The account, not one profile, is restricted.
	AccountRestricted,
	/// An appeal of a suspension or a restriction was received, or is being reviewed.
	AppealReceived,
	/// The appeal was granted: the profile is reinstated.
	AppealApproved,
	/// The appeal was refused.
	AppealDenied,
	/// The appeal goes on only once the profile is verified further.
	AppealVerification,
	/// New reviews are paused: a spike of spam.
	ReviewsPaused,
	/// Fake or incentivised reviews were found, the profile's reviews restricted.
	ReviewsRestricted,
	/// New reviews are taken again, or the profile's restriction is over.
	RestrictionLifted,
	/// A review was removed.
	ReviewRemoved,
	/// Support will not bring the reviews back.
	ReinstatementRefused,
	/// Support brought the reviews back.
	ReinstatementDone,
	/// Support brought part of the reviews back, or those only that it can.
	ReinstatementPartial,
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
	/// The profile breaks a policy, and is restricted or removed unless mended.
	PolicyWarning,
	/// The profile, not a request, is another's duplicate.
	Duplicate,
	/// A request was received, nothing more.
	Acknowledged,
	/// Handed to a specialist or another team, who answers.
	Routed,
	/// Asks for documents, photos, changes or answers before going on: which profile, which reviews.
	Asks,
	/// The case is closed now, whatever was settled: one closed later unless answered asks.
	Closed,
	/// The request, not the profile, is a duplicate: another one open carries it.
	DuplicateTicket,
	/// About a profile, what it says cannot be told.
	Unclear,
	/// Performance, promotions, a survey alone, new reviews, posts.
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
			Self::VideoRejected
			| Self::VideoInReview
			| Self::CodeFailed
			| Self::Verified
			| Self::ReverificationRequired
			| Self::SelfServe
			| Self::CallOffered
			| Self::CallFailed
			| Self::OptionsExhausted => Topic::Verification,
			Self::Suspended | Self::AccountRestricted | Self::AppealReceived | Self::AppealApproved | Self::AppealDenied | Self::AppealVerification => Topic::Suspension,
			Self::ReviewsPaused
			| Self::ReviewsRestricted
			| Self::RestrictionLifted
			| Self::ReviewRemoved
			| Self::ReinstatementRefused
			| Self::ReinstatementDone
			| Self::ReinstatementPartial
			| Self::ReviewsLiveAlready
			| Self::ReviewsNoData => Topic::Reviews,
			Self::StillActive => Topic::Activity,
			Self::Invited | Self::AccessGranted | Self::AccessLost | Self::OthersAccess | Self::OwnershipRequested | Self::ManagementRequested => Topic::Access,
			Self::EditSuggested | Self::EditRejected | Self::ChangedByGoogle | Self::ContentRemoved | Self::PolicyWarning | Self::Duplicate => Topic::Profile,
			Self::Noise | Self::NotBusiness | Self::Impersonation => Topic::Other,
			Self::Acknowledged | Self::Routed | Self::Asks | Self::Closed | Self::DuplicateTicket | Self::Unclear => return None,
		})
	}

	/// Whose turn it leaves.
	fn turn(self) -> Turn {
		match self {
			Self::SelfServe | Self::OptionsExhausted | Self::ReinstatementRefused | Self::ReviewsNoData | Self::ReviewsLiveAlready | Self::Asks | Self::Unclear => Turn::Reply,
			// partly back: what came back is seen within its term
			Self::Acknowledged | Self::Routed | Self::VideoInReview | Self::AppealReceived | Self::ReinstatementPartial => Turn::Google,
			Self::Verified | Self::AppealApproved | Self::RestrictionLifted | Self::ReinstatementDone => Turn::Settled,
			Self::Closed | Self::DuplicateTicket => Turn::Closed,
			Self::ReviewRemoved
			| Self::Invited
			| Self::AccessGranted
			| Self::EditRejected
			| Self::ChangedByGoogle
			| Self::ContentRemoved
			| Self::Noise
			| Self::NotBusiness
			| Self::Impersonation => Turn::Nobody,
			Self::VideoRejected
			| Self::CodeFailed
			| Self::ReverificationRequired
			| Self::CallOffered
			| Self::CallFailed
			| Self::Suspended
			| Self::AccountRestricted
			| Self::AppealDenied
			| Self::AppealVerification
			| Self::ReviewsPaused
			| Self::ReviewsRestricted
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
		self.turn() == Turn::Settled && self.topic() == Some(topic)
	}

	/// Whether what it says of a GBP stands no more once `later` is said of it.
	fn superseded_by(self, later: Self) -> bool {
		let by: &[Self] = match self {
			Self::VideoRejected | Self::VideoInReview | Self::CodeFailed | Self::ReverificationRequired =>
				&[Self::VideoRejected, Self::VideoInReview, Self::CodeFailed, Self::CallOffered, Self::Verified],
			Self::CallOffered | Self::CallFailed => &[Self::CallOffered, Self::CallFailed, Self::Verified],
			Self::Suspended | Self::AppealReceived | Self::AppealDenied | Self::AppealVerification =>
				&[Self::AppealReceived, Self::AppealApproved, Self::AppealDenied, Self::AppealVerification],
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
	#[serde(deserialize_with = "moment")]
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
			a if (a.starts_with("googlemybusiness-") || a.starts_with("google-my-business-")) && a.ends_with("@google.com") => Self::Noreply,
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

/// Whether a message is Google's: its one From is of google.com or a subdomain, and the first
/// `Authentication-Results` Gmail itself wrote (`results`, topmost first) passes DMARC for that domain.
pub fn authentic(from: &[&str], results: &[&str]) -> bool {
	let [from] = from else { return false };
	let Some(domain) = domain(from).filter(|d| d == "google.com" || d.ends_with(".google.com")) else {
		return false;
	};
	let gmails = results.iter().find_map(|r| {
		let bare = bare(r)?;
		(bare.split(';').next()?.split_whitespace().next()? == "mx.google.com").then_some(bare)
	});
	let Some(gmails) = gmails else { return false };
	let mut dmarc = gmails.split(';').skip(1).map(str::trim).filter(|s| s.starts_with("dmarc="));
	let (Some(one), None) = (dmarc.next(), dmarc.next()) else { return false };
	let mut words = one.split_whitespace();
	words.next() == Some("dmarc=pass") && words.any(|w| w.strip_prefix("header.from=") == Some(&domain))
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

/// `from`: its address; `shown`: its sender as a mail client shows it; `answers`: a message of one of its gmail's cases.
pub fn ruled(from: Option<&str>, shown: &str, authentic: bool, subject: &str, answers: bool) -> Ruled {
	let Some(from) = from else { return Ruled::Not };
	match domain(from).as_deref() {
		Some(d) if authentic && answers && (d == "google.com" || d.ends_with(".google.com")) && Sender::of(from) == Sender::Other => Ruled::Model,
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
	sender == Sender::Support && !REFUSES.is_match(&squeezed(text))
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
#[derive(Clone, Debug, PartialEq)]
pub struct Named {
	pub id: i64,
	/// Its name now, then those it had.
	pub names: Vec<String>,
	pub place_id: Option<String>,
	pub address: Option<String>,
}

/// The GBPs `text` names, by name or place id, else by their address's first line. A longer name takes
/// its words first, and one whose words go on in capitals is another's, known here or not: one GBP's name
/// may begin another's.
pub fn named(text: &str, gbps: &[Named]) -> BTreeSet<i64> {
	let Words { text, capitals } = Words::of(text);
	let goes_on = |end: usize| {
		let rest = &text[end..];
		let next = rest.trim_start_matches([' ', ',', '-']);
		next.len() < rest.len() && capitals.contains(&(text.len() - next.len()))
	};
	// a name's rank: the one it has now before one it had
	let by = |marks: Vec<(i64, usize, String)>, whole_name: bool| {
		let mut marks: Vec<(i64, usize, String)> = marks
			.into_iter()
			.map(|(id, rank, m)| (id, rank, squeezed(&m)))
			.filter(|(.., m)| m.chars().count() >= NAMES_FROM)
			.collect();
		marks.sort_by_key(|(_, rank, m)| (std::cmp::Reverse(m.len()), *rank));
		let mut left = text.clone();
		let mut found = BTreeSet::new();
		for (id, _, mark) in marks {
			let at: Vec<usize> = left.match_indices(&mark).map(|(i, _)| i).filter(|&i| whole(&left, i, mark.len())).collect();
			for i in at {
				if !(whole_name && goes_on(i + mark.len())) {
					found.insert(id);
				}
				left.replace_range(i..i + mark.len(), &"\0".repeat(mark.len()));
			}
		}
		found
	};
	let names = by(gbps.iter().flat_map(|g| g.names.iter().enumerate().map(|(rank, n)| (g.id, rank, n.clone()))).collect(), true);
	let places = by(gbps.iter().filter_map(|g| Some((g.id, 0, g.place_id.clone()?))).collect(), false);
	let named: BTreeSet<i64> = names.union(&places).copied().collect();
	if !named.is_empty() {
		return named;
	}
	by(
		gbps.iter()
			.filter_map(|g| Some((g.id, 0, g.address.as_deref()?.split(',').next().expect("split yields one").to_owned())))
			.collect(),
		false,
	)
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
	/// Not sent: Gmail refused its login.
	pub login: bool,
}

/// The GBP a case is about.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum About {
	Unknown,
	/// A live one, as its reader saw it or its conversation tells two: shown as its, never answered by itself.
	Likely,
	Live,
	Removed,
}

/// What is said of a GBP.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[schemars(rename = "MailSaid")]
pub struct Said {
	pub kind: Kind,
	pub at: Timestamp,
	/// Whose reading it is; `null`: the GBP's move.
	pub case_id: Option<i64>,
}

/// What settled a case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "is", rename_all = "snake_case")]
#[schemars(rename = "MailSettled")]
pub enum Settled {
	/// A person, by hand.
	Hand,
	/// What Google said in it.
	Here,
	/// What is said of its GBP elsewhere.
	Elsewhere(Said),
}

/// Why nothing is to do in a case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "is", rename_all = "snake_case")]
#[schemars(rename = "MailQuietBecause")]
pub enum QuietBecause {
	/// What was said asks for nothing.
	Nothing,
	/// Nothing was said in it for long.
	Old,
	/// Its GBP is in the trash.
	Removed,
	/// Its GBP's latest conversation on the topic, that case, carries it.
	Carried { case_id: i64 },
}

/// A person's word over a case's mail, until Google speaks in it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mark {
	Done,
	/// Open, whatever settled it: nothing in it is written or sent unasked.
	Open,
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
	/// A person's, and the case's last message then.
	pub mark: Option<(Mark, i64)>,
	/// What its GBP's other cases and its moves say of it.
	pub elsewhere: &'a [Said],
	/// Another conversation, open, its latest of that topic, which carries what is asked of Google for its GBP.
	pub carried_by: Option<i64>,
	/// Its GBP's replies on its topic go by themselves.
	pub switched: bool,
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
		by: Settled,
	},
	/// Nothing to do.
	Quiet {
		because: QuietBecause,
	},
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
	/// Google answers nothing new, asked that often.
	AskedOften { asked: usize },
	/// Google is silent, and nothing asks it again by itself: nothing here takes a reply, its specialist was asked again
	/// already, or its person reopened the case.
	Unanswered { since: Timestamp },
	/// Its person reopened it, and nothing was said to Google since.
	Reopened,
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
	/// The reply was not sent: Gmail refused its gmail's login.
	Login { error: String },
}

pub fn state(c: &Case<'_>, now: Timestamp) -> State {
	let heard = heard(c);
	let quiet = |because| State::Quiet { because };
	let Some(last) = heard.last() else { return quiet(QuietBecause::Nothing) };
	let mark = mark(c);
	if let Some((Mark::Done, _)) = mark {
		return State::Done { by: Settled::Hand };
	}
	let open = mark.is_some();
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
		return quiet(QuietBecause::Removed);
	}
	let sent_at = draft.filter(|d| d.state == DraftState::Sent).map(|d| d.sent_at.expect("a sent draft went out at a moment"));
	let word = word(c, &heard);
	let deadline = word.and_then(|w| w.deadline).filter(|at| now < *at);
	if !open && deadline.is_none() && now.duration_since(sent_at.unwrap_or(last.at)) > QUIET_AFTER {
		return quiet(QuietBecause::Old);
	}
	if !open && let Some(by) = over(c, &heard) {
		return State::Done { by };
	}
	let unread = !last.sent && last.reading.is_none();
	let carried = word.is_none_or(|w| match w.turn {
		Turn::Reply => w.kind != Kind::Asks,
		Turn::Google | Turn::Closed => true,
		Turn::Operator => matches!(w.kind, Kind::VideoRejected | Kind::CodeFailed | Kind::CallFailed),
		Turn::Settled | Turn::Nobody => false,
	});
	if let Some(case_id) = c.carried_by.filter(|_| !open && !unread && carried) {
		return quiet(QuietBecause::Carried { case_id });
	}
	// what Google asks is the operator's to give or to confirm
	let asks = last.reading.as_ref().is_some_and(|r| r.kind == Kind::Asks);
	match draft {
		Some(Draft {
			state: DraftState::Held,
			send_at,
			needs: lacks,
			..
		}) if lacks.is_empty() && press(c, now).is_ok() => {
			return State::Sending {
				at: send_at.expect("a held draft goes at a moment"),
			};
		}
		Some(Draft {
			state: DraftState::Proposed | DraftState::Held,
			needs: lacks,
			..
		}) =>
			return match (lacks.is_empty(), asks, asked(c.messages)) {
				(false, ..) => needs(Why::Needs { needs: lacks.clone() }),
				(true, true, _) => needs(Why::Kind { kind: Kind::Asks }),
				(true, false, asked) if asked >= MAX_ASKS => needs(Why::AskedOften { asked }),
				(true, false, _) => State::ReplyReady,
			},
		Some(Draft {
			state: DraftState::Failed,
			error,
			login,
			..
		}) => {
			let error = error.clone().expect("a failed draft says why");
			return needs(if *login { Why::Login { error } } else { Why::Failed { error } });
		}
		// nothing of what came back is missing
		Some(Draft {
			state: DraftState::Dismissed,
			error: Some(_),
			..
		}) if !open && word.is_some_and(|w| matches!(w.kind, Kind::ReinstatementPartial | Kind::ReviewsLiveAlready)) => return State::Done { by: Settled::Here },
		Some(Draft {
			state: DraftState::Dismissed,
			error: Some(why),
			..
		}) => return needs(Why::NothingToSay { why: why.clone() }),
		Some(Draft { state: DraftState::Dismissed, .. }) if !open && asks => return needs(Why::Kind { kind: Kind::Asks }),
		Some(Draft {
			state: DraftState::Dismissed,
			written_at,
			..
		}) if !open && now < later(*written_at, RE_ASK) => return State::Waiting { until: later(*written_at, RE_ASK) },
		_ => {}
	}
	if mark.is_some_and(|(_, ours)| !ours) && sent_at.is_none() {
		return needs(Why::Reopened);
	}
	let silence = || {
		let until = term(&heard, sent_at);
		// an inbox not read since may hold the answer
		if now < until || c.read.is_none_or(|read| read < until) {
			return State::Waiting { until };
		}
		match word {
			Some(Word {
				kind,
				turn: Turn::Operator | Turn::Closed,
				..
			}) => needs(Why::Kind { kind }),
			_ if sent_at.is_some() => needs(Why::MayHaveGoneOut),
			Some(Word { kind: Kind::Routed, .. }) if last.sent => needs(Why::Unanswered { since: until }),
			_ => reply(c, Some(until)),
		}
	};
	if last.sent || sent_at.is_some() {
		return match word {
			Some(w) if !open && w.kind.resolves(c.topic) => State::Done { by: Settled::Here },
			Some(w) if w.at > sent_at.unwrap_or(last.at) => reply(c, None),
			_ => silence(),
		};
	}
	let Some(w) = word.filter(|_| !unread) else {
		return needs(match (&last.read_error, c.stuck) {
			(None, Some(error)) => Why::Stuck { error: error.to_owned() },
			(error, _) => Why::Unread { error: error.clone() },
		});
	};
	let or_deadline = |otherwise| deadline.map_or(otherwise, |at| needs(Why::Deadline { at }));
	match w.turn {
		Turn::Reply => reply(c, None),
		Turn::Google => or_deadline(silence()),
		Turn::Nobody => or_deadline(quiet(QuietBecause::Nothing)),
		Turn::Settled if w.kind.resolves(c.topic) => State::Done { by: Settled::Here },
		// resolved, and not what the case is about
		Turn::Settled | Turn::Operator => needs(Why::Kind { kind: w.kind }),
		Turn::Closed => or_deadline(match before(&heard) {
			Some(k) if k.resolves(c.topic) => State::Done { by: Settled::Here },
			Some(k) if k.turn() == Turn::Operator => needs(Why::Kind { kind: k }),
			_ => needs(Why::Kind { kind: w.kind }),
		}),
	}
}

/// Whether a reply is to be written: one is due, or past `MAX_ASKS` there for its operator.
pub fn due(c: &Case<'_>, now: Timestamp) -> bool {
	let last = heard(c).last().map(|m| m.id);
	let written = c.draft.is_some_and(|d| Some(d.answers) == last && d.state != DraftState::Superseded);
	match state(&Case { stuck: None, ..c.clone() }, now) {
		State::ReplyDue => true,
		State::NeedsYou { why: Why::AskedOften { .. } } => !written,
		_ => false,
	}
}

/// Why nothing goes to Google by itself in a case.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(tag = "is", rename_all = "snake_case")]
#[schemars(rename = "MailHalt")]
pub enum Halt {
	/// Its GBP's replies on its topic are not switched on.
	Off,
	/// Which GBP it is about is not certain.
	Uncertain,
	/// Its GBP is in the trash.
	Removed,
	/// Google's last word is not read.
	Unread,
	/// Nothing in it takes a reply.
	NoReply,
	/// Its GBP's latest conversation on the topic carries it.
	Elsewhere,
	/// What it asks for is given.
	Settled,
	/// Its person reopened it.
	Reopened,
	/// What Google asks is its operator's to give.
	Asked,
	/// Google was asked that often and gave nothing new.
	Limit { asked: usize },
	/// Google's turn, until then.
	Google { until: Timestamp },
	/// What Google said is its operator's to act on.
	Operators,
	/// Its inbox is two accounts', or it was written to another.
	Account,
	/// Its inbox was not read for an hour.
	Stale,
}

/// From when a reply lacking nothing may go to Google by itself.
pub fn press(c: &Case<'_>, now: Timestamp) -> Result<Timestamp, Halt> {
	if let Some((Mark::Open, _)) = mark(c) {
		return Err(Halt::Reopened);
	}
	if !c.switched {
		return Err(Halt::Off);
	}
	match c.about {
		About::Unknown | About::Likely => return Err(Halt::Uncertain),
		About::Removed => return Err(Halt::Removed),
		About::Live => {}
	}
	let heard = heard(c);
	let (w, r) = word(c, &heard).zip(said(&heard)).ok_or(Halt::Unread)?;
	let to = counterpart(c.messages.iter()).ok_or(Halt::NoReply)?;
	if c.carried_by.is_some() {
		return Err(Halt::Elsewhere);
	}
	if settled(c).is_some() || over(c, &heard).is_some() {
		return Err(Halt::Settled);
	}
	if !r.asks.is_empty() || w.kind == Kind::Asks {
		return Err(Halt::Asked);
	}
	let asked = asked(c.messages);
	if asked >= MAX_ASKS {
		return Err(Halt::Limit { asked });
	}
	let until = term(&heard, None);
	match w.kind {
		_ if w.kind.topic().is_some_and(|t| t != c.topic) => return Err(Halt::Operators),
		Kind::SelfServe | Kind::OptionsExhausted | Kind::ReinstatementRefused | Kind::ReviewsNoData | Kind::ReviewsLiveAlready | Kind::CallFailed => {}
		_ if w.turn != Turn::Google => return Err(Halt::Operators),
		_ if now < until => return Err(Halt::Google { until }),
		_ => {}
	}
	if !(to.addressed || heard.iter().any(|m| m.sent && m.addressed)) {
		return Err(Halt::Account);
	}
	if c.read.is_some_and(|read| now.duration_since(read) > STALE) {
		return Err(Halt::Stale);
	}
	Ok(heard.iter().rev().find(|m| m.sent).map_or(to.at, |m| later(m.at, APART)))
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
	/// By phone, text, email or postcard.
	Code,
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

/// What Google is asked for by mail.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize, schemars::JsonSchema, strum::VariantArray)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "MailGoal")]
pub enum Goal {
	/// The live verification call.
	Call,
	/// The missing reviews.
	Reviews,
}
impl Goal {
	pub fn topic(self) -> Topic {
		match self {
			Self::Call => Topic::Verification,
			Self::Reviews => Topic::Reviews,
		}
	}

	/// Whether `kind` gives it.
	pub fn given_by(self, kind: Kind) -> bool {
		match self {
			Self::Call => kind == Kind::CallOffered,
			Self::Reviews => matches!(kind, Kind::ReinstatementDone | Kind::ReinstatementPartial),
		}
	}

	/// Whether nothing is left to ask once `kind` is said.
	fn over_by(self, kind: Kind) -> bool {
		self.given_by(kind) || kind == Kind::Verified && self == Self::Call
	}
}

/// Said of a GBP on a goal's topic.
#[derive(Clone, Copy, Debug)]
pub struct Spoken {
	pub at: Timestamp,
	/// Its message; `None`: the GBP's move.
	pub message: Option<i64>,
	pub case: Option<i64>,
	/// `None`: ours, to support.
	pub said: Option<Kind>,
}

/// A goal asked for, until it is over.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Pursuit {
	pub goal: Goal,
	/// Our messages, each with when it went and Google's last word before the next of ours.
	pub asks: Vec<(i64, Timestamp, Option<Kind>)>,
	/// By what, and when.
	pub over: Option<(Timestamp, Kind)>,
}

/// What was asked of Google for one GBP and `goal`, `spoken` being all said of it, oldest first.
pub fn pursuits(goal: Goal, spoken: &[Spoken]) -> Vec<Pursuit> {
	let mut done: Vec<Pursuit> = Vec::new();
	let mut open: Option<(Pursuit, Option<i64>)> = None;
	for s in spoken {
		match (s.said, s.message) {
			(None, Some(message)) => {
				let (p, case) = open.get_or_insert_with(|| {
					let p = Pursuit { goal, asks: Vec::new(), over: None };
					(p, s.case)
				});
				p.asks.push((message, s.at, None));
				*case = s.case;
			}
			(None, None) => {}
			(Some(kind), _) => {
				let Some((p, case)) = &mut open else { continue };
				let over = goal.over_by(kind);
				let answers = s.case == *case && !matches!(kind, Kind::Acknowledged | Kind::Noise | Kind::NotBusiness);
				if goal.given_by(kind) || answers {
					p.asks.last_mut().expect("opened by an ask").2 = Some(kind);
				}
				if over {
					p.over = Some((s.at, kind));
					done.extend(open.take().map(|(p, _)| p));
				}
			}
		}
	}
	done.extend(open.map(|(p, _)| p));
	done
}

/// What a reading says of a try to verify its GBP.
pub fn attempt(kind: Kind) -> Option<(Method, Outcome)> {
	Some(match kind {
		Kind::VideoInReview => (Method::Video, Outcome::InReview),
		Kind::VideoRejected => (Method::Video, Outcome::Rejected),
		Kind::CodeFailed => (Method::Code, Outcome::Failed),
		Kind::CallOffered => (Method::Call, Outcome::Offered),
		Kind::CallFailed => (Method::Call, Outcome::Failed),
		Kind::Verified => (Method::Unknown, Outcome::Verified),
		_ => return None,
	})
}

/// Ours since Google last gave something new.
pub fn asked(messages: &[Message]) -> usize {
	let new = |k: Kind| matches!(k.turn(), Turn::Operator | Turn::Settled) || matches!(k, Kind::Asks | Kind::ReinstatementPartial);
	messages
		.iter()
		.rev()
		.filter(|m| m.heard())
		.take_while(|m| m.sent || !m.reading.as_ref().is_some_and(|r| new(r.kind)))
		.filter(|m| m.sent)
		.count()
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Turn {
	Reply,
	Google,
	Operator,
	/// What a case asked for is given.
	Settled,
	/// Its conversation is over, whatever it came to.
	Closed,
	Nobody,
}

/// Google's last word of a case, as it stands.
#[derive(Clone, Copy)]
struct Word {
	kind: Kind,
	at: Timestamp,
	turn: Turn,
	deadline: Option<Timestamp>,
}

/// A moment as a mail tells it: UTC where it names no zone, a day's end where it names the day alone.
fn moment<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Timestamp>, D::Error> {
	let Some(said) = Option::<String>::deserialize(d)? else { return Ok(None) };
	let at = said.parse::<Timestamp>().or_else(|_| {
		let on = match said.parse::<jiff::civil::Date>() {
			Ok(day) if !said.contains(':') => day.at(23, 59, 59, 0),
			_ => said.parse::<jiff::civil::DateTime>()?,
		};
		on.to_zoned(jiff::tz::TimeZone::UTC).map(|z| z.timestamp())
	});
	at.map(Some).map_err(serde::de::Error::custom)
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

/// Lowercase, apostrophes and dashes plain, runs of whitespace one space: Google sets and wraps names its own way.
fn squeezed(s: &str) -> String {
	let plain = plain(s).to_lowercase();
	plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn plain(s: &str) -> String {
	s.replace(['’', 'ʼ', '‘'], "'").replace(['–', '—', '‑', '‐'], "-")
}

/// A text as [`squeezed`], each paragraph a line.
struct Words {
	text: String,
	/// Where a letter was a capital.
	capitals: BTreeSet<usize>,
}
impl Words {
	fn of(s: &str) -> Self {
		static PARAGRAPH: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n").expect("a constant"));
		let (mut text, mut capitals) = (String::new(), BTreeSet::new());
		for (n, paragraph) in PARAGRAPH.split(&plain(s)).enumerate() {
			for (at, word) in paragraph.split_whitespace().enumerate() {
				match (n, at) {
					(0, 0) => {}
					(_, 0) => text.push('\n'),
					_ => text.push(' '),
				}
				for c in word.chars() {
					if c.is_uppercase() {
						capitals.insert(text.len());
					}
					text.extend(c.to_lowercase());
				}
			}
		}
		Self { text, capitals }
	}
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

/// A person's mark that still holds, and whether ours went since.
fn mark(c: &Case<'_>) -> Option<(Mark, bool)> {
	let (mark, at) = c.mark?;
	let since = || c.messages.iter().rev().take_while(|m| m.id != at).filter(|m| m.heard());
	since().all(|m| m.sent).then(|| (mark, since().next().is_some()))
}

/// Google's last word, once read.
fn said<'a>(heard: &[&'a Message]) -> Option<&'a Reading> {
	heard.iter().rev().find(|m| !m.sent)?.reading.as_ref()
}

fn word(c: &Case<'_>, heard: &[&Message]) -> Option<Word> {
	let m = heard.iter().rev().find(|m| !m.sent)?;
	let r = m.reading.as_ref()?;
	let (mut kind, mut at) = (r.kind, m.at);
	let asks = counterpart(c.messages.iter()).is_some();
	if kind == Kind::CallOffered && asks {
		// the call it offered is told failed by a mail that takes no reply
		let since = c.elsewhere.iter().filter(|s| s.at > m.at && kind.superseded_by(s.kind)).max_by_key(|s| s.at);
		if let Some(failed) = since.filter(|s| s.kind == Kind::CallFailed) {
			(kind, at) = (failed.kind, failed.at);
		}
	}
	let turn = if kind == Kind::CallFailed && asks { Turn::Reply } else { kind.turn() };
	Some(Word {
		kind,
		at,
		turn,
		deadline: r.deadline,
	})
}

/// What Google's last word before a closure said, its promises aside.
fn before(heard: &[&Message]) -> Option<Kind> {
	let kinds = heard.iter().rev().filter_map(|m| Some(m.reading.as_ref()?.kind));
	kinds.into_iter().find(|k| !matches!(k.turn(), Turn::Closed | Turn::Google) && *k != Kind::Unclear)
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

/// What of its GBP's other mail, or the GBP's moves, leaves nothing of it to do.
fn over(c: &Case<'_>, heard: &[&Message]) -> Option<Settled> {
	let (last, w) = (heard.last()?, word(c, heard)?);
	let operators = w.turn == Turn::Operator;
	let settles = |k: Kind| !operators && matches!(k, Kind::Verified | Kind::AppealApproved | Kind::ReinstatementDone) && k.resolves(c.topic);
	let since = last.at.max(w.at);
	let later = c
		.elsewhere
		.iter()
		.filter(|s| s.at > since && (w.kind.superseded_by(s.kind) || settles(s.kind)))
		.max_by_key(|s| s.at);
	later.map(|s| Settled::Elsewhere(*s)).or_else(|| settled(c).filter(|_| !operators && w.kind != Kind::Asks))
}

/// What verified its GBP, by all that is said of it, of a verification; reinstated it, of a suspension.
fn settled(c: &Case<'_>) -> Option<Settled> {
	let own = c.messages.iter().filter_map(|m| Some((m.at, m.reading.as_ref()?.kind, Settled::Here)));
	let mut all: Vec<(Timestamp, Kind, Settled)> = c.elsewhere.iter().map(|s| (s.at, s.kind, Settled::Elsewhere(*s))).chain(own).collect();
	all.sort_by_key(|(at, ..)| *at);
	let (mut verified, mut suspended, mut reinstated) = (None, false, None);
	for (_, kind, by) in all {
		match kind {
			Kind::Verified => {
				verified = Some(by);
				reinstated = if suspended { Some(by) } else { reinstated };
				suspended = false;
			}
			Kind::ReverificationRequired | Kind::VideoRejected | Kind::CodeFailed => verified = None,
			Kind::Suspended => (suspended, reinstated) = (true, None),
			Kind::AppealApproved => (suspended, reinstated) = (false, Some(by)),
			_ => {}
		}
	}
	match c.topic {
		Topic::Verification => verified.filter(|_| !suspended),
		Topic::Suspension => reinstated,
		_ => None,
	}
}

/// A reply of ours is due: of Google's word, or its `silence` since.
fn reply(c: &Case<'_>, silence: Option<Timestamp>) -> State {
	let why = match (counterpart(c.messages.iter()), silence) {
		(_, None) if mark(c).is_some() => Why::Reopened,
		(_, Some(since)) if mark(c).is_some() => Why::Unanswered { since },
		(None, Some(since)) => Why::Unanswered { since },
		(None, None) => Why::NoReply,
		(Some(_), _) if asked(c.messages) >= MAX_ASKS => Why::AskedOften { asked: asked(c.messages) },
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
	const DONE: State = State::Done { by: Settled::Here };

	fn t(s: &str) -> Timestamp {
		s.parse().unwrap()
	}

	#[test]
	fn only_gmails_own_first_verdict_makes_a_message_googles() {
		assert!(authentic(&[NOTICE], &[GMAIL]));
		assert!(!authentic(&[NOTICE], &[]), "nobody checked it");
		assert!(!authentic(&[NOTICE, "a@google.com"], &[GMAIL]), "two senders");
		assert!(!authentic(&["businessprofile-noreply@google.com.verify.io"], &[GMAIL]));
		assert!(!authentic(&["no-reply@accounts.google.com"], &[GMAIL]), "its own domain's verdict");
		let of_sub = GMAIL.replace("header.from=google.com", "header.from=xwf.google.com");
		assert!(authentic(&["jean@xwf.google.com"], &[&of_sub]));
		assert!(!authentic(&[NOTICE], &[&of_sub]));
		assert!(!authentic(&["jean@evilgoogle.com"], &[&GMAIL.replace("header.from=google.com", "header.from=evilgoogle.com")]));
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
		let notice = |subject| ruled(Some(NOTICE), "Google Business Profile <businessprofile-noreply@google.com>", true, subject, false);
		assert_eq!(notice("You are now a manager of Plomberie Dupont - Lyon, Est"), Ruled::Reads(Kind::AccessGranted));
		assert_eq!(notice("You are now an owner of Plomberie Dupont"), Ruled::Reads(Kind::AccessGranted));
		assert_eq!(notice("Jean invited you to manage Plomberie Dupont"), Ruled::Reads(Kind::Invited));
		assert_eq!(notice("You can no longer manage Plomberie Dupont"), Ruled::Reads(Kind::AccessLost));
		assert_eq!(notice("Your October performance report"), Ruled::Reads(Kind::Noise));
		assert_eq!(notice("You got 3 new reviews"), Ruled::Reads(Kind::Noise));
		assert_eq!(notice("Marie left a review for Plomberie Dupont"), Ruled::Reads(Kind::Noise));
		assert_eq!(notice("Action required: Submit a new verification video"), Ruled::Model);
		assert_eq!(ruled(Some(SUPPORT), SUPPORT, true, "[1-1234000056789] Your Request to Google", false), Ruled::Model);
		for staff in ["jean@google.com", "jean@xwf.google.com"] {
			assert_eq!(ruled(Some(staff), staff, true, "Re: your request", true), Ruled::Model, "an answer to ours");
			assert_eq!(ruled(Some(staff), staff, true, "Re: your request", false), Ruled::Not);
			assert_ne!(ruled(Some(staff), staff, false, "Re: your request", true), Ruled::Model);
		}
		assert_eq!(
			ruled(Some("noreply@google.com"), "Google", true, "You can no longer manage x", false),
			Ruled::Model,
			"a notice's alone are fixed"
		);
		assert_eq!(ruled(Some("calendar-notification@google.com"), "Google Calendar", true, "Invitation", false), Ruled::Not);
		assert_eq!(ruled(Some("no-reply@accounts.google.com"), "Google", false, "Security alert", false), Ruled::Not);
		assert_eq!(ruled(Some("jean@plomberie.fr"), "Jean <jean@plomberie.fr>", false, "cc a Googler", false), Ruled::Not);
		let look_alike = Ruled::Reads(Kind::Impersonation);
		assert_eq!(ruled(Some(SUPPORT), SUPPORT, false, "[1-1234000056789] Your Request", true), look_alike);
		assert_eq!(
			ruled(Some("support@gbp-verify.io"), "Google Business Profile <support@gbp-verify.io>", false, "Verify now", false),
			look_alike
		);
		assert_eq!(ruled(Some("businessprofile-noreply@google.com.verify.io"), "Support", false, "Verify now", false), look_alike);

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
		let notice = "Submit new video\n<https://business.google.com/verify/l/00000000000000000002?trk=x>\n…\n<https://business.google.com/verify/l/00000000000000000002?trk=y>";
		assert_eq!(listing(notice).as_deref(), Some("00000000000000000002"), "as written, its zeros too");
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
			names: vec![name.to_owned()],
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
		assert_eq!(one("Plomberie Dupont – Lyon’s"), [1], "a dash and an apostrophe are Google's own");
		assert_eq!(one("Plomberie Dupont - Lyonnaise"), [] as [i64; 0], "a word is whole");
		assert_eq!(one("A video"), [] as [i64; 0]);
		let alone = |text: &str| named(text, &gbps[..1]).into_iter().collect::<Vec<_>>();
		assert_eq!(alone("video for Plomberie Dupont - Lyon, Est (at 9 Av. Basse)"), [] as [i64; 0], "another's, not known here");
		assert_eq!(alone("video for Plomberie Dupont - Lyon\n- Nord"), [] as [i64; 0], "wrapped");
		assert_eq!(alone("You can now manage Plomberie Dupont - Lyon\n\nView profile"), [1]);
		assert_eq!(alone("“Plomberie Dupont - Lyon, 3 Rue Haute”"), [1]);
		let renamed = [
			Named {
				names: vec!["Dupont Lyon".into(), "Plomberie Dupont - Lyon".into()],
				..gbps[0].clone()
			},
			gbp(4, "Plomberie Dupont - Lyon", None),
		];
		assert_eq!(named("Dupont Lyon is verified", &renamed), BTreeSet::from([1]));
		assert_eq!(named("Plomberie Dupont - Lyon is verified", &renamed), BTreeSet::from([4]), "whose name it is now");
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

	fn told(at: &str, kind: Kind) -> Said {
		Said { kind, at: t(at), case_id: Some(9) }
	}

	fn case(messages: &[Message]) -> Case<'_> {
		Case {
			topic: Topic::Verification,
			about: About::Live,
			messages,
			draft: None,
			mark: None,
			elsewhere: &[],
			carried_by: None,
			switched: true,
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
			login: false,
		}
	}

	fn needs(why: Why) -> State {
		State::NeedsYou { why }
	}

	fn quiet(because: QuietBecause) -> State {
		State::Quiet { because }
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
		let later = [told("2026-10-06T10:10:00Z", Kind::CallFailed)];
		let failed = Case { elsewhere: &later, ..case(&m) };
		assert_eq!(state(&failed, now), State::ReplyDue, "the call was held, and failed");
		assert_eq!(press(&failed, now), Ok(t("2026-10-05T22:00:00Z")));
		let held = [later[0], told("2026-10-06T10:15:00Z", Kind::Verified)];
		assert_eq!(state(&Case { elsewhere: &held, ..case(&m) }, now), State::Done { by: Settled::Elsewhere(held[1]) });
		assert_eq!(
			state(
				&Case {
					mark: Some((Mark::Done, 5)),
					..case(&m)
				},
				now
			),
			State::Done { by: Settled::Hand }
		);
		assert_eq!(
			state(
				&Case {
					mark: Some((Mark::Done, 4)),
					..case(&m)
				},
				now
			),
			needs(Why::Kind { kind: Kind::CallOffered }),
			"Google spoke since"
		);
		assert_eq!(state(&case(&m), t("2026-11-20T00:00:00Z")), quiet(QuietBecause::Old));
		m.push(google(6, "2026-10-06T10:20:00Z", Kind::Closed));
		assert_eq!(state(&case(&m), now), needs(Why::Kind { kind: Kind::CallOffered }), "closed, the call there to take");
		let closed = |before: Kind| state(&case(&[google(1, "2026-10-06T09:00:00Z", before), google(2, "2026-10-06T09:30:00Z", Kind::Closed)]), now);
		assert_eq!(closed(Kind::Verified), DONE);
		assert_eq!(closed(Kind::SelfServe), needs(Why::Kind { kind: Kind::Closed }), "nothing came of it");
		let mut later = google(1, "2026-10-06T09:00:00Z", Kind::Closed);
		later.reading.as_mut().unwrap().deadline = Some(t("2026-10-09T09:00:00Z"));
		assert_eq!(state(&case(&[later]), now), needs(Why::Deadline { at: t("2026-10-09T09:00:00Z") }));
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
		let often = || needs(Why::AskedOften { asked: 6 });
		assert_eq!(state(&case(&asked), now), often());
		assert!(due(&case(&asked), now), "there for its operator all the same");
		let written = draft(DraftState::Proposed, 7, &[]);
		let limit = Case {
			draft: Some(&written),
			..case(&asked)
		};
		assert_eq!((state(&limit, now), due(&limit, now)), (often(), false));
		asked.insert(5, google(19, "2026-10-02T10:00:00Z", Kind::OptionsExhausted));
		assert_eq!(state(&case(&asked), now), often());
		for new in [Kind::CallOffered, Kind::Asks] {
			let mut since = asked.clone();
			since.insert(5, google(18, "2026-10-02T10:00:00Z", new));
			since.push(google(20, "2026-10-06T09:00:00Z", Kind::SelfServe));
			assert_eq!(state(&case(&since), now), State::ReplyDue, "{new:?} is something new");
		}
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
		assert_eq!(state(&case(std::slice::from_ref(&dated)), now), quiet(QuietBecause::Nothing));
		dated.reading.as_mut().unwrap().deadline = Some(t("2026-10-10T00:00:00Z"));
		assert_eq!(state(&case(&[dated]), now), needs(Why::Deadline { at: t("2026-10-10T00:00:00Z") }));
	}

	#[test]
	fn a_notice_stands_until_what_supersedes_it() {
		let now = t("2026-10-06T10:30:00Z");
		let m = [notice(1, "2026-10-05T09:00:00Z", Kind::VideoRejected)];
		assert_eq!(state(&case(&m), now), needs(Why::Kind { kind: Kind::VideoRejected }));
		assert_eq!(
			state(&Case { carried_by: Some(9), ..case(&m) }, now),
			quiet(QuietBecause::Carried { case_id: 9 }),
			"a conversation asks for its verification"
		);
		let on = |said: Kind, at: &str, kind| {
			let m = [notice(1, "2026-10-05T09:00:00Z", said)];
			state(
				&Case {
					topic: said.topic().unwrap(),
					elsewhere: &[told(at, kind)],
					..case(&m)
				},
				now,
			)
		};
		let after = "2026-10-06T09:00:00Z";
		for kind in [Kind::Verified, Kind::VideoRejected, Kind::CodeFailed] {
			assert_eq!(
				on(Kind::VideoRejected, after, kind),
				State::Done {
					by: Settled::Elsewhere(told(after, kind))
				},
				"the newer one stands"
			);
		}
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
		assert_eq!(state(&case(&[to_noreply]), now), quiet(QuietBecause::Nothing));
		for k in [Kind::Noise, Kind::Impersonation, Kind::AccessGranted] {
			assert_eq!(state(&case(&[notice(1, "2026-10-06T09:00:00Z", k)]), now), quiet(QuietBecause::Nothing));
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

	#[test]
	fn a_goal_is_pursued_from_our_first_ask_until_google_gives_it() {
		let ours = |at, id| Spoken {
			at: t(at),
			message: Some(id),
			case: Some(1),
			said: None,
		};
		let said = |at, case, kind| Spoken {
			at: t(at),
			message: Some(99),
			case: Some(case),
			said: Some(kind),
		};
		let spoken = [
			said("2026-10-01T00:00:00Z", 1, Kind::SelfServe),
			ours("2026-10-01T09:00:00Z", 1),
			said("2026-10-01T10:00:00Z", 1, Kind::Acknowledged),
			said("2026-10-02T00:00:00Z", 2, Kind::VideoRejected),
			ours("2026-10-03T09:00:00Z", 2),
			said("2026-10-03T10:00:00Z", 1, Kind::SelfServe),
			ours("2026-10-04T09:00:00Z", 3),
			said("2026-10-05T09:00:00Z", 3, Kind::CallOffered),
			said("2026-10-06T09:00:00Z", 2, Kind::CallFailed),
			ours("2026-10-06T10:00:00Z", 4),
		];
		let [got, open] = pursuits(Goal::Call, &spoken).try_into().unwrap();
		let answers: Vec<_> = got.asks.iter().map(|(id, _, a)| (*id, *a)).collect();
		assert_eq!(answers, [(1, None), (2, Some(Kind::SelfServe)), (3, Some(Kind::CallOffered))]);
		assert_eq!(got.over, Some((t("2026-10-05T09:00:00Z"), Kind::CallOffered)));
		assert_eq!((open.asks.len(), open.over), (1, None), "asked again once the call failed");
		let verified = [
			ours("2026-10-01T09:00:00Z", 1),
			Spoken {
				at: t("2026-10-02T00:00:00Z"),
				message: None,
				case: None,
				said: Some(Kind::Verified),
			},
		];
		let [over] = pursuits(Goal::Call, &verified).try_into().unwrap();
		assert_eq!((over.asks[0].2, over.over.unwrap().1), (None, Kind::Verified), "over, and not by what we wrote");
	}
}
