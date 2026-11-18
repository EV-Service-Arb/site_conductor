//! The conductor's own Grafana dashboards, over its `/api/v1` (`sa_core::dashboards`).

use ev_lib::api::{OpenApi, op_id};
use sa_core::dashboards::{Board, Column, Query, Select};
use serde_json::{Value, json};

use crate::grafana::CONDUCTOR;

const OUR_BILL: &str = r#"days[bill = "ours" and $type(usd) = "number"]"#;

const RANGE: &[(&str, &str)] = &[("from", "${__from}"), ("to", "${__to}")];

/// Each dashboard by its uid.
pub(crate) fn own(doc: &OpenApi) -> [(&'static str, Value); 2] {
	[("sa-tokens", tokens(doc)), ("sa-meters", meters(doc))]
}

fn tokens(doc: &OpenApi) -> Value {
	Board::new(doc, CONDUCTOR)
		.panel(
			1,
			"stat",
			"In the range",
			[0, 0, 24, 5],
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
				"lastNotNull",
				true,
				"value_and_name",
				json!({
					"unit": "none",
					"decimals": 0,
					"displayName": "${__data.fields.kind}"
				}),
			),
		)
		.panel(
			2,
			"timeseries",
			"Spent per day, by service",
			[0, 5, 12, 9],
			in_range(
				op_id(crate::tokens::token_flows),
				Select {
					root_selector: &per_day(r#"flows[kind in ["spend"]]"#, "$substring(at, 0, 10)", "service", r#""tokens": -$sum($r.tokens)"#),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("tokens", Column::Number)],
				},
			),
			bars_per_day("tokens", "none", 0),
		)
		.panel(
			3,
			"timeseries",
			"Given per day: bought, granted, renewed, refunded",
			[12, 5, 12, 9],
			in_range(
				op_id(crate::tokens::token_flows),
				Select {
					root_selector: &per_day(
						r#"flows[kind in ["purchase", "granted", "renewal", "refund"]]"#,
						"$substring(at, 0, 10)",
						"kind",
						r#""tokens": $sum($r.tokens)"#,
					),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("tokens", Column::Number)],
				},
			),
			bars_per_day("tokens", "none", 0),
		)
		.panel(
			4,
			"bargauge",
			"Spent by person",
			[0, 14, 12, 10],
			in_range(
				op_id(crate::tokens::token_flows),
				Select {
					root_selector: r##"(
  $rows := flows[kind = "spend"];
  [$distinct($rows.email).(
    $e := $;
    $r := $rows[email = $e];
    {"who": $r[0].name ? $r[0].name : $e, "spent": -$sum($r.tokens)}
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
			5,
			"table",
			"Spent on",
			[12, 14, 12, 10],
			in_range(
				op_id(crate::tokens::token_flows),
				Select {
					root_selector: r##"(
  $rows := flows[kind = "spend"];
  [$distinct($rows.(service & "|" & label)).(
    $k := $;
    $r := $rows[service & "|" & label = $k];
    {"service": $r[0].service, "what": $r[0].label, "times": $count($r), "tokens": -$sum($r.tokens)}
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
			6,
			"table",
			"Purchases, grants and balances set",
			[0, 24, 24, 9],
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
			8,
			"stat",
			"Our bill in the range",
			[0, 34, 24, 4],
			in_range(
				op_id(crate::usage::spend),
				Select {
					root_selector: &per_day(OUR_BILL, "day", "tool_title", r#""value": $sum($r.usd)"#),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("value", Column::Number)],
				},
			),
			stat(
				"sum",
				false,
				"value",
				json!({
					"unit": "currencyUSD",
					"decimals": 2,
					"noValue": "no unit costs set"
				}),
			),
		)
		.panel(
			9,
			"timeseries",
			"$ per day, by tool (our keys and accounts)",
			[0, 38, 12, 9],
			in_range(
				op_id(crate::usage::spend),
				Select {
					root_selector: &per_day(OUR_BILL, "day", "tool_title", r#""value": $sum($r.usd)"#),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("value", Column::Number)],
				},
			),
			bars_per_day("$", "currencyUSD", 2),
		)
		.panel(
			10,
			"timeseries",
			"Actions per day, by tool (in its unit)",
			[12, 38, 12, 9],
			in_range(
				op_id(crate::usage::spend),
				Select {
					root_selector: &per_day("days[true]", "day", r#"(tool_title & " · " & unit & "s")"#, r#""value": $sum($r.units)"#),
					columns: &[("day", Column::Timestamp), ("series", Column::String), ("value", Column::Number)],
				},
			),
			bars_per_day("units", "none", 0),
		)
		.panel(
			11,
			"table",
			"Every costly action in the range",
			[0, 47, 24, 10],
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
		.build(dashboard("sa-tokens", "Tokens and costs · everyone", "now-30d"))
}

fn meters(doc: &OpenApi) -> Value {
	Board::new(doc, CONDUCTOR)
		.panel(
			1,
			"table",
			"This period, every meter on each account",
			[0, 0, 24, 10],
			Query {
					operation: op_id(crate::meters::meters),
					path: &[],
					params: &[],
					headers: &[],
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
			2,
			"bargauge",
			"Used of the free ones, every account",
			[0, 10, 12, 9],
			Query {
				operation: op_id(crate::meters::meters),
				path: &[],
				params: &[],
				headers: &[],
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
			3,
			"timeseries",
			"Google's count on each account, at each reading",
			[12, 10, 12, 9],
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
		)
		.build(dashboard("sa-meters", "Vendors' meters · everyone", "now-90d"))
}

/// `rows` grouped by `day` and `series`, each group `$r` given the `day`, the `series` and `value` (a JSONata `"key": …` pair).
fn per_day(rows: &str, day: &str, series: &str, value: &str) -> String {
	format!(
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

fn in_range<'a>(operation: &'a str, select: Select<'a>) -> Query<'a> {
	Query {
		operation,
		path: &[],
		params: RANGE,
		headers: &[],
		select: Some(select),
	}
}

/// Each row's figure, the range reduced by `calc`.
fn stat(calc: &str, values: bool, text_mode: &str, defaults: Value) -> Value {
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

fn dashboard(uid: &str, title: &str, from: &str) -> Value {
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
		"templating": { "list": [] }
	})
}

/// A day's bars, one per `series`, stacked.
fn bars_per_day(axis: &str, unit: &str, decimals: u8) -> Value {
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
