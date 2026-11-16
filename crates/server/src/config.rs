//! The config file (`~/.config/sa_conductor.nix`, or `--config`): what repeats and nests, which
//! the flat environment ([`crate::settings`]) cannot carry. Its secrets are `{ env = "…" }`.

use v_utils::macros::{ConfigJsonSchema, MyConfigPrimitives, Settings};

#[derive(Clone, ConfigJsonSchema, Debug, MyConfigPrimitives, Settings)]
#[settings(config_name = "sa_conductor")]
pub struct Config {
	/// The Google billing accounts our keys bill on, in the order ties are broken in.
	#[settings(skip)]
	pub google: Vec<sa_places::Account>,
}
