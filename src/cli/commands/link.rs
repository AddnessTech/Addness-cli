use anyhow::{Result, bail};
use clap::Subcommand;

use crate::api::ApiClient;

#[derive(Subcommand)]
pub enum LinkCommands {
    /// Link a PR or URL to a goal
    Pr {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// PR URL (e.g., https://github.com/org/repo/pull/42)
        #[arg(long)]
        url: String,
        /// Display name (auto-detected from URL if omitted)
        #[arg(long)]
        name: Option<String>,
        /// Also post a comment on the goal
        #[arg(long)]
        comment: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Record a progress note on a goal (comment + optional status update)
    Progress {
        /// Goal ID
        #[arg(long)]
        goal: String,
        /// Progress message
        #[arg(long)]
        message: String,
        /// Update status: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED
        #[arg(long)]
        status: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn pr_display_name(url: &str) -> String {
    // https://github.com/org/repo/pull/42 → "org/repo#42"
    let parts: Vec<&str> = url.trim_end_matches('/').split('/').collect();
    if parts.len() >= 5 {
        let idx = parts.len();
        if parts[idx - 2] == "pull" || parts[idx - 2] == "pulls" {
            return format!("{}/{}#{}", parts[idx - 4], parts[idx - 3], parts[idx - 1]);
        }
    }
    url.to_string()
}

pub async fn handle_link(cmd: &LinkCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        LinkCommands::Pr {
            goal,
            url,
            name,
            comment,
            json,
        } => {
            let display_name = name.clone().unwrap_or_else(|| pr_display_name(url));

            let resp = client
                .create_link_deliverable(goal, url, &display_name)
                .await?;

            if let Some(msg) = comment {
                let comment_body = format!("{msg}\n\n{url}");
                client.create_comment(goal, &comment_body).await?;
            }

            if *json {
                println!("{}", serde_json::to_string_pretty(&resp.data)?);
            } else {
                println!("Linked {} to goal {goal}", display_name);
                if comment.is_some() {
                    println!("  Comment posted.");
                }
            }
            Ok(())
        }
        LinkCommands::Progress {
            goal,
            message,
            status,
            json,
        } => {
            let comment = client.create_comment(goal, message).await?;

            if let Some(status_str) = status {
                use crate::api::{ApiResponse, Goal, GoalStatus, UpdateGoalRequest};
                let (completed_at, goal_status) = match status_str.to_uppercase().as_str() {
                    "NOT_STARTED" => (Some(None), Some(GoalStatus::None)),
                    "IN_PROGRESS" => (Some(None), Some(GoalStatus::InProgress)),
                    "COMPLETED" => {
                        let now = chrono::Utc::now().to_rfc3339();
                        (Some(Some(now)), None)
                    }
                    "CANCELLED" => (Some(None), Some(GoalStatus::Cancelled)),
                    _ => bail!(
                        "Invalid status: '{status_str}'. Use one of: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED"
                    ),
                };
                let req = UpdateGoalRequest {
                    status: goal_status,
                    completed_at,
                    title: None,
                    description: None,
                    body: None,
                    due_date: None,
                };
                let _: ApiResponse<Goal> = client.update_goal(goal, &req).await?;
            }

            if *json {
                println!("{}", serde_json::to_string_pretty(&comment)?);
            } else {
                println!("Progress recorded on goal {goal}");
                if let Some(s) = status {
                    println!("  Status updated to {s}");
                }
            }
            Ok(())
        }
    }
}
