use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::commands::org::resolve_org_id;

/// 自分自身の組織内メンバーIDを解決する。
/// `--member` が未指定のstreak/activityコマンドで「自分の」情報をデフォルト表示するために使う。
pub async fn resolve_self_member_id(client: &ApiClient, org_id: &str) -> Result<String> {
    let resp = client.get_members(org_id).await?;
    match resp.data.members.into_iter().find(|m| m.is_current_user) {
        Some(member) => Ok(member.id),
        None => bail!(
            "Could not determine your member ID in organization {org_id}. Specify --member <ID> explicitly."
        ),
    }
}

#[derive(Subcommand)]
pub enum MemberCommands {
    /// List members in the organization
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a member's display name
    Update {
        /// Member ID
        id: String,
        /// New display name
        #[arg(long)]
        name: String,
    },
    /// Pin a member to the top of lists
    Pin {
        /// Member ID
        id: String,
    },
    /// Unpin a member
    Unpin {
        /// Member ID
        id: String,
    },
    /// Remove a member from the organization
    Rm {
        /// Member ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Manage admin role on a member
    Admin {
        #[command(subcommand)]
        command: AdminCommands,
    },
    /// Set the source organization for an external member
    SetSourceOrg {
        /// Member ID
        id: String,
        /// Source organization ID (UUID)
        #[arg(long)]
        org: String,
    },
}

#[derive(Subcommand)]
pub enum AdminCommands {
    /// Grant admin role to a member
    Grant {
        /// Member ID
        id: String,
    },
    /// Revoke admin role from a member
    Revoke {
        /// Member ID
        id: String,
    },
}

pub async fn handle_member(cmd: &MemberCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MemberCommands::List { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client.get_members(&org_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.members)?);
            } else if resp.data.members.is_empty() {
                println!("No members in organization {org_id}");
            } else {
                println!("Members in organization {org_id}:");
                for m in &resp.data.members {
                    let marker = if m.is_current_user { " (you)" } else { "" };
                    println!("  {} — {}{marker}", m.id, m.name);
                }
            }
            Ok(())
        }
        MemberCommands::Update { id, name } => {
            client.update_member(id, name).await?;
            println!("Member {id} updated");
            Ok(())
        }
        MemberCommands::Pin { id } => {
            client.pin_member(id, true).await?;
            println!("Member {id} pinned");
            Ok(())
        }
        MemberCommands::Unpin { id } => {
            client.pin_member(id, false).await?;
            println!("Member {id} unpinned");
            Ok(())
        }
        MemberCommands::Rm { id, force } => {
            if !*force && !crate::cli::commands::confirm(&format!("Remove member {id}?"))? {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_member(id).await?;
            println!("Member {id} removed");
            Ok(())
        }
        MemberCommands::Admin { command } => match command {
            AdminCommands::Grant { id } => {
                client.assign_admin(id).await?;
                println!("Member {id} promoted to admin");
                Ok(())
            }
            AdminCommands::Revoke { id } => {
                client.revoke_admin(id).await?;
                println!("Admin role revoked from member {id}");
                Ok(())
            }
        },
        MemberCommands::SetSourceOrg { id, org } => {
            client.set_member_source_organization(id, org).await?;
            println!("Member {id} source organization set to {org}");
            Ok(())
        }
    }
}
