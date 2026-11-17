//! The origin's path table (`forward::paths`) against docs/ARCHITECTURE.md's Forward section.

use std::collections::BTreeSet;

use sa_server::forward;

#[test]
fn the_forward_section_names_every_prefix_and_what_the_edge_does_with_it() {
	let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ARCHITECTURE.md")).unwrap();
	let section = &doc[doc.find("## Forward\n").expect("the section")..];
	let section = &section[..section[1..].find("\n## ").expect("the next section")];
	let blocks: Vec<&str> = section.split("```text\n").skip(1).map(|b| &b[..b.find("```").expect("closed")]).collect();
	let [forwarded, edge] = blocks[..] else {
		panic!("the Forward section has the forward's block and the edge's")
	};
	let listed: BTreeSet<(String, bool, bool)> = edge
		.lines()
		.map(|l| {
			let mut columns = l.split("  ").map(str::trim).filter(|c| !c.is_empty());
			let (prefix, at_edge) = (columns.next().expect("a prefix"), columns.next().expect("what the edge does"));
			(prefix.to_owned(), at_edge.contains("rate-limited"), !at_edge.contains("in-cluster"))
		})
		.collect();
	let table: BTreeSet<(String, bool, bool)> = forward::paths().map(|p| (p.prefix.to_owned(), p.rate_limited, p.published)).collect();
	assert_eq!(listed, table);
	for l in forwarded.lines() {
		let prefix = l.split_whitespace().next().expect("a prefix").trim_end_matches("/*");
		assert!(table.iter().any(|(p, ..)| p == prefix), "{prefix}: forwarded, and a prefix of the origin");
	}
}
