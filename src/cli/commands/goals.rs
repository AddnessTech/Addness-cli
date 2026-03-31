use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{
    ApiClient, ApiResponse, Deliverable, Goal, GoalStatus, TreeData, UpdateGoalRequest,
};
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::{
    print_children_table, print_goal_detail, print_goals_table, print_search_results,
    resolve_status,
};

#[derive(Subcommand)]
pub enum GoalsCommands {
    /// List goals in the organization tree
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Tree depth (default: 3)
        #[arg(long, default_value = "3")]
        depth: usize,
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
    /// Get child goals with their deliverables and descriptions
    Context {
        /// Goal ID
        id: String,
        /// Max number of children to return (default: 50)
        #[arg(long, default_value = "50")]
        limit: usize,
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
    /// Update a goal's status or title
    Update {
        /// Goal ID
        id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        #[arg(long)]
        status: Option<String>,
        /// Title
        #[arg(long)]
        title: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

use crate::api::ChildItem;
use colored::Colorize;
use std::collections::HashMap;

/// 各ゴールの成果物を並行取得してマップで返す
async fn fetch_deliverables_map(
    client: &ApiClient,
    goals: &[ChildItem],
) -> HashMap<String, Vec<Deliverable>> {
    let futures: Vec<_> = goals
        .iter()
        .map(|g| client.get_goal_deliverables(&g.id))
        .collect();
    let results = futures::future::join_all(futures).await;

    let mut map = HashMap::new();
    for (i, result) in results.into_iter().enumerate() {
        match result {
            Ok(resp) => {
                map.insert(goals[i].id.clone(), resp.data.deliverables);
            }
            Err(e) => {
                eprintln!(
                    "Warning: failed to fetch deliverables for {}: {e}",
                    goals[i].id
                );
            }
        }
    }
    map
}

/// ゴール一覧+成果物マップをJSON出力
fn print_goals_with_deliverables_json(
    goals: &[ChildItem],
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
    children: &[ChildItem],
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
                    let type_icon = match d.node_type.as_str() {
                        "folder" => "📁",
                        "document" => "📄",
                        "file" => "📎",
                        "link" => "🔗",
                        _ => "  ",
                    };
                    println!("     {type_icon} {}", d.display_name);
                }
            }
        } else {
            println!("   {}: {}", "成果物".dimmed(), "(取得失敗)".dimmed());
        }
    }
}

/// Parse CLI status into (is_completed, api_status) pair.
/// CLI status      → is_completed  | API status
/// NOT_STARTED     → false         | NONE
/// IN_PROGRESS     → false         | IN_PROGRESS
/// COMPLETED       → true          | (unchanged)
/// CANCELLED       → false         | CANCELLED
fn parse_status(status: &str) -> Result<(Option<bool>, Option<GoalStatus>)> {
    match status.to_uppercase().as_str() {
        "NOT_STARTED" => Ok((Some(false), Some(GoalStatus::None))),
        "IN_PROGRESS" => Ok((Some(false), Some(GoalStatus::InProgress))),
        "COMPLETED" => Ok((Some(true), Some(GoalStatus::None))),
        "CANCELLED" => Ok((Some(false), Some(GoalStatus::Cancelled))),
        _ => bail!(
            "Invalid status: '{status}'. Use one of: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED"
        ),
    }
}

pub async fn handle_goals(cmd: &GoalsCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        GoalsCommands::List { org, depth, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp: ApiResponse<TreeData> = client.get_goal_tree(&org_id, *depth).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_goals_table(&resp.data.items);
            }
            Ok(())
        }
        GoalsCommands::Get { id, json } => {
            let resp: ApiResponse<Goal> = client.get_goal(id).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                print_goal_detail(&resp.data);
            }
            Ok(())
        }
        GoalsCommands::Children {
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
        GoalsCommands::Tree { id, json } => {
            let resp: ApiResponse<TreeData> = client.get_goal_subtree(id).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_goals_table(&resp.data.items);
            }
            Ok(())
        }
        GoalsCommands::Context { id, limit, json } => {
            let children_resp = client.get_goal_children(id, *limit, 0).await?;
            let children = children_resp.data.children;

            if children.is_empty() {
                if *json {
                    println!("[]");
                } else {
                    println!("No children found.");
                }
                return Ok(());
            }

            let deliverables_map = fetch_deliverables_map(client, &children).await;

            if *json {
                print_goals_with_deliverables_json(&children, &deliverables_map)?;
            } else {
                print_context_table(&children, &deliverables_map);
            }
            Ok(())
        }
        GoalsCommands::Siblings { id, limit, json } => {
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

            let deliverables_map = fetch_deliverables_map(client, &siblings).await;

            if *json {
                print_goals_with_deliverables_json(&siblings, &deliverables_map)?;
            } else {
                print_context_table(&siblings, &deliverables_map);
            }
            Ok(())
        }
        GoalsCommands::Search { query, json } => {
            let resp = client.search_goals(query).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data.items)?);
            } else {
                print_search_results(&resp.data.items);
            }
            Ok(())
        }
        GoalsCommands::Update {
            id,
            org,
            status,
            title,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;

            let (is_completed, goal_status) = if let Some(s) = status {
                parse_status(s)?
            } else {
                (None, None)
            };

            let mut req = UpdateGoalRequest {
                status: goal_status,
                is_completed,
                title: None,
                description: None,
            };

            if let Some(t) = title {
                req.title = Some(t.clone());
            }

            if req.status.is_none() && req.is_completed.is_none() && req.title.is_none() {
                bail!("Nothing to update. Specify --status or --title.");
            }

            let resp: ApiResponse<Goal> = client.update_goal(&org_id, id, &req).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let (label, _) = resolve_status(resp.data.is_completed, resp.data.status.as_ref());
                println!("Updated goal: {} [{label}]", resp.data.title);
            }
            Ok(())
        }
    }
}
