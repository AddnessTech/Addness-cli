use anyhow::Result;
use colored::Colorize;

use crate::api::{ApiClient, SearchQueryParams};
use crate::cli::commands::org::resolve_org_id;

fn summarize_item(data: &serde_json::Value) -> String {
    for key in ["title", "content", "displayName", "name"] {
        if let Some(text) = data.get(key).and_then(|v| v.as_str()) {
            let flattened = text.replace('\n', " ");
            let truncated: String = flattened.chars().take(80).collect();
            return truncated;
        }
    }
    "-".to_string()
}

/// `addness search <query>` — unified search across objectives/comments/members
/// (GET /api/v1/team/search). Not to be confused with `addness goal search`,
/// which only searches objective titles via the v1 objectives endpoint.
pub async fn handle_search(
    query: &str,
    org: Option<&str>,
    limit: Option<u16>,
    offset: Option<u16>,
    json: bool,
    client: &ApiClient,
) -> Result<()> {
    let org_id = resolve_org_id(org)?;
    let result = client
        .unified_search(SearchQueryParams {
            query,
            organization_id: &org_id,
            limit,
            offset,
        })
        .await?;

    if json {
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    if result.items.is_empty() {
        println!("{}", "No results found.".dimmed());
        return Ok(());
    }

    println!(
        "{:<10} {:<38} {}",
        "TYPE".bold(),
        "ID".bold(),
        "SUMMARY".bold()
    );
    println!("{}", "─".repeat(100));
    for item in &result.items {
        let id = item.data.get("id").and_then(|v| v.as_str()).unwrap_or("-");
        println!(
            "{:<10} {:<38} {}",
            item.kind,
            id.dimmed(),
            summarize_item(&item.data)
        );
    }
    if result.has_more {
        println!();
        println!(
            "{}",
            "More results available. Use --offset to page further.".dimmed()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::summarize_item;
    use serde_json::json;

    #[test]
    fn summarize_item_prefers_title() {
        let data = json!({"title": "Launch plan", "content": "ignored"});
        assert_eq!(summarize_item(&data), "Launch plan");
    }

    #[test]
    fn summarize_item_falls_back_to_content() {
        let data = json!({"content": "line one\nline two"});
        assert_eq!(summarize_item(&data), "line one line two");
    }

    #[test]
    fn summarize_item_returns_dash_when_no_known_field() {
        let data = json!({"other": "value"});
        assert_eq!(summarize_item(&data), "-");
    }
}
