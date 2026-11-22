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
	/// APIs of the slice nothing of ours calls: held at no request a day.
	#[serde(default)]
	pub closed: Vec<String>,
}

/// A [`Slice`], its regexes compiled.
#[derive(Clone, Debug)]
pub struct Drawn {
	pub name: String,
	pub service: Regex,
	pub sku: Option<Regex>,
	pub closed: Vec<String>,
}

impl Drawn {
	pub fn draws(&self, service: &str, sku: &str) -> bool {
		self.service.is_match(service) && self.sku.as_ref().is_none_or(|k| k.is_match(sku))
	}
}

/// `slices` compiled, refused when a regex does not compile or an account's `allow` names no slice.
pub fn drawn<'a>(slices: &[Slice], allows: impl IntoIterator<Item = (&'a str, &'a BTreeMap<String, f64>)>) -> Result<Vec<Drawn>, Invalid> {
	let regex = |s: &str| Regex::new(s).map_err(|e| Invalid::new(format!("a slice's regex {s:?}: {e}")));
	let drawn = slices
		.iter()
		.map(|s| {
			Ok(Drawn {
				name: s.name.clone(),
				service: regex(&s.service)?,
				sku: s.sku.as_deref().map(regex).transpose()?,
				closed: s.closed.clone(),
			})
		})
		.collect::<Result<Vec<_>, Invalid>>()?;
	for (account, allow) in allows {
		for k in allow.keys() {
			if !drawn.iter().any(|s| &s.name == k) {
				return Err(Invalid::new(format!("{account}: `allow` names {k:?}, which is no slice")));
			}
		}
	}
	Ok(drawn)
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
	/// Real money a month per slice, in the account's currency; a slice not named, none.
	pub allow: BTreeMap<String, f64>,
}

/// The definition's secret-free part, hashed: two deploys of it (the conductor's and the guard's)
/// tell whether they hold the same one.
pub fn digest(accounts: &[Held], slices: &[Slice]) -> String {
	let canonical = serde_json::to_string(&(accounts, slices)).expect("plain data serializes");
	hex::encode(Sha256::digest(canonical))[..16].to_owned()
}
