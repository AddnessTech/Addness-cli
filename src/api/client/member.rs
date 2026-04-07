use anyhow::Result;

use crate::api::{ApiClient, ApiResponse, MembersListData};

impl ApiClient {
    pub async fn get_members(&self, org_id: &str) -> Result<ApiResponse<MembersListData>> {
        let path = format!("/api/v2/organizations/{org_id}/members?pageSize=100");
        self.get(&path).await
    }
}
