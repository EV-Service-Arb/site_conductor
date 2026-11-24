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
		/// The category whose window is open: this gcid, on `country`'s tab.
		trade: Option<String>,
		country: Option<Country>,
		/// The brand whose window is open.
		brand: Option<i64>,
		/// The gmail whose window is open.
		account: Option<i64>,
		/// The open window's tab.
		tab: Option<Tab>,
		/// The GBP's Browser tab: the step of Google's form it is on, 1 to 8.
		step: Option<u8>,
		/// The GBP's Log tab: the moment it is scrolled to.
		log_at: Option<String>,
		/// The categories' window.
		categories: bool,
		/// The library's window: a listing's id; empty, the list.
		library: Option<String>,
		/// The trash's window.
		trash: bool,
		/// The Add a GBP panel, with what it starts from.
		add: Option<Add>,
		/// The review kit floating over the map: its slug; empty, the list (or, with `kit_edit`, a new one).
		kit: Option<String>,
		/// The kit's GBP open.
		kit_gmb: Option<i64>,
		/// The kit's pool open: `<gcid>.<country>`.
		kit_pool: Option<String>,
		/// The kit's editor, its area drawn on the map.
		kit_edit: bool,
		/// Where the map looks: latitude, longitude, zoom.
		at: Option<(f64, f64, f64)>,
		/// The schematic base map, not the satellite.
		schematic: bool,
		/// The study drawn: `<gcid>.<country>`, the caller's category's head; `v<id>`, a category
		/// version; `s<sha>`, a published study; empty, none picked whatever the scope.
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
	#[view("/user/account", key = 'y')]
	Account {
		/// One of concierge's account sections.
		section: Option<String>,
	},
	/// Source keys and the sites they connect.
	#[view("/admin/sources", needs = Sources::Manage, key = 'c')]
	Sources,
	/// The caller's tokens: balance, ledger, what they used.
	#[view("/user/tokens", key = 't')]
	Tokens,
	/// Everyone's tokens, prices and flows.
	#[view("/admin/tokens", needs = Tokens::Grant, key = 'a')]
	AdminTokens,
	/// What the caller's actions cost.
	#[view("/user/usage", key = 'u')]
	Usage,
	/// Everyone, to act as; and the way back while acting.
	#[view("/admin/members", needs = Members::ActAs, key = 'm')]
	Members,
	/// The caller's wallet.
	#[view("/user/wallet", needs = Own::Payments, key = 'w')]
	Wallet,
	/// The caller's alerts, newest first.
	#[view("/user/alerts", key = 'e')]
	Alerts {
		/// The one opened.
		open: Option<i64>,
		tier: Option<sa_alerts::Tier>,
		family: Option<sa_alerts::Family>,
		/// The unread alone.
		unread: bool,
	},
	/// Where the caller's alerts go, when, and which count as what.
	#[view("/user/alerts/settings")]
	AlertSettings {
		/// A mailed link's token: the address it proves.
		confirm: Option<String>,
	},
	/// Asking for a permission the caller lacks, then on to where they were going.
	#[view("/access")]
	Access {
		need: String,
		/// A path on this origin.
		r#continue: String,
	},
	/// The caller's review kits.
	#[view("/reviews/kit", key = 'k')]
	Kits,
	/// A review kit: what it collected, its GBPs, its pools; on a phone, its GBPs nearest first.
	#[view("/reviews/kit/{slug}")]
	Kit { slug: String },
	/// A kit's GBP: its QR's target, drafts, photos and tally.
	#[view("/reviews/kit/{slug}/{gmb}")]
	KitGmb { slug: String, gmb: i64 },
	/// A kit's GBP's QR, full screen, for the reviewer; a swipe goes through the kit.
	#[view("/reviews/kit/{slug}/{gmb}/qr")]
	KitQr { slug: String, gmb: i64 },
	/// A category's drafts and photos, from a kit.
	#[view("/reviews/kit/{slug}/pool/{trade}/{country}")]
	KitPool { slug: String, trade: String, country: Country },
	/// A GBP's window as its own page.
	#[view("/gbps/{gmb}")]
	Gbp {
		gmb: i64,
		tab: Option<Tab>,
		step: Option<u8>,
		log_at: Option<String>,
	},
	/// A gmail's window as its own page.
	#[view("/gmail/{account}")]
	Gmail { account: i64, tab: Option<Tab> },
	/// Every category of the caller's, as a page.
	#[view("/gbps/categories")]
	Categories,
	/// A category's window as its own page: a gcid, on `country`'s tab.
	#[view("/gbps/categories/{trade}")]
	Category { trade: String, country: Option<Country> },
	/// The library's window as its own page, on a listing.
	#[view("/gbps/library")]
	Library { listing: Option<i64> },
	/// A brand's window as its own page.
	#[view("/brand/{brand}")]
	Brand { brand: i64 },
	/// What the caller removed, as a page.
	#[view("/gbps/trash")]
	Trash,
	/// The Add a GBP panel as a page, with what it starts from: a gmail (`account`), a trade.
	#[view("/gbps/add")]
	Adding { account: Option<i64>, trade: Option<String> },
	/// sites' pages, its element's to route.
	#[view("/sites", nest = "sites", key = 's', scope = 'v')]
	Sites { below: String },
	/// The review archive's dashboard.
	#[view("/reviews/archive", nest = "review_archive", key = 'r')]
	ReviewArchive { below: String },
}

/// Beside every page: the outline's scope and selection, the assistant's place.
#[derive(Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
pub struct Outline {
	/// The GBPs selected; empty, being picked.
	gmbs: Option<Vec<i64>>,
	/// The gmails in scope.
	gmail: Option<Vec<i64>>,
	/// The categories in scope: `<gcid|*>.<country|*>`.
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
	/// A gcid.
	trade: Option<String>,
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
	/// Its trade is not the primary category Google gives it.
	NotGooglesPrimary,
	/// Switched off.
	Off,
}

#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
	Sidebar,
	Floating,
}
