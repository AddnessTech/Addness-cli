use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use crate::api::{ApiClient, ApiKey, ApiKeyCreateRequest};
use crate::cli::commands::confirm;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `codex_job::client_for_org` / `skill::client_for_org`. Only
/// `create` actually consults the header on the backend (`list`/`revoke`
/// are scoped to the authenticated user regardless of org), but `--org` is
/// accepted on all three subcommands for consistency with the rest of the CLI.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Human-readable one-line summary shared by `list`.
fn format_key_line(key: &ApiKey) -> String {
    let status = if key.revoked_at.is_some() {
        "revoked"
    } else {
        "active"
    };
    format!("{} [{status}] {} ({}...)", key.id, key.name, key.key_prefix)
}

#[derive(Subcommand)]
pub enum ApiKeyCommands {
    /// List your personal API keys (auto-generated MCP/CLI/Codex keys are
    /// hidden; only manually created keys are shown)
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new API key. IMPORTANT: the plaintext key is only ever
    /// returned in this response — it cannot be retrieved again afterwards
    Create {
        /// Key name (backend defaults to "API Key (<date>)" when omitted)
        #[arg(long)]
        name: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke (delete) an API key
    Rm {
        /// Key ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip the confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_api_key(cmd: &ApiKeyCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ApiKeyCommands::List { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let keys = scoped.list_api_keys().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&keys)?);
            } else if keys.is_empty() {
                println!("{}", "No API keys found.".dimmed());
            } else {
                for key in &keys {
                    println!("{}", format_key_line(key));
                }
            }
            Ok(())
        }
        ApiKeyCommands::Create { name, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let req = ApiKeyCreateRequest {
                name: name.clone().unwrap_or_default(),
            };
            let created = scoped.create_api_key(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&created)?);
            } else {
                println!("Created API key {} ({})", created.name, created.id);
                println!(
                    "{}",
                    "この場でしか表示されません。今すぐ保存してください（再表示不可）:".yellow()
                );
                println!("{}", created.key.bold());
            }
            Ok(())
        }
        ApiKeyCommands::Rm {
            id,
            org,
            force,
            json,
        } => {
            if !*force && !confirm(&format!("Revoke API key {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.revoke_api_key(id).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"revoked": true, "id": id, "message": resp.message})
                    )?
                );
            } else if resp.message.is_empty() {
                println!("Revoked API key {id}");
            } else {
                println!("{}", resp.message);
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::format_key_line;
    use crate::api::ApiKey;

    fn sample_key() -> ApiKey {
        ApiKey {
            id: "key-1".to_string(),
            key_prefix: "sk-abcd12".to_string(),
            name: "CI token".to_string(),
            last_used_at: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            revoked_at: None,
        }
    }

    #[test]
    fn format_key_line_marks_active_keys() {
        assert_eq!(
            format_key_line(&sample_key()),
            "key-1 [active] CI token (sk-abcd12...)"
        );
    }

    #[test]
    fn format_key_line_marks_revoked_keys() {
        let mut key = sample_key();
        key.revoked_at = Some("2026-02-01T00:00:00Z".to_string());
        assert_eq!(
            format_key_line(&key),
            "key-1 [revoked] CI token (sk-abcd12...)"
        );
    }
}
