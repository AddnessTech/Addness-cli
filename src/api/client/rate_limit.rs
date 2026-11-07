//! 429 の待機期限を同じ API キーを使う CLI プロセス間で共有する。
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

#[derive(Clone)]
pub(super) struct RateLimitGuard {
    path: PathBuf,
}

#[derive(Default, Deserialize, Serialize)]
struct State {
    until: u64,
    backoff: u64,
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

    pub(super) fn check(&self) -> Result<()> {
        self.check_at(now())
    }

    fn check_at(&self, now: u64) -> Result<()> {
        let mut file = match File::open(&self.path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error).context("Could not read the rate-limit cooldown"),
        };
        file.lock_shared()
            .context("Could not lock the rate-limit cooldown")?;
        let state = read_state(&mut file)?;
        if state.until > now {
            bail!("{} No request was sent.", wait_message(state.until - now));
        }
        Ok(())
    }

    pub(super) fn record(&self, retry_after: Option<&str>) -> Result<String> {
        let now = now();
        let wait = self.record_at(retry_after, now)?;
        Ok(wait_message(wait))
    }

    fn record_at(&self, retry_after: Option<&str>, now: u64) -> Result<u64> {
        fs::create_dir_all(self.path.parent().expect("rate-limit directory"))
            .context("Could not create the rate-limit directory")?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&self.path)
            .context("Could not save the rate-limit cooldown")?;
        file.lock()
            .context("Could not lock the rate-limit cooldown")?;
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
        };
        let bytes = serde_json::to_vec(&state)?;
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&bytes)?;
        file.set_len(bytes.len() as u64)?;
        file.sync_data()?;
        Ok(state.until.saturating_sub(now))
    }
}

fn read_state(file: &mut File) -> Result<State> {
    let mut bytes = Vec::new();
    file.take(4_097).read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return Ok(State::default());
    }
    serde_json::from_slice(&bytes).context("Invalid local rate-limit cooldown; no request was sent")
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
