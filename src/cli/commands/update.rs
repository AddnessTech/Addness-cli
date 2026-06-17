use std::process::Command;

use anyhow::{Result, bail};

use crate::update_check::{CDN_VERSION_URL, fetch_latest_version};

/// `addness update` — 公式インストーラ経由で最新版へ更新する。
///
/// ダウンロード・sha256検証・バイナリ置換は install.sh / install.ps1 に委譲し、
/// ここではバージョン比較とインストーラ起動のみを行う（新規依存ゼロ）。
pub async fn handle_update(check_only: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    let Some(latest) = fetch_latest_version().await else {
        bail!("Failed to fetch the latest version from {CDN_VERSION_URL}");
    };

    if latest == current {
        println!("Already up to date (v{current})");
        return Ok(());
    }

    println!("Update available: v{current} → v{latest}");

    if check_only {
        if cfg!(windows) {
            println!("Run `addness update` (or: irm https://cli.addness.com/install.ps1 | iex)");
        } else {
            println!(
                "Run `addness update` (or: curl -fsSL https://cli.addness.com/install.sh | sh)"
            );
        }
        return Ok(());
    }

    println!("Updating via the official installer...");
    run_installer()?;
    println!("Updated to v{latest}. Restart any running `addness` sessions to use it.");
    Ok(())
}

/// プラットフォーム別に公式インストーラを実行する。
/// インストーラがターゲット判定・チェックサム検証・バイナリ置換を担う。
fn run_installer() -> Result<()> {
    #[cfg(windows)]
    let status = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "irm https://cli.addness.com/install.ps1 | iex",
        ])
        .status();

    #[cfg(not(windows))]
    let status = Command::new("sh")
        .arg("-c")
        .arg("curl -fsSL https://cli.addness.com/install.sh | sh")
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => bail!("Installer exited with status {s}"),
        Err(e) => bail!("Failed to launch the installer: {e}"),
    }
}
