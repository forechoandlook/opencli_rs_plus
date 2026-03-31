pub mod types;
pub mod loader;
pub mod executor;

pub use types::ExternalCli;
pub use loader::load_external_clis;
pub use executor::{execute_external_cli, is_binary_installed};
