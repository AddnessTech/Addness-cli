use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// 現在選択中の組織のID
    /// getter, setterを介して、取得、更新(およびsave)する
    #[serde(skip_serializing_if = "Option::is_none")]
    current_organization_id: Option<String>,
}

fn settings_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not find home directory")?;
    Ok(home.join(".addness").join("config.json"))
}

impl Settings {
    pub fn load() -> Result<Self> {
        let path = settings_path()?;
        if !path.exists() {
            return Ok(Settings::default());
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let settings = serde_json::from_str(&content).context("Failed to parse config.json")?;

        Ok(settings)
    }

    /// 現在のSettingsであるselfをsaveする
    /// Settingsの各フィールドのset_関数が責任を持ってsave()を呼び出す
    fn save(&self) -> Result<()> {
        let path = settings_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory {}", parent.display()))?;
        }
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, &content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        Ok(())
    }

    pub fn current_organization_id(&self) -> Option<&str> {
        self.current_organization_id.as_deref()
    }

    pub fn set_current_organization_id(&mut self, org_id: String) -> Result<()> {
        self.current_organization_id = Some(org_id);
        self.save()
    }
}
