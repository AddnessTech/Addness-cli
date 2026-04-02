use serde::{Deserialize, Serialize};

/// Goal status values used by the API.
/// "COMPLETED" is represented by `is_completed = true` rather than this enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GoalStatus {
    None,
    Active,
    InProgress,
    Completed,
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
    pub body: Option<String>,
    #[serde(default)]
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
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: Option<GoalStatus>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub has_children: bool,
    pub order_no: f64,
    #[serde(default)]
    pub owner: Option<Owner>,
}

// GET /api/v1/team/objectives/:id/deliverables
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverableListData {
    pub deliverables: Vec<Deliverable>,
    pub total: i64,
}

/// Deliverable node type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliverableType {
    Folder,
    Document,
    File,
    Link,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Deliverable {
    pub id: String,
    pub display_name: String,
    pub node_type: DeliverableType,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub link_url: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub objective_id: String,
    #[serde(default)]
    pub parent_deliverable_id: Option<String>,
    pub order_no: f64,
    pub depth: i32,
    pub is_root: bool,
    pub has_children: bool,
    #[serde(default)]
    pub children_count: i64,
}

// GET /api/v1/team/:org-id/objectives/search
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    pub items: Vec<SearchItem>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub owner: Option<Owner>,
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

// GET /v1/team/comments
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentsResponse {
    pub comments: Vec<Comment>,
    pub total_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comment {
    pub id: String,
    pub content: String,
    pub commentable_type: String,
    pub commentable_id: String,
    #[serde(default)]
    pub parent_id: Option<String>,
    pub author: CommentAuthor,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub resolved_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAuthor {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_ai_agent: bool,
}

// POST /v1/team/comments
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateCommentRequest {
    pub commentable_type: String,
    pub commentable_id: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
}
