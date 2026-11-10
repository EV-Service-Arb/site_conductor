//! A managing gmail's antidetect browser (docs/ARCHITECTURE.md, Browsers): the devices it poses
//! as, and what the conductor hands the runner (the `sa_browsers` binary) to launch one with.
//!
//! ```text
//! POST   /profiles/{gmail}/launch  Launch → 204 once its devtools answer; 409 running for another
//!                                  `run`; 503 BROWSERS_MAX running
//! POST   /profiles/{gmail}/stop    → 204
//! DELETE /profiles/{gmail}         → 204: stopped, its session's directory gone
//! GET    /profiles                 Profiles: what runs
//! GET    /profiles/{gmail}/cdp     WebSocket: the browser's own devtools socket, passed through
//! ```
//! Every request carries the conductor's assertion (`sa_auth`, audience `browsers`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The Chrome major of the Orbita the runner runs (the flake's `orbita`); every device's UA says it.
pub const CHROME: &str = "151";

/// A phone a browser poses as, from emulate_phone_cam's measured devices: a fingerprint is
/// measured, never written.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, strum::EnumString, strum::IntoStaticStr, strum::VariantArray)]
pub enum Device {
	#[serde(rename = "pixel_7")]
	#[strum(serialize = "pixel_7")]
	Pixel7,
	#[serde(rename = "galaxy_s23")]
	#[strum(serialize = "galaxy_s23")]
	GalaxyS23,
	/// Captured from the phone itself (emulate_phone_cam `captures/usb-24117RN76E.*`).
	#[serde(rename = "redmi_note_14")]
	#[strum(serialize = "redmi_note_14")]
	RedmiNote14,
}

/// What a page reads off a [`Device`].
pub struct Hardware {
	/// Build.MODEL; only UA-CH says it, and Orbita sends none.
	pub model: &'static str,
	pub gpu_vendor: &'static str,
	pub gpu_renderer: &'static str,
	/// CSS px.
	pub screen: (u32, u32),
	pub dpr: f64,
	pub cores: u32,
	/// GB, as `navigator.deviceMemory` caps it.
	pub memory: u32,
	pub touch: u32,
	/// UA-CH `platformVersion`.
	pub android: &'static str,
}

impl Device {
	pub fn hardware(self) -> Hardware {
		match self {
			Self::Pixel7 => Hardware {
				model: "Pixel 7",
				gpu_vendor: "ARM",
				gpu_renderer: "Mali-G710",
				screen: (412, 915),
				dpr: 2.625,
				cores: 8,
				memory: 8,
				touch: 5,
				android: "16.0.0",
			},
			Self::GalaxyS23 => Hardware {
				model: "SM-S911B",
				gpu_vendor: "Qualcomm",
				gpu_renderer: "Adreno (TM) 740",
				screen: (360, 780),
				dpr: 3.0,
				cores: 8,
				memory: 8,
				touch: 5,
				android: "16.0.0",
			},
			Self::RedmiNote14 => Hardware {
				model: "24117RN76E",
				gpu_vendor: "Google Inc. (ARM)",
				gpu_renderer: "ANGLE (ARM, Mali-G57 MC2, OpenGL ES 3.2)",
				screen: (384, 854),
				dpr: 2.8125,
				cores: 8,
				memory: 8,
				touch: 5,
				android: "16.0.0",
			},
		}
	}

	/// Chrome on Android has sent this frozen string since the UA reduction: model and OS version
	/// go out only as client hints.
	pub fn user_agent() -> String {
		format!("Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{CHROME}.0.0.0 Mobile Safari/537.36")
	}
}

/// A launch of a gmail's browser.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Launch {
	/// What this run is for (a GBP's point, or the gmail alone): a launch for another while it
	/// runs is refused, never folded into it.
	pub run: String,
	pub browser: Browser,
}

/// The browser, by provider: each answers devtools, the one seam the conductor drives.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum Browser {
	/// GoLogin's Chromium.
	Orbita {
		/// Merged into the session's `Default/Preferences` (RFC 7386).
		preferences: Value,
		/// Its `orbita.config`, the profile token among it: read once at start.
		config: Value,
		/// `scheme://host:port`; its credentials are the preferences'.
		proxy: String,
		zone: String,
		/// BCP 47.
		locale: String,
		/// CSS px.
		screen: (u32, u32),
	},
}

/// `GET /profiles`.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profiles {
	pub running: Vec<Running>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Running {
	pub gmail: i64,
	pub run: String,
}

/// The UI locale Orbita shows for `locale` (BCP 47), by Chrome's own fallback: the locale, else
/// its language, else en-GB for English outside the US. `None`: Orbita ships none for it.
pub fn ui_locale(locale: &str) -> Option<&'static str> {
	const PAKS: [&str; 54] = [
		"am", "ar", "bg", "bn", "ca", "cs", "da", "de", "el", "en-GB", "en-US", "es", "es-419", "et", "fa", "fi", "fil", "fr", "fr-CA", "gu", "he", "hi", "hr", "hu", "id", "it", "ja", "kn",
		"ko", "lt", "lv", "ml", "mr", "ms", "nb", "nl", "pl", "pt-BR", "pt-PT", "ro", "ru", "sk", "sl", "sr", "sv", "sw", "ta", "te", "th", "tr", "uk", "vi", "zh-CN", "zh-TW",
	];
	let lang = locale.split('-').next().expect("split yields at least one");
	let english = (lang == "en").then_some("en-GB");
	[Some(locale), Some(lang), english].into_iter().flatten().find_map(|l| PAKS.iter().find(|p| **p == l).copied())
}
