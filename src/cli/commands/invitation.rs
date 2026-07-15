use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;
use crate::cli::commands::org::resolve_org_id;

/// Statuses accepted by `invitation invited-members --status`.
const VALID_INVITED_MEMBER_STATUSES: &[&str] =
    &["invited", "accepted", "declined", "expired", "revoked"];

/// Validate `--status` against the Go API's `InvitedMemberStatus` enum before
/// sending the request, so invalid values fail fast with a clear message.
fn validate_invited_member_status(status: &str) -> Result<()> {
    if VALID_INVITED_MEMBER_STATUSES.contains(&status) {
        Ok(())
    } else {
        bail!(
            "Invalid --status '{status}'. Use one of: {}",
            VALID_INVITED_MEMBER_STATUSES.join(", ")
        );
    }
}

/// Print a JSON value as pretty-printed text (mirrors `org::print_json_value`,
/// used by the read-only subcommands whose response shapes vary per endpoint).
fn print_json_value(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

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
    /// Accept or verify a legacy (v1) organization invitation by token
    LegacyAccept {
        /// Acceptance token
        #[arg(long)]
        token: String,
        /// Only verify the token/email match without accepting
        #[arg(long)]
        verify_only: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Check whether adding members would require a plan upgrade
    CheckPlanUpgrade {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Number of additional members being added
        #[arg(long)]
        additional_members: i64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Preview a v2 invitation by its token (no authentication required)
    Preview {
        /// Invitation token
        token: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Accept a v2 invitation by its token
    AcceptToken {
        /// Invitation token
        token: String,
        /// Source organization ID (for external members joining via this org)
        #[arg(long)]
        source_org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage your pending invitations
    Pending {
        #[command(subcommand)]
        command: PendingCommands,
    },
    /// Decline a pending invitation
    Decline {
        /// Invited member ID
        #[arg(long)]
        invited_member: String,
        /// Invitation token
        #[arg(long)]
        token: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List invited (not-yet-accepted) members of an organization
    InvitedMembers {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Filter by status: invited, accepted, declined, expired, revoked
        #[arg(long)]
        status: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the organization's invitation overview (counts/summary)
    Overview {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum PendingCommands {
    /// List your pending invitations (resolved from your account email)
    List {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a short-lived access token for a pending invitation
    Access {
        /// Invited member ID
        inv_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
    /// List invite links for an organization
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Join an organization via an invite link's code
    Join {
        /// Invite link code
        code: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
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
            if !*force && !crate::cli::commands::confirm(&format!("Revoke invitation {id}?"))? {
                println!("Cancelled.");
                return Ok(());
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
                if !*force
                    && !crate::cli::commands::confirm(&format!("Deactivate invite link {id}?"))?
                {
                    println!("Cancelled.");
                    return Ok(());
                }
                client.deactivate_invite_link(&org_id, id).await?;
                println!("Invite link {id} deactivated");
                Ok(())
            }
            InviteLinkCommands::List { org, json: _ } => {
                let org_id = resolve_org_id(org.as_deref())?;
                let data = client.list_invite_links(&org_id).await?;
                print_json_value(&data)
            }
            InviteLinkCommands::Join { code, json: _ } => {
                let data = client.join_invite_link(code).await?;
                print_json_value(&data)
            }
        },
        InvitationCommands::LegacyAccept {
            token,
            verify_only,
            json: _,
        } => {
            let data = client.legacy_accept_invitation(token, *verify_only).await?;
            print_json_value(&data)
        }
        InvitationCommands::CheckPlanUpgrade {
            org,
            additional_members,
            json: _,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client
                .check_invitation_plan_upgrade(&org_id, *additional_members)
                .await?;
            print_json_value(&data)
        }
        InvitationCommands::Preview { token, json: _ } => {
            let data = client.preview_invitation(token).await?;
            print_json_value(&data)
        }
        InvitationCommands::AcceptToken {
            token,
            source_org,
            json: _,
        } => {
            let data = client
                .accept_invitation_by_token(token, source_org.as_deref())
                .await?;
            print_json_value(&data)
        }
        InvitationCommands::Pending { command } => match command {
            PendingCommands::List { json: _ } => {
                let data = client.list_pending_invitations().await?;
                print_json_value(&data)
            }
            PendingCommands::Access { inv_id, json: _ } => {
                let data = client.create_invitation_access_token(inv_id).await?;
                print_json_value(&data)
            }
        },
        InvitationCommands::Decline {
            invited_member,
            token,
            force,
        } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Decline invitation for invited member {invited_member}? This cannot be undone."
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.decline_invitation(invited_member, token).await?;
            println!("Invitation for invited member {invited_member} declined");
            Ok(())
        }
        InvitationCommands::InvitedMembers {
            org,
            status,
            json: _,
        } => {
            if let Some(status) = status {
                validate_invited_member_status(status)?;
            }
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client
                .list_invited_members(&org_id, status.as_deref())
                .await?;
            print_json_value(&data)
        }
        InvitationCommands::Overview { org, json: _ } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client.get_invitation_overview(&org_id).await?;
            print_json_value(&data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_invited_member_status;

    #[test]
    fn validate_invited_member_status_accepts_known_values() {
        for status in ["invited", "accepted", "declined", "expired", "revoked"] {
            assert!(validate_invited_member_status(status).is_ok());
        }
    }

    #[test]
    fn validate_invited_member_status_rejects_unknown_values() {
        let err = validate_invited_member_status("bogus")
            .unwrap_err()
            .to_string();
        assert!(err.contains("Invalid --status 'bogus'"));
        assert!(err.contains("invited, accepted, declined, expired, revoked"));
    }
}
