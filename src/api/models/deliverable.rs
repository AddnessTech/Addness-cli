use serde::{Deserialize, Serialize};

// POST /api/v1/team/objectives/:id/deliverables
//
// node_type ごとに必要なフィールドが異なる：
// - link:     link_url 必須
// - document: content（任意。空でもOK）
// - file:     file 必須（実ファイルは別途 uploadRequest 経由でS3へPOSTする）
// - folder:   display_name 必須
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateDeliverableRequest {
    pub node_type: DeliverableType,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<AttachmentUploadRequest>,
}

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
#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentUploadResponse {
    pub url: String,
    pub values: std::collections::HashMap<String, String>,
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

// PATCH /:deliverableId
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateDeliverableRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<String>,
}

// PATCH /:deliverableId/rename
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenameDeliverableRequest {
    pub display_name: String,
}

// PATCH /:deliverableId/move
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MoveDeliverableRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_parent_deliverable_id: Option<String>,
    pub order_no: f64,
}

// POST /batch_move
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchMoveDeliverableRequest {
    pub node_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_parent_deliverable_id: Option<String>,
}

// POST /batch_delete
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchDeleteDeliverableRequest {
    pub node_ids: Vec<String>,
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
