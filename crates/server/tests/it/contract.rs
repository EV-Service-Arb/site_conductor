//! nix/contract.nix against what the binary prints of itself.

use std::{collections::BTreeSet, process::Command};

fn contract() -> serde_json::Value {
	let out = Command::new("nix")
		.args([
			"eval",
			"--json",
			"--file",
			concat!(env!("CARGO_MANIFEST_DIR"), "/../../nix/contract.nix"),
			"--apply",
			r#"f: f { pname = "panel"; }"#,
		])
		.output()
		.expect("nix is on PATH: the dev shell's and CI's");
	assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
	serde_json::from_slice(&out.stdout).unwrap()
}

fn printed(args: &[&str]) -> BTreeSet<String> {
	let out = Command::new(env!("CARGO_BIN_EXE_panel")).args(args).output().unwrap();
	assert!(out.status.success(), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
	String::from_utf8(out.stdout).unwrap().lines().map(str::to_owned).collect()
}

fn strings(v: &serde_json::Value) -> BTreeSet<String> {
	v.as_array().expect("a list").iter().map(|s| s.as_str().expect("a string").to_owned()).collect()
}

#[test]
fn the_contract_is_what_the_binary_requires_mounts_and_forwards() {
	let c = contract();
	let deploy = &c["deploy"];
	assert_eq!(strings(&deploy["requiredEnv"]), printed(&["--print-required-vars", "production"]), "deploy.requiredEnv");
	assert_eq!(
		strings(&deploy["ingress"]["rateLimitPathPrefixes"]),
		printed(&["--print-paths", "rate-limited"]),
		"ingress.rateLimitPathPrefixes"
	);
	assert_eq!(
		strings(&deploy["ingress"]["excludePathPrefixes"]),
		printed(&["--print-paths", "unpublished"]),
		"ingress.excludePathPrefixes"
	);
	let ports: BTreeSet<u64> = deploy["egress"]["ports"].as_array().expect("a list").iter().map(|p| p.as_u64().expect("a port")).collect();
	assert_eq!(ports, sa_panel::mail::PORTS.map(u64::from).into(), "egress.ports");
	let consumes: BTreeSet<String> = c["consumes"].as_object().expect("an attrset").keys().cloned().collect();
	assert_eq!(consumes, printed(&["--print-paths", "consumed"]), "consumes");
	let provides: BTreeSet<String> = c["provides"].as_object().expect("an attrset").values().map(|v| v.as_str().expect("a path").to_owned()).collect();
	assert_eq!(provides, printed(&["--print-provided-paths"]), "provides");
}
