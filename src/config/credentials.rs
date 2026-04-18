use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize)]
pub struct Credentials {
    token: String,
    #[serde(default = "default_api_url")]
    api_url: String,
}

pub const DEFAULT_API_URL: &str = "https://vt.api.addness.com";

fn default_api_url() -> String {
    DEFAULT_API_URL.to_string()
}

fn credentials_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".addness").join("credentials.json"))
}

impl Default for Credentials {
    fn default() -> Self {
        Self::new(String::new(), default_api_url())
    }
}

impl Credentials {
    pub fn new(token: String, api_url: String) -> Self {
        Self { token, api_url }
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub fn load() -> Result<Option<Credentials>> {
        let path = credentials_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let creds: Credentials =
            serde_json::from_str(&content).context("Failed to parse credentials.json")?;
        Ok(Some(creds))
    }

    pub fn save(&self) -> Result<()> {
        let path = credentials_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)?;

        #[cfg(unix)]
        {
            use std::io::Write;
            use std::os::unix::fs::OpenOptionsExt;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .with_context(|| format!("Failed to create {}", path.display()))?;
            file.write_all(content.as_bytes())
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }

        #[cfg(not(unix))]
        {
            fs::write(&path, &content)
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }

        Ok(())
    }

    pub fn delete() -> Result<()> {
        let path = credentials_path()?;
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete {}", path.display()))?;
        }
        Ok(())
    }
}
