use anyhow::Result;
use colored::Colorize;

use crate::api::{ApiClient, ApiResponse, GoalStatus, GoalTreeData, GoalTreeItem};
use crate::cli::commands::org::resolve_org_id;

#[derive(Debug, serde::Serialize)]
struct SummaryData {
    total: usize,
    completed: usize,
    in_progress: usize,
    not_started: usize,
    cancelled: usize,
    completed_goals: Vec<GoalSummaryItem>,
    in_progress_goals: Vec<GoalSummaryItem>,
    stalled_goals: Vec<GoalSummaryItem>,
    not_started_goals: Vec<GoalSummaryItem>,
}

#[derive(Debug, serde::Serialize)]
struct GoalSummaryItem {
    id: String,
    title: String,
    owner: Option<String>,
}

fn classify(item: &GoalTreeItem) -> &'static str {
    if item.is_completed {
        return "completed";
    }
    match &item.status {
        Some(GoalStatus::InProgress) => "in_progress",
        Some(GoalStatus::Cancelled) => "cancelled",
        _ => "not_started",
    }
}

fn to_summary_item(item: &GoalTreeItem) -> GoalSummaryItem {
    GoalSummaryItem {
        id: item.id.clone(),
        title: item.title.clone(),
        owner: item.owner.as_ref().map(|o| o.name.clone()),
    }
}

pub async fn handle_summary(
    org: Option<&str>,
    depth: usize,
    json: bool,
    client: &ApiClient,
) -> Result<()> {
    let org_id = resolve_org_id(org)?;
    let resp: ApiResponse<GoalTreeData> = client.get_goal_tree_with_completed(&org_id, depth).await?;
    let items = &resp.data.items;

    let mut completed_goals = Vec::new();
    let mut in_progress_goals = Vec::new();
    let mut not_started_goals = Vec::new();
    let mut stalled_goals = Vec::new();
    let mut cancelled = 0usize;

    for item in items {
        match classify(item) {
            "completed" => completed_goals.push(to_summary_item(item)),
            "in_progress" => in_progress_goals.push(to_summary_item(item)),
            "not_started" => {
                // 停滞 = not_started かつ子ゴールがあるのに進んでないもの
                if item.has_children {
                    stalled_goals.push(to_summary_item(item));
                } else {
                    not_started_goals.push(to_summary_item(item));
                }
            }
            "cancelled" => cancelled += 1,
            _ => {}
        }
    }

    let summary = SummaryData {
        total: items.len(),
        completed: completed_goals.len(),
        in_progress: in_progress_goals.len(),
        not_started: not_started_goals.len(),
        cancelled,
        completed_goals,
        in_progress_goals,
        stalled_goals,
        not_started_goals,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        print_summary(&summary);
    }

    Ok(())
}

fn print_summary(s: &SummaryData) {
    println!("{}", "Addness Summary".bold());
    println!();

    // 概要
    println!(
        "  {} goals total:  {} completed  {} in progress  {} not started",
        s.total.to_string().bold(),
        s.completed.to_string().green().bold(),
        s.in_progress.to_string().cyan().bold(),
        s.not_started.to_string().dimmed(),
    );
    if s.cancelled > 0 {
        println!("  {} cancelled", s.cancelled.to_string().dimmed());
    }
    println!();

    // 完了
    if !s.completed_goals.is_empty() {
        println!("  {} {}", "✅".green(), "Completed".green().bold());
        for g in &s.completed_goals {
            let owner = g
                .owner
                .as_deref()
                .map(|o| format!(" ({})", o).dimmed().to_string())
                .unwrap_or_default();
            println!("     {} {}{}", "·".dimmed(), g.title, owner);
        }
        println!();
    }

    // 進行中
    if !s.in_progress_goals.is_empty() {
        println!("  {} {}", "🔄".cyan(), "In Progress".cyan().bold());
        for g in &s.in_progress_goals {
            let owner = g
                .owner
                .as_deref()
                .map(|o| format!(" ({})", o).dimmed().to_string())
                .unwrap_or_default();
            println!("     {} {}{}", "·".dimmed(), g.title, owner);
        }
        println!();
    }

    // 停滞
    if !s.stalled_goals.is_empty() {
        println!(
            "  {} {}",
            "⏸".yellow(),
            "Stalled (has children, not started)".yellow().bold()
        );
        for g in &s.stalled_goals {
            let owner = g
                .owner
                .as_deref()
                .map(|o| format!(" ({})", o).dimmed().to_string())
                .unwrap_or_default();
            println!("     {} {}{}", "·".dimmed(), g.title, owner);
        }
        println!();
    }
}
