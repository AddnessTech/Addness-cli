use anyhow::{Result, bail};
use std::process::Command;

/// Detect goal ID from current git branch name.
/// Supports patterns:
///   goal/<UUID>/description
///   goal/<UUID>
///   feature/goal-<UUID>/description
pub fn detect_goal_id() -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output();

    let branch = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return Ok(None),
    };

    // Before any commits exist, git rev-parse --abbrev-ref HEAD returns "HEAD"
    if branch == "HEAD" {
        return Ok(None);
    }

    Ok(extract_goal_id(&branch))
}

fn extract_goal_id(branch: &str) -> Option<String> {
    // UUID pattern: 8-4-4-4-12 hex chars
    let uuid_pattern =
        regex::Regex::new(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}").ok()?;

    // Look for goal/<UUID> pattern first
    for segment in branch.split('/') {
        if let Some(m) = uuid_pattern.find(segment) {
            return Some(m.as_str().to_string());
        }
    }

    None
}

pub fn handle_detect_goal(json: bool) -> Result<()> {
    match detect_goal_id()? {
        Some(id) => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "goal_id": id,
                        "detected": true
                    }))?
                );
            } else {
                println!("{id}");
            }
        }
        None => {
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "goal_id": null,
                        "detected": false
                    }))?
                );
            } else {
                bail!(
                    "No goal ID found in current branch name.\nUse branch naming: goal/<GOAL_ID>/description"
                );
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_goal_uuid_branch() {
        assert_eq!(
            extract_goal_id("goal/19453a2d-6524-4bbb-8e4f-f8fd69f3fce4/add-email-notifications"),
            Some("19453a2d-6524-4bbb-8e4f-f8fd69f3fce4".to_string())
        );
    }

    #[test]
    fn test_goal_uuid_only() {
        assert_eq!(
            extract_goal_id("goal/19453a2d-6524-4bbb-8e4f-f8fd69f3fce4"),
            Some("19453a2d-6524-4bbb-8e4f-f8fd69f3fce4".to_string())
        );
    }

    #[test]
    fn test_feature_goal_branch() {
        assert_eq!(
            extract_goal_id("feature/goal-19453a2d-6524-4bbb-8e4f-f8fd69f3fce4/fix-ui"),
            Some("19453a2d-6524-4bbb-8e4f-f8fd69f3fce4".to_string())
        );
    }

    #[test]
    fn test_no_goal() {
        assert_eq!(extract_goal_id("feature/add-login"), None);
    }

    #[test]
    fn test_main_branch() {
        assert_eq!(extract_goal_id("main"), None);
    }

    #[test]
    fn test_head_literal() {
        // git rev-parse --abbrev-ref HEAD returns "HEAD" when no commits exist
        assert_eq!(extract_goal_id("HEAD"), None);
    }
}
