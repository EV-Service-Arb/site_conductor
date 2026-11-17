//! The viewer's socket: what the operator's window and the browser say to each other. The
//! conductor speaks it for a devtools browser (its relay), the runner for a cloud phone (the
//! conductor passing it through).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// The most a frame from the viewer holds, in bytes.
pub const FRAME_MAX: usize = 4096;

/// The most characters one [`In::Text`] carries.
pub const TEXT_MAX: usize = 600;

// JSON writes a character in 6 bytes at most (`\u001f`); the rest of the frame is its tag
const _: () = assert!(TEXT_MAX * 6 + 64 <= FRAME_MAX, "a text frame of TEXT_MAX characters fits a frame");

/// Why the conductor closes a viewer's or a live socket: its close frame's code.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr, strum::VariantArray)]
#[repr(u16)]
pub enum Close {
	/// The session ended: sign in again.
	SignedOut = 4401,
	/// Nothing runs for the gmail: start it first.
	NotRunning = 4404,
	/// Another viewer of the gmail took over.
	Replaced = 4409,
	/// The browser stopped, or its devtools went.
	Stopped = 4410,
	/// RFC 6455's normal closure: the viewer left, or did not keep up.
	Normal = 1000,
	/// RFC 6455's "going away": the server is stopping.
	GoingAway = 1001,
	/// RFC 6455's "unsupported data": a binary frame.
	TextOnly = 1003,
	/// RFC 6455's "invalid frame payload data": not a frame of the viewer's.
	NotAFrame = 1007,
}

impl Close {
	pub fn code(self) -> u16 {
		self as u16
	}
}

impl JsonSchema for Close {
	fn schema_name() -> Cow<'static, str> {
		"BrowserClose".into()
	}

	fn json_schema(_: &mut SchemaGenerator) -> Schema {
		use strum::VariantArray;
		let codes: Vec<u16> = Self::VARIANTS.iter().map(|c| c.code()).collect();
		let names: Vec<&str> = Self::VARIANTS.iter().map(|&c| c.into()).collect();
		json_schema!({
			"type": "integer",
			"description": "Why the conductor closed the socket: its close frame's code.",
			"enum": codes,
			"x-enum-varnames": names,
		})
	}
}

/// A frame to the viewer.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[schemars(rename = "BrowserOut")]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Out {
	/// The screen: an image, base64, of `width` × `height`, the space the viewer's points are in
	/// (a page's CSS px, a phone's screen px).
	Frame {
		data: String,
		width: f64,
		height: f64,
	},
	/// A phone's: none.
	Tabs {
		tabs: Vec<Tab>,
		active: Option<String>,
	},
	/// A page's `alert`, `confirm`, `prompt` or `beforeunload`, waiting on an answer.
	Dialog {
		kind: String,
		message: String,
		default: Option<String>,
	},
	DialogClosed,
	/// What the browser refused of the viewer's.
	Error {
		message: String,
	},
}

#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[schemars(rename = "BrowserTab")]
pub struct Tab {
	pub id: String,
	pub title: String,
	pub url: String,
}

/// A frame from the viewer; at most [`FRAME_MAX`] bytes. Points are in the last frame's space.
#[derive(Clone, Debug, Deserialize, JsonSchema, Serialize)]
#[schemars(rename = "BrowserIn")]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum In {
	/// A finger: the browser is a phone.
	Touch {
		phase: TouchPhase,
		x: f64,
		y: f64,
	},
	/// A wheel's turn, swiped.
	Wheel {
		x: f64,
		y: f64,
		dx: f64,
		dy: f64,
	},
	/// A key that types nothing: Enter, Backspace, arrows, Tab, Escape, Delete, Home, End; `ctrl`
	/// with `a`, select all.
	Key {
		key: String,
		ctrl: bool,
	},
	/// Text, as a phone's keyboard commits it, into the focused field.
	Text {
		#[schemars(length(max = TEXT_MAX))]
		text: String,
	},
	Navigate {
		url: String,
	},
	/// -1 back, 1 forward.
	History {
		delta: i32,
	},
	Activate {
		id: String,
	},
	NewTab,
	CloseTab {
		id: String,
	},
	/// The open dialog answered; `text` a prompt's.
	Dialog {
		accept: bool,
		text: Option<String>,
	},
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TouchPhase {
	Start,
	Move,
	End,
}

/// A typed address as a phone's address bar takes it: http(s) only, `https://` when it names none.
pub fn web_url(typed: &str) -> Option<String> {
	let typed = typed.trim();
	let url = url::Url::parse(typed).or_else(|_| url::Url::parse(&format!("https://{typed}"))).ok()?;
	matches!(url.scheme(), "http" | "https").then(|| url.to_string())
}
