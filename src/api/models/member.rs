use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    pub name: String,
    pub is_current_user: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MembersListData {
    pub members: Vec<Member>,
}

// PUT /api/v2/members/:id (no content response)
#[derive(Debug, Serialize)]
pub struct UpdateMemberRequest {
    pub name: String,
}

// PUT /api/v2/members/:id/pin
#[derive(Debug, Serialize)]
pub struct PinMemberRequest {
    pub pinned: bool,
}

// PATCH /api/v2/members/:id/source-organization
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetSourceOrganizationRequest {
    pub source_organization_id: String,
}
