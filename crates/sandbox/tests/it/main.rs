use std::{path::Path, time::Duration};
mod execute;
mod runner;

/// The runner's answer wait: an answer comes this long after its cell started, or as it ends.
const WAIT: Duration = Duration::from_millis(100);
/// Past what any step takes on a loaded machine: a passing test never waits it out.
const SLACK: Duration = Duration::from_secs(4);

/// `crates/sandbox/js`, its node_modules installed: a fresh checkout's, CI's, by deno; an installed one
/// may be js-local's links, never replaced.
fn js() -> &'static str {
	static INSTALLED: std::sync::Once = std::sync::Once::new();
	let js = concat!(env!("CARGO_MANIFEST_DIR"), "/js");
	INSTALLED.call_once(|| {
		// nextest runs each test in a process of its own: one installs, the rest wait for it rather than read a half-written node_modules
		let lock = std::fs::File::create(concat!(env!("CARGO_TARGET_TMPDIR"), "/sandbox-js-install.lock")).unwrap();
		lock.lock().unwrap();
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
