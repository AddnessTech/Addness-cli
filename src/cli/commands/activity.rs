use anyhow::Result;
use clap::Subcommand;

use crate::api::{
    ActivityLogByGoalParams, ActivityLogByMemberParams, ActivityLogListResponse,
    ActivityLogSummaryParams, ApiClient, GoalActivitySummaryParams,
};
use crate::cli::commands::member::resolve_self_member_id;
use crate::cli::commands::org::resolve_org_id;

#[derive(Subcommand)]
pub enum ActivityCommands {
    /// List a member's activity log (defaults to your own)
    List {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Member ID (defaults to yourself)
        #[arg(long)]
        member: Option<String>,
        /// Filter from this date (RFC3339, e.g. 2026-07-01T00:00:00Z)
        #[arg(long)]
        start: Option<String>,
        /// Filter until this date (RFC3339)
        #[arg(long)]
        end: Option<String>,
        /// Filter by event type. Comma-separated or repeatable.
        #[arg(long = "event-type")]
        event_type: Vec<String>,
        /// Filter by event category. Comma-separated or repeatable.
        #[arg(long = "event-category")]
        event_category: Vec<String>,
        /// Max items to return (1-1000, default 50)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a goal's activity log
    Goal {
        /// Goal ID
        goal_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Filter from this date (RFC3339)
        #[arg(long)]
        start: Option<String>,
        /// Filter until this date (RFC3339)
        #[arg(long)]
        end: Option<String>,
        /// Filter by event type. Comma-separated or repeatable.
        #[arg(long = "event-type")]
        event_type: Vec<String>,
        /// Include immediate child goals' activity
        #[arg(long)]
        include_children: bool,
        /// Max items to return (1-1000, default 50)
        #[arg(long)]
        limit: Option<u32>,
        /// Pagination offset
        #[arg(long)]
        offset: Option<u32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show an organization-wide activity summary
    Summary {
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Filter from this date (RFC3339)
        #[arg(long)]
        start: Option<String>,
        /// Filter until this date (RFC3339)
        #[arg(long)]
        end: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show a goal's activity summary widget (created/completed counts by member)
    GoalSummary {
        /// Goal ID
        goal_id: String,
        /// Organization ID (uses default if not specified)
        #[arg(long)]
        org: Option<String>,
        /// Filter from this date (RFC3339)
        #[arg(long)]
        start: Option<String>,
        /// Filter until this date (RFC3339)
        #[arg(long)]
        end: Option<String>,
        /// Include descendant goals' activity (recursive)
        #[arg(long)]
        include_children: bool,
        /// Max members to return (1-50, default 10)
        #[arg(long)]
        limit: Option<u32>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

/// Comma-separated IDのリストをトリム済みの `Vec<String>` に分割する
/// （複数指定可能な `--event-type`/`--event-category` 引数の共通処理。notification.rsの流儀に合わせる）。
fn split_csv(csv: &str) -> Vec<String> {
    csv.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// `--event-type`/`--event-category` は繰り返し指定・カンマ区切りの両方を許容する。
fn flatten_values(raw: &[String]) -> Vec<String> {
    raw.iter().flat_map(|s| split_csv(s)).collect()
}

fn print_activity_list(resp: &ActivityLogListResponse) {
    if resp.items.is_empty() {
        println!("No activity found.");
        return;
    }
    for item in &resp.items {
        let target = item.goal_info.as_ref().map(|g| g.goal_title.as_str());
        match target {
            Some(title) => println!(
                "[{}] {} - {} ({})",
                item.occurred_at, item.event_type, title, item.actor.name
            ),
            None => println!(
                "[{}] {} - {}",
                item.occurred_at, item.event_type, item.actor.name
            ),
        }
    }
    println!(
        "Showing {} of {} (offset {})",
        resp.items.len(),
        resp.total_count,
        resp.offset
    );
}

pub async fn handle_activity(cmd: &ActivityCommands, client: &ApiClient) -> Result<()> {
    match cmd {
        ActivityCommands::List {
            org,
            member,
            start,
            end,
            event_type,
            event_category,
            limit,
            offset,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let member_id = match member {
                Some(id) => id.clone(),
                None => resolve_self_member_id(client, &org_id).await?,
            };
            let event_types = flatten_values(event_type);
            let event_categories = flatten_values(event_category);
            let resp = client
                .list_activity_logs_by_member(
                    &org_id,
                    ActivityLogByMemberParams {
                        member_id: &member_id,
                        start_date: start.as_deref(),
                        end_date: end.as_deref(),
                        event_types: &event_types,
                        event_categories: &event_categories,
                        limit: *limit,
                        offset: *offset,
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                print_activity_list(&resp);
            }
            Ok(())
        }
        ActivityCommands::Goal {
            goal_id,
            org,
            start,
            end,
            event_type,
            include_children,
            limit,
            offset,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let event_types = flatten_values(event_type);
            let resp = client
                .list_activity_logs_by_goal(
                    &org_id,
                    goal_id,
                    ActivityLogByGoalParams {
                        start_date: start.as_deref(),
                        end_date: end.as_deref(),
                        event_types: &event_types,
                        include_children: *include_children,
                        limit: *limit,
                        offset: *offset,
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                print_activity_list(&resp);
            }
            Ok(())
        }
        ActivityCommands::Summary {
            org,
            start,
            end,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client
                .get_activity_log_summary(
                    &org_id,
                    ActivityLogSummaryParams {
                        start_date: start.as_deref(),
                        end_date: end.as_deref(),
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!("Total activity: {}", resp.total_count);
                if !resp.count_by_category.is_empty() {
                    println!("By category:");
                    for (category, count) in &resp.count_by_category {
                        println!("  {category}: {count}");
                    }
                }
                if !resp.most_active_members.is_empty() {
                    println!("Most active members:");
                    for m in &resp.most_active_members {
                        println!("  {} - {}", m.member_name, m.count);
                    }
                }
            }
            Ok(())
        }
        ActivityCommands::GoalSummary {
            goal_id,
            org,
            start,
            end,
            include_children,
            limit,
            json,
        } => {
            let org_id = resolve_org_id(org.as_deref())?;
            let resp = client
                .get_goal_activity_summary(
                    &org_id,
                    goal_id,
                    GoalActivitySummaryParams {
                        start_date: start.as_deref(),
                        end_date: end.as_deref(),
                        include_children: *include_children,
                        limit: *limit,
                    },
                )
                .await?;
            if *json {
                println!("{}", serde_json::to_string_pretty(&resp)?);
            } else {
                println!(
                    "Goal {goal_id}: created={} completed={}",
                    resp.created_total, resp.completed_total
                );
                for m in &resp.members {
                    println!(
                        "  {} - created={} completed={}",
                        m.member_name, m.created, m.completed
                    );
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{flatten_values, split_csv};

    #[test]
    fn split_csv_trims_and_drops_empty_entries() {
        assert_eq!(
            split_csv(" objective.create, objective.update ,,kpi.update"),
            vec!["objective.create", "objective.update", "kpi.update"]
        );
    }

    #[test]
    fn flatten_values_supports_repeated_and_comma_separated_forms() {
        let raw = vec![
            "objective.create,objective.update".to_string(),
            "kpi.update".to_string(),
        ];
        assert_eq!(
            flatten_values(&raw),
            vec!["objective.create", "objective.update", "kpi.update"]
        );
    }

    #[test]
    fn flatten_values_returns_empty_for_no_input() {
        let raw: Vec<String> = vec![];
        assert!(flatten_values(&raw).is_empty());
    }
}
