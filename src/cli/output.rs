use colored::Colorize;

use crate::api::{Organization, TreeItem};

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
        let status = if item.is_completed {
            "COMPLETED".green().to_string()
        } else {
            match item.status.as_deref() {
                Some("IN_PROGRESS") => "IN_PROGRESS".cyan().to_string(),
                Some("CANCELLED") => "CANCELLED".red().to_string(),
                _ => "NOT_STARTED".yellow().to_string(),
            }
        };

        let indent = if item.parent_id.is_some() { "  " } else { "" };
        let children_mark = if item.has_children { " +" } else { "" };

        let owner = item
            .owner
            .as_ref()
            .map(|o| o.name.as_str())
            .unwrap_or("-");

        println!(
            "{:<38} {}{:<38}{} {:<10} {}",
            item.id.dimmed(),
            indent,
            item.title,
            children_mark.dimmed(),
            status,
            owner.dimmed()
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
