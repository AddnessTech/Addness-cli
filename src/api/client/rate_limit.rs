//! 送信前の呼び出し上限と 429 の待機期限を CLI プロセス間で共有する。
#[cfg(test)]
mod tests;

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const MIN_WAIT_SECS: u64 = 60;
const MAX_BACKOFF_SECS: u64 = 3_600;
const REQUEST_WINDOWS: [(u64, usize); 2] = [(60, 60), (3_600, 600)];
const MAX_STATE_BYTES: u64 = 32_768;

#[derive(Clone)]
pub(super) struct RateLimitGuard {
    path: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
struct State {
    until: u64,
    backoff: u64,
    #[serde(default)]
    requests: Vec<u64>,
}

impl RateLimitGuard {
    pub(super) fn new(token: &str, base_url: &str) -> Result<Self> {
        let directory = std::env::var_os("ADDNESS_CACHE_DIR")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .or_else(|| dirs::cache_dir().map(|path| path.join("addness")))
            .context("Could not find the Addness cache directory")?;
        Ok(Self::in_directory(directory, token, base_url))
    }

    fn in_directory(directory: PathBuf, token: &str, base_url: &str) -> Self {
        // キー・組織・URL の生値をファイル名や内容に残さない。
        let mut hash = Sha256::new();
        hash.update(base_url.trim_end_matches('/').as_bytes());
        hash.update([0]);
        hash.update(token.as_bytes());
        Self {
            path: directory
                .join("rate-limits")
                .join(format!("{:x}.json", hash.finalize())),
        }
    }

    pub(super) fn reserve(&self) -> Result<()> {
        self.reserve_at(now())
    }

    fn reserve_at(&self, now: u64) -> Result<()> {
        // 確認と予約を同じ排他ロック内で行い、並列起動で上限をすり抜けない。
        let mut file = self.lock_state()?;
        let mut state = read_state(&mut file)?;
        if state.until > now {
            bail!("{} No request was sent.", wait_message(state.until - now));
        }
        state
            .requests
            .retain(|sent| sent.saturating_add(3_600) > now);
        let mut wait = 0;
        for (window, limit) in REQUEST_WINDOWS {
            let recent: Vec<_> = state
                .requests
                .iter()
                .filter(|sent| sent.saturating_add(window) > now)
                .collect();
            if recent.len() >= limit {
                let oldest = **recent.iter().min().expect("non-empty request window");
                wait = wait.max(oldest.saturating_add(window).saturating_sub(now));
            }
        }
        if wait > 0 {
            bail!(
                "Local request budget exhausted (60 requests/minute, 600 requests/hour). \
                 {} No request was sent.",
                wait_message(wait)
            );
        }
        // 接続失敗や読み取りの再試行も消費する。保存できなければ送信しない。
        state.requests.push(now);
        write_state(&mut file, &state)
    }

    pub(super) fn record(&self, retry_after: Option<&str>) -> Result<String> {
        let now = now();
        let wait = self.record_at(retry_after, now)?;
        Ok(wait_message(wait))
    }

    fn lock_state(&self) -> Result<File> {
        fs::create_dir_all(self.path.parent().expect("rate-limit directory"))
            .context("Could not create the rate-limit directory")?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options
            .open(&self.path)
            .context("Could not save the rate-limit cooldown")?;
        file.lock()
            .context("Could not lock the rate-limit cooldown")?;
        Ok(file)
    }

    fn record_at(&self, retry_after: Option<&str>, now: u64) -> Result<u64> {
        let mut file = self.lock_state()?;
        let previous = read_state(&mut file)?;
        // 待機後も 429 が続く場合は最大 1 時間まで延ばす。同時に返った 429 で
        // 指数的に延ばさず、他プロセスが既に保存した長い期限も短縮しない。
        let backoff = if previous.until > now {
            previous.backoff.max(MIN_WAIT_SECS)
        } else if now.saturating_sub(previous.until) <= MAX_BACKOFF_SECS {
            previous
                .backoff
                .saturating_mul(2)
                .clamp(MIN_WAIT_SECS, MAX_BACKOFF_SECS)
        } else {
            MIN_WAIT_SECS
        };
        let wait = retry_after_seconds(retry_after, now)
            .unwrap_or(0)
            .max(backoff);
        let state = State {
            until: previous.until.max(now.saturating_add(wait)),
            backoff,
            requests: previous.requests,
        };
        write_state(&mut file, &state)?;
        Ok(state.until.saturating_sub(now))
    }
}

fn read_state(file: &mut File) -> Result<State> {
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("Local rate-limit state is too large; no request was sent");
    }
    if bytes.is_empty() {
        return Ok(State::default());
    }
    serde_json::from_slice(&bytes).context("Invalid local rate-limit cooldown; no request was sent")
}

fn write_state(file: &mut File, state: &State) -> Result<()> {
    let bytes = serde_json::to_vec(state)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&bytes)?;
    file.set_len(bytes.len() as u64)?;
    file.sync_data()?;
    Ok(())
}

fn retry_after_seconds(value: Option<&str>, now: u64) -> Option<u64> {
    let value = value?.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    httpdate::parse_http_date(value)
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|date| date.as_secs().saturating_sub(now))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn wait_message(seconds: u64) -> String {
    format!(
        "Rate limit: wait at least {seconds}s before using this API key again. \
         Stop polling or retry loops; the cooldown is shared across CLI processes."
    )
}
