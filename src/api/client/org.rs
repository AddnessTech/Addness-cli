use crate::api::{
    ApiClient, ApiResponse, CreateOrganizationRequest, Organization, OrganizationsResponse,
    UpdateContextRequest, UpdateOrganizationRequest,
};
use anyhow::Result;

pub struct CreateOrganizationParams {
    pub name: String,
    pub organization_type: String,
    pub team_scale: Option<String>,
    pub plan_type: Option<String>,
    pub industry: Option<String>,
    pub phone_number: Option<String>,
    pub browser_timezone: Option<String>,
    pub logo_url: Option<String>,
}

impl ApiClient {
    pub async fn list_organizations(&self) -> Result<OrganizationsResponse> {
        self.get_without_org("/api/v2/organizations/me").await
    }

    pub async fn create_organization(
        &self,
        params: CreateOrganizationParams,
    ) -> Result<ApiResponse<Organization>> {
        let body = CreateOrganizationRequest {
            name: params.name,
            organization_type: params.organization_type,
            team_scale: params.team_scale,
            plan_type: params.plan_type,
            industry: params.industry,
            phone_number: params.phone_number,
            browser_timezone: params.browser_timezone,
            logo_url: params.logo_url,
        };
        self.post_without_org("/api/v1/team/organizations", &body)
            .await
    }

    pub async fn update_organization(
        &self,
        org_id: &str,
        name: &str,
    ) -> Result<ApiResponse<Organization>> {
        let path = format!("/api/v2/organizations/{org_id}");
        let body = UpdateOrganizationRequest {
            name: name.to_string(),
        };
        self.patch(&path, &body).await
    }

    pub async fn delete_organization(&self, org_id: &str) -> Result<()> {
        let path = format!("/api/v1/team/organizations/{org_id}");
        self.delete_no_body(&path).await
    }

    pub async fn update_organization_context(
        &self,
        org_id: &str,
        context_text: &str,
    ) -> Result<ApiResponse<Organization>> {
        let path = format!("/api/v2/organizations/{org_id}/context");
        let body = UpdateContextRequest {
            context_text: context_text.to_string(),
        };
        self.patch(&path, &body).await
    }
}
