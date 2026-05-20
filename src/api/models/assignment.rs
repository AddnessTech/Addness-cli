use serde::Serialize;

// POST /api/v2/objectives/:id/assignments
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAssignmentRequest {
    pub organization_member_id: String,
    /// OWNER, EDITOR, MEMBER. Omit for backend default (MEMBER).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

// PATCH /api/v2/objectives/:id/assignments/:assignmentId
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAssignmentRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

// PUT /api/v2/objectives/:id/transfer-ownership
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferOwnershipRequest {
    pub new_owner_member_id: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub actor_as_editor: bool,
}
