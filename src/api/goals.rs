use crate::api::{ApiClient, ApiResponse, TreeData};
use anyhow::Result;

impl ApiClient {
    pub async fn get_goal_tree(&self, org_id: &str, depth: usize) -> Result<ApiResponse<TreeData>> {
        let path = format!(
            "/api/v2/organizations/{}/objectives/tree?depth={}&include_owner=true",
            org_id, depth
        );

        self.get(&path).await
    }
}
