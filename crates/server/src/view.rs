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
		/// The brand whose window is open.
		brand: Option<i64>,
		/// The gmail whose window is open.
		account: Option<i64>,
		/// The open window's tab.
		tab: Option<Tab>,
		/// The GBP's Browser tab: the step of Google's form it is on, 1 to 8.
		step: Option<u8>,
		/// The categories' window.
		categories: bool,
		/// The trash's window.
		trash: bool,
		/// The Add a GBP panel, with what it starts from.
		add: Option<Add>,
		/// The review kit floating over the map: its slug; empty, the list (or, with `kit_edit`, a new one).
		kit: Option<String>,
		/// The kit's GBP open.
		kit_gmb: Option<i64>,
		/// The kit's pool open: `<trade id>.<country>`.
		kit_pool: Option<String>,
		/// The kit's editor, its area drawn on the map.
		kit_edit: bool,
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
	#[view("/admin/tokens", needs = Tokens::Grant, key = 'a')]
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
	/// The caller's review kits.
	#[view("/kit", key = 'k')]
	Kits,
	/// A review kit: what it collected, its GBPs, its pools; on a phone, its GBPs nearest first.
	#[view("/kit/{slug}")]
	Kit { slug: String },
	/// A kit's GBP: its QR's target, drafts, photos and tally.
	#[view("/kit/{slug}/{gmb}")]
	KitGmb { slug: String, gmb: i64 },
	/// A kit's GBP's QR, full screen, for the reviewer; a swipe goes through the kit.
	#[view("/kit/{slug}/{gmb}/qr")]
	KitQr { slug: String, gmb: i64 },
	/// A category's drafts and photos, from a kit.
	#[view("/kit/{slug}/pool/{trade}/{country}")]
	KitPool { slug: String, trade: i64, country: Country },
	/// A GBP's window as its own page.
	#[view("/gbp/{gmb}")]
	Gbp { gmb: i64, tab: Option<Tab>, step: Option<u8> },
	/// A gmail's window as its own page.
	#[view("/gmail/{account}")]
	Gmail { account: i64, tab: Option<Tab> },
	/// Every trade by country, as a page.
	#[view("/categories")]
	Categories,
	/// A trade's window as its own page, on `country`'s tab.
	#[view("/category/{trade}")]
	Category { trade: i64, country: Option<Country> },
	/// A brand's window as its own page.
	#[view("/brand/{brand}")]
	Brand { brand: i64 },
	/// What the caller removed, as a page.
	#[view("/trash")]
	Trash,
	/// The Add a GBP panel as a page, with what it starts from: a gmail (`account`), a trade.
	#[view("/add")]
	Adding { account: Option<i64>, trade: Option<i64> },
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
	/// The brands in scope.
	brands: Option<Vec<i64>>,
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
	/// A GBP as Google shows it, and what the conductor knows of it.
	Profile,
	/// What happened to a GBP.
	Log,
	/// A draft's launch onto Google.
	Launch,
	/// The gmail's browser.
	Browser,
	/// A gmail's inbox and Maps key; a GBP's verification kit and what it is connected to.
	Connections,
	/// A GBP's output: leads, reviews, its changes.
	Stats,
	/// Where a GBP ranks for its category's keywords.
	Ranking,
	/// A GBP's reviews as review_archive tracks them.
	Reviews,
	/// A verifying GBP's road to Verified.
	Verification,
	/// A suspended GBP's appeal.
	Appeal,
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
