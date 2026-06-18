use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

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

pub async fn check_for_update() {
    if !should_check() {
        return;
    }

    touch_check_file();

    let current = env!("CARGO_PKG_VERSION");

    let Some(latest) = fetch_latest_version().await else {
        return;
    };
    let latest = latest.as_str();

    if latest != current {
        eprintln!();
        eprintln!("  \x1b[33mA new version of addness is available: v{current} → v{latest}\x1b[0m");
        eprintln!("  \x1b[2mUpdate: addness update\x1b[0m");
        eprintln!();
    }
}
