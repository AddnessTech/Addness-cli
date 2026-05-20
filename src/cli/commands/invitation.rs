use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::commands::org::resolve_org_id;

#[derive(Subcommand)]
pub enum InvitationCommands {
    /// Invite members by email
    Create {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Comma-separated list of email addresses
        #[arg(long)]
        emails: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Resend an existing invitation
    Resend {
        /// Invitation ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke (cancel) a pending invitation
    Revoke {
        /// Invitation ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Accept a pending invitation
    Accept {
        /// Invited member ID
        #[arg(long)]
        invited_member: String,
        /// Acceptance token
        #[arg(long)]
        token: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage organization-wide invite links
    Link {
        #[command(subcommand)]
        command: InviteLinkCommands,
    },
}

#[derive(Subcommand)]
pub enum InviteLinkCommands {
    /// Create an invite link
    Create {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Invite code (alphanumeric)
        #[arg(long)]
        code: String,
        /// Max uses (omit for unlimited)
        #[arg(long)]
        max_uses: Option<i32>,
        /// Expiry as RFC3339 timestamp (e.g. 2026-12-31T23:59:59Z)
        #[arg(long)]
        expires_at: Option<String>,
        /// Mark as external-facing link
        #[arg(long)]
        external: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Deactivate an invite link
    Deactivate {
        /// Link ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

pub async fn handle_invitation(cmd: &InvitationCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        InvitationCommands::Create { org, emails, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let email_list: Vec<String> = emails
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if email_list.is_empty() {
                bail!("--emails must contain at least one address");
            }
            let resp = client
                .create_invitations(&org_id, email_list.clone())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Created {} invitation(s) for organization {org_id}",
                    email_list.len()
                );
            }
            Ok(())
        }
        InvitationCommands::Resend { id, org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client.resend_invitation(&org_id, id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Invitation {id} resent");
            }
            Ok(())
        }
        InvitationCommands::Revoke { id, org, force } => {
            let org_id = resolve_org_id(org.as_deref())?;
            if !*force {
                eprint!("Revoke invitation {id}? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
            client.revoke_invitation(&org_id, id).await?;
            println!("Invitation {id} revoked");
            Ok(())
        }
        InvitationCommands::Accept {
            invited_member,
            token,
            json,
        } => {
            let resp = client.accept_invitation(invited_member, token).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Invitation accepted");
            }
            Ok(())
        }
        InvitationCommands::Link { command } => match command {
            InviteLinkCommands::Create {
                org,
                code,
                max_uses,
                expires_at,
                external,
                json,
            } => {
                let org_id = resolve_org_id(org.as_deref())?;
                let resp = client
                    .create_invite_link(&org_id, code, *max_uses, expires_at.clone(), *external)
                    .await?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&resp)?);
                } else {
                    println!("Invite link created with code {code}");
                }
                Ok(())
            }
            InviteLinkCommands::Deactivate { id, org, force } => {
                let org_id = resolve_org_id(org.as_deref())?;
                if !*force {
                    eprint!("Deactivate invite link {id}? [y/N] ");
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    if !input.trim().eq_ignore_ascii_case("y") {
                        println!("Cancelled.");
                        return Ok(());
                    }
                }
                client.deactivate_invite_link(&org_id, id).await?;
                println!("Invite link {id} deactivated");
                Ok(())
            }
        },
    }
}
