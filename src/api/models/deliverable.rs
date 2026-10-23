use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentUploadRequest {
    pub file_name: String,
    pub content_type: String,
    pub file_size: i64,
}

/// 成果物作成のレスポンス本体（list の Deliverable と異なり has_children/children_count は無い）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliverableCreateData {
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
    pub upload_request: Option<AttachmentUploadResponse>,
}

/// S3 presigned POST のフォーム情報。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentUploadResponse {
    pub url: String,
    pub values: std::collections::HashMap<String, String>,
}

// Existing CLI/TUI output shape, adapted from the current Drive API.
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
