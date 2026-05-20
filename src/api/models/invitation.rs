use serde::Serialize;

// POST /api/v2/organizations/:id/invitations
#[derive(Debug, Serialize)]
pub struct CreateInvitationsRequest {
    pub emails: Vec<String>,
}

// POST /api/v2/organizations/:id/invite-links
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInviteLinkRequest {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_uses: Option<i32>,
    /// RFC3339 timestamp; omit for no expiry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_external: bool,
}

// POST /api/v2/invitations/accept
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInvitationRequest {
    pub invited_member_id: String,
    pub token: String,
}
