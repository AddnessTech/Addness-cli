use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// TUIから起動するエージェントの応答言語設定。
///
/// - `Auto`: 環境変数 `LANG`（無ければ `LC_ALL`）から判定する
/// - `Ja` / `En`: 明示指定
/// - `Off`: 言語指示を注入しない
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentLanguage {
    #[default]
    Auto,
    Ja,
    En,
    Off,
}

impl AgentLanguage {
    /// `/lang <値>` や config.json の文字列から解釈する。未知の値は `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "ja" | "japanese" | "日本語" => Some(Self::Ja),
            "en" | "english" => Some(Self::En),
            "off" | "none" => Some(Self::Off),
            _ => None,
        }
    }

    /// config / CLI 引数で使う正規値。
    pub fn config_value(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Ja => "ja",
            Self::En => "en",
            Self::Off => "off",
        }
    }

    /// ピッカー・ログ表示用のラベル。
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto（自動判定）",
            Self::Ja => "日本語（ja）",
            Self::En => "English（en）",
            Self::Off => "off（指示しない）",
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    /// 現在選択中の組織のID
    /// getter, setterを介して、取得、更新(およびsave)する
    #[serde(skip_serializing_if = "Option::is_none")]
    current_organization_id: Option<String>,
    /// TUIから起動するエージェントの応答言語設定。
    /// 既存configとの後方互換のため serde default（未設定は `Auto`）。
    #[serde(default)]
    agent_language: AgentLanguage,
    /// Codex TUIでAddnessの左ペインを表示するか。未設定の旧configはCodex風にする。
    #[serde(default = "default_codex_left_panel_collapsed")]
    codex_left_panel_collapsed: bool,
}

fn default_codex_left_panel_collapsed() -> bool {
    true
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

    pub fn current_organization_id(&self) -> Option<&str> {
        self.current_organization_id.as_deref()
    }

    pub fn set_current_organization_id(&mut self, org_id: String) -> Result<()> {
        self.current_organization_id = Some(org_id);
        self.save()
    }

    pub fn agent_language(&self) -> AgentLanguage {
        self.agent_language
    }

    pub fn set_agent_language(&mut self, language: AgentLanguage) -> Result<()> {
        self.agent_language = language;
        self.save()
    }

    pub fn codex_left_panel_collapsed(&self) -> bool {
        self.codex_left_panel_collapsed
    }

    #[cfg_attr(test, allow(dead_code))]
    pub fn set_codex_left_panel_collapsed(&mut self, collapsed: bool) -> Result<()> {
        self.codex_left_panel_collapsed = collapsed;
        self.save()
    }

    pub fn delete() -> Result<()> {
        let path = settings_path()?;
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("Failed to delete {}", path.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_language_parse_accepts_known_values() {
        assert_eq!(AgentLanguage::parse("auto"), Some(AgentLanguage::Auto));
        assert_eq!(AgentLanguage::parse("JA"), Some(AgentLanguage::Ja));
        assert_eq!(AgentLanguage::parse("english"), Some(AgentLanguage::En));
        assert_eq!(AgentLanguage::parse(" off "), Some(AgentLanguage::Off));
        assert_eq!(AgentLanguage::parse("日本語"), Some(AgentLanguage::Ja));
        assert_eq!(AgentLanguage::parse("bogus"), None);
    }

    #[test]
    fn agent_language_config_value_round_trips_through_parse() {
        for lang in [
            AgentLanguage::Auto,
            AgentLanguage::Ja,
            AgentLanguage::En,
            AgentLanguage::Off,
        ] {
            assert_eq!(AgentLanguage::parse(lang.config_value()), Some(lang));
        }
    }

    #[test]
    fn settings_defaults_agent_language_to_auto_for_legacy_config() {
        // agent_language を持たない旧 config.json でも読み込めること。
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings.agent_language(), AgentLanguage::Auto);
    }

    #[test]
    fn settings_defaults_codex_left_panel_to_collapsed_for_legacy_config() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(settings.codex_left_panel_collapsed());
    }

    #[test]
    fn settings_round_trip_codex_left_panel_preference() {
        let settings: Settings =
            serde_json::from_str(r#"{"codex_left_panel_collapsed":true}"#).unwrap();
        assert!(settings.codex_left_panel_collapsed());
    }
}
