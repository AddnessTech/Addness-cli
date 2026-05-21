use crate::api::{ApiClient, OrganizationsResponse};
use anyhow::Result;

impl ApiClient {
    pub async fn list_organizations(&self) -> Result<OrganizationsResponse> {
        self.get_without_org("/api/v2/organizations/me").await
    }
}
