use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=OPENCLI_GIT_COMMIT");
    println!("cargo:rerun-if-changed=.git/HEAD");

    let commit = std::env::var("OPENCLI_GIT_COMMIT")
        .ok()
        .or_else(|| std::env::var("GITHUB_SHA").ok())
        .or_else(|| git_commit().ok())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=OPENCLI_GIT_COMMIT={commit}");
}

fn git_commit() -> Result<String, std::io::Error> {
    let output = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()?;
    if !output.status.success() {
        return Ok("unknown".to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
