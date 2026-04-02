use serde::{Deserialize, Serialize};

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
