//! The conductor's own Grafana dashboards, over its `/api/v1` (`sa_core::dashboards`).

use ev_lib::api::{OpenApi, op_id};
use sa_core::dashboards::{Board, Column, Panel, Query, Select};
use serde_json::{Value, json};

use crate::grafana::CONDUCTOR;

const OUR_BILL: &str = r#"days[bill = "ours" and $type(usd) = "number"]"#;

const RANGE: &[(&str, &str)] = &[("from", "${__from}"), ("to", "${__to}")];

/// Each account's credits.
const CREDITS: Table = (
	r##"$map(accounts, function($a) { $map($a.credits, function($c) { {"account": $a.name, "credit": $c.name, "left": $c.left, "per day": $c.per_day, "runs out": $c.runs_out, "ends": $c.ends, "days left": $c.days_left, "first": $c.binds, "as of": $c.as_of} }) })"##,
	&[
		("account", Column::String),
		("credit", Column::String),
		("left", Column::Number),
		("per day", Column::Number),
		("runs out", Column::String),
		("ends", Column::String),
		("days left", Column::Number),
		("first", Column::String),
		("as of", Column::Timestamp),
	],
);

/// Each slice's allowance and levers, as the guard last held them.
const ALLOWANCES: Table = (
	r##"$map(accounts, function($a) { $map($a.allowances, function($s) { {"account": $a.name, "slice": $s.slice, "allowed": $s.allowed, "real used": $s.real, "headroom": $s.headroom, "is": $s.state, "levers": $join($map($s.levers, function($l) { ($l.project ? $l.project & " " : "") & $l.lever & " = " & $string($l.setting) & " (" & ($l.state ? $l.state : "new") & ")" }), "; "), "guard's tick": $a.guard.tick, "holds this list": $string($a.guard.holds_this_list)} }) })"##,
	&[
		("account", Column::String),
		("slice", Column::String),
		("allowed", Column::Number),
		("real used", Column::Number),
		("headroom", Column::Number),
		("is", Column::String),
		("levers", Column::String),
		("guard's tick", Column::Timestamp),
		("holds this list", Column::String),
	],
);

/// The bill's days summed per slice.
const BY_SLICE: Table = (
	r##"(
  $rows := days;
  [$distinct($rows.slice).(
    $s := $;
    $d := $rows[slice = $s];
    {"slice": $s, "used": $sum($d.used), "from credits": $sum($d.credit), "discounts": $sum($d.discount), "paid": $sum($d.paid)}
  )]
)"##,
	&[
		("slice", Column::String),
		("used", Column::Number),
		("from credits", Column::Number),
		("discounts", Column::Number),
		("paid", Column::Number),
	],
);

/// The bill's days summed per account, slice, service and SKU.
const BY_SKU: Table = (
	r##"(
  $rows := days;
  [$distinct($rows.(account & "|" & slice & "|" & service & "|" & sku)).(
    $k := $;
    $d := $rows[account & "|" & slice & "|" & service & "|" & sku = $k];
    {"account": $d[0].account, "slice": $d[0].slice, "service": $d[0].service, "sku": $d[0].sku, "used": $sum($d.used), "paid": $sum($d.paid)}
  )]
)"##,
	&[
		("account", Column::String),
		("slice", Column::String),
		("service", Column::String),
		("sku", Column::String),
		("used", Column::Number),
		("paid", Column::Number),
	],
);

/// This month's tiers.
const TIERS: Table = (
	r##"$map(tiers, function($t) { {"account": $t.account, "service": $t.service, "sku": $t.sku, "unit": $t.unit, "units": $t.units, "free to": $t.free_to, "until paid": $t.free_to = null ? null : $max([$t.free_to - $t.units, 0]), "paid units": $t.paid_units, "list": $t.list, "free": $t.free} })"##,
	&[
		("account", Column::String),
		("service", Column::String),
		("sku", Column::String),
		("unit", Column::String),
		("units", Column::Number),
		("free to", Column::Number),
		("until paid", Column::Number),
		("paid units", Column::Number),
		("list", Column::Number),
		("free", Column::Number),
	],
);

/// A table's JSONata over its operation's answer, and its columns in their order.
type Table = (&'static str, &'static [(&'static str, Column)]);

/// Each dashboard by its uid.
pub(crate) fn own(doc: &OpenApi) -> [(&'static str, Value); 2] {
	[("sa-tokens", tokens(doc)), ("sa-meters", meters(doc))]
}

fn tokens(doc: &OpenApi) -> Value {
	let board = Board::new(doc, CONDUCTOR).panel(
		Panel {
			id: 1,
			kind: "stat",
			title: "In the range",
			grid: [0, 0, 24, 5],
		},
		in_range(
			op_id(crate::tokens::token_flows),
			Select {
				root_selector: r##"(
  $rows := flows[kind in ["spend", "purchase", "granted", "renewal", "refund"]];
  [$distinct($rows.kind).(
    $k := $;
    {"kind": $k, "tokens": $abs($sum($rows[kind = $k].tokens))}
  )]
)"##,
				columns: &[("kind", Column::String), ("tokens", Column::Number)],
			},
		),
		stat(
			Calc::LastNotNull,
			true,
			TextMode::ValueAndName,
			json!({
				"unit": "none",
				"decimals": 0,
				"displayName": "${__data.fields.kind}"
			}),
		),
	);
	let board = tokens_per_day(
		board,
		Panel {
			id: 2,
			kind: "timeseries",
			title: "Spent per day, by service, ${measure:text}",
			grid: [0, 5, 12, 9],
		},
		r#"flows[kind in ["spend"]]"#,
		"service",
		r#""tokens": -("${measure}" = "at_cost" ? $sum($r.at_cost) : $sum($r.tokens))"#,
	);
	tokens_per_day(
		board,
		Panel {
			id: 3,
			kind: "timeseries",
			title: "Given per day: bought, granted, renewed, refunded",
			grid: [12, 5, 12, 9],
		},
		r#"flows[kind in ["purchase", "granted", "renewal", "refund"]]"#,
		"kind",
		r#""tokens": $sum($r.tokens)"#,
	)
	.panel(
		Panel {
			id: 4,
			kind: "bargauge",
			title: "Spent by person, ${measure:text}",
			grid: [0, 14, 12, 10],
		},
		in_range(
			op_id(crate::tokens::token_flows),
			Select {
				root_selector: r##"(
  $rows := flows[kind = "spend"];
  [$distinct($rows.email).(
    $e := $;
    $r := $rows[email = $e];
    {"who": $r[0].name ? $r[0].name : $e, "spent": -("${measure}" = "at_cost" ? $sum($r.at_cost) : $sum($r.tokens))}
  )]^(>spent)
)"##,
				columns: &[("who", Column::String), ("spent", Column::Number)],
			},
		),
		json!({
				"options": {
				"displayMode": "basic",
				"orientation": "horizontal",
				"reduceOptions": {
					"calcs": [
						"lastNotNull"
					],
					"fields": "/^spent$/",
					"values": true
				},
				"showUnfilled": true,
				"valueMode": "color",
				"namePlacement": "left",
				"sizing": "manual",
				"minVizHeight": 16,
				"maxVizHeight": 22,
				"text": {
					"titleSize": 13,
					"valueSize": 13
				}
				},
				"fieldConfig": {
				"defaults": {
					"unit": "none",
					"decimals": 0,
					"min": 0,
					"color": {
						"mode": "fixed",
						"fixedColor": "blue"
					}
				},
				"overrides": []
				}
		}),
	)
	.panel(
		Panel {
			id: 5,
			kind: "table",
			title: "Spent on, ${measure:text}",
			grid: [12, 14, 12, 10],
		},
		in_range(
			op_id(crate::tokens::token_flows),
			Select {
				root_selector: r##"(
  $rows := flows[kind = "spend"];
  [$distinct($rows.(service & "|" & label)).(
    $k := $;
    $r := $rows[service & "|" & label = $k];
    {"service": $r[0].service, "what": $r[0].label, "times": $count($r), "tokens": -("${measure}" = "at_cost" ? $sum($r.at_cost) : $sum($r.tokens))}
  )]^(>tokens)
)"##,
				columns: &[("service", Column::String), ("what", Column::String), ("times", Column::Number), ("tokens", Column::Number)],
			},
		),
		json!({
				"options": {
				"showHeader": true,
				"cellHeight": "sm",
				"footer": {
					"show": false
				}
				},
				"fieldConfig": {
				"defaults": {
					"custom": {
						"align": "auto"
					}
				},
				"overrides": [
					width("service", 130),
					width("times", 70),
					width("tokens", 80)
				]
				},
				"transformations": [
				order(&["what", "service", "times", "tokens"])
				]
		}),
	)
	.panel(
		Panel {
			id: 6,
			kind: "table",
			title: "Purchases, grants and balances set",
			grid: [0, 24, 24, 9],
		},
		in_range(
			op_id(crate::tokens::token_flows),
			Select {
				root_selector: r##"[flows[kind in ["purchase", "granted", "grant_claimed", "set"]].{
  "at": at, "who": name ? name : email, "kind": kind, "tokens": tokens, "by": by, "note": note
}]^(>at)"##,
				columns: &[
					("at", Column::Timestamp),
					("who", Column::String),
					("kind", Column::String),
					("tokens", Column::Number),
					("by", Column::String),
					("note", Column::String),
				],
			},
		),
		json!({
				"options": {
				"showHeader": true,
				"cellHeight": "sm",
				"footer": {
					"show": false
				}
				},
				"fieldConfig": {
				"defaults": {
					"custom": {
						"align": "auto"
					}
				},
				"overrides": [
					width("at", 170),
					width("kind", 120),
					width("tokens", 80)
				]
				},
				"transformations": [
				order(&["at", "who", "kind", "tokens", "by", "note"])
				]
		}),
	)
	.row(7, "What it costs us", [0, 33, 24, 1])
	.panel(
		Panel {
			id: 8,
			kind: "stat",
			title: "Our bill in the range",
			grid: [0, 34, 24, 4],
		},
		in_range(
			op_id(crate::usage::spend),
			Select {
				root_selector: &PerDay {
					rows: OUR_BILL,
					day: "day",
					series: "tool_title",
					value: r#""value": $sum($r.usd)"#,
				}
				.to_string(),
				columns: &[("day", Column::Timestamp), ("series", Column::String), ("value", Column::Number)],
			},
		),
		stat(
			Calc::Sum,
			false,
			TextMode::Value,
			json!({
				"unit": "currencyUSD",
				"decimals": 2,
				"noValue": "no unit costs set"
			}),
		),
	)
	.panel(
		Panel {
			id: 9,
			kind: "timeseries",
			title: "$ per day, by tool (our keys and accounts)",
			grid: [0, 38, 12, 9],
		},
		in_range(
			op_id(crate::usage::spend),
			Select {
				root_selector: &PerDay {
					rows: OUR_BILL,
					day: "day",
					series: "tool_title",
					value: r#""value": $sum($r.usd)"#,
				}
				.to_string(),
				columns: &[("day", Column::Timestamp), ("series", Column::String), ("value", Column::Number)],
			},
		),
		bars_per_day("$", Unit::Usd, 2),
	)
	.panel(
		Panel {
			id: 10,
			kind: "timeseries",
			title: "Actions per day, by tool (in its unit)",
			grid: [12, 38, 12, 9],
		},
		in_range(
			op_id(crate::usage::spend),
			Select {
				root_selector: &PerDay {
					rows: "days[true]",
					day: "day",
					series: r#"(tool_title & " · " & unit & "s")"#,
					value: r#""value": $sum($r.units)"#,
				}
				.to_string(),
				columns: &[("day", Column::Timestamp), ("series", Column::String), ("value", Column::Number)],
			},
		),
		bars_per_day("units", Unit::Plain, 0),
	)
	.panel(
		Panel {
			id: 11,
			kind: "table",
			title: "Every costly action in the range",
			grid: [0, 47, 24, 10],
		},
		in_range(
			op_id(crate::usage::spend),
			Select {
				root_selector: r##"(
  $d := days;
  $count($d) = 0 ? [] : [$append([], $distinct($d.item)).(
    $i := $;
    $r := $d[item = $i];
    {
      "what": $r[0].tool_title & " · " & $r[0].title,
      "unit": $r[0].unit,
      "ours": $sum($append([], $r[bill = "ours"].units)),
      "their keys": $sum($append([], $r[bill = "theirs"].units)),
      "times": $sum($r.times),
      "$": $sum($append([], $r[bill = "ours" and $type(usd) = "number"].usd)),
      "by": $join($append([], $distinct($r.source)), ", ")
    }
  )]^(>$."$", >ours)
)"##,
				columns: &[
					("what", Column::String),
					("unit", Column::String),
					("ours", Column::Number),
					("their keys", Column::Number),
					("times", Column::Number),
					("$", Column::Number),
					("by", Column::String),
				],
			},
		),
		json!({
				"transformations": [
				order(&["what", "unit", "ours", "their keys", "times", "$", "by"])
				],
				"options": {
				"showHeader": true,
				"cellHeight": "sm",
				"footer": {
					"show": true,
					"reducer": [
						"sum"
					],
					"fields": [
						"$"
					]
				}
				},
				"fieldConfig": {
				"defaults": {
					"custom": {
						"align": "auto"
					}
				},
				"overrides": [
					{
						"matcher": {
							"id": "byName",
							"options": "$"
						},
						"properties": [
							{
								"id": "unit",
								"value": "currencyUSD"
							},
							{
								"id": "decimals",
								"value": 2
							},
							{
								"id": "custom.width",
								"value": 110
							}
						]
					},
					width("unit", 90),
					width("ours", 90),
					width("their keys", 100),
					width("times", 80)
				]
				}
		}),
	)
	.build(
		Dashboard {
			uid: "sa-tokens",
			title: "Tokens and costs · everyone",
			from: "now-30d",
			variables: vec![json!({
				"type": "custom",
				"name": "measure",
				"label": "Spent",
				"query": "charged : tokens, at cost : at_cost",
				"current": { "text": "charged", "value": "tokens" },
				"options": [
					{ "text": "charged", "value": "tokens", "selected": true },
					{ "text": "at cost", "value": "at_cost", "selected": false }
				],
				"hide": 0,
				"multi": false,
				"includeAll": false
			})],
		}
		.into(),
	)
}

fn meters(doc: &OpenApi) -> Value {
	let board = Board::new(doc, CONDUCTOR)
		.panel(
			Panel { id: 1, kind: "table", title: "This period, every meter on each account", grid: [0, 0, 24, 10] },
			Query {
					operation: op_id(crate::meters::meters),
					path: &[],
					params: &[],
					select: Some(Select {
					root_selector: r##"$map(meters, function($m) { $map($m.accounts, function($a) { {"meter": $m.name, "account": $a.account, "used": $a.used, "free": $m.free, "left": $a.left, "stops": $m.stop = "at_free" ? "at free" : "never", "google's count": $a.baseline, "as of": $a.since, "ours since": $a.delta, "synced": $a.reading = null ? "never" : $a.reading.synced_at, "vendor's cost": $a.reading = null ? null : $a.reading.cost, "net": $a.reading = null ? null : $a.reading.net, "currency": $a.reading = null ? null : $a.reading.currency} }) })"##,
					columns: &[("meter", Column::String), ("account", Column::String), ("used", Column::Number), ("free", Column::Number), ("left", Column::Number), ("stops", Column::String), ("google's count", Column::Number), ("as of", Column::Timestamp), ("ours since", Column::Number), ("synced", Column::String), ("vendor's cost", Column::Number), ("net", Column::Number), ("currency", Column::String)],
				}),
			},
			json!({
				"options": {
					"showHeader": true,
					"cellHeight": "sm",
					"footer": {
						"show": false
					}
				},
				"fieldConfig": {
					"defaults": {
						"custom": {
							"align": "auto"
						}
					},
					"overrides": [
						{
							"matcher": {
								"id": "byName",
								"options": "left"
							},
							"properties": [
								{
									"id": "custom.cellOptions",
									"value": {
										"type": "color-text"
									}
								},
								{
									"id": "thresholds",
									"value": {
										"mode": "absolute",
										"steps": [
											{
												"color": "red",
												"value": null
											},
											{
												"color": "green",
												"value": 1
											}
										]
									}
								}
							]
						}
					]
				},
				"transformations": [
					order(&["meter", "account", "used", "free", "left", "stops", "google's count", "as of", "ours since", "synced", "vendor's cost", "net", "currency"])
				]
			}),
		)
		.panel(
			Panel { id: 2, kind: "bargauge", title: "Used of the free ones, every account", grid: [0, 10, 12, 9] },
			Query {
				operation: op_id(crate::meters::meters),
				path: &[],
				params: &[],
				select: Some(Select {
					root_selector: r##"$map(meters, function($m) { {"meter": $m.name, "used": $m.total.used / $m.total.free * 100} })"##,
					columns: &[("meter", Column::String), ("used", Column::Number)],
				}),
			},
			json!({
				"options": {
					"orientation": "horizontal",
					"displayMode": "gradient",
					"showUnfilled": true,
					"valueMode": "color",
					"reduceOptions": {
						"calcs": [
							"lastNotNull"
						],
						"fields": "",
						"values": true
					}
				},
				"fieldConfig": {
					"defaults": {
						"unit": "percent",
						"min": 0,
						"max": 100,
						"decimals": 1,
						"displayName": "${__data.fields.meter}",
						"thresholds": {
							"mode": "absolute",
							"steps": [
								{
									"color": "green",
									"value": null
								},
								{
									"color": "orange",
									"value": 80
								},
								{
									"color": "red",
									"value": 100
								}
							]
						}
					},
					"overrides": []
				}
			}),
		)
		.panel(
			Panel { id: 3, kind: "timeseries", title: "Google's count on each account, at each reading", grid: [12, 10, 12, 9] },
			in_range(
				op_id(crate::meters::meter_readings),
				Select {
					root_selector: r##"$map(readings, function($r) { {"at": $r.synced_at, "series": $r.account & " · " & $r.meter, "units": $r.units} })"##,
					columns: &[("at", Column::Timestamp), ("series", Column::String), ("units", Column::Number)],
				},
			),
			json!({
				"transformations": [
					{
						"id": "prepareTimeSeries",
						"options": {
							"format": "multi"
						}
					}
				],
				"options": {
					"legend": {
						"displayMode": "list",
						"placement": "bottom",
						"showLegend": true
					},
					"tooltip": {
						"mode": "multi",
						"sort": "desc"
					}
				},
				"fieldConfig": {
					"defaults": {
						"custom": {
							"drawStyle": "line",
							"lineWidth": 2,
							"showPoints": "always",
							"pointSize": 4,
							"axisLabel": "units"
						},
						"unit": "none",
						"decimals": 0,
						"displayName": "${__field.labels.series}"
					},
					"overrides": []
				}
			}),
		);
	let board = table(
		board,
		(4, "Credits, each account's, in its currency", [0, 19, 12, 7]),
		op_id(crate::meters::meters),
		false,
		CREDITS,
	);
	let board = table(
		board,
		(5, "Allowances, as the guard last held them", [12, 19, 12, 7]),
		op_id(crate::meters::meters),
		false,
		ALLOWANCES,
	);
	let board = board
		.panel(
			Panel { id: 6, kind: "stat", title: "The bill over the range, in the accounts' currency", grid: [0, 26, 24, 4] },
			in_range(
				op_id(crate::meters::bill),
				Select {
					root_selector: r##"[{"used": $sum(days.used), "from credits": $sum(days.credit), "discounts": $sum(days.discount), "paid": $sum(days.paid)}]"##,
					columns: &[("used", Column::Number), ("from credits", Column::Number), ("discounts", Column::Number), ("paid", Column::Number)],
				},
			),
			stat(Calc::LastNotNull, false, TextMode::ValueAndName, json!({ "unit": "none", "decimals": 2 })),
		)
		.panel(
			Panel { id: 7, kind: "timeseries", title: "Paid and not, per day", grid: [0, 30, 12, 9] },
			in_range(
				op_id(crate::meters::bill),
				Select {
					root_selector: &PerDay {
						rows: r##"days.([{"d": $substring(day, 0, 10), "s": "paid", "v": paid}, {"d": $substring(day, 0, 10), "s": "from credits", "v": credit}, {"d": $substring(day, 0, 10), "s": "discounts", "v": discount}])"##,
						day: "d",
						series: "s",
						value: r#""money": $sum($r.v)"#,
					}
					.to_string(),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("money", Column::Number)],
				},
			),
			bars_per_day("money", Unit::Plain, 2),
		)
		.panel(
			Panel { id: 8, kind: "timeseries", title: "Used per day at list price, by slice", grid: [12, 30, 12, 9] },
			in_range(
				op_id(crate::meters::bill),
				Select {
					root_selector: &PerDay {
						rows: "days",
						day: "$substring(day, 0, 10)",
						series: "slice",
						value: r#""used": $sum($r.used)"#,
					}
					.to_string(),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("used", Column::Number)],
				},
			),
			bars_per_day("money", Unit::Plain, 2),
		);
	let board = table(board, (9, "By slice, over the range", [0, 39, 12, 8]), op_id(crate::meters::bill), true, BY_SLICE);
	let board = table(board, (10, "By SKU, over the range", [12, 39, 12, 8]), op_id(crate::meters::bill), true, BY_SKU);
	let board = table(board, (11, "Free vs paid this month, by SKU", [0, 47, 24, 8]), op_id(crate::meters::bill), true, TIERS);
	board.build(
		Dashboard {
			uid: "sa-meters",
			title: "Vendors' meters · everyone",
			from: "now-90d",
			variables: Vec::new(),
		}
		.into(),
	)
}

/// `rows` grouped by `day` and `series`, each group `$r` given the `day`, the `series` and `value` (a JSONata `"key": …` pair).
struct PerDay<'a> {
	rows: &'a str,
	day: &'a str,
	series: &'a str,
	value: &'a str,
}

impl std::fmt::Display for PerDay<'_> {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let Self { rows, day, series, value } = self;
		write!(
			f,
			r#"(
  $rows := {rows};
  [$distinct($rows.({day} & "|" & {series})).(
    $k := $;
    $r := $rows[{day} & "|" & {series} = $k];
    {{"day": $substringBefore($k, "|") & "T00:00:00Z", "series": $substringAfter($k, "|"), {value}}}
  )]
)"#
		)
	}
}

/// `rows` of the token flows summed per day into `value`, a series per `series`, as bars.
fn tokens_per_day<'a>(board: Board<'a>, panel: Panel, rows: &str, series: &str, value: &str) -> Board<'a> {
	let selector = PerDay {
		rows,
		day: "$substring(at, 0, 10)",
		series,
		value,
	}
	.to_string();
	board.panel(
		panel,
		in_range(
			op_id(crate::tokens::token_flows),
			Select {
				root_selector: &selector,
				columns: &[("day", Column::Timestamp), ("series", Column::String), ("tokens", Column::Number)],
			},
		),
		bars_per_day("tokens", Unit::Plain, 0),
	)
}

/// Table `panel` (its id, title and grid) of what `selector` makes of `operation`'s answer, over the range when `ranged`.
fn table<'a>(board: Board<'a>, (id, title, grid): (u32, &'a str, [u32; 4]), operation: &'a str, ranged: bool, (root_selector, columns): Table) -> Board<'a> {
	let names: Vec<&str> = columns.iter().map(|(name, _)| *name).collect();
	let query = Query {
		operation,
		path: &[],
		params: if ranged { RANGE } else { &[] },
		select: Some(Select { root_selector, columns }),
	};
	board.panel(
		Panel { id, kind: "table", title, grid },
		query,
		json!({
			"options": { "showHeader": true, "cellHeight": "sm", "footer": { "show": false } },
			"fieldConfig": { "defaults": { "custom": { "align": "auto" } }, "overrides": [] },
			"transformations": [order(&names)]
		}),
	)
}

fn in_range<'a>(operation: &'a str, select: Select<'a>) -> Query<'a> {
	Query {
		operation,
		path: &[],
		params: RANGE,
		select: Some(select),
	}
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum Calc {
	LastNotNull,
	Sum,
}

#[derive(Clone, Copy, serde::Serialize)]
#[serde(rename_all = "snake_case")]
enum TextMode {
	ValueAndName,
	Value,
}

/// Each row's figure, the range reduced by `calc`.
fn stat(calc: Calc, values: bool, text_mode: TextMode, defaults: Value) -> Value {
	json!({
		"options": {
			"reduceOptions": { "calcs": [calc], "fields": "", "values": values },
			"orientation": "vertical",
			"textMode": text_mode,
			"colorMode": "none",
			"graphMode": "none",
			"justifyMode": "center",
			"text": { "titleSize": 13, "valueSize": 28 }
		},
		"fieldConfig": { "defaults": defaults, "overrides": [] }
	})
}

struct Dashboard<'a> {
	uid: &'a str,
	title: &'a str,
	from: &'a str,
	/// The variables nothing answers.
	variables: Vec<Value>,
}

impl From<Dashboard<'_>> for Value {
	fn from(Dashboard { uid, title, from, variables }: Dashboard<'_>) -> Self {
		json!({
			"uid": uid,
			"title": title,
			"tags": ["conductor"],
			"timezone": "utc",
			"editable": false,
			"graphTooltip": 1,
			"time": { "from": from, "to": "now" },
			"refresh": "",
			"schemaVersion": 39,
			"templating": { "list": variables }
		})
	}
}

#[derive(Clone, Copy, serde::Serialize)]
enum Unit {
	#[serde(rename = "none")]
	Plain,
	#[serde(rename = "currencyUSD")]
	Usd,
}

/// A day's bars, one per `series`, stacked.
fn bars_per_day(axis: &str, unit: Unit, decimals: u8) -> Value {
	json!({
		"transformations": [{ "id": "prepareTimeSeries", "options": { "format": "multi" } }],
		"options": {
			"legend": { "displayMode": "list", "placement": "bottom", "showLegend": true },
			"tooltip": { "mode": "multi", "sort": "desc" }
		},
		"fieldConfig": {
			"defaults": {
				"custom": {
					"drawStyle": "bars",
					"fillOpacity": 80,
					"lineWidth": 1,
					"showPoints": "never",
					"stacking": { "mode": "normal", "group": "A" },
					"axisLabel": axis,
					"barAlignment": 0
				},
				"unit": unit,
				"decimals": decimals,
				"displayName": "${__field.labels.series}"
			},
			"overrides": []
		}
	})
}

fn width(column: &str, px: u32) -> Value {
	json!({ "matcher": { "id": "byName", "options": column }, "properties": [{ "id": "custom.width", "value": px }] })
}

/// A table's columns in this order.
fn order(columns: &[&str]) -> Value {
	json!({ "id": "organize", "options": { "indexByName": columns.iter().zip(0..).map(|(c, i)| (c.to_string(), json!(i))).collect::<serde_json::Map<_, _>>() } })
}
