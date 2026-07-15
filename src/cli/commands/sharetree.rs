use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use crate::api::ApiClient;
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`.
/// Mirrors `member::client_for_org` — share-tree create/list/clone resolve
/// their organization purely from the header, not the URL path.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

#[derive(Subcommand)]
pub enum ShareTreeCommands {
    /// Create a portable, cloneable public export of a goal (and its subtree)
    Create {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Source goal ID to export
        #[arg(long)]
        goal: String,
        /// Restrict the export to specific descendant goal IDs (repeatable).
        /// If omitted, the entire subtree is exported.
        #[arg(long = "select-goal")]
        select_goals: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke a share-tree export
    Revoke {
        /// Share-tree ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// List share-tree exports you created
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Clone a shared goal tree into your own organization
    Clone {
        /// Public ID of the share-tree export (from `share-tree get-public` or a share URL)
        #[arg(long)]
        public_id: String,
        /// Organization ID to clone into (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Parent goal ID to attach the cloned root under (defaults to organization root)
        #[arg(long)]
        parent: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Fetch a publicly shared goal tree by its public ID (no auth required)
    GetPublic {
        /// Public ID
        public_id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_sharetree(cmd: &ShareTreeCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ShareTreeCommands::Create {
            org,
            goal,
            select_goals,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let created = scoped.create_share_tree(goal, select_goals.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&created)?);
            } else {
                println!(
                    "Share tree created: {} (public_id={})",
                    created.id, created.public_id
                );
                if created.skipped_attachments_count > 0 {
                    println!(
                        "{}",
                        format!(
                            "Note: {} attachment(s) were skipped (not exportable).",
                            created.skipped_attachments_count
                        )
                        .dimmed()
                    );
                }
            }
            Ok(())
        }
        ShareTreeCommands::Revoke { id, org, force } => {
            let org_id = resolve_org_id(org.as_deref())?;
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Revoke share tree {id}? Existing share links will stop working."
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            let scoped = client_for_org(client, &org_id);
            scoped.revoke_share_tree(id).await?;
            println!("Share tree {id} revoked");
            Ok(())
        }
        ShareTreeCommands::List { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let items = scoped.list_my_share_trees().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else if items.is_empty() {
                println!("{}", "No share trees found.".dimmed());
            } else {
                println!(
                    "{:<38} {:<38} {}",
                    "PUBLIC ID".bold(),
                    "SOURCE GOAL".bold(),
                    "TITLE".bold()
                );
                println!("{}", "─".repeat(110));
                for item in &items {
                    println!(
                        "{:<38} {:<38} {}",
                        item.public_id.dimmed(),
                        item.source_objective_id.dimmed(),
                        item.root_title
                    );
                }
            }
            Ok(())
        }
        ShareTreeCommands::Clone {
            public_id,
            org,
            parent,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let cloned = scoped.clone_share_tree(public_id, parent.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&cloned)?);
            } else {
                println!(
                    "Cloned {} goal(s) into {}",
                    cloned.node_count, cloned.root_objective_id
                );
            }
            Ok(())
        }
        ShareTreeCommands::GetPublic { public_id, json } => {
            let tree = client.get_public_shared_goal_tree(public_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&tree)?);
            } else {
                println!(
                    "{} — shared by {} on {}",
                    tree.public_id, tree.creator_display_name, tree.created_at
                );
                println!("{} node(s)", tree.nodes.len());
            }
            Ok(())
        }
    }
}
