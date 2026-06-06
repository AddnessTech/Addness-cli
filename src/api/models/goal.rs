use serde::{Deserialize, Deserializer, Serialize};

use crate::api::{MemberId, UserId};

/// Goal status values used by the backend API.
/// Backend uses: "NONE", "IN_PROGRESS", "CANCELLED".
/// Completion is tracked via `completedAt`, not status.
///
/// Only these transitions are allowed.
/// None => InProgress or Cancelled,
/// InProgress => None or Cancelled,
/// Cancelled => None or InProgress,
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
    pub id: UserId,
    pub organization_member_id: MemberId,
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
