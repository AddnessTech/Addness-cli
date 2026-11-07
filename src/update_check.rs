use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

use anyhow::{Context, Result, bail};
use semver::Version;

pub const CDN_BASE_URL: &str = "https://d22sm2skny3y0p.cloudfront.net";
pub const CDN_VERSION_URL: &str =
    "https://d22sm2skny3y0p.cloudfront.net/releases/latest/version.txt";
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
    parse_version(&latest).map(|version| version.to_string())
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
    let executable = match run_installer(latest) {
        Ok(executable) => executable,
        Err(e) => {
            eprintln!("  \x1b[2mAuto-update failed ({e}); continuing with v{current}\x1b[0m");
            eprintln!("  \x1b[2mYou can retry manually: addness update\x1b[0m");
            eprintln!();
            return;
        }
    };
    eprintln!("  \x1b[32mUpdated to v{latest}. Relaunching...\x1b[0m");
    eprintln!();

    relaunch_with_current_args(&executable);
    // 到達した場合は再起動に失敗しているので、現在のバイナリでそのまま続行する。
}

/// このバイナリに同梱された公式インストーラで、確認済みの版を実行中の場所へ入れる。
/// インストーラがターゲット判定・チェックサム検証・バイナリ置換を担う。
/// `addness update` コマンドと起動時の自動更新の両方から使う。
pub(crate) fn run_installer(version: &str) -> Result<PathBuf> {
    let version = Version::parse(version).context("Invalid update version")?;
    let executable = std::env::current_exe().context("Could not locate the installed CLI")?;
    let install_dir = executable
        .parent()
        .context("CLI has no install directory")?;

    #[cfg(windows)]
    let mut installer = {
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-Command", include_str!("../install.ps1")]);
        command
    };

    #[cfg(not(windows))]
    let mut installer = {
        let mut command = Command::new("sh");
        command.args(["-c", include_str!("../install.sh")]);
        command
    };

    // Pass values as environment variables, never interpolate them into shell code.
    // Updates must replace this executable, even for custom installation paths.
    let status = installer
        .env("ADDNESS_CDN_BASE", CDN_BASE_URL)
        .env("ADDNESS_VERSION", format!("v{version}"))
        .env("ADDNESS_INSTALL_DIR", install_dir)
        .env("ADDNESS_NO_MODIFY_PATH", "1")
        .status();

    match status {
        Ok(s) if s.success() => {
            verify_installed_version(&executable, &version)?;
            Ok(executable)
        }
        Ok(s) => bail!("Installer exited with status {s}"),
        Err(e) => bail!("Failed to launch the installer: {e}"),
    }
}

fn verify_installed_version(executable: &Path, expected: &Version) -> Result<()> {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .context("Could not verify the installed CLI")?;
    let actual = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || actual.trim() != format!("addness {expected}") {
        bail!("Update verification failed: expected addness {expected}, got {actual:?}");
    }
    Ok(())
}

/// 更新直後、同じコマンドライン引数で新しいバイナリへ実行を引き継ぐ。
/// Unix ではプロセスイメージを置き換えるため成功時は戻らない。失敗時のみ return する。
fn relaunch_with_current_args(exe: &Path) {
    let args: Vec<String> = std::env::args().skip(1).collect();

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(exe).args(&args).exec();
        eprintln!("  \x1b[2mFailed to relaunch after update: {err}\x1b[0m");
    }

    #[cfg(not(unix))]
    {
        match Command::new(exe).args(&args).status() {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(e) => eprintln!("  \x1b[2mFailed to relaunch after update: {e}\x1b[0m"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{is_update_available, parse_version};

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

    #[test]
    fn release_metadata_must_be_a_version() {
        assert_eq!(parse_version("v0.16.1\n").unwrap().to_string(), "0.16.1");
        for invalid in [
            "",
            "<html>503</html>",
            "latest",
            "0.16.1\n0.16.2",
            "0.16.1; echo x",
        ] {
            assert!(parse_version(invalid).is_none(), "{invalid}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn installer_success_requires_the_expected_executable_version() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            std::env::temp_dir().join(format!("addness-update-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let executable = directory.join("addness");
        let expected = semver::Version::parse("0.16.1").unwrap();
        for (script, succeeds) in [
            ("#!/bin/sh\necho 'addness 0.16.1'\n", true),
            ("#!/bin/sh\necho 'addness 0.16.0'\n", false),
            ("#!/bin/sh\necho 'addness 0.16.1'\nexit 1\n", false),
        ] {
            std::fs::write(&executable, script).unwrap();
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
            assert_eq!(
                super::verify_installed_version(&executable, &expected).is_ok(),
                succeeds
            );
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}
