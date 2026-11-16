//! Google billing accounts, each declared once with the slots we recognize. Free units, credits and
//! the money kept are per billing account, so every key bills on the account it is listed under.

use std::ops::Deref;

/// One Google billing account.
#[derive(Clone)]
#[cfg_attr(feature = "settings", derive(v_utils::macros::MyConfigPrimitives, v_utils::macros::ConfigJsonSchema))]
pub struct Account {
	/// Stable: stored with each use and reading.
	pub name: String,
	/// Its Cloud Billing export, `project.dataset.table`.
	pub export: String,
	/// The JSON key of a service account that may query `export`.
	pub reader: String,
	/// Its server key (Places, Geocoding).
	pub places: Option<String>,
	/// Its Maps JS key, referrer-restricted.
	pub browser: Option<String>,
	/// Money a month past its free units and credits, in its currency; 0, free units only.
	pub allow: f64,
}

impl std::fmt::Debug for Account {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let tail = |k: &Option<String>| k.as_deref().map(|k| format!("…{}", crate::keys::tail(k)));
		f.debug_struct("Account")
			.field("name", &self.name)
			.field("export", &self.export)
			.field("places", &tail(&self.places))
			.field("browser", &tail(&self.browser))
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
			assert!(a.allow >= 0.0 && a.allow.is_finite(), "{}'s allow is {}", a.name, a.allow);
			let earlier = &accounts[..i];
			assert!(earlier.iter().all(|e| e.name != a.name), "the Google account {} is listed twice", a.name);
			for key in [&a.places, &a.browser].into_iter().flatten() {
				assert!(
					earlier.iter().flat_map(|e| [&e.places, &e.browser]).flatten().all(|k| k != key),
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
