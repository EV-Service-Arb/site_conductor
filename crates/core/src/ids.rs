//! A brand a source key writes for, the shape of a key id, and a site key's origin.

use std::fmt;

use crate::Invalid;

/// [`is_slug`] as a JSON Schema `pattern`.
const SLUG_PATTERN: &str = "^[a-z0-9][a-z0-9_-]{0,63}$";

/// A brand: aquafix, vifnet, … Lowercase slug, 1–64 of `[a-z0-9_-]`, starting alphanumeric.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Deserialize, serde::Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct BrandId(String);

impl BrandId {
	pub fn parse(raw: &str) -> Result<Self, Invalid> {
		if is_slug(raw) {
			Ok(Self(raw.to_owned()))
		} else {
			Err(Invalid::new("brand is not a valid id"))
		}
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl TryFrom<String> for BrandId {
	type Error = Invalid;

	fn try_from(raw: String) -> Result<Self, Invalid> {
		Self::parse(&raw)
	}
}

impl From<BrandId> for String {
	fn from(b: BrandId) -> Self {
		b.0
	}
}

impl schemars::JsonSchema for BrandId {
	fn schema_name() -> std::borrow::Cow<'static, str> {
		"BrandId".into()
	}

	fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
		schemars::json_schema!({ "type": "string", "pattern": SLUG_PATTERN, "description": "A brand: a lowercase slug, 1–64 of `[a-z0-9_-]`, starting alphanumeric." })
	}
}

impl fmt::Display for BrandId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

/// Where a site key's landing answers: `https://<domain>[:port]`, or plain http on loopback (a
/// landing on this machine), spelled exactly as the URL's own origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SiteOrigin(String);

impl SiteOrigin {
	pub fn parse(raw: &str) -> Result<Self, Invalid> {
		let invalid = || Invalid::new(format!("{raw:?} is not a site origin: https://<domain>[:port], nothing after it"));
		let url = url::Url::parse(raw).map_err(|_| invalid())?;
		let shaped = matches!(
			(url.scheme(), url.host()),
			("https", Some(url::Host::Domain(_))) | ("http", Some(url::Host::Domain("localhost") | url::Host::Ipv4(std::net::Ipv4Addr::LOCALHOST)))
		);
		if shaped && url.origin().ascii_serialization() == raw {
			Ok(Self(raw.to_owned()))
		} else {
			Err(invalid())
		}
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

impl fmt::Display for SiteOrigin {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str(&self.0)
	}
}

/// 1–64 of `[a-z0-9_-]`, starting alphanumeric.
pub fn is_slug(s: &str) -> bool {
	(1..=64).contains(&s.len())
		&& s.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
		&& s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn slugs() {
		assert!(BrandId::parse("aquafix").is_ok());
		for bad in ["", "Aquafix", "-x", "a b", &"a".repeat(65)] {
			assert!(BrandId::parse(bad).is_err(), "{bad:?}");
		}
	}

	#[test]
	fn a_brand_is_its_slug_on_the_wire() {
		assert_eq!(serde_json::to_string(&BrandId::parse("aquafix").unwrap()).unwrap(), r#""aquafix""#);
		assert!(serde_json::from_str::<BrandId>(r#""Aquafix""#).is_err());
	}

	#[test]
	fn site_origins() {
		for good in ["https://aquafix.top", "https://vifnet.site:8443", "http://localhost:59081", "http://127.0.0.1:9"] {
			assert_eq!(SiteOrigin::parse(good).map(|o| o.to_string()), Ok(good.to_owned()));
		}
		for bad in [
			"https://aquafix.top/",
			"https://aquafix.top/fr",
			"https://aquafix.top?x",
			"https://Aquafix.top",
			"https://aquafix.top:443",
			"https://u@aquafix.top",
			"https://1.2.3.4",
			"http://aquafix.top",
			"ftp://aquafix.top",
			"aquafix.top",
		] {
			assert!(SiteOrigin::parse(bad).is_err(), "{bad:?}");
		}
	}
}
