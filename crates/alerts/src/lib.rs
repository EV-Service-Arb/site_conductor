//! Alerts' shared, I/O-free contract: rules and the notes publishing services expose to the
//! conductor.

use std::{fmt, str::FromStr};

use jiff::Timestamp;
use sa_auth::{Leads, PermissionSet, SA_ADMIN, Service, Sources};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Audience {
	Permission,
	GmbOwners,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
	NewLead,
	ContactOverdue,
	PaymentReceived,
	SourceSilent,
	Booked,
	AccessRequested,
	ReviewNew,
	ReviewChanged,
	ReviewGone,
	ReviewReappeared,
	ScanFailed,
}

impl Rule {
	pub const ALL: [Self; 11] = [
		Self::NewLead,
		Self::ContactOverdue,
		Self::PaymentReceived,
		Self::SourceSilent,
		Self::Booked,
		Self::AccessRequested,
		Self::ReviewNew,
		Self::ReviewChanged,
		Self::ReviewGone,
		Self::ReviewReappeared,
		Self::ScanFailed,
	];

	pub const fn as_str(self) -> &'static str {
		match self {
			Self::NewLead => "new_lead",
			Self::ContactOverdue => "contact_overdue",
			Self::PaymentReceived => "payment_received",
			Self::SourceSilent => "source_silent",
			Self::Booked => "booked",
			Self::AccessRequested => "access_requested",
			Self::ReviewNew => "review_new",
			Self::ReviewChanged => "review_changed",
			Self::ReviewGone => "review_gone",
			Self::ReviewReappeared => "review_reappeared",
			Self::ScanFailed => "scan_failed",
		}
	}

	/// The service whose feed publishes it; `None`: the conductor's own.
	pub const fn feed(self) -> Option<Service> {
		match self {
			Self::NewLead | Self::ContactOverdue | Self::PaymentReceived | Self::Booked => Some(Service::Sites),
			Self::ReviewNew | Self::ReviewChanged | Self::ReviewGone | Self::ReviewReappeared | Self::ScanFailed => Some(Service::ReviewArchive),
			Self::SourceSilent | Self::AccessRequested => None,
		}
	}

	pub const fn audience(self) -> Audience {
		match self {
			Self::ReviewNew | Self::ReviewChanged | Self::ReviewGone | Self::ReviewReappeared | Self::ScanFailed => Audience::GmbOwners,
			_ => Audience::Permission,
		}
	}

	pub const fn on_by_default(self) -> bool {
		matches!(self, Self::NewLead | Self::ContactOverdue | Self::Booked | Self::AccessRequested | Self::ReviewGone)
	}

	pub fn open_to(self, permissions: &PermissionSet) -> bool {
		match self {
			Self::NewLead | Self::ContactOverdue | Self::Booked => permissions.may(Leads::Edit),
			Self::PaymentReceived | Self::SourceSilent => permissions.may(Sources::Manage),
			Self::AccessRequested => SA_ADMIN.members.iter().all(|member| permissions.iter().any(|held| held == *member)),
			Self::ReviewNew | Self::ReviewChanged | Self::ReviewGone | Self::ReviewReappeared | Self::ScanFailed => true,
		}
	}
}

impl fmt::Display for Rule {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(self.as_str())
	}
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("unknown alert rule {0:?}")]
pub struct UnknownRule(String);

impl FromStr for Rule {
	type Err = UnknownRule;

	fn from_str(value: &str) -> Result<Self, Self::Err> {
		Self::ALL.into_iter().find(|rule| rule.as_str() == value).ok_or_else(|| UnknownRule(value.to_owned()))
	}
}

/// What pressing a button does: an operation of the note's service, as the user who pressed.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
	/// `op_id(handler)` of the service's table: the conductor resolves it in its catalog.
	pub operation: String,
	/// The operation path's parameters by name.
	pub path: Map<String, Value>,
	pub body: Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Button {
	pub id: String,
	pub label: String,
	pub pressed: String,
	pub call: Call,
	pub leaves: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NoteText {
	pub text: String,
	pub entities: Vec<Value>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Note {
	pub seq: i64,
	pub rule: Rule,
	pub event_id: Uuid,
	pub created_at: Timestamp,
	pub text: NoteText,
	pub text_pii: Option<NoteText>,
	pub skip_user: Option<Uuid>,
	pub buttons: Vec<Button>,
	#[serde(default, skip_serializing_if = "Vec::is_empty")]
	pub gmbs: Vec<i64>,
	#[serde(default, skip_serializing_if = "Option::is_none")]
	pub photo: Option<String>,
}
impl Note {
	/// Its shape, against the feed it came from; its buttons' calls are the conductor's to resolve.
	pub fn check(&self, feed: Service) -> Result<(), InvalidNote> {
		let fail = |reason: String| InvalidNote {
			seq: self.seq,
			rule: self.rule,
			reason,
		};
		if self.seq <= 0 {
			return Err(fail("seq is not positive".into()));
		}
		if self.rule.feed() != Some(feed) {
			return Err(fail(format!("rule belongs to {}, not {}", self.rule.feed().map_or("the conductor", Service::as_str), feed)));
		}
		match self.rule.audience() {
			Audience::GmbOwners if self.gmbs.is_empty() => return Err(fail("GMB-owned rule has no GMBs".into())),
			Audience::Permission if !self.gmbs.is_empty() => return Err(fail("permission rule carries GMBs".into())),
			_ => {}
		}
		if self.photo.is_some() && self.rule != Rule::ReviewGone {
			return Err(fail("only review_gone may carry a photo".into()));
		}
		if let Some(photo) = &self.photo {
			if !self.buttons.is_empty() {
				return Err(fail("a note with a photo carries no buttons".into())); // a caption is not edited as a text is
			}
			if !valid_path(photo) {
				return Err(fail(format!("photo path {photo:?} is not an absolute service path")));
			}
			if self.text.text.encode_utf16().count() > 1024 || self.text_pii.as_ref().is_some_and(|text| text.text.encode_utf16().count() > 1024) {
				return Err(fail("photo caption exceeds 1024 UTF-16 code units".into()));
			}
		}
		for button in &self.buttons {
			if !is_button_id(&button.id) || self.buttons.iter().filter(|other| other.id == button.id).count() != 1 {
				return Err(fail(format!("button id {:?} is not 2-16 of [a-z0-9_], once", button.id)));
			}
			if let Some(missing) = button.leaves.iter().find(|left| !self.buttons.iter().any(|other| other.id == **left)) {
				return Err(fail(format!("button {} leaves {missing:?}, which is not one of the note's", button.id)));
			}
		}
		for text in std::iter::once(&self.text).chain(&self.text_pii) {
			if let Some(entity) = text
				.entities
				.iter()
				.find(|entity| !entity["type"].is_string() || !entity["offset"].is_u64() || !entity["length"].is_u64())
			{
				return Err(fail(format!("{entity} is not a MessageEntity")));
			}
		}
		Ok(())
	}
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NotePage {
	pub notes: Vec<Note>,
	pub next: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("note {seq} ({rule}): {reason}")]
pub struct InvalidNote {
	pub seq: i64,
	pub rule: Rule,
	pub reason: String,
}

pub fn is_button_id(value: &str) -> bool {
	(2..=16).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_path(path: &str) -> bool {
	path.starts_with('/')
		&& !path.starts_with("//")
		&& path.split('/').skip(1).all(|part| !part.is_empty() && part != "." && part != "..")
		&& path.bytes().all(|byte| byte.is_ascii_graphic() && byte != b'?' && byte != b'#' && byte != b'\\')
}

#[cfg(test)]
mod tests {
	use super::*;

	fn note(rule: Rule) -> Note {
		Note {
			seq: 1,
			rule,
			event_id: Uuid::nil(),
			created_at: "2026-01-01T00:00:00Z".parse().unwrap(),
			text: NoteText {
				text: "text".into(),
				entities: vec![],
			},
			text_pii: None,
			skip_user: None,
			buttons: vec![],
			gmbs: vec![],
			photo: None,
		}
	}

	#[test]
	fn rules_round_trip_and_describe_their_feed() {
		for rule in Rule::ALL {
			assert_eq!(rule.as_str().parse::<Rule>().unwrap(), rule);
		}
		assert!("review_low".parse::<Rule>().is_err());
		assert_eq!(Rule::ReviewGone.feed(), Some(Service::ReviewArchive));
		assert_eq!(Rule::SourceSilent.feed(), None);
		assert_eq!(Rule::ReviewGone.audience(), Audience::GmbOwners);
		assert!(Rule::ReviewGone.on_by_default());
		assert!(!Rule::ReviewNew.on_by_default());
	}

	#[test]
	fn permission_rules_are_scoped_and_gmb_rules_open_to_owners() {
		let operator: PermissionSet = sa_auth::SA_OPERATOR.members.iter().copied().collect();
		let admin: PermissionSet = SA_ADMIN.members.iter().copied().collect();
		let nobody: PermissionSet = std::iter::empty::<&str>().collect();
		assert!(Rule::Booked.on_by_default());
		assert!(Rule::Booked.open_to(&operator));
		assert!(Rule::NewLead.open_to(&operator));
		assert!(Rule::ContactOverdue.open_to(&operator));
		assert!(!Rule::PaymentReceived.open_to(&operator));
		assert!(!Rule::SourceSilent.open_to(&operator));
		assert!(!Rule::AccessRequested.open_to(&operator));
		assert!(Rule::ALL.iter().all(|r| r.open_to(&admin)));
		for rule in Rule::ALL {
			assert_eq!(rule.open_to(&nobody), rule.audience() == Audience::GmbOwners, "{rule}: owning the GMB is the hub's to check");
		}
	}

	#[test]
	fn note_validation_binds_a_note_to_its_feed_and_audience() {
		let mut review = note(Rule::ReviewGone);
		assert_eq!(review.check(Service::ReviewArchive).unwrap_err().reason, "GMB-owned rule has no GMBs");
		review.gmbs.push(7);
		assert!(review.check(Service::ReviewArchive).is_ok());
		assert_eq!(review.check(Service::Sites).unwrap_err().reason, "rule belongs to review_archive, not sites");

		let mut sites = note(Rule::NewLead);
		sites.gmbs.push(7);
		assert_eq!(sites.check(Service::Sites).unwrap_err().reason, "permission rule carries GMBs");
	}

	#[test]
	fn photos_are_review_gone_captions() {
		let mut review = note(Rule::ReviewGone);
		review.gmbs.push(7);
		review.photo = Some("/api/conductor/captures/abc.png".into());
		assert!(review.check(Service::ReviewArchive).is_ok());
		review.text.text = "x".repeat(1025);
		assert_eq!(review.check(Service::ReviewArchive).unwrap_err().reason, "photo caption exceeds 1024 UTF-16 code units");
	}
}
