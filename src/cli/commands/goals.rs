use anyhow::{bail, Result};
use clap::Subcommand;

use crate::api::{ApiClient, ApiResponse, Goal, TreeData, UpdateGoalRequest};
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::print_goals_table;

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

fn parse_status(status: &str) -> Result<UpdateGoalRequest> {
    match status.to_uppercase().as_str() {
        "NOT_STARTED" => Ok(UpdateGoalRequest {
            status: Some("NONE".to_string()),
            completed_at: Some(serde_json::Value::Null),
            title: None,
            description: None,
        }),
        "IN_PROGRESS" => Ok(UpdateGoalRequest {
            status: Some("IN_PROGRESS".to_string()),
            completed_at: Some(serde_json::Value::Null),
            title: None,
            description: None,
        }),
        "COMPLETED" => {
            let now = chrono::Utc::now().to_rfc3339();
            Ok(UpdateGoalRequest {
                status: Some("NONE".to_string()),
                completed_at: Some(serde_json::Value::String(now)),
                title: None,
                description: None,
            })
        }
        "CANCELLED" => Ok(UpdateGoalRequest {
            status: Some("CANCELLED".to_string()),
            completed_at: Some(serde_json::Value::Null),
            title: None,
            description: None,
        }),
        _ => bail!(
            "Invalid status: '{}'. Use one of: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED",
            status
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
            status,
            title,
            json,
        } => {
            let mut req = if let Some(s) = status {
                parse_status(s)?
            } else {
                UpdateGoalRequest {
                    status: None,
                    completed_at: None,
                    title: None,
                    description: None,
                }
            };

            if let Some(t) = title {
                req.title = Some(t.clone());
            }

            if req.status.is_none() && req.completed_at.is_none() && req.title.is_none() {
                bail!("Nothing to update. Specify --status or --title.");
            }

            let resp: ApiResponse<Goal> = client.update_goal(id, &req).await?;

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                let display_status = if resp.data.is_completed {
                    "COMPLETED"
                } else {
                    match resp.data.status.as_deref() {
                        Some("IN_PROGRESS") => "IN_PROGRESS",
                        Some("CANCELLED") => "CANCELLED",
                        _ => "NOT_STARTED",
                    }
                };
                println!("Updated: {} → {}", resp.data.title, display_status);
            }
            Ok(())
        }
    }
}
