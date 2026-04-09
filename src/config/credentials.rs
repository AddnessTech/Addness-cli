use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::Settings;

/// Key used when migrating a legacy token without a known org ID.
const DEFAULT_ORGANIZATION_KEY: &str = "_default";

#[derive(Debug, Serialize, Deserialize)]
pub struct Credentials {
    /// Legacy single-token field — read-only for migration, never written back.
    #[serde(default, skip_serializing)]
    token: Option<String>,

    #[serde(default = "default_api_url")]
    api_url: String,

    /// org_id -> API token map.
    #[serde(default)]
    organizations: HashMap<String, CredOrganization>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct CredOrganization {
    api_key: String,
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
        Self {
            token: None,
            api_url: default_api_url(),
            organizations: HashMap::new(),
        }
    }
}

impl Credentials {
    pub fn new(api_url: String) -> Self {
        Self {
            token: None,
            api_url,
            organizations: HashMap::new(),
        }
    }

    pub fn api_url(&self) -> &str {
        &self.api_url
    }

    pub fn set_api_url(&mut self, url: String) {
        self.api_url = url;
    }

    /// Get the token stored for a specific org.
    pub fn token_for_org(&self, org_id: &str) -> Option<&str> {
        self.organizations
            .get(org_id)
            .map(|org| org.api_key.as_str())
    }

    /// Get the token for the current org (from Settings).
    /// Falls back to `_default` if the current org has no dedicated key.
    #[allow(dead_code)]
    pub fn token_for_current_org(&self) -> Result<Option<&str>> {
        let settings = Settings::load()?;
        if let Some(org_id) = settings.current_organization_id()
            && let Some(org) = self.organizations.get(org_id)
        {
            return Ok(Some(org.api_key.as_str()));
        }
        // Fall back to _default (legacy migration)
        Ok(self
            .organizations
            .get(DEFAULT_ORGANIZATION_KEY)
            .map(|org| org.api_key.as_str()))
    }

    /// Return any stored token — used for org-independent endpoints.
    pub fn any_token(&self) -> Option<&str> {
        self.organizations
            .values()
            .next()
            .map(|org| org.api_key.as_str())
    }

    /// Store (or overwrite) a token for the given org.
    pub fn set_token(&mut self, org_id: String, token: String) {
        self.organizations
            .insert(org_id, CredOrganization { api_key: token });
    }

    /// Whether a token exists for the given org.
    pub fn has_token_for_org(&self, org_id: &str) -> bool {
        self.organizations.contains_key(org_id)
    }

    /// Remove the token for a given org (e.g. `_default`).
    pub fn remove_token(&mut self, org_id: &str) {
        self.organizations.remove(org_id);
    }

    /// All stored organizations.
    #[allow(dead_code)]
    pub fn organizations(&self) -> &HashMap<String, CredOrganization> {
        &self.organizations
    }

    /// Number of stored org tokens.
    pub fn organization_count(&self) -> usize {
        self.organizations.len()
    }

    pub fn load() -> Result<Option<Credentials>> {
        let path = credentials_path()?;
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let mut creds: Credentials =
            serde_json::from_str(&content).context("Failed to parse credentials.json")?;

        // Migrate legacy single-token format
        if let Some(legacy_token) = creds.token.take()
            && !legacy_token.is_empty()
            && creds.organizations.is_empty()
        {
            let settings = Settings::load()?;
            let org_key = settings
                .current_organization_id()
                .unwrap_or(DEFAULT_ORGANIZATION_KEY)
                .to_string();
            creds.organizations.insert(
                org_key,
                CredOrganization {
                    api_key: legacy_token,
                },
            );
            // Persist the migrated format immediately
            creds.save()?;
        }

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
