use anyhow::Result;
use serde_json::Value;

use crate::api::{
    ApiClient, ApiResponse, CreateInvitationsRequest, CreateInviteLinkRequest, Invitation,
    InvitationsData, InviteLink,
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

    /// GET /api/v2/organizations/:id/invite-links
    /// Response shape isn't modeled to `InviteLink` (unlike `create_invite_link`)
    /// because the list payload hasn't been confirmed to match that struct;
    /// surfaced as raw JSON like the other read-only `Value` endpoints.
    pub async fn list_invite_links(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/invite-links");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/invitations/:token
    /// Public preview endpoint: no auth/organization header required.
    pub async fn preview_invitation(&self, token: &str) -> Result<Value> {
        let path = format!("/api/v2/invitations/{token}");
        let resp: ApiResponse<Value> = self.get_without_org(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/invited-members?status=
    pub async fn list_invited_members(&self, org_id: &str, status: Option<&str>) -> Result<Value> {
        let suffix = match status {
            Some(status) => format!("?status={status}"),
            None => String::new(),
        };
        let path = format!("/api/v2/organizations/{org_id}/invited-members{suffix}");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/organizations/:id/invitation-overview
    pub async fn get_invitation_overview(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/invitation-overview");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }
}
