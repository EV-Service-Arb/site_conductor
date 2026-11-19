//! Google billing accounts, each declared once with the slots we recognize. Free units, credits and
//! the money kept are per billing account, so every key bills on the account it is listed under.

use std::{collections::BTreeMap, ops::Deref};

/// One Google billing account, as `accounting/google.nix` lists it.
// r[impl google.keys-on-accounts]
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Account {
	/// Stable: stored with each use and reading.
	pub name: String,
	pub export: Export,
	/// Money a month past its free units and credits per slice of accounting's, in its currency; a
	/// slice not named, none.
	pub allow: BTreeMap<String, f64>,
	pub provides: Provides,
}

/// An account's Cloud Billing export.
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Export {
	/// `project.dataset.table`.
	pub table: String,
	/// The JSON key of a service account that may query `table`.
	pub reader: String,
}

/// What an account's keys are for.
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Provides {
	/// Places, Geocoding.
	pub places: Option<Key>,
	/// Maps JS, referrer-restricted.
	pub browser: Option<Key>,
	/// Business Profile APIs; refused (0 QPM) until Google approves the project.
	pub business: Option<Business>,
}

/// The OAuth client a gmail grants `business.manage` to; the grants stay with whoever holds them.
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::ConfigJsonSchema, v_utils::macros::MyConfigPrimitives))]
pub struct Business {
	pub client_id: String,
	pub client_secret: String,
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
			.field("export", &self.export.table)
			.field("places", &tail(self.provides.places.as_deref()))
			.field("browser", &tail(self.provides.browser.as_deref()))
			.field("business", &self.provides.business.as_ref().map(|b| &b.client_id))
			.field("allow", &self.allow)
			.finish_non_exhaustive()
	}
}

/// Every account, in the order ties are broken in.
#[derive(Clone, Debug)]
pub struct Accounts(Vec<Account>);

impl Accounts {
	pub fn new(accounts: Vec<Account>) -> Self {
		for (i, a) in accounts.iter().enumerate() {
			assert!(!a.name.is_empty(), "a Google account without a name");
			for (slice, allow) in &a.allow {
				assert!(*allow >= 0.0, "{}'s allow of {slice} is {allow}", a.name);
				assert!(allow.is_finite(), "{}'s allow of {slice} is {allow}", a.name);
			}
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
		Self(accounts)
	}

	pub fn get(&self, name: &str) -> Option<&Account> {
		self.0.iter().find(|a| a.name == name)
	}
}

impl Deref for Accounts {
	type Target = [Account];

	fn deref(&self) -> &[Account] {
		&self.0
	}
}
