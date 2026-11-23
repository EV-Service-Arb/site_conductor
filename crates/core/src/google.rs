//! A Google billing account's definition, less its secrets (nix/google.nix, docs/ARCHITECTURE.md,
//! Google accounts): what the conductor reads its bill by and the guard holds it to, alike.

use std::collections::BTreeMap;

use jiff::{Timestamp, civil::Date};
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::Invalid;

/// Credit types that draw a balance down; every other type is a discount.
pub const BALANCES: &str = "('FREE_TRIAL', 'PROMOTION')";

/// A credit's balance, as the console read it: Google gives none out.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Credit {
	/// Google's, once the export first shows it drawn.
	pub id: Option<String>,
	pub name: String,
	/// When the console showed `left`.
	pub read: Timestamp,
	pub left: f64,
	pub ends: Date,
}

impl Credit {
	/// What it has left `on`: the console's reading less what the export drew on it since
	/// (`drawn`, negative); none once `on` reaches its end.
	pub fn left_on(&self, drawn: f64, on: Date) -> f64 {
		if on >= self.ends { 0.0 } else { self.left + drawn }
	}
}

/// What usage is for: the first slice whose `service` (and `sku`, where given) regexes match draws it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Slice {
	pub name: String,
	pub service: String,
	pub sku: Option<String>,
}

/// Real money a slice may spend past free units and credits, in the account's currency.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Allow {
	/// A Pacific day: Google's quota day.
	pub day: f64,
	/// A Pacific month: Google's billing month.
	pub month: f64,
}

/// A [`Slice`], its regexes compiled.
#[derive(Clone, Debug)]
pub struct Drawn {
	pub name: String,
	pub service: Regex,
	pub sku: Option<Regex>,
}

impl Drawn {
	pub fn draws(&self, service: &str, sku: &str) -> bool {
		self.service.is_match(service) && self.sku.as_ref().is_none_or(|k| k.is_match(sku))
	}
}

/// `slices` compiled, refused when a regex does not compile.
pub fn drawn(slices: &[Slice]) -> Result<Vec<Drawn>, Invalid> {
	let regex = |s: &str| Regex::new(s).map_err(|e| Invalid::new(format!("a slice's regex {s:?}: {e}")));
	slices
		.iter()
		.map(|s| {
			Ok(Drawn {
				name: s.name.clone(),
				service: regex(&s.service)?,
				sku: s.sku.as_deref().map(regex).transpose()?,
			})
		})
		.collect()
}

/// What an admin sets an account to: its real money per slice, and the APIs the guard leaves alone.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Hold {
	/// Every slice's.
	pub allow: BTreeMap<String, Allow>,
	/// APIs on its billed projects the guard leaves alone: spending with no lever but the backstop.
	pub unguarded: Vec<String>,
}

impl Hold {
	/// Refused when no slice is [`crate::meters::SLICE`], or `allow` misses a slice, names one that is not, or holds a
	/// day past its month or no amount.
	pub fn check(&self, slices: &[Drawn]) -> Result<(), Invalid> {
		if !slices.iter().any(|s| s.name == crate::meters::SLICE) {
			return Err(Invalid::new(format!("no slice is {:?}, which every meter falls in", crate::meters::SLICE)));
		}
		for s in slices {
			if !self.allow.contains_key(&s.name) {
				return Err(Invalid::new(format!("`allow` names no {:?}: each slice's day and month, 0 for none", s.name)));
			}
		}
		for (k, a) in &self.allow {
			if !slices.iter().any(|s| &s.name == k) {
				return Err(Invalid::new(format!("`allow` names {k:?}, which is no slice")));
			}
			if !(a.day.is_finite() && a.month.is_finite() && 0.0 <= a.day && a.day <= a.month) {
				return Err(Invalid::new(format!("`allow.{k}` is {a:?}: 0 <= day <= month")));
			}
		}
		Ok(())
	}
}

/// The slice usage of `service`'s `sku` falls in: the first drawing it, else the service itself.
pub fn slice_of(slices: &[Drawn], service: &str, sku: &str) -> String {
	slices.iter().find(|s| s.draws(service, sku)).map_or_else(|| service.to_owned(), |s| s.name.clone())
}

/// An account as the guard holds it and the conductor reads its bill: no key, no reader.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Held {
	pub name: String,
	/// Its export, `project.dataset.table`.
	pub table: String,
	/// Usage before this day is not drawn.
	pub since: Date,
	pub credits: Vec<Credit>,
	#[serde(flatten)]
	pub hold: Hold,
}

/// What the conductor publishes beside an account's export for the guard to hold it to.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Definition {
	pub held: Held,
	pub slices: Vec<Slice>,
}

impl Definition {
	/// Hashed: the conductor's and the guard's tell whether they hold the same one.
	pub fn digest(&self) -> String {
		let canonical = serde_json::to_string(self).expect("plain data serializes");
		hex::encode(Sha256::digest(canonical))[..16].to_owned()
	}
}
