use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Invitation {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub invited_member_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum InvitationsData {
    Items(Vec<Invitation>),
    Object {
        #[serde(default)]
        invitations: Vec<Invitation>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InviteLink {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub max_uses: Option<i32>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub is_external: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInvitationResponse {
    #[serde(default)]
    pub organization_id: Option<String>,
    #[serde(default)]
    pub organization_member_id: Option<String>,
}

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

// POST /api/v1/team/organization_invitations/accept
#[derive(Debug, Serialize)]
pub struct LegacyAcceptInvitationRequest {
    pub token: String,
}

// POST /api/v1/team/organization_invitations/check_plan_upgrade
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckPlanUpgradeRequest {
    pub organization_id: String,
    pub additional_members_count: i64,
}

// POST /api/v2/invitations/:token/accept
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptInvitationByTokenRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_organization_id: Option<String>,
}

// POST /api/v2/invitations/decline
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclineInvitationRequest {
    pub invited_member_id: String,
    pub token: String,
}
