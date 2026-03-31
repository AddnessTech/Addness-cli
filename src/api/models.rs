use serde::{Deserialize, Serialize};

/// Goal status values used by the API.
/// API uses "NONE" for not-started, "IN_PROGRESS", "CANCELLED".
/// "COMPLETED" is represented by `is_completed = true` rather than this enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoalStatus {
    None,
    InProgress,
    Cancelled,
}

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}

// GET /api/v2/organizations/:id/objectives/tree
// Response: { "data": { "items": [...] } }
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeData {
    pub items: Vec<TreeItem>,
    pub pagination: Option<TreePage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeItem {
    pub id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub title: String,
    #[serde(default)]
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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub id: String,
    pub name: String,
}

// PATCH /api/v2/organizations/:org_id/objectives/:id
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateGoalRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<GoalStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_completed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Goal {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<GoalStatus>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub completed_at: Option<String>,
}

// GET /api/v1/team/organizations/my_organizations
// Response: { "data": [ { "id": "...", "name": "...", ... } ] }
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub plan_type: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationsResponse {
    pub data: Vec<Organization>,
}
