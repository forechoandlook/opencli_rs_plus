//! Boot-time autostart management for the opencli daemon (macOS launchd / Linux systemd --user).

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Command;

const LABEL: &str = "com.opencli.daemon";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Library/LaunchAgents")
        .join(format!("{}.plist", LABEL))
}

fn systemd_unit_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config/systemd/user/opencli-daemon.service")
}

pub struct AutostartStatus {
    pub platform: &'static str,
    pub file_path: PathBuf,
    pub installed: bool,
    pub loaded: bool,
}

pub fn status() -> AutostartStatus {
    if cfg!(target_os = "macos") {
        let path = plist_path();
        let installed = path.exists();
        let loaded = Command::new("launchctl")
            .args(["list", LABEL])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        AutostartStatus {
            platform: "launchd",
            file_path: path,
            installed,
            loaded,
        }
    } else {
        let path = systemd_unit_path();
        let installed = path.exists();
        let loaded = Command::new("systemctl")
            .args(["--user", "is-enabled", "opencli-daemon.service"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        AutostartStatus {
            platform: "systemd --user",
            file_path: path,
            installed,
            loaded,
        }
    }
}

pub fn install(addr: &str) -> Result<()> {
    let exe = std::env::current_exe()?;
    let log_path = super::default_log_path();
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if cfg!(target_os = "macos") {
        let path = plist_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let plist = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>daemon</string>
        <string>--addr</string>
        <string>{addr}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{log}</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
            label = LABEL,
            exe = exe.display(),
            addr = addr,
            log = log_path.display(),
        );
        std::fs::write(&path, plist)?;
        let out = Command::new("launchctl")
            .args(["load", "-w"])
            .arg(&path)
            .output()?;
        if !out.status.success() {
            return Err(anyhow!(
                "launchctl load failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    } else {
        let path = systemd_unit_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        let unit = format!(
            r#"[Unit]
Description=opencli daemon

[Service]
ExecStart={exe} daemon --addr {addr}
Restart=on-failure
StandardOutput=append:{log}
StandardError=append:{log}

[Install]
WantedBy=default.target
"#,
            exe = exe.display(),
            addr = addr,
            log = log_path.display(),
        );
        std::fs::write(&path, unit)?;
        Command::new("systemctl")
            .args(["--user", "daemon-reload"])
            .output()?;
        let out = Command::new("systemctl")
            .args(["--user", "enable", "--now", "opencli-daemon.service"])
            .output()?;
        if !out.status.success() {
            return Err(anyhow!(
                "systemctl enable failed: {}",
                String::from_utf8_lossy(&out.stderr)
            ));
        }
        Ok(())
    }
}

pub fn uninstall() -> Result<()> {
    if cfg!(target_os = "macos") {
        let path = plist_path();
        if path.exists() {
            let _ = Command::new("launchctl").args(["unload", "-w"]).arg(&path).output();
            std::fs::remove_file(&path)?;
        }
        Ok(())
    } else {
        let path = systemd_unit_path();
        if path.exists() {
            let _ = Command::new("systemctl")
                .args(["--user", "disable", "--now", "opencli-daemon.service"])
                .output();
            std::fs::remove_file(&path)?;
            let _ = Command::new("systemctl")
                .args(["--user", "daemon-reload"])
                .output();
        }
        Ok(())
    }
}
