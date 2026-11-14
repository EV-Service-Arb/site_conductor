//! `nix run .#gen`: the TS the front end would otherwise re-type from the Rust. The output is
//! committed, and the `generated` pre-commit hook re-runs this and re-stages it.

use std::path::Path;

use ev_lib::ts_gen::Ts;
use sa_alerts::Rule;
use sa_auth::{Catalog, SourceKind};
use sa_browsers::view::{In as ViewerIn, Out as ViewerOut};
use sa_core::{
	gbp::OnGoogle,
	prices::{Item, Scope},
};
use sa_panel::{store::browsers::Provider, tokens::Kind};
use sa_server::{
	access::AccessRequestDto,
	alerts::AlertsDto,
	assistant::{AccessDto, ChatDto, ChatsDto, PeopleDto},
	browser::{SetBrowser, StartBrowser},
	connections::ConnectionsDto,
	gmbs::{CandidatesDto, GmailsDto, GoogleCategoriesDto, MapDto, SuggestionsDto, TradesDto},
	signin::Caller,
	tokens::{BalanceChangeDto, BalanceDto, FLOW_KINDS, FlowsDto, LedgerDto, PricesDto, SetPricesDto, TokenHoldersDto, TokensDto, UsageDto},
	trash::{SetSettingsDto, TrashDto},
};

fn main() {
	// a leaked string per item: the union's items are `&'static str`, and this runs once
	let priced_items = || Ts::Union {
		name: "PRICED_ITEMS",
		ty: "PricedItem",
		items: Item::all().map(|i| &*i.to_string().leak()).collect(),
	};
	let scopes = || Ts::Union {
		name: "SCOPES",
		ty: "Scope",
		items: <Scope as strum::VariantArray>::VARIANTS.iter().map(|s| <&str>::from(*s)).collect(),
	};
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src");
	let [permissions, aliases] = Catalog::collect("sa", 0).ts();
	Ts::write(
		&root.join("entities/session/model/generated.ts"),
		&[Ts::types::<Caller>(), Ts::types::<AccessRequestDto>(), permissions, aliases],
	);
	Ts::write(
		&root.join("entities/source/model/generated.ts"),
		&[
			Ts::Union {
				name: "SOURCE_KINDS",
				ty: "SourceKind",
				items: SourceKind::ALL.map(SourceKind::as_str).into(),
			},
			Ts::Union {
				name: "KEYED_SOURCE_KINDS",
				ty: "KeyedSourceKind",
				items: SourceKind::ALL.into_iter().filter(|k| k.keyed()).map(SourceKind::as_str).collect(),
			},
		],
	);
	Ts::write(
		&root.join("entities/gmb/model/generated.ts"),
		&[
			Ts::types::<(
				GmailsDto,
				(CandidatesDto, SuggestionsDto),
				MapDto,
				(TradesDto, GoogleCategoriesDto),
				OnGoogle,
				(ConnectionsDto, TrashDto, SetSettingsDto),
				SetBrowser,
				StartBrowser,
				ViewerIn,
				ViewerOut,
			)>(),
			Ts::Union {
				name: "DEVICES",
				ty: "Device",
				items: <sa_browsers::Device as strum::VariantArray>::VARIANTS.iter().map(|d| <&str>::from(*d)).collect(),
			},
			Ts::Union {
				name: "PROVIDERS",
				ty: "Provider",
				items: <Provider as strum::VariantArray>::VARIANTS.iter().map(|p| <&str>::from(*p)).collect(),
			},
			priced_items(),
			scopes(),
		],
	);
	Ts::write(
		&root.join("entities/tokens/model/generated.ts"),
		&[
			Ts::types::<(TokensDto, LedgerDto, UsageDto, TokenHoldersDto, BalanceChangeDto, BalanceDto, (PricesDto, SetPricesDto, FlowsDto))>(),
			priced_items(),
			scopes(),
			Ts::Union {
				name: "FLOW_KINDS",
				ty: "FlowKind",
				items: FLOW_KINDS.into(),
			},
			Ts::Union {
				name: "LEDGER_KINDS",
				ty: "LedgerKind",
				items: Kind::ALL.map(Kind::as_str).into(),
			},
		],
	);
	Ts::write(
		&root.join("entities/alerts/model/generated.ts"),
		&[
			Ts::types::<AlertsDto>(),
			Ts::Union {
				name: "RULES",
				ty: "Rule",
				items: Rule::ALL.map(Rule::as_str).into(),
			},
		],
	);
	Ts::write(&root.join("entities/assistant/model/generated.ts"), &[Ts::types::<(ChatsDto, ChatDto, AccessDto, PeopleDto)>()]);
}
