use std::io::{self, Write};

use anyhow::{Result, bail};

use crate::config::{Credentials, Settings};

fn prompt(label: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        print!("{}: ", label);
    } else {
        print!("{} [{}]: ", label, default);
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();

    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input)
    }
}

fn mask_key(key: &str) -> String {
    if key.len() > 10 {
        format!("{}...{}", &key[..6], &key[key.len() - 4..])
    } else {
        "***".to_string()
    }
}

fn ensure_non_empty_api_key(api_key: &str) -> Result<()> {
    if api_key.trim().is_empty() {
        bail!("API Key cannot be empty. Run `addness login` or enter an API key.");
    }
    Ok(())
}

pub fn handle_configure() -> Result<()> {
    println!("Addness CLI Configuration");
    println!();

    let existing_creds = Credentials::load()?.unwrap_or_default();
    let existing_settings = Settings::load()?;

    // Organization ID (ask first so we know where to store the key)
    let default_org = existing_settings
        .current_organization_id()
        .unwrap_or_default();
    let org_id = prompt("Organization ID", default_org)?;

    // API Key — show current key for this org as default
    let current_key = if !org_id.is_empty() {
        existing_creds
            .token_for_org(&org_id)
            .unwrap_or_default()
            .to_string()
    } else {
        existing_creds.any_token().unwrap_or_default().to_string()
    };
    let key_hint = if current_key.is_empty() {
        String::new()
    } else {
        mask_key(&current_key)
    };
    let api_key = prompt("API Key", &key_hint)?;
    let api_key = if api_key == key_hint {
        current_key
    } else {
        api_key
    };
    ensure_non_empty_api_key(&api_key)?;

    // API URL
    let api_url = prompt("API URL", existing_creds.api_url())?;

    // Save credentials
    let mut creds = Credentials::load()?.unwrap_or_else(|| Credentials::new(api_url.clone()));
    creds.set_api_url(api_url.clone());

    let store_org = if org_id.is_empty() {
        "_default".to_string()
    } else {
        org_id.clone()
    };
    creds.set_token(store_org, api_key.clone());
    creds.save()?;

    if !org_id.is_empty() {
        let mut settings = Settings::load()?;
        settings.set_current_organization_id(org_id.clone())?;
    }

    println!();
    println!("Configuration saved.");
    println!("  API Key: {}", mask_key(&api_key));
    println!("  API URL: {}", api_url);
    if !org_id.is_empty() {
        println!("  Organization: {}", org_id);
    }

    Ok(())
}

pub fn handle_status(json: bool) -> Result<()> {
    match Credentials::load()? {
        Some(creds) => {
            let settings = Settings::load()?;
            let org_id = settings.current_organization_id();

            // Resolve the token for the current org
            let current_token = org_id
                .and_then(|id| creds.token_for_org(id))
                .or_else(|| creds.any_token());

            if json {
                let output = serde_json::json!({
                    "authenticated": current_token.is_some(),
                    "api_key": current_token.map(mask_key),
                    "api_url": creds.api_url(),
                    "organization_id": org_id,
                    "stored_organizations": creds.organization_count(),
                });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                if let Some(token) = &current_token {
                    println!("Authenticated");
                    println!("  API Key: {}", mask_key(token));
                } else {
                    println!("Not authenticated for current organization");
                }
                println!("  API URL: {}", creds.api_url());
                if let Some(id) = org_id {
                    println!("  Organization: {}", id);
                    if creds.token_for_org(id).is_none() {
                        println!(
                            "  Warning: No API key for this organization. Run 'addness login' to authenticate this org, or 'addness configure' if you have a key for it."
                        );
                    }
                }
                if creds.organization_count() > 1 {
                    println!(
                        "  Stored keys: {} organization(s)",
                        creds.organization_count()
                    );
                }
            }
        }
        None => {
            if json {
                let output = serde_json::json!({ "authenticated": false });
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                println!(
                    "Not configured. Run: addness login (or 'addness configure' if you already have an API key)."
                );
            }
        }
    }
    Ok(())
}

pub fn handle_logout() -> Result<()> {
    Credentials::delete()?;
    Settings::delete()?;
    println!("Logged out. Credentials and settings removed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ensure_non_empty_api_key;

    #[test]
    fn api_key_validation_rejects_blank_values() {
        assert!(ensure_non_empty_api_key("").is_err());
        assert!(ensure_non_empty_api_key("   ").is_err());
    }

    #[test]
    fn api_key_validation_accepts_non_blank_values() {
        assert!(ensure_non_empty_api_key("ak_test").is_ok());
    }
}
