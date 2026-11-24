//! Readings replayed through `guard plan`, against plan.snap; `GUARD_SNAP_ACCEPT=1` rewrites it.
//! Each case is a config and what a tick read; the snap is every lever's setting and what the guard would change.

use std::{fs, path::Path, process::Command};

const SLICES: &str = r#"[
  {"name": "video generation", "service": "^Vertex AI$", "sku": "(?i)veo|omni"},
  {"name": "LLM", "service": "^Vertex AI$|^Gemini API$"},
  {"name": "maps", "service": "(?i)maps|places|geocoding|routes"}
]"#;
const SERVICES: &str = r#"{"AEFD-7695-64FA": "Gemini API", "C7E2-9256-1C43": "Vertex AI", "152E-C115-5142": "Cloud Run", "29E7-DA93-CA13": "Cloud Run functions"}"#;
const TABLE: &str = "p.billing.gcp_billing_export_v1_000000_AAAAAA_BBBBBB";
/// p is the guard's own project and holds the export; q is billed beside it.
const ENABLED: &str = r#""p": ["aiplatform.googleapis.com", "bigquery.googleapis.com", "iam.googleapis.com", "run.googleapis.com"],
	"q": ["aiplatform.googleapis.com", "cloudfunctions.googleapis.com", "generativelanguage.googleapis.com", "run.googleapis.com"]"#;
/// What the guard runs and reads through on p.
const OWN: &str = r#"["bigquery.googleapis.com", "run.googleapis.com"]"#;

/// Every slice's `{day, month}`: 0 but those given.
fn allow(given: &[(&str, f64, f64)]) -> String {
	let each = ["video generation", "LLM", "maps"].map(|s| {
		let (day, month) = given.iter().find(|g| g.0 == s).map_or((0.0, 0.0), |g| (g.1, g.2));
		format!(r#""{s}": {{"day": {day}, "month": {month}}}"#)
	});
	format!("{{{}}}", each.join(", "))
}

/// What `guard plan` prints: each lever (stderr), then what would change (stdout); an error by its message alone.
/// The account's definition is as the conductor published it: `allow`, `unguarded` and its credit.
fn replay(dir: &Path, name: &str, allow: &str, unguarded: &str, left: f64, ends: &str, seen: &str) -> String {
	let definition = format!(
		r#"{{"held": {{"name": "a", "table": "{TABLE}", "since": "2026-10-01", "allow": {allow}, "unguarded": {unguarded},
		"credits": [{{"id": "c1", "name": "trial", "read": "2026-11-01T00:00:00Z", "left": {left}, "ends": "{ends}"}}]}}, "slices": {SLICES}}}"#
	);
	let read = dir.join("read.json");
	fs::write(
		&read,
		format!(
			r#"{{"now": "2026-11-12T12:00:00Z", "own": "p", "services": {SERVICES}, "exports": ["{TABLE}"], "accounts": [{{"account": "a", "definition": {definition},
			"billing": "000000-AAAAAA-BBBBBB", "usd": 0.9, "projects": {{"p": 1, "q": 2}}, {seen}}}]}}"#
		),
	)
	.unwrap();
	let out = Command::new(env!("CARGO_BIN_EXE_guard")).arg("plan").arg(&read).output().unwrap();
	let stderr = String::from_utf8(out.stderr).unwrap();
	let stderr = match stderr.split_once("\n\nLocation:") {
		Some((message, _)) => format!("{message}\n"), // where in the source it was raised moves with every edit
		None => stderr,
	};
	format!("== {name}\n{stderr}{}", String::from_utf8(out.stdout).unwrap())
}

// r[verify google.guarded]
#[test]
fn plan() {
	let dir = tempfile::tempdir().unwrap();
	let d = dir.path();
	let none = allow(&[]);
	let got = [
		replay(
			d,
			"credits left, nothing allowed: Vertex in use takes every credit; nothing capped yet",
			&none,
			OWN,
			100.0,
			"2027-01-13",
			&format!(
				r#""enabled": {{{ENABLED}}}, "gross": [{{"project": "p", "service": "C7E2-9256-1C43", "month": 40, "recent": 55}}],
				"real": [{{"project": "p", "service": "Vertex AI", "sku": "Veo 3 video", "sku_id": "V3", "month": 0, "today": 0}}],
				"drawn": {{"c1": -40}}, "caps": [], "quotas": []"#
			),
		),
		replay(
			d,
			"credits ending tomorrow count as gone: caps fall to what was spent",
			&none,
			OWN,
			100.0,
			"2026-11-13",
			&format!(
				r#""enabled": {{{ENABLED}}}, "gross": [{{"project": "p", "service": "C7E2-9256-1C43", "month": 40, "recent": 55}}],
				"real": [], "drawn": {{"c1": -40}},
				"caps": [{{"name": "billingAccounts/x/budgets/v", "project": "p", "service": "C7E2-9256-1C43", "amount": 100, "state": "CONFIGURED"}}], "quotas": []"#
			),
		),
		replay(
			d,
			"an enforced cap whose allowance was raised is lifted, then set",
			&allow(&[("video generation", 10.0, 10.0)]),
			OWN,
			0.0,
			"2026-11-01",
			&format!(
				r#""enabled": {{{ENABLED}}}, "gross": [{{"project": "p", "service": "C7E2-9256-1C43", "month": 40, "recent": 55}}],
				"real": [{{"project": "p", "service": "Vertex AI", "sku": "Veo 3 video", "sku_id": "V3", "month": 0.05, "today": 0}}], "drawn": {{}},
				"caps": [{{"name": "billingAccounts/x/budgets/v", "project": "p", "service": "C7E2-9256-1C43", "amount": 40, "state": "ENFORCED"}}], "quotas": []"#
			),
		),
		replay(
			d,
			"a metered API at allow 0 holds every SKU through it in its free units: the least-free paces it; an unpriced quota'd API, 0",
			&none,
			OWN,
			0.0,
			"2026-11-01",
			r#""enabled": {"p": ["bigquery.googleapis.com", "run.googleapis.com"], "q": ["maps-backend.googleapis.com", "places.googleapis.com", "routes.googleapis.com"]},
			"gross": [], "real": [], "drawn": {}, "caps": [], "quotas": [
			{"project": "q", "api": "maps-backend.googleapis.com", "id": "MapLoadsPerDay", "value": -1, "today": 7},
			{"project": "q", "api": "places.googleapis.com", "id": "SearchTextRequestPerDay", "value": -1, "today": 3},
			{"project": "q", "api": "places.googleapis.com", "id": "GetPlaceRequestPerDay", "value": 50, "today": 9},
			{"project": "q", "api": "routes.googleapis.com", "id": "ComputeRoutesPerDay", "value": 32, "today": 0}]"#,
		),
		replay(
			d,
			"a day's allowance below its month's: the cap and the quota hold what is left of the day",
			&allow(&[("video generation", 2.0, 10.0), ("maps", 1.0, 30.0)]),
			OWN,
			0.0,
			"2026-11-01",
			r#""enabled": {"p": ["aiplatform.googleapis.com", "bigquery.googleapis.com", "run.googleapis.com"], "q": ["aiplatform.googleapis.com", "places.googleapis.com"]},
			"gross": [{"project": "q", "service": "C7E2-9256-1C43", "month": 20, "recent": 25}],
			"real": [{"project": "q", "service": "Vertex AI", "sku": "Veo 3 video", "sku_id": "V3", "month": 3, "today": 1.5},
			{"project": "q", "service": "Places API (New)", "sku": "Places API Text Search Pro", "sku_id": "4FDA-34B1-A910", "month": 0.4, "today": 0.4}],
			"drawn": {}, "caps": [], "quotas": [{"project": "q", "api": "places.googleapis.com", "id": "SearchTextRequestPerDay", "value": -1, "today": 12}]"#,
		),
		replay(
			d,
			"the backstop: real money past a slice's month, or its day, unlinks the projects it spent in",
			&allow(&[("video generation", 1.0, 100.0), ("maps", 10.0, 10.0)]),
			OWN,
			0.0,
			"2026-11-01",
			r#""enabled": {"p": ["bigquery.googleapis.com", "run.googleapis.com"], "q": []}, "gross": [],
			"real": [{"project": "q", "service": "Geocoding API", "sku": "Geocoding", "sku_id": "BAC8-4E68-E261", "month": 10.5, "today": 0},
			{"project": "q", "service": "Vertex AI", "sku": "Veo 3 video", "sku_id": "V3", "month": 1.2, "today": 1.2},
			{"project": "p", "service": "BigQuery", "sku": "Analysis", "sku_id": "BQ", "month": 0.01, "today": 0.01}],
			"drawn": {}, "caps": [], "quotas": []"#,
		),
		replay(
			d,
			"an enabled API no class holds is disabled; Google's free ones are let be",
			&none,
			OWN,
			0.0,
			"2026-11-01",
			r#""enabled": {"p": ["bigquery.googleapis.com", "iam.googleapis.com", "run.googleapis.com"], "q": ["serviceusage.googleapis.com", "weather.googleapis.com"]},
			"gross": [], "real": [], "drawn": {}, "caps": [], "quotas": []"#,
		),
		replay(
			d,
			"the same API named in unguarded is left alone, its quotas too",
			&none,
			r#"["bigquery.googleapis.com", "run.googleapis.com", "weather.googleapis.com"]"#,
			0.0,
			"2026-11-01",
			r#""enabled": {"p": ["bigquery.googleapis.com", "iam.googleapis.com", "run.googleapis.com"], "q": ["serviceusage.googleapis.com", "weather.googleapis.com"]},
			"gross": [], "real": [], "drawn": {}, "caps": [], "quotas": []"#,
		),
		replay(
			d,
			"the guard's own Cloud Run not in unguarded: refused, nothing set",
			&none,
			r#"["bigquery.googleapis.com"]"#,
			0.0,
			"2026-11-01",
			&format!(r#""enabled": {{{ENABLED}}}, "gross": [], "real": [], "drawn": {{}}, "caps": [], "quotas": []"#),
		),
		replay(
			d,
			"BigQuery on an export's project not in unguarded: refused, nothing set",
			&none,
			r#"["run.googleapis.com"]"#,
			0.0,
			"2026-11-01",
			&format!(r#""enabled": {{{ENABLED}}}, "gross": [], "real": [], "drawn": {{}}, "caps": [], "quotas": []"#),
		),
		replay(
			d,
			"a slice missing from allow: the definition refused",
			r#"{"video generation": {"day": 0, "month": 0}, "maps": {"day": 0, "month": 0}}"#,
			OWN,
			0.0,
			"2026-11-01",
			&format!(r#""enabled": {{{ENABLED}}}, "gross": [], "real": [], "drawn": {{}}, "caps": [], "quotas": []"#),
		),
		replay(
			d,
			"a credit drawn that its hold does not name stops the tick",
			&none,
			OWN,
			100.0,
			"2027-01-13",
			&format!(r#""enabled": {{{ENABLED}}}, "gross": [], "real": [], "drawn": {{"c2": -1}}, "caps": [], "quotas": []"#),
		),
	]
	.concat();
	let snap = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/plan.snap");
	if std::env::var_os("GUARD_SNAP_ACCEPT").is_some() {
		fs::write(snap, &got).unwrap();
	}
	assert_eq!(got, fs::read_to_string(snap).unwrap());
}
