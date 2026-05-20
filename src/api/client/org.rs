use crate::api::{
    ApiClient, CreateOrganizationRequest, OrganizationsResponse, UpdateContextRequest,
    UpdateOrganizationRequest,
};
use anyhow::Result;

impl ApiClient {
    pub async fn list_organizations(&self) -> Result<OrganizationsResponse> {
        self.get_without_org("/api/v2/organizations/me").await
    }

    pub async fn create_organization(
        &self,
        name: &str,
        organization_type: &str,
        team_scale: Option<String>,
    ) -> Result<serde_json::Value> {
        let body = CreateOrganizationRequest {
            name: name.to_string(),
            organization_type: organization_type.to_string(),
            team_scale,
        };
        self.post("/api/v1/team/organizations", &body).await
    }

    pub async fn update_organization(&self, org_id: &str, name: &str) -> Result<serde_json::Value> {
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
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/organizations/{org_id}/context");
        let body = UpdateContextRequest {
            context_text: context_text.to_string(),
        };
        self.patch(&path, &body).await
    }
}
