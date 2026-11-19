//! `contracts/proto/concierge/v1/*.proto` (vendored from concierge, see its README) → tonic
//! clients and servers for the sign-in: the conductor is a relying party of concierge.
//!
//! The protos are parsed by protox, in Rust, rather than by `protoc`: the build then needs
//! nothing installed, which the CI runners (plain Ubuntu) and the nix sandbox both rely on.

use std::{
	fs,
	path::{Path, PathBuf},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
	let proto_root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?).join("../../contracts/proto");
	let dir = proto_root.join("concierge/v1");
	let watched = Path::new("../../contracts/proto/concierge/v1"); // relative to the package: the same from any checkout of it
	println!("cargo:rerun-if-changed={}", watched.display());
	let mut protos = Vec::new();
	for entry in fs::read_dir(&dir)? {
		let path = entry?.path();
		if path.extension().is_some_and(|ext| ext == "proto") {
			println!("cargo:rerun-if-changed={}", watched.join(path.file_name().expect("read_dir names its entries")).display());
			protos.push(path);
		}
	}
	protos.sort();
	let descriptors = protox::compile(&protos, [proto_root])?;
	tonic_prost_build::configure().build_client(true).build_server(true).compile_fds(descriptors)?;
	Ok(())
}
