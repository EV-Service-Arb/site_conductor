//! The Places API keys: a person's own first, in the order they were given, then the rotation's.
//! A key sits out while Google says it is spent, and the next one is asked. The rotation's free
//! keys go first, so billed ones pay only for what the free ones could not.

use std::{
	collections::HashMap,
	sync::Mutex,
	time::{Duration, SystemTime, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// Why a key is sitting out. Only a refusal says so: an API key cannot read its quota ahead.
#[derive(Clone, Debug, PartialEq)]
pub enum Spent {
	/// A quota of its project's, as the refusal spelled it.
	Quota {
		/// The quota's name, `SearchTextRequestPerDayPerProject` and the like.
		limit: String,
		/// Unix seconds its window rolls over at.
		reset: u64,
	},
	/// Its project's billing is off, which is what an ended trial looks like: back only at
	/// restart, after a human has fixed it.
	Billing,
}
impl Spent {
	/// What a refusal says about the key, if it says the key is done for a window. Anything else —
	/// a malformed query, a key with the wrong referrer — is `None`: rotating on it would spend
	/// the next project's quota on the same mistake.
	pub(crate) fn of(body: &str) -> Option<Self> {
		let v = serde_json::from_str::<serde_json::Value>(body).ok()?; // not JSON: no reason given, the refusal travels as is
		let details = v["error"]["details"].as_array()?;
		if details.iter().any(|d| d["reason"].as_str() == Some("BILLING_DISABLED")) {
			return Some(Self::Billing);
		}
		if v["error"]["status"].as_str() != Some("RESOURCE_EXHAUSTED") {
			return None;
		}
		details.iter().map(|d| &d["metadata"]).find_map(|m| {
			let limit = m["quota_limit"].as_str()?;
			let start: u64 = m["window_start_time"].as_str()?.parse().ok()?;
			let window = if limit.contains("PerMinute") { 60 } else { 86_400 };
			Some(Self::Quota {
				limit: limit.to_owned(),
				reset: start + window,
			})
		})
	}

	fn back_by(&self, now: SystemTime) -> bool {
		match self {
			Self::Quota { reset, .. } => now >= UNIX_EPOCH + Duration::from_secs(*reset),
			Self::Billing => false,
		}
	}
}

impl std::fmt::Display for Spent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Quota { limit, reset } => write!(f, "its project's {limit} is used, until {:02}:{:02} UTC", reset % 86_400 / 3600, reset % 3600 / 60),
			Self::Billing => write!(f, "its project's billing is disabled"),
		}
	}
}

/// The rotation's keys and which of them sit out, and the own keys sitting out (by digest: an own
/// key is held only for the request that brings it).
pub struct Keys(Mutex<Held>);
impl Keys {
	/// The rotation, `keys` in the order they are used in.
	pub fn new(keys: Vec<String>) -> Self {
		for (i, k) in keys.iter().enumerate() {
			assert!(!keys[..i].contains(k), "a Places key is listed twice");
		}
		Self(Mutex::new(Held {
			rotation: keys.into_iter().map(|k| (Zeroizing::new(k), None)).collect(),
			own: HashMap::new(),
		}))
	}

	pub(crate) fn is_empty(&self) -> bool {
		self.0.lock().expect("never poisoned: no panic while held").rotation.is_empty()
	}

	/// The first of `own`, then of the rotation, not sitting out and not `tried` (by this request:
	/// a window Google says is over already must not loop it), else why each sits out.
	pub(crate) fn next(&self, own: &[Zeroizing<String>], tried: &[Zeroizing<String>]) -> Result<(Zeroizing<String>, Tier), Vec<Spent>> {
		let mut held = self.0.lock().expect("never poisoned: no panic while held");
		let now = SystemTime::now();
		for key in own.iter().filter(|k| !tried.contains(k)) {
			let digest = digest(key);
			if held.own.get(&digest).is_some_and(|s| s.back_by(now)) {
				held.own.remove(&digest);
			}
			if !held.own.contains_key(&digest) {
				return Ok((key.clone(), Tier::Own));
			}
		}
		for (key, out) in held.rotation.iter_mut().filter(|(k, _)| !tried.contains(k)) {
			if out.as_ref().is_some_and(|s| s.back_by(now)) {
				*out = None;
			}
			if out.is_none() {
				return Ok((key.clone(), Tier::Rotation));
			}
		}
		let own_out = own.iter().filter_map(|k| held.own.get(&digest(k)).cloned());
		Err(own_out.chain(held.rotation.iter().filter_map(|(_, out)| out.clone())).collect())
	}

	pub(crate) fn sit_out(&self, key: &str, why: Spent) {
		let mut held = self.0.lock().expect("never poisoned: no panic while held");
		tracing::warn!(key = %format!("…{}", tail(key)), "Places key spent: {why}");
		match held.rotation.iter_mut().find(|(k, _)| k.as_str() == key) {
			Some((_, out)) => *out = Some(why),
			None => {
				held.own.insert(digest(key), why);
			}
		}
	}

	/// Why `key`, an own key, sits out; `None` once Google takes it again.
	pub(crate) fn out(&self, key: &str) -> Option<Spent> {
		let mut held = self.0.lock().expect("never poisoned: no panic while held");
		let digest = digest(key);
		if held.own.get(&digest).is_some_and(|s| s.back_by(SystemTime::now())) {
			held.own.remove(&digest);
		}
		held.own.get(&digest).cloned()
	}

	/// `key`, an own key, answered: back in at once.
	pub(crate) fn back(&self, key: &str) {
		self.0.lock().expect("never poisoned: no panic while held").own.remove(&digest(key));
	}
}

struct Held {
	rotation: Vec<(Zeroizing<String>, Option<Spent>)>,
	own: HashMap<[u8; 32], Spent>,
}

/// Where a key handed out by [`Keys::next`] is from.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Tier {
	Own,
	Rotation,
}

fn digest(key: &str) -> [u8; 32] {
	Sha256::digest(key.as_bytes()).into()
}

/// A key's last four, what a person tells it by.
pub(crate) fn tail(key: &str) -> &str {
	&key[key.len().saturating_sub(4)..]
}

impl std::fmt::Debug for Keys {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let held = self.0.lock().expect("never poisoned: no panic while held");
		f.debug_struct("Keys")
			.field("rotation", &held.rotation.iter().map(|(_, out)| out).collect::<Vec<_>>())
			.field("own_out", &held.own.len())
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// The body a spent window actually returns, trimmed to the detail it is read for.
	const REFUSAL: &str = r#"{"error":{"code":429,"status":"RESOURCE_EXHAUSTED","details":[
		{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"RATE_LIMIT_EXCEEDED","metadata":{
			"window_start_time":"1789369200","quota_limit_value":"100",
			"quota_metric":"places.googleapis.com/SearchTextRequest",
			"quota_limit":"SearchTextRequestPerDayPerProject"}}]}}"#;

	/// What an ended trial returns.
	const NO_BILLING: &str = r#"{"error":{"code":403,"status":"PERMISSION_DENIED","details":[
		{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"BILLING_DISABLED","domain":"googleapis.com",
			"metadata":{"consumer":"projects/1","service":"places.googleapis.com"}}]}}"#;

	#[test]
	fn a_refusal_is_read_for_what_it_says() {
		assert_eq!(
			Spent::of(REFUSAL),
			Some(Spent::Quota {
				limit: "SearchTextRequestPerDayPerProject".into(),
				reset: 1789369200 + 86_400
			})
		);
		assert_eq!(Spent::of(NO_BILLING), Some(Spent::Billing));
		assert_eq!(
			Spent::of(&REFUSAL.replace("SearchTextRequestPerDayPerProject", "GetPlaceRequestPerMinutePerProject")).map(|s| s.to_string().contains("GetPlace")),
			Some(true)
		);
		assert!(Spent::of(r#"{"error":{"code":400,"status":"INVALID_ARGUMENT","details":[{"reason":"API_KEY_INVALID"}]}}"#).is_none());
		assert!(Spent::of("not json at all").is_none());
		assert!(Spent::of(r#"{"error":{}}"#).is_none());
		assert!(Spent::of(r#"{"error":{"status":"RESOURCE_EXHAUSTED","details":[{"metadata":{"quota_limit":"PlacesPerMinute"}}]}}"#).is_none());
	}
}
