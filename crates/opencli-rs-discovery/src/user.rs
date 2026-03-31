// Adapters are now unified — both built-in and user adapters come from the same
// directory (~/.opencli-rs/adapters/). User files shadow built-ins via
// HashMap insertion order.

pub use crate::builtin::{adapters_dir, discover_adapters};

use opencli_rs_core::{CliError, Registry};

/// Discover user adapters from the adapters directory.
/// Identical to discover_adapters — kept for API compatibility.
pub fn discover_user_adapters(registry: &mut Registry) -> Result<usize, CliError> {
    crate::builtin::discover_adapters(registry)
}
