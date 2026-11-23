//! Google billing accounts, each declared once with the slots we recognize. Free units, credits and
//! the money kept are per billing account, so every key bills on the account it is listed under.

use std::ops::Deref;

/// One Google billing account, as site_conductor's `nix/google.nix` lists it.
// r[impl google.keys-on-accounts]
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Account {
	/// Stable: stored with each use and reading.
	pub name: String,
	/// `None` until its billing account is open and exported: nothing of ours uses it, and the guard has nothing to hold.
	pub export: Option<Export>,
	/// Each credit's balance, as the console read it.
	pub credits: Vec<sa_core::google::Credit>,
	pub provides: Provides,
}

impl Account {
	/// What the guard holds it to and its bill is read by, under `hold`; `None` without an export.
	pub fn held(&self, hold: sa_core::google::Hold) -> Option<sa_core::google::Held> {
		let export = self.export.as_ref()?;
		Some(sa_core::google::Held {
			name: self.name.clone(),
			table: export.table.clone(),
			since: export.since,
			credits: self.credits.clone(),
			hold,
		})
	}
}

/// An account's Cloud Billing export.
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Export {
	/// `project.dataset.table`.
	pub table: String,
	/// The JSON key of a service account that may query `table`.
	pub reader: String,
	/// Usage before this day is not drawn.
	pub since: jiff::civil::Date,
}

/// What an account's keys are for.
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Provides {
	/// Places, Geocoding.
	pub places: Option<Key>,
	/// Maps JS, referrer-restricted.
	pub browser: Option<Key>,
}

#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Key {
	pub key: String,
}

impl Deref for Key {
	type Target = str;

	fn deref(&self) -> &str {
		&self.key
	}
}

impl std::fmt::Debug for Account {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let tail = |k: Option<&str>| k.map(|k| format!("…{}", crate::keys::tail(k)));
		f.debug_struct("Account")
			.field("name", &self.name)
			.field("export", &self.export.as_ref().map(|e| &e.table))
			.field("places", &tail(self.provides.places.as_deref()))
			.field("browser", &tail(self.provides.browser.as_deref()))
			.finish_non_exhaustive()
	}
}

/// Every account, in the order ties are broken in, and the slices their holds are per.
#[derive(Clone, Debug)]
pub struct Accounts {
	accounts: Vec<Account>,
	slices: Vec<sa_core::google::Slice>,
	drawn: Vec<sa_core::google::Drawn>,
}

impl Accounts {
	pub fn new(accounts: Vec<Account>, slices: Vec<sa_core::google::Slice>) -> Self {
		let drawn = sa_core::google::drawn(&slices).unwrap_or_else(|e| panic!("the Google accounts' slices: {}", e.0));
		assert!(
			accounts.is_empty() || drawn.iter().any(|s| s.name == sa_core::meters::SLICE),
			"no slice is {:?}, which every meter falls in",
			sa_core::meters::SLICE
		);
		for (i, a) in accounts.iter().enumerate() {
			assert!(!a.name.is_empty(), "a Google account without a name");
			let earlier = &accounts[..i];
			assert!(earlier.iter().all(|e| e.name != a.name), "the Google account {} is listed twice", a.name);
			for key in [a.provides.places.as_deref(), a.provides.browser.as_deref()].into_iter().flatten() {
				assert!(
					earlier
						.iter()
						.flat_map(|e| [e.provides.places.as_deref(), e.provides.browser.as_deref()])
						.flatten()
						.all(|k| k != key),
					"{}'s key …{} is another account's too",
					a.name,
					crate::keys::tail(key)
				);
			}
		}
		Self { accounts, slices, drawn }
	}

	pub fn get(&self, name: &str) -> Option<&Account> {
		self.accounts.iter().find(|a| a.name == name)
	}

	/// Compiled, in their order.
	pub fn slices(&self) -> &[sa_core::google::Drawn] {
		&self.drawn
	}

	/// As configured: what a definition carries.
	pub fn defined_slices(&self) -> &[sa_core::google::Slice] {
		&self.slices
	}
}

impl Deref for Accounts {
	type Target = [Account];

	fn deref(&self) -> &[Account] {
		&self.accounts
	}
}
