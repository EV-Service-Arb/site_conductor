//! Points and rectangles on the globe, as Google draws them.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct LatLng {
	pub lat: f64,
	pub lng: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize, schemars::JsonSchema)]
pub struct Viewport {
	pub south: f64,
	pub west: f64,
	pub north: f64,
	pub east: f64,
}
