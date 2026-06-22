use anyhow::Result;
use clap::Subcommand;
use colored::Colorize;

use crate::api::{
    ApiClient, ApiResponse, CreateGoalRequest, Goal, GoalStatus, TodaysGoalsData, UpdateGoalRequest,
};
use crate::cli::commands::goal::parse_status;
use crate::cli::commands::org::resolve_org_id;
use crate::cli::output::resolve_status;

#[derive(Subcommand)]
pub enum TodayCommands {
    /// List today's goals (default when no subcommand is given)
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Add a new goal as today's todo
    Add {
        /// Goal title
        #[arg(long)]
        title: String,
        /// Parent goal ID (omit to create as root goal)
        #[arg(long)]
        parent: Option<String>,
        /// Description (definition of done)
        #[arg(long)]
        description: Option<String>,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Mark a today's goal as completed
    Done {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reopen a completed goal (mark as not completed)
    Reopen {
        /// Goal ID
        id: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Change a goal's status (NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED)
    Status {
        /// Goal ID
        id: String,
        /// Status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        status: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Handle `today` command. `None` means no subcommand → list today's goals.
pub async fn handle_today(cmd: Option<&TodayCommands>, client: &ApiClient) -> Result<()> {
    match cmd {
        None => list_todays_goals(None, None, false, client).await,
        Some(TodayCommands::List { org, date, json }) => {
            list_todays_goals(org.as_deref(), date.as_deref(), *json, client).await
        }
        Some(TodayCommands::Add {
            title,
            parent,
            description,
            org,
            json,
        }) => {
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
                println!("Added today's todo: {} ({})", resp.data.title, resp.data.id);
            }
            Ok(())
        }
        Some(TodayCommands::Done { id, json }) => {
            let req = UpdateGoalRequest {
                status: None,
                completed_at: Some(Some(chrono::Utc::now().to_rfc3339())),
                title: None,
                description: None,
            };
            update_and_report(id, &req, "Completed", *json, client).await
        }
        Some(TodayCommands::Reopen { id, json }) => {
            let req = UpdateGoalRequest {
                status: Some(GoalStatus::None),
                completed_at: Some(None),
                title: None,
                description: None,
            };
            update_and_report(id, &req, "Reopened", *json, client).await
        }
        Some(TodayCommands::Status { id, status, json }) => {
            let (completed_at, goal_status) = parse_status(status)?;
            let req = UpdateGoalRequest {
                status: goal_status,
                completed_at,
                title: None,
                description: None,
            };
            update_and_report(id, &req, "Updated", *json, client).await
        }
    }
}

async fn list_todays_goals(
    org: Option<&str>,
    date: Option<&str>,
    json: bool,
    client: &ApiClient,
) -> Result<()> {
    let org_id = resolve_org_id(org)?;
    let resp: ApiResponse<TodaysGoalsData> = client.get_todays_goals(&org_id, date, None).await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&resp.data)?);
        return Ok(());
    }

    if resp.data.nodes.is_empty() {
        println!("No goals for today.");
        return Ok(());
    }

    for node in &resp.data.nodes {
        let indent = "  ".repeat(node.depth.max(0) as usize);
        let check = if node.is_completed() { "[x]" } else { "[ ]" };
        let parsed = node.parsed_status();
        let (_, colored_status) = resolve_status(node.is_completed(), parsed.as_ref());
        println!(
            "{indent}{check} {} {}  {}",
            colored_status,
            node.title,
            node.id.dimmed()
        );
    }

    Ok(())
}

async fn update_and_report(
    id: &str,
    req: &UpdateGoalRequest,
    verb: &str,
    json: bool,
    client: &ApiClient,
) -> Result<()> {
    let resp: ApiResponse<Goal> = client.update_goal(id, req).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&resp.data)?);
    } else {
        let (label, _) = resolve_status(resp.data.is_completed, resp.data.status.as_ref());
        println!("{verb} today's todo: {} [{label}]", resp.data.title);
    }
    Ok(())
}
