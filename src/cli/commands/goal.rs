use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{
    ApiClient, ApiResponse, Comment, CreateGoalRequest, Deliverable, DeliverableType, Goal,
    GoalStatus, GoalTreeData, GoalTreeItem, UpdateGoalRequest,
};
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::{
    print_children_table, print_goals_table, print_search_results, resolve_status,
};

fn read_text_arg(inline: Option<&String>, file: Option<&String>) -> Result<Option<String>> {
    match (inline, file) {
        (Some(s), None) => Ok(Some(s.clone())),
        (None, Some(p)) => Ok(Some(std::fs::read_to_string(p)?)),
        (Some(_), Some(_)) => bail!("Specify only one of --description or --description-file"),
        (None, None) => Ok(None),
    }
}

#[derive(Subcommand)]
pub enum GoalCommands {
    /// List goals in the organization tree
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tree depth (default: 3)
        #[arg(long, default_value = "3")]
        depth: usize,
        /// Filter by owner name (use "me" for yourself)
        #[arg(long)]
        assigned_to: Option<String>,
        /// Filter by status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        #[arg(long)]
        status: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get a goal's details
    Get {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// With deliverables information
        #[arg(long)]
        with_deliverable: bool,
        /// With comments information
        #[arg(long)]
        with_comment: bool,
    },
    /// List children of a goal
    Children {
        /// Goal ID
        id: String,
        /// Max number of children to return (default: 20)
        #[arg(long, default_value = "20")]
        limit: usize,
        /// Offset for pagination
        #[arg(long, default_value = "0")]
        offset: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show subtree of a goal
    Tree {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Get sibling goals with their deliverables and descriptions
    Siblings {
        /// Goal ID
        id: String,
        /// Max number of siblings to return (default: 50)
        #[arg(long, default_value = "50")]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search goals by keyword
    Search {
        /// Search query
        query: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Create a new goal
    Create {
        /// Goal title
        #[arg(long)]
        title: String,
        /// Parent goal ID (omit to create as root goal)
        #[arg(long)]
        parent: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update a goal's status, title, or description
    Update {
        /// Goal ID
        id: String,
        /// Status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        #[arg(long)]
        status: Option<String>,
        /// Title
        #[arg(long)]
        title: Option<String>,
        /// Description (definition of done) - replaces the current value
        #[arg(long)]
        description: Option<String>,
        /// Description from a file path (alternative to --description)
        #[arg(long)]
        description_file: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Delete a goal (soft delete)
    Delete {
        /// Goal ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Archive a goal (move out of active tree, keep data)
    Archive {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Unarchive (restore from archive) a goal
    Unarchive {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Restore a soft-deleted goal
    Restore {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Duplicate a goal under a specified parent
    Duplicate {
        /// Source goal ID
        id: String,
        /// Parent goal ID where the duplicate will be placed
        #[arg(long)]
        parent: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Move a goal under a different parent (or to root with --root)
    Move {
        /// Goal ID to move
        id: String,
        /// New parent goal ID
        #[arg(long, conflicts_with = "root")]
        parent: Option<String>,
        /// Move to root (clears parent)
        #[arg(long, conflicts_with = "parent")]
        root: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage public share links for a goal
    Share {
        #[command(subcommand)]
        command: ShareCommands,
    },
    /// Manage goal aliases (link an existing goal under another parent)
    Alias {
        #[command(subcommand)]
        command: AliasCommands,
    },
}

#[derive(Subcommand)]
pub enum ShareCommands {
    /// Create (or fetch existing) public share link
    Create {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Revoke the public share link
    Revoke {
        /// Goal ID
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub enum AliasCommands {
    /// Add an alias under a parent goal (links an existing goal)
    Add {
        /// Parent goal ID (where the alias appears)
        parent_id: String,
        /// Target goal ID to reference
        #[arg(long)]
        target: String,
        /// Display order (1 or greater; backend rejects 0)
        #[arg(long, default_value = "1")]
        order: i32,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Remove an alias
    Rm {
        /// Parent goal ID
        parent_id: String,
        /// Alias ID
        alias_id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        force: bool,
    },
    /// Reorder aliases under a parent goal
    Reorder {
        /// Parent goal ID
        parent_id: String,
        /// Comma-separated alias IDs in the desired order
        #[arg(long)]
        order: String,
    },
}

/// GoalInfo はゴールとその関連情報を保持するための
/// 汎用的な構造体
/// 木構造を表現できる
#[derive(Debug, serde::Serialize)]
struct GoalNode {
    goal: Goal,
    deliverables: Option<Vec<Deliverable>>,
    comments: Option<Vec<Comment>>,
    children: Option<Vec<GoalChildNode>>,
}

#[derive(Debug, serde::Serialize)]
struct GoalChildNode {
    goal: GoalTreeItem,
    deliverables: Option<Vec<Deliverable>>,
    comments: Option<Vec<Comment>>,
    children: Option<Vec<Self>>,
}

impl GoalChildNode {
    fn build_children(
        parent_id: &str,
        children_map: &mut HashMap<String, Vec<GoalTreeItem>>,
        deliverables_map: &mut HashMap<String, Vec<Deliverable>>,
        comments_map: &mut HashMap<String, Vec<Comment>>,
    ) -> Vec<Self> {
        match children_map.remove(parent_id) {
            Some(goals) => goals
                .into_iter()
                .map(|goal| {
                    let children = Self::build_children(
                        &goal.id,
                        children_map,
                        deliverables_map,
                        comments_map,
                    );

                    let deliverables = deliverables_map.remove(&goal.id);
                    let comments = comments_map.remove(&goal.id);

                    Self {
                        goal,
                        deliverables,
                        comments,
                        children: Some(children),
                    }
                })
                .collect(),
            None => vec![],
        }
    }
}

impl GoalNode {
    fn build_forest(
        root_id: &str,
        goals: Vec<GoalTreeItem>,
        mut deliverables_map: HashMap<String, Vec<Deliverable>>,
        mut comments_map: HashMap<String, Vec<Comment>>,
    ) -> Vec<GoalChildNode> {
        let mut children_map: HashMap<String, Vec<GoalTreeItem>> = HashMap::new();

        for goal in goals {
            if let Some(parent_id) = goal.parent_id.clone() {
                children_map.entry(parent_id).or_default().push(goal);
            }
        }

        GoalChildNode::build_children(
            root_id,
            &mut children_map,
            &mut deliverables_map,
            &mut comments_map,
        )
    }

    fn build_tree(
        goal: Goal,
        deliverables: Option<Vec<Deliverable>>,
        comments: Option<Vec<Comment>>,
        children: Option<Vec<GoalTreeItem>>,
        children_deliverables: HashMap<String, Vec<Deliverable>>,
        children_comments: HashMap<String, Vec<Comment>>,
    ) -> Self {
        let children = children.map(|children| {
            Self::build_forest(&goal.id, children, children_deliverables, children_comments)
        });

        Self {
            goal,
            comments,
            deliverables,
            children,
        }
    }
}

use crate::api::GoalChildItem;
use colored::Colorize;
use std::collections::HashMap;

/// ゴール一覧+成果物マップをJSON出力
fn print_goals_with_deliverables_json(
    goals: &[GoalChildItem],
    deliverables_map: &HashMap<String, Vec<Deliverable>>,
) -> Result<()> {
    let output: Vec<serde_json::Value> = goals
        .iter()
        .map(|g| {
            let deliverables = deliverables_map.get(&g.id).cloned().unwrap_or_default();
            serde_json::json!({
                "id": g.id,
                "title": g.title,
                "description": g.description,
                "status": g.status,
                "is_completed": g.is_completed,
                "deliverables": deliverables.iter().map(|d| {
                    serde_json::json!({
                        "id": d.id,
                        "display_name": d.display_name,
                        "node_type": d.node_type,
                    })
                }).collect::<Vec<_>>(),
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn print_context_table(
    children: &[GoalChildItem],
    deliverables_map: &std::collections::HashMap<String, Vec<Deliverable>>,
) {
    for (i, child) in children.iter().enumerate() {
        if i > 0 {
            println!();
        }
        let (_, colored_status) = resolve_status(child.is_completed, child.status.as_ref());
        println!(
            "{} {} [{}]",
            format!("{}.", i + 1).bold(),
            child.title.bold(),
            colored_status
        );
        println!("   {}: {}", "ID".dimmed(), child.id.dimmed());

        if let Some(desc) = &child.description
            && !desc.is_empty()
        {
            println!("   {}: {desc}", "完了条件".dimmed());
        }

        if let Some(deliverables) = deliverables_map.get(&child.id) {
            if deliverables.is_empty() {
                println!("   {}: {}", "成果物".dimmed(), "(なし)".dimmed());
            } else {
                println!("   {}:", "成果物".dimmed());
                for d in deliverables {
                    println!("     {} {}", d.node_type.as_icon(), d.display_name);
                }
            }
        } else {
            println!("   {}: {}", "成果物".dimmed(), "(取得失敗)".dimmed());
        }
    }
}

/// Parse CLI status into (completed_at, api_status) pair.
/// COMPLETED sets completed_at to current UTC timestamp.
/// Others clear completed_at (null).
fn parse_status(status: &str) -> Result<(Option<Option<String>>, Option<GoalStatus>)> {
    match status.to_uppercase().as_str() {
        "NOT_STARTED" => Ok((Some(None), Some(GoalStatus::None))),
        "IN_PROGRESS" => Ok((Some(None), Some(GoalStatus::InProgress))),
        "COMPLETED" => {
            let now = chrono::Utc::now().to_rfc3339();
            Ok((Some(Some(now)), None))
        }
        "CANCELLED" => Ok((Some(None), Some(GoalStatus::Cancelled))),
        _ => bail!(
            "Invalid status: '{status}'. Use one of: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED"
        ),
    }
}

pub async fn handle_goals(cmd: &GoalCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        GoalCommands::List {
            org,
            depth,
            assigned_to,
            status,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp: ApiResponse<GoalTreeData> = client.get_goal_tree(&org_id, *depth).await?;

            let mut items = resp.data.items;

            // --assigned-to filter
            if let Some(filter) = assigned_to {
                let owner_name = if filter.eq_ignore_ascii_case("me") {
                    let members_resp = client.get_members(&org_id).await?;
                    members_resp
                        .data
                        .members
                        .iter()
                        .find(|m| m.is_current_user)
                        .map(|m| m.name.clone())
                        .ok_or_else(|| anyhow::anyhow!("Could not determine current user. Try using your name instead of 'me'."))?
                } else {
                    filter.clone()
                };
                let name_lower = owner_name.to_lowercase();
                items.retain(|item| {
                    item.owner
                        .as_ref()
                        .is_some_and(|o| o.name.to_lowercase().contains(&name_lower))
                });
            }

            // --status filter
            if let Some(status_filter) = status {
                items.retain(|item| match status_filter.to_uppercase().as_str() {
                    "COMPLETED" => item.is_completed,
                    "NOT_STARTED" => {
                        !item.is_completed
                            && item.status.as_ref().is_none_or(|s| *s == GoalStatus::None)
                    }
                    "IN_PROGRESS" => {
                        !item.is_completed && item.status.as_ref() == Some(&GoalStatus::InProgress)
                    }
                    "CANCELLED" => {
                        !item.is_completed && item.status.as_ref() == Some(&GoalStatus::Cancelled)
                    }
                    _ => true,
                });
            }

            if *json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else {
                print_goals_table(&items);
            }
            Ok(())
        }
        GoalCommands::Get {
            id,
            json,
            with_deliverable,
            with_comment,
        } => {
            // id で指定されたゴール自身の情報を取得
            let resp: ApiResponse<Goal> = client.get_goal(id).await?;
            let deliverables = if *with_deliverable {
                Some(client.get_goal_deliverables(id).await?.data.deliverables)
            } else {
                None
            };
            let comments = if *with_comment {
                Some(client.list_comments(id).await?.comments)
            } else {
                None
            };

            // サブツリーの情報を取得（階層の終わりまで）
            let subtree_resp: ApiResponse<GoalTreeData> = client.get_goal_subtree(id).await?;
            let subtree_items: Vec<GoalTreeItem> = subtree_resp
                .data
                .items
                .into_iter()
                .filter(|item| item.id != *id)
                .collect();

            let children_deliverables = client
                .get_deliverables_map(
                    &subtree_items
                        .iter()
                        .map(|g| g.id.as_str())
                        .collect::<Vec<_>>(),
                )
                .await;
            let children_comments = client
                .get_comments_map(
                    &subtree_items
                        .iter()
                        .map(|g| g.id.as_str())
                        .collect::<Vec<_>>(),
                )
                .await;

            // 階層構造を構成
            let goal_tree = GoalNode::build_tree(
                resp.data,
                deliverables,
                comments,
                Some(subtree_items),
                children_deliverables,
                children_comments,
            );

            // 出力
            if *json {
                let output = serde_json::json!(goal_tree);
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                goal_tree.print_goal_detail_tree();
            }

            Ok(())
        }
        GoalCommands::Children {
            id,
            limit,
            offset,
            json,
        } => {
            let resp = client.get_goal_children(id, *limit, *offset).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.children)?);
            } else {
                print_children_table(&resp.data.children);
            }
            Ok(())
        }
        GoalCommands::Tree { id, json } => {
            let resp: ApiResponse<GoalTreeData> = client.get_goal_subtree(id).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_goals_table(&resp.data.items);
            }
            Ok(())
        }
        GoalCommands::Siblings { id, limit, json } => {
            // 1. 対象ゴールの詳細を取得して親IDを得る
            let goal_resp: ApiResponse<Goal> = client.get_goal(id).await?;
            let parent_id = match &goal_resp.data.parent_id {
                Some(pid) => pid.clone(),
                None => {
                    if *json {
                        println!("[]");
                    } else {
                        println!("This goal has no parent (root goal). No siblings.");
                    }
                    return Ok(());
                }
            };

            // 2. 親の子ゴール一覧を取得（自分自身を除外）
            let children_resp = client.get_goal_children(&parent_id, *limit, 0).await?;
            let siblings: Vec<_> = children_resp
                .data
                .children
                .into_iter()
                .filter(|c| c.id != *id)
                .collect();

            if siblings.is_empty() {
                if *json {
                    println!("[]");
                } else {
                    println!("No sibling goals found.");
                }
                return Ok(());
            }

            let deliverables_map = client
                .get_deliverables_map(&siblings.iter().map(|g| g.id.as_str()).collect::<Vec<_>>())
                .await;

            if *json {
                print_goals_with_deliverables_json(&siblings, &deliverables_map)?;
            } else {
                print_context_table(&siblings, &deliverables_map);
            }
            Ok(())
        }
        GoalCommands::Search { query, json } => {
            let resp = client.search_goals(query).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_search_results(&resp.data.items);
            }
            Ok(())
        }
        GoalCommands::Create {
            title,
            parent,
            org,
            description,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let req = CreateGoalRequest {
                organization_id: org_id,
                title: title.clone(),
                parent_objective_id: parent.clone(),
                description: description.clone(),
            };
            let resp: ApiResponse<Goal> = client.create_goal(&req).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                println!("Created goal: {} ({})", resp.data.title, resp.data.id);
            }
            Ok(())
        }
        GoalCommands::Delete { id, force, json } => {
            if !force {
                let resp: ApiResponse<Goal> = client.get_goal(id).await?;
                if !crate::cli::commands::confirm(&format!(
                    "Delete goal \"{}\" ({id})?",
                    resp.data.title
                ))? {
                    println!("Cancelled.");
                    return Ok(());
                }
            }

            client.delete_goal(id).await?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "deleted": true,
                        "id": id
                    }))?
                );
            } else {
                println!("Deleted goal {id}");
            }
            Ok(())
        }
        GoalCommands::Update {
            id,
            status,
            title,
            description,
            description_file,
            json,
        } => {
            let (completed_at, goal_status) = if let Some(s) = status {
                parse_status(s)?
            } else {
                (None, None)
            };

            let desc = read_text_arg(description.as_ref(), description_file.as_ref())?;

            let mut req = UpdateGoalRequest {
                status: goal_status,
                completed_at,
                title: None,
                description: desc,
            };

            if let Some(t) = title {
                req.title = Some(t.clone());
            }

            if req.status.is_none()
                && req.completed_at.is_none()
                && req.title.is_none()
                && req.description.is_none()
            {
                bail!("Nothing to update. Specify --status, --title, or --description.");
            }

            let resp: ApiResponse<Goal> = client.update_goal(id, &req).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let (label, _) = resolve_status(resp.data.is_completed, resp.data.status.as_ref());
                println!("Updated goal: {} [{label}]", resp.data.title);
            }
            Ok(())
        }
        GoalCommands::Archive { id, json } => {
            client.archive_goals(vec![id.clone()]).await?;
            print_status_result(*json, "archived", id)
        }
        GoalCommands::Unarchive { id, json } => {
            client.unarchive_goals(vec![id.clone()]).await?;
            print_status_result(*json, "unarchived", id)
        }
        GoalCommands::Restore { id, json } => {
            client.restore_goals(vec![id.clone()]).await?;
            print_status_result(*json, "restored", id)
        }
        GoalCommands::Duplicate { id, parent, json } => {
            let resp = client.duplicate_goal(id, parent).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                println!(
                    "Duplicated goal: {} ({}) under parent {parent}",
                    resp.data.title, resp.data.id
                );
            }
            Ok(())
        }
        GoalCommands::Move {
            id,
            parent,
            root,
            json,
        } => {
            if parent.is_none() && !*root {
                bail!("Specify --parent <ID> or --root.");
            }
            let new_parent = if *root { None } else { parent.clone() };
            let resp = client.change_goal_parent(id, new_parent.clone()).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let dest = new_parent.as_deref().unwrap_or("(root)");
                println!("Moved goal {} to {dest}", resp.data.id);
            }
            Ok(())
        }
        GoalCommands::Share { command } => handle_share(command, client).await,
        GoalCommands::Alias { command } => handle_alias(command, client).await,
    }
}

fn print_status_result(json: bool, action: &str, id: &str) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "action": action,
                "id": id,
            }))?
        );
    } else {
        println!("Goal {id} {action}");
    }
    Ok(())
}

async fn handle_share(cmd: &ShareCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ShareCommands::Create { id, json } => {
            let resp = client.create_share_link(id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if let Some(url) = &resp.share_url {
                println!("Share URL: {url}");
            } else if let Some(pid) = &resp.public_id {
                println!("Public ID: {pid}");
            } else {
                println!("Share link created for goal {id}");
            }
            Ok(())
        }
        ShareCommands::Revoke { id, force } => {
            if !*force
                && !crate::cli::commands::confirm(&format!("Revoke share link for goal {id}?"))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.revoke_share_link(id).await?;
            println!("Share link revoked for goal {id}");
            Ok(())
        }
    }
}

async fn handle_alias(cmd: &AliasCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        AliasCommands::Add {
            parent_id,
            target,
            order,
            json,
        } => {
            let alias = client.create_alias(parent_id, target, *order).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&alias)?);
            } else {
                // Backend wraps the alias in {"data": {...}} (ApiResponse).
                let inner = alias.get("data").unwrap_or(&alias);
                let alias_id = inner
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(unknown)");
                println!(
                    "Alias created: {alias_id} (parent={parent_id}, target={target}, order={order})"
                );
            }
            Ok(())
        }
        AliasCommands::Rm {
            parent_id,
            alias_id,
            force,
        } => {
            if !*force
                && !crate::cli::commands::confirm(&format!(
                    "Delete alias {alias_id} from parent {parent_id}?"
                ))?
            {
                println!("Cancelled.");
                return Ok(());
            }
            client.delete_alias(parent_id, alias_id).await?;
            println!("Alias {alias_id} deleted");
            Ok(())
        }
        AliasCommands::Reorder { parent_id, order } => {
            let ids: Vec<String> = order
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if ids.is_empty() {
                bail!("--order must contain at least one alias ID");
            }
            client.reorder_aliases(parent_id, ids).await?;
            println!("Aliases reordered under parent {parent_id}");
            Ok(())
        }
    }
}

impl GoalChildNode {
    fn print_goal_detail_subtree(
        &self,
        current_depth: usize,
        with_deliverable: bool,
        with_comment: bool,
    ) {
        let indent = " ".repeat(current_depth * 4);
        let (_, colored_status) = resolve_status(self.goal.is_completed, self.goal.status.as_ref());
        println!(
            "{}└─ {} [{colored_status}]",
            &indent[3.min(indent.len())..],
            self.goal.title.bold()
        );
        println!("{indent}   {}: {}", "ID".dimmed(), self.goal.id.dimmed());

        // 成果物も表示するオプションが立っているとき
        if with_deliverable {
            if let Some(deliverables) = &self.deliverables {
                if deliverables.is_empty() {
                    println!("{indent}   {}: {}", "成果物".dimmed(), "(なし)".dimmed());
                } else {
                    println!("{indent}   {}:", "成果物".dimmed());
                    for d in deliverables {
                        println!(
                            "{indent}     - {} {}",
                            d.node_type.as_icon(),
                            d.display_name
                        );
                        match &d.node_type {
                            DeliverableType::Link => {
                                if let Some(link) = &d.link_url {
                                    println!("       {link}");
                                }
                            }
                            DeliverableType::Document => {
                                if let Some(content) = &d.content {
                                    let truncated = if content.chars().count() > 30 {
                                        let end = content
                                            .char_indices()
                                            .nth(27)
                                            .map(|(i, _)| i)
                                            .unwrap_or(content.len());
                                        &format!("{}...", &content[..end])
                                    } else {
                                        content
                                    };
                                    println!("       {truncated}");
                                }
                            }
                            DeliverableType::File | DeliverableType::Folder => {
                                // nothing to do
                            }
                        }
                    }
                }
            } else {
                println!(
                    "{indent}   {}: {}",
                    "成果物".dimmed(),
                    "(取得失敗)".dimmed()
                );
            }
        }

        if with_comment {
            if let Some(comments) = &self.comments {
                if comments.is_empty() {
                    println!("{indent}   {}: {}", "コメント".dimmed(), "(なし)".dimmed());
                } else {
                    println!("{indent}   {}:", "コメント".dimmed());
                    for comment in comments {
                        let content = comment.content.replace('\n', " ");
                        let truncated = if content.chars().count() > 30 {
                            let end = content
                                .char_indices()
                                .nth(27)
                                .map(|(i, _)| i)
                                .unwrap_or(content.len());
                            format!("{}...", &content[..end])
                        } else {
                            content
                        };

                        let author_name = if comment.author.is_ai_agent {
                            format!("{} (AI)", comment.author.name)
                        } else {
                            comment.author.name.clone()
                        };

                        let date = &comment.created_at[..10.min(comment.created_at.len())];

                        println!(
                            "{indent}     - \"{}\" {} {}",
                            truncated,
                            author_name.dimmed(),
                            date.dimmed(),
                        );
                    }
                }
            } else {
                println!(
                    "{indent}   {}: {}",
                    "コメント".dimmed(),
                    "(取得失敗)".dimmed()
                );
            }
        }

        if let Some(children) = &self.children {
            for c in children {
                c.print_goal_detail_subtree(current_depth + 1, with_deliverable, with_comment);
            }
        }
    }
}

impl GoalNode {
    fn print_goal_detail_tree(&self) {
        let (_, colored_status) = resolve_status(self.goal.is_completed, self.goal.status.as_ref());
        println!("{} [{colored_status}]", self.goal.title.bold());
        println!("   {}: {}", "ID".dimmed(), self.goal.id.dimmed());

        if let Some(desc) = &self.goal.description
            && !desc.is_empty()
        {
            println!("   {}: {desc}", "完了条件".dimmed());
        }
        if let Some(parent_id) = &self.goal.parent_id {
            println!("   {}: {}", "Parent".dimmed(), parent_id.dimmed());
        }
        if let Some(owner) = &self.goal.owner {
            println!("   {}: {}", "Owner".dimmed(), owner.name);
        }
        if let Some(due) = &self.goal.due_date {
            println!("   {}: {}", "Due".dimmed(), &due[..10.min(due.len())]);
        }
        if let Some(body) = &self.goal.body
            && !body.is_empty()
        {
            println!("\n   {}", "Body".dimmed());
            println!("{body}");
        }

        // 成果物も表示するオプションが立っているとき
        if let Some(deliverables) = &self.deliverables {
            if deliverables.is_empty() {
                println!("   {}: {}", "成果物".dimmed(), "(なし)".dimmed());
            } else {
                println!("   {}:", "成果物".dimmed());
                for d in deliverables {
                    println!("     - {} {}", d.node_type.as_icon(), d.display_name);
                    match &d.node_type {
                        DeliverableType::Link => {
                            if let Some(link) = &d.link_url {
                                println!("       {link}");
                            }
                        }
                        DeliverableType::Document => {
                            if let Some(content) = &d.content {
                                let truncated = if content.chars().count() > 30 {
                                    let end = content
                                        .char_indices()
                                        .nth(27)
                                        .map(|(i, _)| i)
                                        .unwrap_or(content.len());
                                    &format!("{}...", &content[..end])
                                } else {
                                    content
                                };
                                println!("       {truncated}");
                            }
                        }
                        DeliverableType::File | DeliverableType::Folder => {
                            // nothing to do
                        }
                    }
                }
            }
        }
        let with_deliverable = self.deliverables.is_some();

        // コメントも表示するオプションが立っているとき
        if let Some(comments) = &self.comments {
            if comments.is_empty() {
                println!("   {}: {}", "コメント".dimmed(), "(なし)".dimmed());
            } else {
                println!("   {}:", "コメント".dimmed());
                for comment in comments {
                    let content = comment.content.replace('\n', " ");
                    let truncated = if content.chars().count() > 30 {
                        let end = content
                            .char_indices()
                            .nth(27)
                            .map(|(i, _)| i)
                            .unwrap_or(content.len());
                        format!("{}...", &content[..end])
                    } else {
                        content
                    };

                    let author_name = if comment.author.is_ai_agent {
                        format!("{} (AI)", comment.author.name)
                    } else {
                        comment.author.name.clone()
                    };

                    let date = &comment.created_at[..10.min(comment.created_at.len())];

                    println!(
                        "     - \"{}\" {} {}",
                        truncated,
                        author_name.dimmed(),
                        date.dimmed(),
                    );
                }
            }
        }
        let with_comment = self.comments.is_some();

        // 子の階層を表示するオプションが立っているとき
        if let Some(children) = &self.children {
            for c in children {
                c.print_goal_detail_subtree(1, with_deliverable, with_comment);
            }
        }
    }
}
