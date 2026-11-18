//! Where a person can be on the conductor's pages: one enum, a variant per view, the services'
//! nested under their pages (`ev_lib::views`). The front end's routes, query keys and nav letters,
//! the assistant's `open`, and which path is the page (`web.rs`) all read it.

use std::collections::BTreeMap;

use ev_lib::{api::JsonSchema, views::Views};
use sa_auth::{Members, Own, Sources, Tokens};
use sa_core::gbp::Country;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, PartialEq, Views)]
#[views(carried = Outline)]
#[expect(clippy::large_enum_variant, reason = "a parsed route, one per request; Views derives over named fields, which a Box would hide")]
pub enum View {
	/// The map of the caller's GBPs, and every window over it.
	#[view("/", key = 'h')]
	Home {
		/// The GBP whose window is open.
		gmb: Option<i64>,
		/// The trade whose window is open, on `country`'s tab.
		trade: Option<i64>,
		country: Option<Country>,
		/// The gmail whose window is open.
		account: Option<i64>,
		/// The open window's tab.
		tab: Option<Tab>,
		/// The categories' window.
		categories: bool,
		/// The trash's window.
		trash: bool,
		/// The Add a GBP panel, with what it starts from.
		add: Option<Add>,
		/// The review kit's area: empty, one being drawn.
		kit: Option<String>,
		/// Where the map looks: latitude, longitude, zoom.
		at: Option<(f64, f64, f64)>,
		/// The schematic base map, not the satellite.
		schematic: bool,
		/// The study drawn: `<trade id>.<country>`; empty, none picked whatever the scope.
		study: Option<String>,
		/// The heat layer drawn.
		heat: Option<String>,
		/// The area a study is framed by; empty, one being drawn.
		area: Option<String>,
		/// The pressure field, at this radius in metres.
		pressure: Option<f64>,
		/// The competitors drawn.
		competitors: bool,
		/// Imputed demand left out.
		hide_imputed: bool,
		/// Tiers whose weight or visibility was moved off the study's own.
		tiers: Option<BTreeMap<String, Tier>>,
		/// The heat's opacity, 0 to 1.
		opacity: Option<f64>,
	},
	/// The assistant, full screen.
	#[view("/assistant")]
	Assistant { chat: Option<Uuid> },
	/// The caller's account: their sign-in, their Claude, what they spend.
	#[view("/account", key = 'y')]
	Account {
		/// One of concierge's account sections.
		section: Option<String>,
	},
	/// Source keys and the sites they connect.
	#[view("/sources", needs = Sources::Manage, key = 'c')]
	Sources,
	/// The caller's tokens: balance, ledger, what they used.
	#[view("/tokens", key = 't')]
	Tokens,
	/// Everyone's tokens, prices and flows.
	#[view("/admin/tokens", needs = Tokens::Grant, key = 'k')]
	AdminTokens,
	/// What the caller's actions cost.
	#[view("/usage", key = 'u')]
	Usage,
	/// Everyone, to act as; and the way back while acting.
	#[view("/members", needs = Members::ActAs, key = 'm')]
	Members,
	/// The caller's wallet.
	#[view("/wallet", needs = Own::Payments, key = 'w')]
	Wallet,
	/// Where the caller's alerts go, and which.
	#[view("/alerts", key = 'e')]
	Alerts,
	/// Asking for a permission the caller lacks, then on to where they were going.
	#[view("/access")]
	Access {
		need: String,
		/// A path on this origin.
		r#continue: String,
	},
	/// The review kit on the operator's phone: an area's GBPs and their QRs.
	#[view("/{area}")]
	Area { area: String },
	/// sites' pages, its element's to route.
	#[view("/sites", nest = "sites", key = 's', scope = 'v')]
	Sites { below: String },
	/// The review archive's dashboard.
	#[view("/review_archive", nest = "review_archive", key = 'r')]
	ReviewArchive { below: String },
}

/// Beside every page: the outline's scope and selection, the assistant's place.
#[derive(Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Outline {
	/// The GBPs selected; empty, being picked.
	gmbs: Option<Vec<i64>>,
	/// The gmails in scope.
	gmail: Option<Vec<i64>>,
	/// The categories in scope: `<trade id|*>.<country|*>`.
	category: Option<Vec<String>>,
	/// What the outline's list is filtered by.
	find: Option<String>,
	/// The GBPs needing this, first.
	issue: Option<Issue>,
	/// Where the assistant is shown; without it, as this browser last left it.
	assistant: Option<Mode>,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tab {
	/// A draft's launch onto Google.
	Launch,
	/// The gmail's browser.
	Browser,
	/// A verified GBP's review kit.
	Kit,
	/// A gmail's inbox and Maps key.
	Connections,
}

/// What the Add a GBP panel starts from.
#[derive(Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Add {
	gmail: Option<i64>,
	trade: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Tier {
	weight: f64,
	show: bool,
}

/// What a GBP needs of the operator, most pressing first.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Issue {
	/// A lead waits past its contact time.
	Overdue,
	/// Suspended on Google.
	Suspended,
	/// The archive's walks of it fail.
	Failing,
	/// Not on Google yet.
	Draft,
	/// No point on the map.
	Unlocated,
	/// No site page.
	NoSite,
	/// No category set up.
	NoCategory,
	/// Switched off.
	Off,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
	Sidebar,
	Floating,
}
