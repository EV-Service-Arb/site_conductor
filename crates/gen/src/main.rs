//! `nix run .#gen`: the conductor's OpenAPI document, which the front end's client and types are
//! generated from; the services' documents, copied in from their checkouts (`_.siblings-local.json`
//! at the repo's root, gitignored, names each one's root; [`DOCUMENTS`] where each commits it);
//! and the views' table (`VIEWS`, for `@evinvest/views`), the services' nested in the conductor's.
//! All into `frontend/src/shared/api/`, committed; the `generated` pre-commit hook re-runs it. And the
//! catalog those documents make, as the conductor merges them, for the sandbox's client
//! (`crates/sandbox/js/catalog.json`).

use std::{
	collections::BTreeMap,
	path::{Path, PathBuf},
};

use ev_lib::{
	api::{openapi::SchemaObject, schemars::generate::SchemaSettings},
	views::Views,
};
use sa_browsers::view::{Close as ViewerClose, In as ViewerIn, Out as ViewerOut};
use sa_server::view::View;

/// Where each sibling commits its document in its checkout; `None`: none committed yet, the copy kept.
const DOCUMENTS: [(sa_auth::Service, Option<&str>); 3] = [
	(sa_auth::Service::Sites, Some("frontend/src/shared/api/openapi.json")),
	(sa_auth::Service::ReviewArchive, Some("crates/review_archive_server/openapi.json")),
	(sa_auth::Service::Playbook, Some("playbook_web/openapi.json")),
];

fn main() {
	let api = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src/shared/api");
	let released = std::env::args().skip(1).any(|a| a == "--released");
	let out = api.join("openapi.json");
	let mut doc = sa_server::http::document();
	// The browser viewer's socket is outside the operations; its frames are wire types all the same.
	let mut frames = SchemaSettings::draft07().with(|s| s.definitions_path = "#/components/schemas/".into()).into_generator();
	frames.subschema_for::<ViewerIn>();
	frames.subschema_for::<ViewerOut>();
	frames.subschema_for::<ViewerClose>();
	let schemas = &mut doc.components.as_mut().expect("the document has components").schemas;
	for (name, schema) in frames.take_definitions(true) {
		assert!(!schemas.contains_key(&name), "{name}: a frame's schema is named as one of the document's");
		let json_schema = schema.try_into().expect("schemars defines schemas as objects");
		schemas.insert(
			name,
			SchemaObject {
				json_schema,
				example: None,
				external_docs: None,
			},
		);
	}
	std::fs::write(&out, serde_json::to_string_pretty(&doc).expect("the document serializes") + "\n").expect("frontend/src/shared/api is writable");
	let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../_.siblings-local.json");
	// by name: the file names checkouts other than the services' too
	let roots: BTreeMap<String, PathBuf> = serde_json::from_str(&std::fs::read_to_string(&local).unwrap_or_else(|e| panic!("{}: each sibling's checkout root: {e}", local.display())))
		.unwrap_or_else(|e| panic!("{}: {e}", local.display()));
	let sources: Vec<(sa_auth::Service, Option<PathBuf>)> = DOCUMENTS
		.iter()
		.map(|(service, at)| {
			let root = roots.get(service.as_str()).unwrap_or_else(|| panic!("{} names no {service}", local.display()));
			(*service, at.map(|at| root.join(at)))
		})
		.collect();
	let vendored = api.join("services");
	if released {
		return check_released(&sources, &vendored);
	}
	std::fs::create_dir_all(&vendored).expect("frontend/src/shared/api is writable");
	let mut table = <View as Views>::table().clone();
	let mut catalog = Vec::new();
	for (service, source) in sources {
		let copy = vendored.join(format!("{service}.json"));
		if let Some(source) = source {
			let raw = std::fs::read_to_string(&source).unwrap_or_else(|e| panic!("{service}'s document at {}: {e}", source.display()));
			let mut doc: serde_json::Value = serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{}: {e}", source.display()));
			forwarded(&mut doc);
			std::fs::write(&copy, serde_json::to_string_pretty(&doc).expect("a document serializes") + "\n").expect("frontend/src/shared/api is writable");
		}
		let theirs = match std::fs::read_to_string(&copy) {
			Ok(theirs) => theirs,
			Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue, // none yet: its nest stays a mark
			Err(e) => panic!("{}: {e}", copy.display()),
		};
		let theirs: serde_json::Value = serde_json::from_str(&theirs).unwrap_or_else(|e| panic!("{}: {e}", copy.display()));
		if let Some(views) = theirs.get("x-views") {
			let views = serde_json::from_value(views.clone()).unwrap_or_else(|e| panic!("{service}'s x-views: {e}"));
			table.nest(service.as_str(), views).unwrap_or_else(|e| panic!("{service}'s views in the conductor's: {e}"));
		}
		catalog.push((service, theirs));
	}
	let catalog = sa_server::catalog::merged(catalog).unwrap_or_else(|e| panic!("the catalog of the vendored documents: {e:#}"));
	std::fs::write(
		Path::new(env!("CARGO_MANIFEST_DIR")).join("../sandbox/js/catalog.json"),
		serde_json::to_string_pretty(&catalog).expect("the catalog serializes") + "\n",
	)
	.expect("crates/sandbox/js is writable");
	let views = serde_json::to_string_pretty(&table).expect("the table serializes");
	std::fs::write(
		api.join("views.ts"),
		format!("// Generated by `nix run .#gen` from the conductor's `View` and the services' documents.\nexport const VIEWS = {views} as const;\n"),
	)
	.expect("frontend/src/shared/api is writable");
	let prefixes: Vec<&str> = sa_server::forward::paths().map(|p| p.prefix).collect();
	let prefixes = serde_json::to_string_pretty(&prefixes).expect("strings serialize");
	std::fs::write(
		api.join("paths.ts"),
		format!("// Generated by `nix run .#gen` from the conductor's path table (`forward::paths`): never a page's.\nexport const PATHS = {prefixes} as const;\n"),
	)
	.expect("frontend/src/shared/api is writable");
}

/// `--released`: each vendored document is the one its service's latest release tag holds, else the
/// conductor's release would go out against a peer contract prod doesn't serve.
fn check_released(sources: &[(sa_auth::Service, Option<PathBuf>)], vendored: &Path) {
	let git = |root: &Path, args: &[&str]| {
		let out = std::process::Command::new("git").arg("-C").arg(root).args(args).output().expect("git runs");
		assert!(out.status.success(), "git {args:?} in {}: {}", root.display(), String::from_utf8_lossy(&out.stderr));
		String::from_utf8(out.stdout).expect("git prints UTF-8")
	};
	let mut lagging = Vec::new();
	for (service, source) in sources {
		let Some(source) = source else { continue };
		let root = source
			.ancestors()
			.find(|a| a.join(".git").exists())
			.unwrap_or_else(|| panic!("{} is in no git checkout", source.display()));
		let at = source.strip_prefix(root).expect("an ancestor").to_str().expect("a UTF-8 path");
		git(root, &["fetch", "-q", "--tags"]);
		let tags = git(root, &["tag", "--list", "v*", "--sort=-v:refname"]);
		let tag = tags.lines().next().unwrap_or_else(|| panic!("{service} has no release tag"));
		let mut theirs: serde_json::Value = serde_json::from_str(&git(root, &["show", &format!("{tag}:{at}")])).unwrap_or_else(|e| panic!("{service} {tag}:{at}: {e}"));
		forwarded(&mut theirs);
		let copy = vendored.join(format!("{service}.json"));
		let ours: serde_json::Value =
			serde_json::from_str(&std::fs::read_to_string(&copy).unwrap_or_else(|e| panic!("{}: {e}", copy.display()))).unwrap_or_else(|e| panic!("{}: {e}", copy.display()));
		if ours != theirs {
			lagging.push(format!("{service} (latest release {tag})"));
		}
	}
	assert!(lagging.is_empty(), "vendored documents not yet released; release these first: {}", lagging.join(", "));
}

/// A service's document as its operations are answered through the forward: the forward's own
/// refusals beside each operation's, as its `x-errors`, its answers and its `ErrorCode` list them.
fn forwarded(doc: &mut serde_json::Value) {
	use ev_lib::api::ErrorCode;
	let items = doc["paths"].as_object_mut().expect("a document has paths").values_mut();
	let operations = items.flat_map(|item| {
		item.as_object_mut()
			.expect("a path item")
			.iter_mut()
			.filter(|(m, _)| ["get", "put", "post", "patch", "delete"].contains(&m.as_str()))
	});
	let mut words: Vec<String> = Vec::new();
	for (_, op) in operations {
		let op = op.as_object_mut().expect("an operation is an object");
		for &code in <sa_auth::Hop as strum::VariantArray>::VARIANTS {
			let word = serde_json::to_value(code).expect("a code serializes").as_str().expect("a code is a word").to_owned();
			let status = code.status().as_u16();
			let errors = op.get_mut("x-errors").and_then(serde_json::Value::as_object_mut).expect("every operation has x-errors");
			match errors.get(&word) {
				Some(s) => assert_eq!(s.as_u64(), Some(u64::from(status)), "{word}: the forward's status and the service's differ"),
				None => {
					errors.insert(word.clone(), serde_json::json!(status));
				}
			}
			op.get_mut("responses")
				.and_then(serde_json::Value::as_object_mut)
				.expect("an operation has its responses")
				.entry(status.to_string())
				.or_insert_with(|| serde_json::json!({ "description": format!("`{word}`"), "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Error" } } } }));
			if !words.contains(&word) {
				words.push(word);
			}
		}
	}
	let listed = doc["components"]["schemas"]["ErrorCode"]["enum"]
		.as_array_mut()
		.expect("a service's document names its ErrorCode");
	for word in words {
		if !listed.iter().any(|w| w == word.as_str()) {
			listed.push(serde_json::Value::String(word));
		}
	}
}
