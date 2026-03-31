use colored::{ColoredString, Colorize};

use crate::api::{ChildItem, Comment, Goal, GoalStatus, Organization, SearchItem, TreeItem};

/// Resolve display status from is_completed + status fields.
/// Returns (label, colored_label).
pub fn resolve_status(
    is_completed: bool,
    status: Option<&GoalStatus>,
) -> (&'static str, ColoredString) {
    if is_completed {
        ("COMPLETED", "COMPLETED".green())
    } else {
        match status {
            Some(GoalStatus::InProgress) => ("IN_PROGRESS", "IN_PROGRESS".cyan()),
            Some(GoalStatus::Cancelled) => ("CANCELLED", "CANCELLED".red()),
            _ => ("NOT_STARTED", "NOT_STARTED".yellow()),
        }
    }
}

pub fn print_goals_table(items: &[TreeItem]) {
    if items.is_empty() {
        println!("{}", "No goals found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:<40} {:<10} {}",
        "ID".bold(),
        "TITLE".bold(),
        "STATUS".bold(),
        "OWNER".bold()
    );
    println!("{}", "─".repeat(100));

    for item in items {
        let (_, colored_status) = resolve_status(item.is_completed, item.status.as_ref());

        let indent = if item.parent_id.is_some() { "  " } else { "" };
        let children_mark = if item.has_children { " +" } else { "" };

        let owner = item.owner.as_ref().map(|o| o.name.as_str()).unwrap_or("-");

        println!(
            "{:<38} {}{:<38}{} {:<10} {}",
            item.id.dimmed(),
            indent,
            item.title,
            children_mark.dimmed(),
            colored_status,
            owner.dimmed()
        );
    }
}

pub fn print_goal_detail(goal: &Goal) {
    let (_, colored_status) = resolve_status(goal.is_completed, goal.status.as_ref());

    println!("{}: {}", "Title".bold(), goal.title);
    println!("{}: {}", "ID".bold(), goal.id.dimmed());
    println!("{}: {colored_status}", "Status".bold());

    if let Some(parent_id) = &goal.parent_id {
        println!("{}: {}", "Parent".bold(), parent_id.dimmed());
    }
    if let Some(owner) = &goal.owner {
        println!("{}: {}", "Owner".bold(), owner.name);
    }
    if let Some(due) = &goal.due_date {
        println!("{}: {}", "Due".bold(), &due[..10.min(due.len())]);
    }
    if let Some(desc) = &goal.description
        && !desc.is_empty()
    {
        println!("{}: {desc}", "Description".bold());
    }
    if let Some(body) = &goal.body
        && !body.is_empty()
    {
        println!("\n{}", "Body".bold());
        println!("{body}");
    }
}

pub fn print_children_table(children: &[ChildItem]) {
    if children.is_empty() {
        println!("{}", "No children found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:<40} {:<12} {}",
        "ID".bold(),
        "TITLE".bold(),
        "STATUS".bold(),
        "OWNER".bold()
    );
    println!("{}", "─".repeat(100));

    for child in children {
        let (_, colored_status) = resolve_status(child.is_completed, child.status.as_ref());
        let children_mark = if child.has_children { " +" } else { "" };
        let owner = child.owner.as_ref().map(|o| o.name.as_str()).unwrap_or("-");

        println!(
            "{:<38} {:<38}{} {:<12} {}",
            child.id.dimmed(),
            child.title,
            children_mark.dimmed(),
            colored_status,
            owner.dimmed()
        );
    }
}

pub fn print_search_results(items: &[SearchItem]) {
    if items.is_empty() {
        println!("{}", "No results found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:<40} {}",
        "ID".bold(),
        "TITLE".bold(),
        "OWNER".bold()
    );
    println!("{}", "─".repeat(90));

    for item in items {
        let owner = item.owner.as_ref().map(|o| o.name.as_str()).unwrap_or("-");
        println!(
            "{:<38} {:<40} {}",
            item.id.dimmed(),
            item.title,
            owner.dimmed()
        );
    }
}

pub fn print_comments_table(comments: &[Comment]) {
    if comments.is_empty() {
        println!("{}", "No comments found.".dimmed());
        return;
    }

    println!(
        "{:<38} {:<20} {:<20} {}",
        "ID".bold(),
        "AUTHOR".bold(),
        "DATE".bold(),
        "CONTENT".bold()
    );
    println!("{}", "─".repeat(120));

    for comment in comments {
        let content = comment.content.replace('\n', " ");
        let truncated = if content.chars().count() > 60 {
            let end = content
                .char_indices()
                .nth(57)
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
            "{:<38} {:<20} {:<20} {}",
            comment.id.dimmed(),
            author_name,
            date.dimmed(),
            truncated
        );
    }
}

pub fn print_organizations_table(orgs: &[Organization], current_org_id: Option<&str>) {
    if orgs.is_empty() {
        println!("{}", "No organizations found.".dimmed());
        return;
    }

    println!("{:<38} {}", "ID".bold(), "NAME".bold());
    println!("{}", "─".repeat(60));

    for org in orgs {
        let marker = if current_org_id == Some(org.id.as_str()) {
            " *".green().to_string()
        } else {
            String::new()
        };
        println!("{:<38} {}{}", org.id.dimmed(), org.name, marker);
    }
}
