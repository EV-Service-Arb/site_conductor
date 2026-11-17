//! The operations table against the human overview of it in docs/ARCHITECTURE.md.

use std::collections::BTreeSet;

use sa_server::http;

/// `{a,b}` in a path is two paths; `{id}` is a parameter.
fn expand(path: &str) -> Vec<String> {
	let mut at = 0;
	while let Some(open) = path[at..].find('{').map(|i| at + i) {
		let close = open + path[open..].find('}').expect("a brace closes");
		if path[open..close].contains(',') {
			return path[open + 1..close]
				.split(',')
				.flat_map(|alt| expand(&format!("{}{alt}{}", &path[..open], &path[close + 1..])))
				.collect();
		}
		at = close;
	}
	vec![path.to_owned()]
}

#[test]
fn the_overview_names_every_operation_and_no_other() {
	let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ARCHITECTURE.md")).unwrap();
	let section = &doc[doc.find("## The conductor's API, `/api/v1`").expect("the section")..];
	let block = &section[section.find("```text\n").expect("its block") + 8..];
	let block = &block[..block.find("```").expect("closed")];
	let overview: BTreeSet<(String, String)> = block
		.lines()
		.flat_map(|l| {
			let mut words = l.split_whitespace();
			let method = words.next().expect("a method").to_owned();
			expand(words.next().expect("a path")).into_iter().map(move |p| (method.clone(), p))
		})
		.collect();
	let document = http::document();
	let table: BTreeSet<(String, String)> = document.operations().map(|(path, method, _)| (method.to_uppercase(), path.to_owned())).collect();
	assert_eq!(overview, table);
}

/// Every operation says what it refuses (`x-errors`), every one behind a gate, and the sockets are
/// rows as well.
#[test]
fn every_operation_names_its_refusals() {
	let document = serde_json::to_value(http::document()).unwrap();
	let mut sockets = BTreeSet::new();
	for (path, item) in document["paths"].as_object().unwrap() {
		for (method, op) in item.as_object().unwrap() {
			let errors = op["x-errors"].as_object().unwrap_or_else(|| panic!("{method} {path} names no x-errors"));
			assert_eq!(errors.get("unauthenticated"), Some(&401.into()), "{method} {path}: a row is gated");
			if op["x-socket"] == true {
				sockets.insert(format!("{} {path}", method.to_uppercase()));
			}
		}
	}
	assert_eq!(sockets, BTreeSet::from(["GET /gmails/{id}/browser".to_owned(), "GET /live".to_owned()]));
}
