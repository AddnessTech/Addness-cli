use serde::{Deserialize, Deserializer, Serialize};

/// Goal status values used by the backend API.
/// Backend uses: "NONE", "IN_PROGRESS", "CANCELLED".
/// Completion is tracked via `completedAt`, not status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    #[serde(rename = "NONE")]
    None,
    #[serde(rename = "IN_PROGRESS")]
    InProgress,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    /// Catch-all for any unknown status value from the backend
    #[serde(untagged)]
    Other(String),
}

/// Deserialize Option<GoalStatus> that tolerates empty strings.
/// Backend may return "" for status (Go string zero value).
pub fn deserialize_optional_status<'de, D>(deserializer: D) -> Result<Option<GoalStatus>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    match opt {
        Some(s) if s.is_empty() => Ok(Some(GoalStatus::None)),
        Some(s) => {
            let status = match s.as_str() {
                "NONE" => GoalStatus::None,
                "IN_PROGRESS" => GoalStatus::InProgress,
                "CANCELLED" => GoalStatus::Cancelled,
                other => GoalStatus::Other(other.to_string()),
            };
            Ok(Some(status))
        }
        None => Ok(None),
    }
}

// GET /api/v2/organizations/:id/objectives/tree
// Response: { "data": { "items": [...] } }
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalTreeData {
    pub items: Vec<GoalTreeItem>,
    pub pagination: Option<TreePage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalTreeItem {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default, deserialize_with = "deserialize_optional_status")]
    pub status: Option<GoalStatus>,
    pub order_no: f64,
    pub is_completed: bool,
    pub has_children: bool,
    pub owner: Option<Owner>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreePage {
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub id: String,
    pub name: String,
}

// POST /api/v2/objectives
//
// CLI の `description` 引数は Backend の `definitionOfDone`（完了の基準）にマップされる。
// Backend には別途 `description`（旧本文）と `body`（V2 Notion 風）カラムがあるが、
// Frontend の「完了の基準」UI が読むのは `definitionOfDone` カラムのみ。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGoalRequest {
    pub organization_id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_objective_id: Option<String>,
    #[serde(rename = "definitionOfDone", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// PATCH /api/v2/organizations/:org_id/objectives/:id
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGoalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<GoalStatus>,
    /// Set to Some(timestamp) to mark completed, Some(None) to uncomplete
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(rename = "definitionOfDone", skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(rename = "dueDate", skip_serializing_if = "Option::is_none")]
    pub due_date: Option<Option<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub title: String,
    /// Backend の `definitionOfDone`（完了の基準）にマップ。
    #[serde(rename = "definitionOfDone", default)]
    pub description: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_status")]
    pub status: Option<GoalStatus>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub completed_at: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub due_date: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
    #[serde(default)]
    pub owner: Option<Owner>,
}

// GET /api/v2/objectives/:id/children
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalChildrenData {
    pub children: Vec<GoalChildItem>,
    #[serde(default)]
    pub pagination: Option<TreePage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalChildItem {
    pub id: String,
    pub title: String,
    /// Backend の `definitionOfDone`（完了の基準）にマップ。
    #[serde(rename = "definitionOfDone", default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_status")]
    pub status: Option<GoalStatus>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub has_children: bool,
    pub order_no: f64,
    #[serde(default)]
    pub owner: Option<Owner>,
}

// POST /api/v2/objectives/{archive,unarchive,restore}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ObjectiveIdsRequest {
    pub objective_ids: Vec<String>,
}

// POST /api/v2/objectives/:id/duplicate
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateRequest {
    pub parent_id: String,
}

// POST /api/v2/objectives/:id/parent
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeParentRequest {
    pub new_parent_id: Option<String>,
}

// POST /api/v1/team/objectives/:id/aliases
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAliasRequest {
    pub target_objective_id: String,
    pub order_no: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Alias {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub parent_objective_id: Option<String>,
    #[serde(default)]
    pub target_objective_id: Option<String>,
    #[serde(default)]
    pub order_no: Option<i32>,
}

// PATCH /api/v1/team/objectives/:id/aliases/reorder
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderAliasesRequest {
    pub alias_ids: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReorderAliasesResponse {
    #[serde(default)]
    pub aliases: Vec<Alias>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLinkResponse {
    #[serde(default)]
    pub share_url: Option<String>,
    #[serde(default)]
    pub public_id: Option<String>,
    #[serde(default)]
    pub token: Option<String>,
}

// GET /api/v1/team/:org-id/objectives/search
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSearchResponse {
    pub items: Vec<GoalSearchItem>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalSearchItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub owner: Option<Owner>,
}

// GET/POST/PUT /api/v2/objectives/:id/recurring
// レスポンスは { "data": ... } でラップされる（他のv2エンドポイントと同様）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringGoal {
    pub id: String,
    pub objective_id: String,
    /// 基本パターン（DAILY/WEEKLY/MONTHLY/WEEKDAYS）。カスタムパターン使用時はNone。
    #[serde(default)]
    pub pattern: Option<String>,
    pub description: String,
    pub is_basic_pattern: bool,
    /// 基本パターンがWEEKLYの場合のみ設定（updatedAtから導出、Go time.Weekday: Sunday=0）
    #[serde(default)]
    pub day_of_week: Option<i32>,
    /// 基本パターンがMONTHLYの場合のみ設定（updatedAtから導出）
    #[serde(default)]
    pub day_of_month: Option<i32>,
    /// カスタムパターンの種別（DAILY/WEEKLY/MONTHLY/YEARLY）。基本パターン使用時はNone。
    #[serde(default)]
    pub recurrence_type: Option<String>,
    #[serde(default)]
    pub interval: Option<i32>,
    #[serde(default)]
    pub days_of_week: Vec<String>,
    #[serde(default)]
    pub days_of_month: Vec<i32>,
    #[serde(default)]
    pub end_date: Option<String>,
    #[serde(default)]
    pub is_last_day: Option<bool>,
    #[serde(default)]
    pub nth_week: Option<i32>,
    #[serde(default)]
    pub repeat_from_completion: Option<bool>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

// POST/PUT /api/v2/objectives/:id/recurring
// Create/Updateで同一のリクエスト形。基本パターン(pattern)とカスタムパターン(recurrenceType以下)は排他。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecurringGoalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recurrence_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interval: Option<i32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub days_of_week: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub days_of_month: Vec<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_last_day: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nth_week: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat_from_completion: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recurring_goal_request_omits_unset_fields() {
        let req = RecurringGoalRequest {
            pattern: Some("WEEKLY".to_string()),
            ..Default::default()
        };

        let value = serde_json::to_value(req).unwrap();

        assert_eq!(value["pattern"], "WEEKLY");
        assert!(value.get("recurrenceType").is_none());
        assert!(value.get("daysOfWeek").is_none());
        assert!(value.get("isLastDay").is_none());
    }

    #[test]
    fn recurring_goal_request_serializes_custom_pattern_fields() {
        let req = RecurringGoalRequest {
            recurrence_type: Some("WEEKLY".to_string()),
            interval: Some(2),
            days_of_week: vec!["MONDAY".to_string(), "THURSDAY".to_string()],
            ..Default::default()
        };

        let value = serde_json::to_value(req).unwrap();

        assert_eq!(value["recurrenceType"], "WEEKLY");
        assert_eq!(value["interval"], 2);
        assert_eq!(value["daysOfWeek"][0], "MONDAY");
        assert!(value.get("pattern").is_none());
    }

    #[test]
    fn recurring_goal_deserializes_basic_pattern_response() {
        let json = serde_json::json!({
            "id": "rg-1",
            "objectiveId": "goal-1",
            "pattern": "WEEKLY",
            "description": "毎週月曜",
            "isBasicPattern": true,
            "dayOfWeek": 1,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z"
        });

        let goal: RecurringGoal = serde_json::from_value(json).unwrap();

        assert_eq!(goal.pattern.as_deref(), Some("WEEKLY"));
        assert!(goal.is_basic_pattern);
        assert_eq!(goal.day_of_week, Some(1));
        assert!(goal.recurrence_type.is_none());
    }

    #[test]
    fn update_goal_request_serializes_writable_goal_fields() {
        let req = UpdateGoalRequest {
            status: None,
            completed_at: None,
            title: None,
            description: Some("完了基準".to_string()),
            body: Some("現在の状態".to_string()),
            due_date: Some(Some("2026-07-01".to_string())),
        };

        let value = serde_json::to_value(req).unwrap();

        assert_eq!(value["definitionOfDone"], "完了基準");
        assert_eq!(value["body"], "現在の状態");
        assert_eq!(value["dueDate"], "2026-07-01");
        assert!(value.get("description").is_none());
    }

    #[test]
    fn update_goal_request_can_clear_due_date() {
        let req = UpdateGoalRequest {
            status: None,
            completed_at: None,
            title: None,
            description: None,
            body: None,
            due_date: Some(None),
        };

        let value = serde_json::to_value(req).unwrap();

        assert!(value.get("dueDate").is_some());
        assert!(value["dueDate"].is_null());
    }
}
