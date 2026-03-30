use std::io::{self, Write};

use anyhow::Result;

use crate::config::{
    delete_credentials, load_credentials, load_settings, save_credentials, save_settings,
    Credentials,
};

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

pub fn handle_configure() -> Result<()> {
    println!("Addness CLI Configuration");
    println!();

    // 既存の設定を読み込み
    let existing_creds = load_credentials()?.unwrap_or(Credentials {
        token: String::new(),
        api_url: "https://api.addness.app".to_string(),
    });
    let existing_settings = load_settings()?;

    // API Key
    let key_hint = if existing_creds.token.is_empty() {
        String::new()
    } else {
        mask_key(&existing_creds.token)
    };
    let api_key = prompt("API Key", &key_hint)?;
    let api_key = if api_key == key_hint {
        existing_creds.token.clone()
    } else {
        api_key
    };

    // API URL
    let api_url = prompt("API URL", &existing_creds.api_url)?;

    // Organization ID
    let default_org = existing_settings
        .default_organization_id
        .unwrap_or_default();
    let org_id = prompt("Default Organization ID", &default_org)?;

    // 保存
    let creds = Credentials {
        token: api_key,
        api_url: api_url.clone(),
    };
    save_credentials(&creds)?;

    let mut settings = load_settings()?;
    if !org_id.is_empty() {
        settings.default_organization_id = Some(org_id.clone());
    }
    save_settings(&settings)?;

    println!();
    println!("Configuration saved.");
    println!("  API Key: {}", mask_key(&creds.token));
    println!("  API URL: {}", api_url);
    if !org_id.is_empty() {
        println!("  Organization: {}", org_id);
    }

    Ok(())
}

pub fn handle_status() -> Result<()> {
    match load_credentials()? {
        Some(creds) => {
            let settings = load_settings()?;
            println!("Authenticated");
            println!("  API Key: {}", mask_key(&creds.token));
            println!("  API URL: {}", creds.api_url);
            if let Some(org_id) = settings.default_organization_id {
                println!("  Organization: {}", org_id);
            }
        }
        None => {
            println!("Not configured. Run: addness configure");
        }
    }
    Ok(())
}

pub fn handle_logout() -> Result<()> {
    delete_credentials()?;
    println!("Logged out. Credentials removed.");
    Ok(())
}
