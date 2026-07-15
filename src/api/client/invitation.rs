use anyhow::Result;
use serde_json::Value;

use crate::api::{
    AcceptInvitationByTokenRequest, AcceptInvitationRequest, AcceptInvitationResponse, ApiClient,
    ApiResponse, CheckPlanUpgradeRequest, CreateInvitationsRequest, CreateInviteLinkRequest,
    DeclineInvitationRequest, Invitation, InvitationsData, InviteLink,
    LegacyAcceptInvitationRequest,
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

    /// GET /api/v2/organizations/:id/invite-links
    /// Response shape isn't modeled to `InviteLink` (unlike `create_invite_link`)
    /// because the list payload hasn't been confirmed to match that struct;
    /// surfaced as raw JSON like the other read-only `Value` endpoints.
    pub async fn list_invite_links(&self, org_id: &str) -> Result<Value> {
        let path = format!("/api/v2/organizations/{org_id}/invite-links");
        let resp: ApiResponse<Value> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/invite-links/:code/join
    pub async fn join_invite_link(&self, code: &str) -> Result<Value> {
        let path = format!("/api/v2/invite-links/{code}/join");
        let resp: ApiResponse<Value> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/organization_invitations/accept?verify_only=true|false
    pub async fn legacy_accept_invitation(&self, token: &str, verify_only: bool) -> Result<Value> {
        let path =
            format!("/api/v1/team/organization_invitations/accept?verify_only={verify_only}");
        let body = LegacyAcceptInvitationRequest {
            token: token.to_string(),
        };
        let resp: ApiResponse<Value> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/organization_invitations/check_plan_upgrade
    pub async fn check_invitation_plan_upgrade(
        &self,
        org_id: &str,
        additional_members_count: i64,
    ) -> Result<Value> {
        let body = CheckPlanUpgradeRequest {
            organization_id: org_id.to_string(),
            additional_members_count,
        };
        let resp: ApiResponse<Value> = self
            .post(
                "/api/v1/team/organization_invitations/check_plan_upgrade",
                &body,
            )
            .await?;
        Ok(resp.data)
    }

    /// GET /api/v2/invitations/:token
    /// Public preview endpoint: no auth/organization header required.
    pub async fn preview_invitation(&self, token: &str) -> Result<Value> {
        let path = format!("/api/v2/invitations/{token}");
        let resp: ApiResponse<Value> = self.get_without_org(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/invitations/:token/accept
    pub async fn accept_invitation_by_token(
        &self,
        token: &str,
        source_organization_id: Option<&str>,
    ) -> Result<Value> {
        let path = format!("/api/v2/invitations/{token}/accept");
        let body = AcceptInvitationByTokenRequest {
            source_organization_id: source_organization_id.map(str::to_string),
        };
        let resp: ApiResponse<Value> = self.post(&path, &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/invitations/pending
    /// Resolved from the authenticated user's email; no organization header needed.
    pub async fn list_pending_invitations(&self) -> Result<Value> {
        let resp: ApiResponse<Value> = self.get_without_org("/api/v2/invitations/pending").await?;
        Ok(resp.data)
    }

    /// POST /api/v2/invitations/pending/:invId/access
    /// No request body; resolved from the authenticated user's email.
    pub async fn create_invitation_access_token(&self, invitation_id: &str) -> Result<Value> {
        let path = format!("/api/v2/invitations/pending/{invitation_id}/access");
        let resp: ApiResponse<Value> = self.post_without_org(&path, &serde_json::json!({})).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/invitations/decline
    pub async fn decline_invitation(&self, invited_member_id: &str, token: &str) -> Result<()> {
        let body = DeclineInvitationRequest {
            invited_member_id: invited_member_id.to_string(),
            token: token.to_string(),
        };
        self.post_no_content("/api/v2/invitations/decline", &body)
            .await
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
