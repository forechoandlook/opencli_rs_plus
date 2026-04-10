pub mod args;
pub mod cli_builder;
pub mod commands;
pub mod dispatch;
pub mod execution;
pub mod runner;

pub use args::coerce_and_validate_args;
pub use execution::execute_command;
pub use runner::run as run_adapter_main;
