use anyhow::Result;

use crate::api::{
    AcceptInvitationRequest, AcceptInvitationResponse, ApiClient, ApiResponse,
    CreateInvitationsRequest, CreateInviteLinkRequest, Invitation, InvitationsData, InviteLink,
};

impl ApiClient {
    pub async fn create_invitations(
        &self,
        org_id: &str,
        emails: Vec<String>,
    ) -> Result<ApiResponse<InvitationsData>> {
        let path = format!("/api/v2/organizations/{org_id}/invitations");
        let body = CreateInvitationsRequest { emails };
        self.post(&path, &body).await
    }

    pub async fn resend_invitation(
        &self,
        org_id: &str,
        invitation_id: &str,
    ) -> Result<ApiResponse<Invitation>> {
        let path = format!("/api/v2/organizations/{org_id}/invitations/{invitation_id}/resend");
        self.post_empty(&path).await
    }

    pub async fn revoke_invitation(&self, org_id: &str, invitation_id: &str) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/invitations/{invitation_id}");
        self.delete_no_body(&path).await
    }

    pub async fn accept_invitation(
        &self,
        invited_member_id: &str,
        token: &str,
    ) -> Result<ApiResponse<AcceptInvitationResponse>> {
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
    ) -> Result<ApiResponse<InviteLink>> {
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
