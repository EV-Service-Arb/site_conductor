//! Readings replayed through `guard plan`, against plan.snap; `GUARD_SNAP_ACCEPT=1` rewrites it.
//! Each case is a config and what a tick read; the snap is every lever's setting and what the guard would change.

use std::{fs, path::Path, process::Command};

const SLICES: &str = r#"[
  {"name": "video generation", "service": "^Vertex AI$", "sku": "(?i)veo|omni"},
  {"name": "LLM", "service": "^Vertex AI$|^Gemini API$"},
  {"name": "maps", "service": "(?i)maps|geocoding", "closed": ["routes.googleapis.com"]}
]"#;
const SERVICES: &str = r#"{"AEFD-7695-64FA": "Gemini API", "C7E2-9256-1C43": "Vertex AI", "152E-C115-5142": "Cloud Run", "29E7-DA93-CA13": "Cloud Run functions"}"#;
const TABLE: &str = "p.billing.gcp_billing_export_v1_000000_AAAAAA_BBBBBB";

/// What `guard plan` prints: each lever (stderr), then what would change (stdout); an error by its message alone.
fn replay(dir: &Path, name: &str, allow: &str, left: f64, ends: &str, seen: &str) -> String {
	let config = format!(
		r#"{{"accounts": [{{"name": "a", "export": {{"table": "{TABLE}", "reader": {{"file": ""}}, "since": "2026-10-01"}}, "allow": {allow},
		"credits": [{{"id": "c1", "name": "trial", "read": "2026-11-01T00:00:00Z", "left": {left}, "ends": "{ends}"}}]}}], "slices": {SLICES}}}"#
	);
	let read = dir.join("read.json");
	fs::write(
		&read,
		format!(
			r#"{{"now": "2026-11-12T12:00:00Z", "own": "p", "services": {SERVICES}, "accounts": [{{"account": "a", "billing": "000000-AAAAAA-BBBBBB",
			"usd": 0.9, "projects": {{"p": 1, "q": 2}}, {seen}}}]}}"#
		),
	)
	.unwrap();
	let out = Command::new(env!("CARGO_BIN_EXE_guard")).arg("plan").arg(&read).env("GUARD_CONFIG", config).output().unwrap();
	let stderr = String::from_utf8(out.stderr).unwrap();
	let stderr = match stderr.split_once("\n\nLocation:") {
		Some((message, _)) => format!("{message}\n"), // where in the source it was raised moves with every edit
		None => stderr,
	};
	format!("== {name}\n{stderr}{}", String::from_utf8(out.stdout).unwrap())
}

#[test]
fn plan() {
	let dir = tempfile::tempdir().unwrap();
	let d = dir.path();
	let got = [
		replay(
			d,
			"credits left, nothing allowed: Vertex in use takes every credit; nothing capped yet",
			"{}",
			100.0,
			"2027-01-13",
			r#""gross": [{"project": "p", "service": "C7E2-9256-1C43", "month": 40, "recent": 55}],
			"real": [{"project": "p", "service": "Vertex AI", "sku": "Veo 3 video", "month": 0}],
			"drawn": {"c1": -40}, "caps": [], "quotas": []"#,
		),
		replay(
			d,
			"credits ending tomorrow count as gone: caps fall to what was spent",
			"{}",
			100.0,
			"2026-11-13",
			r#""gross": [{"project": "p", "service": "C7E2-9256-1C43", "month": 40, "recent": 55}],
			"real": [], "drawn": {"c1": -40},
			"caps": [{"name": "billingAccounts/x/budgets/v", "project": "p", "service": "C7E2-9256-1C43", "amount": 100, "state": "CONFIGURED"}], "quotas": []"#,
		),
		replay(
			d,
			"an enforced cap whose allowance was raised is lifted, then set",
			r#"{"video generation": 10}"#,
			0.0,
			"2026-11-01",
			r#""gross": [{"project": "p", "service": "C7E2-9256-1C43", "month": 40, "recent": 55}],
			"real": [{"project": "p", "service": "Vertex AI", "sku": "Veo 3 video", "month": 0.05}], "drawn": {},
			"caps": [{"name": "billingAccounts/x/budgets/v", "project": "p", "service": "C7E2-9256-1C43", "amount": 40, "state": "ENFORCED"}], "quotas": []"#,
		),
		replay(
			d,
			"maps: the meters' free units and what 10 buys, a day; an API nothing calls, none; real money past the allowance unlinks",
			r#"{"maps": 10}"#,
			0.0,
			"2026-11-01",
			r#""gross": [], "real": [{"project": "q", "service": "Geocoding API", "sku": "Geocoding", "month": 10.5}, {"project": "p", "service": "BigQuery", "sku": "Analysis", "month": 0.01}],
			"drawn": {}, "caps": [], "quotas": [{"project": "q", "api": "geocoding-backend.googleapis.com", "id": "GeocodingPerDay", "value": -1, "today": 120},
			{"project": "q", "api": "routes.googleapis.com", "id": "ComputeRoutesPerDay", "value": 32, "today": 0}]"#,
		),
		replay(
			d,
			"a credit drawn that nix/google.nix does not name stops the tick",
			"{}",
			100.0,
			"2027-01-13",
			r#""gross": [], "real": [], "drawn": {"c2": -1}, "caps": [], "quotas": []"#,
		),
	]
	.concat();
	let snap = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/plan.snap");
	if std::env::var_os("GUARD_SNAP_ACCEPT").is_some() {
		fs::write(snap, &got).unwrap();
	}
	assert_eq!(got, fs::read_to_string(snap).unwrap());
}
