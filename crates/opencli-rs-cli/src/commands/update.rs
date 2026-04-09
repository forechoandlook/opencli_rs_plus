use anyhow::{anyhow, bail, Context, Result};
use semver::Version;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

const REPO: &str = "forechoandlook/opencli_rs_plus";
const RELEASES_API: &str =
    "https://api.github.com/repos/forechoandlook/opencli_rs_plus/releases/latest";

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

fn parse_version(raw: &str) -> Result<Version> {
    Version::parse(raw.trim_start_matches('v'))
        .with_context(|| format!("invalid version string: {raw}"))
}

fn current_version() -> Result<Version> {
    parse_version(env!("CARGO_PKG_VERSION"))
}

fn asset_name_for_current_platform() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("opencli-macos-arm64"),
        ("linux", "x86_64") => Ok("opencli-linux-x86_64"),
        ("windows", "x86_64") => Ok("opencli-windows-x86_64.exe"),
        (os, arch) => bail!("automatic update is not supported on {os}/{arch}"),
    }
}

async fn fetch_latest_release() -> Result<(ReleaseInfo, Version)> {
    let client = reqwest::Client::builder()
        .user_agent(format!("opencli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;

    let response = client
        .get(RELEASES_API)
        .send()
        .await
        .context("failed to query latest release")?
        .error_for_status()
        .context("latest release query failed")?;

    let release: ReleaseInfo = response
        .json()
        .await
        .context("failed to parse latest release response")?;
    let latest = parse_version(&release.tag_name)?;
    Ok((release, latest))
}

fn temp_download_path(asset_name: &str) -> PathBuf {
    let pid = std::process::id();
    std::env::temp_dir().join(format!("{asset_name}.{pid}.download"))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

fn replace_current_exe(downloaded: &Path, current_exe: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        let _ = downloaded;
        let _ = current_exe;
        bail!("automatic in-place replacement is not supported on Windows yet")
    }

    #[cfg(not(windows))]
    {
        fs::rename(downloaded, current_exe).with_context(|| {
            format!(
                "failed to replace current executable at {}",
                current_exe.display()
            )
        })?;
        Ok(())
    }
}

pub async fn run_update(check_only: bool) -> Result<()> {
    let current = current_version()?;
    let (release, latest) = fetch_latest_release().await?;

    println!("Current version: {}", current);
    println!("Latest version:  {}", latest);

    if latest <= current {
        println!("Already up to date.");
        return Ok(());
    }

    println!("Update available: {}", release.html_url);
    if check_only {
        return Ok(());
    }

    let asset_name = asset_name_for_current_platform()?;
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| anyhow!("release asset not found for current platform: {asset_name}"))?;

    let current_exe = std::env::current_exe().context("failed to locate current executable")?;
    let download_path = temp_download_path(asset_name);

    let client = reqwest::Client::builder()
        .user_agent(format!("opencli/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .context("failed to build HTTP client")?;
    let bytes = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .with_context(|| format!("failed to download {asset_name}"))?
        .error_for_status()
        .with_context(|| format!("download failed for {asset_name}"))?
        .bytes()
        .await
        .context("failed to read downloaded binary")?;

    fs::write(&download_path, &bytes)
        .with_context(|| format!("failed to write {}", download_path.display()))?;
    make_executable(&download_path)?;
    replace_current_exe(&download_path, &current_exe)?;

    println!("Updated opencli to {}.", latest);
    println!("Binary path: {}", current_exe.display());
    println!("Restart any running opencli commands to use the new version.");
    println!("Source: https://github.com/{REPO}/releases/tag/{}", release.tag_name);
    Ok(())
}
