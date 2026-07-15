use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;

use crate::api::{ApiClient, CodexTodaysGoalsApplyRequest, UpdateGoalPreferenceRequest};
use crate::cli::commands::org::resolve_org_id;

/// Build a client whose `X-Organization-ID` header targets `org_id`, for the
/// handful of execution-tab endpoints that live outside
/// `/organizations/:id/...` and so resolve the organization purely from the
/// header (`execute-goals/generate`, `execute-goals/:id`,
/// `todays-goals/active-huddles`, `codex/todays-goals/*`). Mirrors
/// `client_for_org` in `org.rs`/`media.rs`.
fn client_for_org(client: &ApiClient, org_id: &str) -> ApiClient {
    let mut scoped = client.clone();
    scoped.set_org_id(Some(org_id.to_string()));
    scoped
}

fn parse_json_arg(raw: &str, flag: &str) -> Result<serde_json::Value> {
    serde_json::from_str(raw).with_context(|| format!("{flag} must be valid JSON"))
}

#[derive(Subcommand)]
pub enum ExecutionCommands {
    /// Show today's goal count summary (total/incomplete/is-goal-uncreated)
    Summary {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (required)
        #[arg(long)]
        date: String,
        /// Filter to a specific organization member (UUID)
        #[arg(long)]
        member_id: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show per-member completed-execution counts (paginated report)
    MemberSummary {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Range start in YYYY-MM-DD
        #[arg(long)]
        from: Option<String>,
        /// Range end in YYYY-MM-DD
        #[arg(long)]
        to: Option<String>,
        /// Member name search
        #[arg(long)]
        query: Option<String>,
        /// Filter by goal assignment (requires --type)
        #[arg(long)]
        objective_id: Option<String>,
        /// Comma-separated assignment types: owner, editor, member, non-member (requires --objective-id)
        #[arg(long = "type")]
        assignment_type: Option<String>,
        /// Comma-separated tag UUIDs (max 20)
        #[arg(long)]
        tag_ids: Option<String>,
        /// Page number (default 1)
        #[arg(long)]
        page: Option<u32>,
        /// Page size (default 10, max 100)
        #[arg(long)]
        page_size: Option<u32>,
        /// Sort field: name, created_at, completed_count
        #[arg(long)]
        sort_by: Option<String>,
        /// Sort direction: asc, desc
        #[arg(long)]
        sort_dir: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate today's execution records for every goal due today
    Generate {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Update an execution record's status/completion
    Update {
        /// Execution record ID
        record_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// New status: NONE or IN_PROGRESS
        #[arg(long)]
        status: Option<String>,
        /// Mark completed at this RFC3339 timestamp
        #[arg(long, conflicts_with = "uncomplete")]
        completed_at: Option<String>,
        /// Mark as not completed (clears completedAt)
        #[arg(long)]
        uncomplete: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show execution history with completion-rate/streak stats
    History {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Range start in YYYY-MM-DD
        #[arg(long)]
        from: String,
        /// Range end in YYYY-MM-DD
        #[arg(long)]
        to: String,
        /// Filter to a single goal
        #[arg(long)]
        objective_id: Option<String>,
        /// Max records to return (default 50, max 200)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Manage the collapsed/expanded state of goals in the execution tab
    Preference {
        #[command(subcommand)]
        command: PreferenceCommands,
    },
    /// List active huddles (voice calls) attached to today's goals
    ActiveHuddles {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Interact with the Codex-agent view of today's goals
    Codex {
        #[command(subcommand)]
        command: CodexCommands,
    },
}

#[derive(Subcommand)]
pub enum PreferenceCommands {
    /// Show which goals are currently collapsed in the execution tab
    Get {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Replace the set of collapsed goal IDs
    Set {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Goal IDs to collapse (repeatable; pass none to expand everything)
        #[arg(long = "goal-id")]
        goal_ids: Vec<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub enum CodexCommands {
    /// Show the Codex-agent read-only view of today's goals (raw JSON; short-id + DSL
    /// format designed for the Codex agent, not for human editing)
    View {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Present for consistency with other commands; the response shape is
        /// opaque (Codex-agent DSL) so it is always printed as JSON
        #[arg(long)]
        json: bool,
    },
    /// Apply a batch of Codex-agent changes to today's goals
    Apply {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Payload version (defaults to 1)
        #[arg(long)]
        version: Option<i32>,
        /// Date in YYYY-MM-DD (defaults to today)
        #[arg(long)]
        date: Option<String>,
        /// Raw JSON array of changes, per the Codex apply DSL returned by `execution codex view`
        #[arg(long)]
        changes_json: String,
        /// Present for consistency with other commands; the response shape is
        /// opaque (Codex-agent DSL) so it is always printed as JSON
        #[arg(long)]
        json: bool,
    },
}

pub async fn handle_execution(cmd: &ExecutionCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ExecutionCommands::Summary {
            org,
            date,
            member_id,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let summary = client
                .get_todays_goals_summary(&org_id, date, member_id.as_deref())
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&summary)?);
            } else {
                println!(
                    "total: {}  incomplete: {}  isGoalUncreated: {}",
                    summary.total_count, summary.incomplete_count, summary.is_goal_uncreated
                );
            }
            Ok(())
        }
        ExecutionCommands::MemberSummary {
            org,
            from,
            to,
            query,
            objective_id,
            assignment_type,
            tag_ids,
            page,
            page_size,
            sort_by,
            sort_dir,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client
                .get_execution_member_summary(
                    &org_id,
                    from.as_deref(),
                    to.as_deref(),
                    query.as_deref(),
                    objective_id.as_deref(),
                    assignment_type.as_deref(),
                    tag_ids.as_deref(),
                    *page,
                    *page_size,
                    sort_by.as_deref(),
                    sort_dir.as_deref(),
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.members.is_empty() {
                println!("No members found.");
            } else {
                for member in &resp.members {
                    println!(
                        "{} — {} completed ({})",
                        member.name, member.completed_count, member.member_id
                    );
                }
                println!(
                    "page {}/{} ({} total)",
                    resp.page, resp.total_pages, resp.total_count
                );
            }
            Ok(())
        }
        ExecutionCommands::Generate { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.generate_execution().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Generated {} execution record(s).", resp.created);
            }
            Ok(())
        }
        ExecutionCommands::Update {
            record_id,
            org,
            status,
            completed_at,
            uncomplete,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let mut body = serde_json::Map::new();
            if let Some(status) = status {
                body.insert(
                    "status".to_string(),
                    serde_json::Value::String(status.clone()),
                );
            }
            if *uncomplete {
                body.insert("completedAt".to_string(), serde_json::Value::Null);
            } else if let Some(completed_at) = completed_at {
                body.insert(
                    "completedAt".to_string(),
                    serde_json::Value::String(completed_at.clone()),
                );
            }
            let record = scoped
                .update_execution(record_id, &serde_json::Value::Object(body))
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&record)?);
            } else {
                match record {
                    Some(record) => println!(
                        "Updated execution record {} [{:?}]",
                        record.id, record.status
                    ),
                    None => println!("No execution record found for {record_id}"),
                }
            }
            Ok(())
        }
        ExecutionCommands::History {
            org,
            from,
            to,
            objective_id,
            limit,
            offset,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let history = client
                .get_execution_history(&org_id, from, to, objective_id.as_deref(), *limit, *offset)
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&history)?);
            } else {
                if let Some(stats) = &history.stats {
                    println!(
                        "completion rate: {:.1}%  streak: {} (longest {})",
                        stats.completion_rate * 100.0,
                        stats.current_streak,
                        stats.longest_streak
                    );
                }
                for record in &history.records {
                    let done = if record.completed_at.is_some() {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    println!("{done} {} {}", record.date, record.objective_id.dimmed());
                }
                println!("total: {}", history.pagination.total);
            }
            Ok(())
        }
        ExecutionCommands::Preference { command } => handle_preference(command, client).await,
        ExecutionCommands::ActiveHuddles { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let resp = scoped.get_active_huddles().await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else if resp.active_huddles.is_empty() {
                println!("No active huddles.");
            } else {
                for huddle in &resp.active_huddles {
                    println!(
                        "{} — {} participant(s) since {}",
                        huddle.objective_id,
                        huddle.participants.len(),
                        huddle.started_at
                    );
                }
            }
            Ok(())
        }
        ExecutionCommands::Codex { command } => handle_codex(command, client).await,
    }
}

async fn handle_preference(cmd: &PreferenceCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        PreferenceCommands::Get { org, json } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let pref = client.get_goal_preference(&org_id).await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&pref)?);
            } else if pref.collapsed_goal_ids.is_empty() {
                println!("No goals are collapsed.");
            } else {
                for id in &pref.collapsed_goal_ids {
                    println!("{id}");
                }
            }
            Ok(())
        }
        PreferenceCommands::Set {
            org,
            goal_ids,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let req = UpdateGoalPreferenceRequest {
                collapsed_goal_ids: goal_ids.clone(),
            };
            client.update_goal_preference(&org_id, &req).await?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(
                        &serde_json::json!({"collapsedGoalIds": goal_ids})
                    )?
                );
            } else {
                println!("Set {} collapsed goal(s).", goal_ids.len());
            }
            Ok(())
        }
    }
}

async fn handle_codex(cmd: &CodexCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        CodexCommands::View { org, date, json: _ } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let view = scoped.get_codex_todays_goals_view(date.as_deref()).await?;
            println!("{}", serde_json::to_string_pretty(&view)?);
            Ok(())
        }
        CodexCommands::Apply {
            org,
            version,
            date,
            changes_json,
            json: _,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let scoped = client_for_org(client, &org_id);
            let changes = parse_json_arg(changes_json, "--changes-json")?;
            let req = CodexTodaysGoalsApplyRequest {
                version: version.unwrap_or(1),
                date: date.clone(),
                changes,
            };
            let resp = scoped.apply_codex_todays_goals(&req).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_json_arg;

    #[test]
    fn parse_json_arg_accepts_valid_json_array() {
        let value =
            parse_json_arg(r#"[{"op":"complete","goalId":"g1"}]"#, "--changes-json").unwrap();
        assert!(value.is_array());
        assert_eq!(value[0]["op"], "complete");
    }

    #[test]
    fn parse_json_arg_rejects_invalid_json() {
        let err = parse_json_arg("not json", "--changes-json").unwrap_err();
        assert!(err.to_string().contains("--changes-json"));
    }
}
