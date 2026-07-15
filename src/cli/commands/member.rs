use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, BrowseMembersParams};
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
///
/// Mirrors `org::client_for_org`. Duplicated here (rather than imported, since
/// the original is private to `org.rs`) because several new member endpoints
/// resolve their organization purely from the header — or need the header to
/// match an explicit `--org` even when the path also carries an organization id.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

/// Print a JSON value as pretty-printed text (mirrors `org::print_json_value`,
/// used by the read-only subcommands whose response shapes vary per endpoint).
fn print_json_value(value: &serde_json::Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Best-effort MIME type for a member avatar upload based on the file
/// extension (mirrors `org::content_type_for_path`).
fn content_type_for_avatar_path(path: &str) -> &'static str {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

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
    /// Search members in the organization by name
    Search {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Name filter (display name or username)
        #[arg(long)]
        name: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<i64>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List members below the current member's topmost owned objective
    Children {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tree depth (default: 1)
        #[arg(long)]
        depth: Option<i64>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List admin members of the organization
    Admins {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Preview the effects of deleting a member (assignment reassignment, etc.)
    DeletePreview {
        /// Member ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Browse members with filtering/sorting/pagination (distinct from `list`)
    Browse {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Free-text search query
        #[arg(long)]
        query: Option<String>,
        /// Page number
        #[arg(long)]
        page: Option<i64>,
        /// Number of results per page
        #[arg(long)]
        page_size: Option<i64>,
        /// Objective ID to filter by assignment (requires --type)
        #[arg(long)]
        objective_id: Option<String>,
        /// Comma-separated assignment types: non-member, member, editor, owner
        #[arg(long)]
        r#type: Option<String>,
        /// Comma-separated member tag IDs to filter by
        #[arg(long)]
        tag_ids: Option<String>,
        /// Field to sort by
        #[arg(long)]
        sort_by: Option<String>,
        /// Sort direction: asc or desc
        #[arg(long)]
        sort_dir: Option<String>,
        /// Member ID whose containing page should be returned
        #[arg(long)]
        target_member_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the objectives assigned to a member
    Objectives {
        /// Member ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Upload a member's avatar image
    SetAvatar {
        /// Member ID
        id: String,
        /// Path to the image file to upload
        #[arg(long)]
        file: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a single member's detail
    Get {
        /// Member ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage member tags
    Tag {
        #[command(subcommand)]
        command: MemberTagCommands,
    },
}

#[derive(Subcommand)]
pub enum MemberTagCommands {
    /// List tags defined in the organization
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new member tag
    Create {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tag name
        #[arg(long)]
        name: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a member tag (removes it from all members)
    Rm {
        /// Tag ID
        tag_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Assign a tag to a member
    Assign {
        /// Member ID
        member_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tag ID to assign
        #[arg(long)]
        tag: String,
    },
    /// List tags assigned to a member
    ListFor {
        /// Member ID
        member_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove a tag from a member
    Unassign {
        /// Member ID
        member_id: String,
        /// Tag ID
        tag_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
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
        MemberCommands::Search {
            org,
            name,
            limit,
            json: _,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client_for_org(client, &org_id)
                .search_members(&org_id, name.as_deref(), *limit)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::Children {
            org,
            depth,
            json: _,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client_for_org(client, &org_id)
                .get_member_children(&org_id, *depth)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::Admins { org, json: _ } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client_for_org(client, &org_id)
                .list_organization_admins(&org_id)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::DeletePreview { id, org, json: _ } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client_for_org(client, &org_id)
                .get_member_delete_preview(id)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::Browse {
            org,
            query,
            page,
            page_size,
            objective_id,
            r#type,
            tag_ids,
            sort_by,
            sort_dir,
            target_member_id,
            json: _,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let params = BrowseMembersParams {
                query: query.as_deref(),
                page: *page,
                page_size: *page_size,
                objective_id: objective_id.as_deref(),
                r#type: r#type.as_deref(),
                tag_ids: tag_ids.as_deref(),
                sort_by: sort_by.as_deref(),
                sort_dir: sort_dir.as_deref(),
                target_member_id: target_member_id.as_deref(),
            };
            let data = client_for_org(client, &org_id)
                .browse_members(&params)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::Objectives { id, org, json: _ } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client_for_org(client, &org_id)
                .get_member_objectives(id)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::SetAvatar {
            id,
            file,
            org,
            json: _,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let bytes = std::fs::read(file)
                .map_err(|e| anyhow::anyhow!("Failed to read avatar file '{file}': {e}"))?;
            let content_type = content_type_for_avatar_path(file);
            let data = client_for_org(client, &org_id)
                .upload_member_avatar(id, bytes, content_type)
                .await?;
            print_json_value(&data)
        }
        MemberCommands::Get { id, org, json: _ } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let data = client_for_org(client, &org_id).get_member(id).await?;
            print_json_value(&data)
        }
        MemberCommands::Tag { command } => handle_member_tag(command, client).await,
    }
}

async fn handle_member_tag(cmd: &MemberTagCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        MemberTagCommands::List { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client_for_org(client, &org_id)
                .list_member_tags(&org_id)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else if resp.data.is_empty() {
                println!("No member tags in organization {org_id}");
            } else {
                println!("Member tags in organization {org_id}:");
                for tag in &resp.data {
                    println!("  {} — {}", tag.id, tag.name);
                }
            }
            Ok(())
        }
        MemberTagCommands::Create { org, name, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client_for_org(client, &org_id)
                .create_member_tag(&org_id, name)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                println!("Member tag created: {} ({})", resp.data.name, resp.data.id);
            }
            Ok(())
        }
        MemberTagCommands::Rm { tag_id, org, force } => {
            let org_id = resolve_org_id(org.as_deref())?;
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Delete member tag {tag_id}? This removes it from all members."
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client_for_org(client, &org_id)
                .delete_member_tag(&org_id, tag_id)
                .await?;
            println!("Member tag {tag_id} deleted");
            Ok(())
        }
        MemberTagCommands::Assign {
            member_id,
            org,
            tag,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            client_for_org(client, &org_id)
                .assign_member_tag(&org_id, member_id, tag)
                .await?;
            println!("Tag {tag} assigned to member {member_id}");
            Ok(())
        }
        MemberTagCommands::ListFor {
            member_id,
            org,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client_for_org(client, &org_id)
                .list_member_tags_for_member(member_id)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else if resp.data.is_empty() {
                println!("Member {member_id} has no tags");
            } else {
                println!("Tags for member {member_id}:");
                for tag in &resp.data {
                    println!("  {} — {}", tag.id, tag.name);
                }
            }
            Ok(())
        }
        MemberTagCommands::Unassign {
            member_id,
            tag_id,
            org,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            client_for_org(client, &org_id)
                .unassign_member_tag(member_id, tag_id)
                .await?;
            println!("Tag {tag_id} removed from member {member_id}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::content_type_for_avatar_path;

    #[test]
    fn content_type_for_avatar_path_maps_known_image_extensions() {
        assert_eq!(content_type_for_avatar_path("avatar.png"), "image/png");
        assert_eq!(content_type_for_avatar_path("AVATAR.PNG"), "image/png");
        assert_eq!(content_type_for_avatar_path("a/b/avatar.jpg"), "image/jpeg");
        assert_eq!(content_type_for_avatar_path("avatar.jpeg"), "image/jpeg");
        assert_eq!(content_type_for_avatar_path("avatar.gif"), "image/gif");
        assert_eq!(content_type_for_avatar_path("avatar.webp"), "image/webp");
        assert_eq!(content_type_for_avatar_path("avatar.svg"), "image/svg+xml");
    }

    #[test]
    fn content_type_for_avatar_path_falls_back_for_unknown_or_missing_extension() {
        assert_eq!(
            content_type_for_avatar_path("avatar.bin"),
            "application/octet-stream"
        );
        assert_eq!(
            content_type_for_avatar_path("avatar"),
            "application/octet-stream"
        );
    }
}
