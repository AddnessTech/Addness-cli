use colored::{ColoredString, Colorize};
use unicode_width::UnicodeWidthStr;

use crate::api::{ChildItem, Comment, Goal, GoalStatus, Organization, SearchItem, TreeItem};

/// Pad `s` with spaces so its display width reaches `target_width`.
fn pad_to_width(s: &str, target_width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= target_width {
        s.to_string()
    } else {
        format!("{s}{}", " ".repeat(target_width - w))
    }
}

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

struct TreeRow {
    id: String,
    title_col: String,
    status_label: &'static str,
    colored_status: ColoredString,
    owner: String,
}

pub fn print_goals_table(items: &[TreeItem]) {
    if items.is_empty() {
        println!("{}", "No goals found.".dimmed());
        return;
    }

    use std::collections::HashMap;

    let mut children_map: HashMap<Option<&str>, Vec<&TreeItem>> = HashMap::new();
    for item in items {
        children_map
            .entry(item.parent_id.as_deref())
            .or_default()
            .push(item);
    }

    let id_set: std::collections::HashSet<&str> = items.iter().map(|i| i.id.as_str()).collect();

    let mut roots: Vec<&TreeItem> = items
        .iter()
        .filter(|i| match &i.parent_id {
            None => true,
            Some(pid) => !id_set.contains(pid.as_str()),
        })
        .collect();
    roots.sort_by(|a, b| a.order_no.partial_cmp(&b.order_no).unwrap_or(std::cmp::Ordering::Equal));

    // Pass 1: collect rows in DFS order
    let mut rows: Vec<TreeRow> = Vec::new();

    fn collect_rows<'a>(
        item: &'a TreeItem,
        prefix: &str,
        is_last: bool,
        is_root: bool,
        children_map: &HashMap<Option<&str>, Vec<&'a TreeItem>>,
        rows: &mut Vec<TreeRow>,
    ) {
        let (status_label, colored_status) =
            resolve_status(item.is_completed, item.status.as_ref());
        let owner = item
            .owner
            .as_ref()
            .map(|o| o.name.as_str())
            .unwrap_or("-");

        let connector = if is_root {
            ""
        } else if is_last {
            "└─ "
        } else {
            "├─ "
        };

        rows.push(TreeRow {
            id: item.id.clone(),
            title_col: format!("{prefix}{connector}{}", item.title),
            status_label,
            colored_status,
            owner: owner.to_string(),
        });

        let child_prefix = if is_root {
            prefix.to_string()
        } else if is_last {
            format!("{prefix}   ")
        } else {
            format!("{prefix}│  ")
        };

        if let Some(children) = children_map.get(&Some(item.id.as_str())) {
            let mut sorted = children.clone();
            sorted.sort_by(|a, b| {
                a.order_no
                    .partial_cmp(&b.order_no)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let len = sorted.len();
            for (i, child) in sorted.iter().enumerate() {
                collect_rows(child, &child_prefix, i == len - 1, false, children_map, rows);
            }
        }
    }

    let roots_len = roots.len();
    for (i, root) in roots.iter().enumerate() {
        collect_rows(root, "", i == roots_len - 1, true, &children_map, &mut rows);
    }

    // Pass 2: compute max title display width, then print
    let title_width = rows
        .iter()
        .map(|r| UnicodeWidthStr::width(r.title_col.as_str()))
        .max()
        .unwrap_or(5)
        .max(5); // at least "TITLE" width

    let id_width = 38;
    let status_width = 12;
    let total = id_width + 1 + title_width + 1 + status_width + 1 + 10;

    println!(
        "{} {} {} {}",
        pad_to_width("ID", id_width).bold(),
        pad_to_width("TITLE", title_width).bold(),
        pad_to_width("STATUS", status_width).bold(),
        "OWNER".bold()
    );
    println!("{}", "─".repeat(total));

    for row in &rows {
        let status_pad = " ".repeat(status_width.saturating_sub(row.status_label.len()));

        println!(
            "{} {} {}{status_pad} {}",
            pad_to_width(&row.id, id_width).dimmed(),
            pad_to_width(&row.title_col, title_width),
            row.colored_status,
            row.owner.dimmed()
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
