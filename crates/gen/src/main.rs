//! `nix run .#gen`: the conductor's OpenAPI document, which the front end's client and types are
//! generated from (`frontend/src/shared/api/`), committed; the `generated` pre-commit hook re-runs
//! it.

use std::path::Path;

use ev_lib::api::{openapi::SchemaObject, schemars::generate::SchemaSettings};
use sa_browsers::view::{In as ViewerIn, Out as ViewerOut};

fn main() {
	let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../frontend/src/shared/api/openapi.json");
	let mut doc = sa_server::http::Catalog::new().document().clone();
	// The browser viewer's socket is outside the operations; its frames are wire types all the same.
	let mut frames = SchemaSettings::draft07().with(|s| s.definitions_path = "#/components/schemas/".into()).into_generator();
	frames.subschema_for::<ViewerIn>();
	frames.subschema_for::<ViewerOut>();
	let schemas = &mut doc.components.as_mut().expect("the document has components").schemas;
	for (name, schema) in frames.take_definitions(true) {
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
}
