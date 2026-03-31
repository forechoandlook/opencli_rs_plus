use clap::Command;
use clap_complete::{generate, Shell};
use std::io;

pub fn run_completion(app: &mut Command, shell: Shell) {
    generate(shell, app, "opencli-rs", &mut io::stdout());
}
