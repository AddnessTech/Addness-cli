use crate::api::{ApiClient, OrganizationsResponse};
use anyhow::Result;

impl ApiClient {
    pub async fn list_organizations(&self) -> Result<OrganizationsResponse> {
        self.get("/api/v1/team/organizations/my_organizations")
            .await
    }
}
