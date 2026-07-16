use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Result, bail};
use semver::Version;

pub const CDN_VERSION_URL: &str = "https://cli.addness.com/releases/latest/version.txt";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// CDN から最新バージョン文字列（先頭 `v` を除去）を取得する。
/// 失敗時は None。`update` コマンドと起動時チェックで共有する。
pub async fn fetch_latest_version() -> Option<String> {
    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::builder()
        .user_agent(format!("addness-cli/{current}"))
        .timeout(Duration::from_secs(5))
        .build()
        .ok()?;

    let resp = client.get(CDN_VERSION_URL).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let latest = resp.text().await.ok()?;
    let latest = latest.trim().trim_start_matches('v').to_string();
    if latest.is_empty() {
        None
    } else {
        Some(latest)
    }
}

fn check_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".addness").join("last_update_check"))
}

fn should_check() -> bool {
    let Some(path) = check_file_path() else {
        return false;
    };
    match fs::metadata(&path).and_then(|m| m.modified()) {
        Ok(modified) => {
            SystemTime::now()
                .duration_since(modified)
                .unwrap_or(Duration::ZERO)
                > CHECK_INTERVAL
        }
        Err(_) => true,
    }
}

fn touch_check_file() {
    if let Some(path) = check_file_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, "");
    }
}

fn parse_version(version: &str) -> Option<Version> {
    Version::parse(version.trim().trim_start_matches('v')).ok()
}

pub fn is_update_available(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(current), Some(latest)) => latest > current,
        _ => false,
    }
}

/// 起動前に更新をチェックし、更新があれば公式インストーラで自動更新した上で
/// 同じ引数で新しいバイナリに実行を委譲する。
///
/// 更新チェック自体は24時間に1回のみ（`should_check`）なので、通常の起動には
/// ネットワーク待ちを追加しない。更新チェックが必要なタイミングで新版が見つかった
/// 場合のみ、インストーラ実行 → 再起動という同期フローが走る。インストーラ失敗時は
/// 警告を出すだけで起動をブロックしない（現在のバイナリでそのまま続行する）。
pub async fn auto_update_before_launch() {
    if !should_check() {
        return;
    }
    touch_check_file();

    let current = env!("CARGO_PKG_VERSION");
    let Some(latest) = fetch_latest_version().await else {
        return;
    };
    let latest = latest.as_str();

    if !is_update_available(current, latest) {
        return;
    }

    eprintln!();
    eprintln!("  \x1b[33mUpdating addness: v{current} → v{latest}...\x1b[0m");
    if let Err(e) = run_installer() {
        eprintln!("  \x1b[2mAuto-update failed ({e}); continuing with v{current}\x1b[0m");
        eprintln!("  \x1b[2mYou can retry manually: addness update\x1b[0m");
        eprintln!();
        return;
    }
    eprintln!("  \x1b[32mUpdated to v{latest}. Relaunching...\x1b[0m");
    eprintln!();

    relaunch_with_current_args();
    // 到達した場合は再起動に失敗しているので、現在のバイナリでそのまま続行する。
}

/// プラットフォーム別に公式インストーラを実行する。
/// インストーラがターゲット判定・チェックサム検証・バイナリ置換を担う。
/// `addness update` コマンドと起動時の自動更新の両方から使う。
pub(crate) fn run_installer() -> Result<()> {
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

/// 更新直後、同じコマンドライン引数で新しいバイナリへ実行を引き継ぐ。
/// Unix ではプロセスイメージを置き換えるため成功時は戻らない。失敗時のみ return する。
fn relaunch_with_current_args() {
    let Ok(exe) = std::env::current_exe() else {
        eprintln!(
            "  \x1b[2mFailed to relaunch after update: could not resolve current executable\x1b[0m"
        );
        return;
    };
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(&exe).args(&args).exec();
        eprintln!("  \x1b[2mFailed to relaunch after update: {err}\x1b[0m");
    }

    #[cfg(not(unix))]
    {
        match Command::new(&exe).args(&args).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(e) => eprintln!("  \x1b[2mFailed to relaunch after update: {e}\x1b[0m"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_update_available;

    #[test]
    fn update_available_only_when_latest_is_newer() {
        assert!(is_update_available("0.5.8", "0.5.9"));
        assert!(is_update_available("v0.5.8", "v0.6.0"));
        assert!(!is_update_available("0.5.8", "0.5.8"));
        assert!(!is_update_available("0.5.8", "0.5.7"));
    }

    #[test]
    fn update_available_ignores_invalid_versions() {
        assert!(!is_update_available("0.5.8", "latest"));
        assert!(!is_update_available("dev", "0.5.9"));
    }
}
