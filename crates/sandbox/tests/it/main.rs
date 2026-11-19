use std::path::Path;
mod execute;
mod runner;

/// `crates/sandbox/js`, its node_modules installed: a fresh checkout's, CI's, by deno; an installed one
/// may be js-local's links, never replaced.
fn js() -> &'static str {
	static INSTALLED: std::sync::Once = std::sync::Once::new();
	let js = concat!(env!("CARGO_MANIFEST_DIR"), "/js");
	INSTALLED.call_once(|| {
		if !Path::new(js).join("node_modules").exists() {
			let deno = std::env::split_paths(&std::env::var_os("PATH").unwrap())
				.map(|d| d.join("deno"))
				.find(|p| p.is_file())
				.expect("deno on PATH: the dev shell has it");
			let installed = std::process::Command::new(deno).args(["install", "--no-lock"]).current_dir(js).status().unwrap();
			assert!(installed.success(), "deno install in {js}: {installed}");
		}
	});
	js
}
