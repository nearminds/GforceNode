//! Cross-platform service installation.
//!
//! Provides `install()` / `uninstall()` / `status()` that each delegate
//! to the right OS-specific implementation:
//!
//! | OS      | Mechanism                                                    |
//! |---------|--------------------------------------------------------------|
//! | Linux   | Write systemd unit to `/etc/systemd/system/gforce-node.service` then `systemctl enable --now`. |
//! | macOS   | Write launchd plist to `/Library/LaunchDaemons/com.nearminds.gforce-node.plist` then `launchctl load`. |
//! | Windows | `sc.exe create gforce-node` pointing at the daemon binary. Accepts stop signals via the default Windows service control manager handling. |
//!
//! The Windows path is intentionally minimal — a full-fidelity
//! integration using the `windows-service` crate (so the daemon
//! cooperates with SCM stop/pause/continue and shows up cleanly in
//! Services.msc) is a follow-up once the feature graduates from
//! private beta.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

/// Name under which the service is registered.
pub const SERVICE_NAME: &str = "gforce-node";
/// Human-readable label shown in service listings.
pub const SERVICE_DISPLAY: &str = "Gforce Node Agent";

/// Which OS we are running on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl Platform {
    pub fn current() -> Option<Self> {
        match std::env::consts::OS {
            "linux" => Some(Self::Linux),
            "macos" => Some(Self::Macos),
            "windows" => Some(Self::Windows),
            _ => None,
        }
    }
}

/// Resolve the absolute path of the installed daemon binary.
///
/// We default to `/usr/local/bin/gforce-node-daemon` on unix and
/// `C:\\Program Files\\Gforce\\gforce-node-daemon.exe` on Windows, both
/// of which match where the install scripts (install.sh / install.ps1)
/// place the binary.
pub fn default_daemon_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Gforce\gforce-node-daemon.exe")
    } else {
        PathBuf::from("/usr/local/bin/gforce-node-daemon")
    }
}

/// Install the daemon as a system service appropriate for this OS.
pub fn install(daemon_path: &Path) -> Result<()> {
    match Platform::current().context("Unsupported platform")? {
        Platform::Linux => install_systemd(daemon_path),
        Platform::Macos => install_launchd(daemon_path),
        Platform::Windows => install_windows(daemon_path),
    }
}

/// Uninstall the service.
pub fn uninstall() -> Result<()> {
    match Platform::current().context("Unsupported platform")? {
        Platform::Linux => uninstall_systemd(),
        Platform::Macos => uninstall_launchd(),
        Platform::Windows => uninstall_windows(),
    }
}

/// Return a short human-readable status line.
pub fn status() -> Result<String> {
    match Platform::current().context("Unsupported platform")? {
        Platform::Linux => run_capture("systemctl", &["is-active", SERVICE_NAME]),
        Platform::Macos => run_capture("launchctl", &["list", "com.nearminds.gforce-node"]),
        Platform::Windows => run_capture("sc.exe", &["query", SERVICE_NAME]),
    }
}

// ── systemd (Linux) ────────────────────────────────────────────────────────

const SYSTEMD_UNIT_PATH: &str = "/etc/systemd/system/gforce-node.service";

fn systemd_unit(daemon_path: &Path) -> String {
    format!(
        "[Unit]\n\
         Description={display}\n\
         After=network.target\n\
         Wants=network-online.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         ExecStart={daemon}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         User=root\n\
         Environment=RUST_LOG=info\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n",
        display = SERVICE_DISPLAY,
        daemon = daemon_path.display(),
    )
}

fn install_systemd(daemon_path: &Path) -> Result<()> {
    let contents = systemd_unit(daemon_path);
    std::fs::write(SYSTEMD_UNIT_PATH, contents)
        .with_context(|| format!("writing {SYSTEMD_UNIT_PATH} (needs root)"))?;
    run("systemctl", &["daemon-reload"])?;
    run("systemctl", &["enable", "--now", SERVICE_NAME])?;
    Ok(())
}

fn uninstall_systemd() -> Result<()> {
    // Best-effort stop first; ignore errors so we can proceed.
    let _ = run("systemctl", &["disable", "--now", SERVICE_NAME]);
    if Path::new(SYSTEMD_UNIT_PATH).exists() {
        std::fs::remove_file(SYSTEMD_UNIT_PATH)
            .with_context(|| format!("removing {SYSTEMD_UNIT_PATH}"))?;
    }
    let _ = run("systemctl", &["daemon-reload"]);
    Ok(())
}

// ── launchd (macOS) ────────────────────────────────────────────────────────

const LAUNCHD_PLIST_PATH: &str = "/Library/LaunchDaemons/com.nearminds.gforce-node.plist";

fn launchd_plist(daemon_path: &Path) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>com.nearminds.gforce-node</string>
  <key>ProgramArguments</key>
  <array>
    <string>{daemon}</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/var/log/gforce-node.log</string>
  <key>StandardErrorPath</key>
  <string>/var/log/gforce-node.err.log</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>RUST_LOG</key>
    <string>info</string>
  </dict>
</dict>
</plist>
"#,
        daemon = daemon_path.display()
    )
}

fn install_launchd(daemon_path: &Path) -> Result<()> {
    let contents = launchd_plist(daemon_path);
    std::fs::write(LAUNCHD_PLIST_PATH, contents)
        .with_context(|| format!("writing {LAUNCHD_PLIST_PATH} (needs sudo)"))?;
    run("launchctl", &["load", "-w", LAUNCHD_PLIST_PATH])?;
    Ok(())
}

fn uninstall_launchd() -> Result<()> {
    if Path::new(LAUNCHD_PLIST_PATH).exists() {
        let _ = run("launchctl", &["unload", "-w", LAUNCHD_PLIST_PATH]);
        std::fs::remove_file(LAUNCHD_PLIST_PATH)
            .with_context(|| format!("removing {LAUNCHD_PLIST_PATH}"))?;
    }
    Ok(())
}

// ── Windows Service (sc.exe) ───────────────────────────────────────────────

fn install_windows(daemon_path: &Path) -> Result<()> {
    // `sc create` syntax: binPath= must be quoted if it contains spaces.
    // We pass a pre-quoted string so the registered command is exactly
    // the daemon binary path.
    let bin_path = format!("\"{}\"", daemon_path.display());
    run(
        "sc.exe",
        &[
            "create",
            SERVICE_NAME,
            "binPath=",
            bin_path.as_str(),
            "start=",
            "auto",
            "DisplayName=",
            SERVICE_DISPLAY,
        ],
    )?;
    // Best-effort start; the user can also do it from Services.msc.
    let _ = run("sc.exe", &["start", SERVICE_NAME]);
    Ok(())
}

fn uninstall_windows() -> Result<()> {
    let _ = run("sc.exe", &["stop", SERVICE_NAME]);
    run("sc.exe", &["delete", SERVICE_NAME])?;
    Ok(())
}

// ── shell helpers ──────────────────────────────────────────────────────────

fn run(cmd: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .status()
        .with_context(|| format!("failed to spawn {cmd}"))?;
    if !status.success() {
        anyhow::bail!("{cmd} {args:?} failed with status {status}");
    }
    Ok(())
}

fn run_capture(cmd: &str, args: &[&str]) -> Result<String> {
    let output = Command::new(cmd)
        .args(args)
        .output()
        .with_context(|| format!("failed to spawn {cmd}"))?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if output.status.success() {
        Ok(if stdout.is_empty() { stderr } else { stdout })
    } else {
        Ok(format!(
            "{} {} exited with {} — stderr: {}",
            cmd,
            args.join(" "),
            output.status,
            stderr
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systemd_unit_contains_daemon_path() {
        let unit = systemd_unit(Path::new("/usr/local/bin/gforce-node-daemon"));
        assert!(unit.contains("ExecStart=/usr/local/bin/gforce-node-daemon"));
        assert!(unit.contains("WantedBy=multi-user.target"));
        assert!(unit.contains("Restart=on-failure"));
    }

    #[test]
    fn launchd_plist_contains_daemon_path() {
        let plist = launchd_plist(Path::new("/usr/local/bin/gforce-node-daemon"));
        assert!(plist.contains("com.nearminds.gforce-node"));
        assert!(plist.contains("<string>/usr/local/bin/gforce-node-daemon</string>"));
        assert!(plist.contains("<key>KeepAlive</key>"));
    }

    #[test]
    fn default_daemon_path_picks_platform() {
        let p = default_daemon_path();
        if cfg!(windows) {
            assert!(p.to_string_lossy().ends_with("gforce-node-daemon.exe"));
        } else {
            assert_eq!(p, PathBuf::from("/usr/local/bin/gforce-node-daemon"));
        }
    }
}
