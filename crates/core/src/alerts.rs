//! Alerts without the I/O: when a destination is told of a note, which notes are for it, and one
//! message of several.

use std::collections::BTreeSet;

use jiff::{SignedDuration, Timestamp, civil, tz::TimeZone};
use sa_alerts::Tier;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{AsRefStr, EnumString, VariantArray};

use crate::{
	Invalid,
	notify::{Locale, Rendered},
};

/// How long a note may stay unread in the panel before an `unread` destination is told of it.
pub const UNREAD_AFTER: SignedDuration = SignedDuration::from_mins(10);

/// A message of several names this many of them, the rest counted.
const DIGEST_ITEMS: usize = 20;

/// The longest a note runs in a message of several, in characters.
const DIGEST_ITEM_CHARS: usize = 160;

/// A network, and what of it an address is.
#[derive(AsRefStr, Clone, Copy, Debug, Deserialize, EnumString, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize, VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Method {
	/// A chat with the bot, a channel, a group or a topic of one.
	Telegram,
	Email,
	/// A channel's webhook.
	Discord,
	/// A channel's incoming webhook.
	Slack,
	/// One browser's Web Push.
	Push,
	/// An https URL of theirs.
	Webhook,
}

/// When a destination is told.
#[derive(AsRefStr, Clone, Copy, Debug, Deserialize, EnumString, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize, VariantArray)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Cadence {
	Now,
	/// Once it has stayed unread in the panel for 10 minutes.
	Unread,
	/// One message an hour, with what came in it.
	Hourly,
	/// One message a day, at the person's hour.
	Daily,
	Off,
}

/// A cadence for each tier.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cadences {
	pub urgent: Cadence,
	pub important: Cadence,
	pub info: Cadence,
}

impl Cadences {
	/// What a new destination starts with; `private`: read by its person alone.
	pub fn starting(method: Method, private: bool) -> Self {
		use Cadence::{Daily, Hourly, Now, Off, Unread};
		let (urgent, important, info) = match method {
			Method::Email => (Unread, Hourly, Daily),
			Method::Webhook => (Now, Now, Now),
			_ if private => (Now, Now, Off),
			_ => (Now, Now, Daily),
		};
		Self { urgent, important, info }
	}

	pub fn of(self, tier: Tier) -> Cadence {
		match tier {
			Tier::Urgent => self.urgent,
			Tier::Important => self.important,
			Tier::Info => self.info,
		}
	}
}

/// What a note is about, for a [`Filter`]: a GBP, or a gmail alone.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Subject {
	pub gmb: Option<i64>,
	pub gmail: Option<i64>,
	pub trade: Option<String>,
	pub brand: Option<i64>,
}

/// Which notes a destination is for. A facet left empty takes all.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Filter {
	pub gmails: BTreeSet<i64>,
	/// Google categories, `gcid:…`.
	pub trades: BTreeSet<String>,
	pub brands: BTreeSet<i64>,
	pub gmbs: BTreeSet<i64>,
}

impl Filter {
	/// Whether a note about `subjects` passes: about none, or one of them in every facet set.
	pub fn passes(&self, subjects: &[Subject]) -> bool {
		fn within<T: Ord>(facet: &BTreeSet<T>, v: &Option<T>) -> bool {
			facet.is_empty() || v.as_ref().is_some_and(|v| facet.contains(v))
		}
		subjects.is_empty()
			|| subjects
				.iter()
				.any(|s| within(&self.gmails, &s.gmail) && within(&self.trades, &s.trade) && within(&self.brands, &s.brand) && within(&self.gmbs, &s.gmb))
	}
}

/// A person's hours.
#[derive(Clone, Debug)]
pub struct Hours {
	tz: TimeZone,
	daily: civil::Time,
	quiet: Option<(civil::Time, civil::Time)>,
	snoozed_until: Option<Timestamp>,
}

impl Hours {
	/// `daily` and `quiet` in minutes since midnight in `tz`, an IANA name; quiet hours that end
	/// before they begin run past midnight.
	pub fn try_new(tz: &str, daily: u16, quiet: Option<(u16, u16)>, snoozed_until: Option<Timestamp>) -> Result<Self, Invalid> {
		let time = |minutes: u16| {
			let (h, m) = (minutes.div_euclid(60), minutes.rem_euclid(60));
			match (i8::try_from(h), i8::try_from(m)) {
				(Ok(h @ 0..24), Ok(m)) => Ok(civil::time(h, m, 0, 0)),
				_ => Err(Invalid::new(format!("{minutes} is not a minute of a day"))),
			}
		};
		let quiet = quiet.map(|(from, to)| Ok::<_, Invalid>((time(from)?, time(to)?))).transpose()?;
		if quiet.is_some_and(|(from, to)| from == to) {
			return Err(Invalid::new("quiet hours that end as they begin"));
		}
		Ok(Self {
			tz: TimeZone::get(tz).map_err(|_| Invalid::new(format!("{tz:?} is not a time zone")))?,
			daily: time(daily)?,
			quiet,
			snoozed_until,
		})
	}

	/// The first moment strictly after `at` whose wall clock there says `time`.
	fn next(&self, at: Timestamp, time: civil::Time) -> Timestamp {
		let here = at.to_zoned(self.tz.clone());
		let on = |day: civil::Date| day.to_datetime(time).to_zoned(self.tz.clone()).expect("compatible disambiguation never fails").timestamp();
		match on(here.date()) {
			today if today > at => today,
			_ => on(here.date().tomorrow().expect("far from the calendar's end")),
		}
	}

	/// When what would go out at `at` may, past the quiet hours and the snooze it falls in.
	fn open_from(&self, at: Timestamp) -> Timestamp {
		let at = self.snoozed_until.filter(|until| *until > at).unwrap_or(at); // not snoozed: as it was
		let Some((from, to)) = self.quiet else { return at };
		let here = at.to_zoned(self.tz.clone()).datetime();
		let quiet = if from < to { from <= here.time() && here.time() < to } else { here.time() >= from || here.time() < to };
		if !quiet {
			return at;
		}
		// the hour clocks repeat says `to` twice
		let repeated = self.tz.to_ambiguous_zoned(here.date().to_datetime(to)).later().expect("every time of a day has a later one").timestamp();
		Some(repeated).filter(|r| *r > at).map_or(self.next(at, to), |r| r.min(self.next(at, to)))
	}

	/// When a note of `tier` taken at `now` goes out to a destination at `cadence`, and whether as one
	/// message with whatever else is due there then; `None`: never.
	pub fn due(&self, cadence: Cadence, tier: Tier, now: Timestamp) -> Option<Due> {
		let (at, together) = match cadence {
			Cadence::Off => return None,
			Cadence::Now => (now, false),
			Cadence::Unread => (now + UNREAD_AFTER, false),
			Cadence::Hourly => {
				let hour = SignedDuration::from_hours(1).as_secs();
				let next = (now.as_second().div_euclid(hour) + 1) * hour;
				(Timestamp::from_second(next).expect("far from the calendar's end"), true)
			}
			Cadence::Daily => (self.next(now, self.daily), true),
		};
		if tier == Tier::Urgent {
			return Some(Due { at, together });
		}
		let open = self.open_from(at);
		Some(Due {
			at: open,
			together: together || open > at,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Due {
	pub at: Timestamp,
	/// Goes out as one message with whatever else is due there then.
	pub together: bool,
}

/// One message of `notes`, most urgent first.
pub fn digest(notes: &[(Tier, Rendered)], locale: Locale) -> Rendered {
	let ru = locale == Locale::Ru;
	let mut out = Rendered::default();
	let count = notes.len();
	out.push(&if ru { format!("Оповещений: {count}") } else { format!("{count} alerts") });
	let mut order: Vec<&(Tier, Rendered)> = notes.iter().collect();
	order.sort_by_key(|(tier, _)| std::cmp::Reverse(*tier));
	for (tier, note) in order.iter().take(DIGEST_ITEMS) {
		out.push("\n\n");
		out.push(mark(*tier));
		out.append(&note.cut(DIGEST_ITEM_CHARS));
	}
	if let Some(more) = count.checked_sub(DIGEST_ITEMS).filter(|n| *n > 0) {
		out.push(&if ru { format!("\n\nИ ещё {more} в панели") } else { format!("\n\nAnd {more} more in the panel") });
	}
	out
}

fn mark(tier: Tier) -> &'static str {
	match tier {
		Tier::Urgent => "🔴 ",
		Tier::Important => "🟠 ",
		Tier::Info => "",
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn at(s: &str) -> Timestamp {
		s.parse().unwrap()
	}

	#[test]
	fn what_goes_out_when() {
		let paris = Hours::try_new("Europe/Paris", 9 * 60, Some((22 * 60, 7 * 60)), None).unwrap();
		let due = |cadence, tier, now: &str| paris.due(cadence, tier, at(now)).map(|d| (d.at, d.together));
		let noon = "2026-11-24T11:00:00Z"; // 12:00 in Paris
		let night = "2026-11-24T22:30:00Z"; // 23:30
		assert_eq!(due(Cadence::Now, Tier::Info, noon), Some((at(noon), false)));
		assert_eq!(due(Cadence::Off, Tier::Urgent, noon), None);
		assert_eq!(due(Cadence::Unread, Tier::Urgent, night), Some((at("2026-11-24T22:40:00Z"), false)), "urgent pierces the night");
		assert_eq!(due(Cadence::Now, Tier::Important, night), Some((at("2026-11-25T06:00:00Z"), true)), "held to 7:00, with the rest of the night");
		assert_eq!(due(Cadence::Hourly, Tier::Info, "2026-11-24T11:20:00Z"), Some((at("2026-11-24T12:00:00Z"), true)));
		assert_eq!(due(Cadence::Hourly, Tier::Info, "2026-11-24T20:20:00Z"), Some((at("2026-11-25T06:00:00Z"), true)));
		assert_eq!(due(Cadence::Daily, Tier::Info, noon), Some((at("2026-11-25T08:00:00Z"), true)));
		assert_eq!(due(Cadence::Daily, Tier::Info, "2026-11-25T08:00:00Z"), Some((at("2026-11-26T08:00:00Z"), true)), "never at the moment itself");
		let snoozed = Hours::try_new("UTC", 0, None, Some(at("2026-11-24T13:00:00Z"))).unwrap();
		assert_eq!(snoozed.due(Cadence::Now, Tier::Info, at(noon)).unwrap().at, at("2026-11-24T13:00:00Z"));
		assert_eq!(snoozed.due(Cadence::Now, Tier::Urgent, at(noon)).unwrap().at, at(noon));
		// 25 October 2026: at 3:00 Paris goes back to 2:00
		let late = Hours::try_new("Europe/Paris", 0, Some((60, 150)), None).unwrap();
		let held = |now: &str| late.due(Cadence::Now, Tier::Info, at(now)).unwrap().at;
		assert_eq!(held("2026-10-25T00:10:00Z"), at("2026-10-25T00:30:00Z"), "2:10, the first");
		assert_eq!(held("2026-10-25T01:10:00Z"), at("2026-10-25T01:30:00Z"), "2:10, the second");
		// 29 March 2026: at 2:00 it goes on to 3:00
		assert_eq!(held("2026-03-29T00:10:00Z"), at("2026-03-29T01:30:00Z"), "1:10 to what would be 2:30");
		assert!(Hours::try_new("Mars/Olympus", 0, None, None).is_err());
		assert!(Hours::try_new("UTC", 1440, None, None).is_err());
		assert!(Hours::try_new("UTC", 0, Some((60, 60)), None).is_err());
	}

	#[test]
	fn a_note_passes_by_one_subject_in_every_facet() {
		let subject = |gmb, gmail, trade: &str| Subject {
			gmb: Some(gmb),
			gmail: Some(gmail),
			trade: Some(trade.into()),
			brand: None,
		};
		let filter = Filter {
			gmails: [1].into(),
			trades: ["gcid:plumber".into()].into(),
			..Filter::default()
		};
		assert!(filter.passes(&[]), "about no GBP");
		assert!(filter.passes(&[subject(7, 2, "gcid:plumber"), subject(8, 1, "gcid:plumber")]));
		assert!(!filter.passes(&[subject(7, 2, "gcid:plumber"), subject(8, 1, "gcid:locksmith")]), "no one GBP is in both");
		let a_gmail = [Subject { gmail: Some(1), ..Subject::default() }];
		assert!(!filter.passes(&a_gmail));
		assert!(Filter { gmails: [1].into(), ..Filter::default() }.passes(&a_gmail));
	}

	#[test]
	fn a_digest_is_most_urgent_first_and_bounded() {
		let note = |text: &str| {
			let mut r = Rendered::default();
			r.push("é ");
			r.code(text);
			r
		};
		let one = digest(&[(Tier::Info, note("quiet")), (Tier::Urgent, note("call"))], Locale::En);
		assert_eq!(one.text, "2 alerts\n\n🔴 é call\n\né quiet");
		assert_eq!(one.entities[0]["offset"], 15, "in UTF-16 code units");
		assert_eq!(one.entities[1]["offset"], 23);
		let many = digest(&vec![(Tier::Info, note(&"x".repeat(5_000))); 50], Locale::En);
		assert!(many.text.encode_utf16().count() < 4096, "one Telegram message");
		assert!(many.text.ends_with("And 30 more in the panel"));
	}
}
