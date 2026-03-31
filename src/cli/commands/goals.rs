use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::{ApiClient, ApiResponse, Goal, GoalStatus, TreeData, UpdateGoalRequest};
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::{print_goals_table, resolve_status};

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
