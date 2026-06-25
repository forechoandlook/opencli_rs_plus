pub mod args;
pub mod cli_builder;
pub mod commands;
pub mod dispatch;
pub mod runner;

pub use args::coerce_and_validate_args;
pub use opencli_rs_engine::execute_command;
pub use runner::run as run_adapter_main;
