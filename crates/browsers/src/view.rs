//! The viewer's socket: what the operator's window and the browser say to each other. The
//! conductor speaks it for a devtools browser (its relay), the runner for a cloud phone (the
//! conductor passing it through).

use serde::{Deserialize, Serialize};

/// A frame to the viewer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BrowserOut"))]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Tab {
	pub id: String,
	pub title: String,
	pub url: String,
}

/// A frame from the viewer; at most 4 KiB. Points are in the last frame's space.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "BrowserIn"))]
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

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
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
