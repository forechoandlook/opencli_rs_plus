use anyhow::{Context, Result};
use std::fs;
use std::path::PathBuf;

fn current_exe() -> Result<PathBuf> {
    std::env::current_exe().context("failed to locate current executable")
}

#[cfg(unix)]
fn remove_binary(path: &PathBuf) -> Result<()> {
    fs::remove_file(path).with_context(|| format!("failed to remove {}", path.display()))
}

#[cfg(windows)]
fn remove_binary(_path: &PathBuf) -> Result<()> {
    anyhow::bail!(
        "self-uninstall is not supported on Windows while the executable is running"
    )
}

pub fn run_uninstall() -> Result<()> {
    let exe = current_exe()?;
    println!("Current executable: {}", exe.display());
    remove_binary(&exe)?;
    println!("Uninstalled opencli.");
    println!("If the binary was installed via a package manager or symlink, remove that wrapper too.");
    Ok(())
}
