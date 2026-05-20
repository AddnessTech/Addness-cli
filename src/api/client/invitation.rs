use anyhow::Result;

use crate::api::{
    AcceptInvitationRequest, ApiClient, CreateInvitationsRequest, CreateInviteLinkRequest,
};

impl ApiClient {
    pub async fn create_invitations(
        &self,
        org_id: &str,
        emails: Vec<String>,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/organizations/{org_id}/invitations");
        let body = CreateInvitationsRequest { emails };
        self.post(&path, &body).await
    }

    pub async fn resend_invitation(
        &self,
        org_id: &str,
        invitation_id: &str,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/organizations/{org_id}/invitations/{invitation_id}/resend");
        let body = serde_json::json!({});
        self.post(&path, &body).await
    }

    pub async fn revoke_invitation(&self, org_id: &str, invitation_id: &str) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/invitations/{invitation_id}");
        self.delete_no_body(&path).await
    }

    pub async fn accept_invitation(
        &self,
        invited_member_id: &str,
        token: &str,
    ) -> Result<serde_json::Value> {
        let body = AcceptInvitationRequest {
            invited_member_id: invited_member_id.to_string(),
            token: token.to_string(),
        };
        self.post("/api/v2/invitations/accept", &body).await
    }

    pub async fn create_invite_link(
        &self,
        org_id: &str,
        code: &str,
        max_uses: Option<i32>,
        expires_at: Option<String>,
        is_external: bool,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/organizations/{org_id}/invite-links");
        let body = CreateInviteLinkRequest {
            code: code.to_string(),
            max_uses,
            expires_at,
            is_external,
        };
        self.post(&path, &body).await
    }

    pub async fn deactivate_invite_link(&self, org_id: &str, link_id: &str) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/invite-links/{link_id}");
        self.delete_no_body(&path).await
    }
}
