use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

const CDN_VERSION_URL: &str = "https://cli.addness.com/releases/latest/version.txt";
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

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

    let Ok(client) = reqwest::Client::builder()
        .user_agent(format!("addness-cli/{current}"))
        .timeout(Duration::from_secs(3))
        .build()
    else {
        return;
    };

    let Ok(resp) = client.get(CDN_VERSION_URL).send().await else {
        return;
    };

    if !resp.status().is_success() {
        return;
    }

    let Ok(latest) = resp.text().await else {
        return;
    };

    let latest = latest.trim().trim_start_matches('v');

    if latest != current && !latest.is_empty() {
        eprintln!();
        eprintln!("  \x1b[33mA new version of addness is available: v{current} → v{latest}\x1b[0m");
        if cfg!(windows) {
            eprintln!("  \x1b[2mUpdate: irm https://cli.addness.com/install.ps1 | iex\x1b[0m");
        } else {
            eprintln!("  \x1b[2mUpdate: curl -fsSL https://cli.addness.com/install.sh | sh\x1b[0m");
        }
        eprintln!();
    }
}
