use serde::{Deserialize, Serialize};

/// User ID - globally unique across the service
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(String);

impl UserId {
    #[allow(dead_code)]
    fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    #[allow(dead_code)]
    fn as_str(&self) -> &str {
        &self.0
    }
}

/// Member ID - unique within an organization
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MemberId(String);

impl MemberId {
    #[allow(dead_code)]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// A member tag defined within an organization.
/// GET /api/v2/organizations/:id/member-tags, GET /api/v2/members/:id/tags
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemberTag {
    pub id: String,
    pub organization_id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}

// POST /api/v2/organizations/:id/member-tags
#[derive(Debug, Serialize)]
pub struct CreateMemberTagRequest {
    pub name: String,
}

// POST /api/v2/organizations/:id/members/:memberId/tags
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignMemberTagRequest {
    pub tag_id: String,
}
