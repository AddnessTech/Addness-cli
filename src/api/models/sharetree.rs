use serde::{Deserialize, Serialize};

// Shared goal tree API models (internal/sharedgoaltree — portable, cloneable
// goal-tree exports; distinct from the single-objective public share link
// implemented by `goal share`).
// Backend reference: internal/sharedgoaltree/handler/endpoints/*.go.

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareTreeCreateRequest {
    pub source_objective_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selected_objective_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareTreeCreateData {
    pub id: String,
    pub public_id: String,
    pub created_at: String,
    #[serde(default)]
    pub skipped_attachments_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareTreeSummary {
    pub id: String,
    pub public_id: String,
    pub source_objective_id: String,
    pub root_title: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareTreeListMineData {
    #[serde(default)]
    pub items: Vec<ShareTreeSummary>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareTreeCloneRequest {
    pub public_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_objective_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareTreeCloneData {
    pub root_objective_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_objective_id: Option<String>,
    pub node_count: i64,
}

/// A single node of a public shared goal tree (GET /api/v1/public/goal-trees/:publicId).
/// No auth required; nested comments/deliverables/kpis are read-only snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedGoalTreePublic {
    pub public_id: String,
    #[serde(default)]
    pub creator_display_name: String,
    pub created_at: String,
    #[serde(default)]
    pub nodes: Vec<serde_json::Value>,
}
