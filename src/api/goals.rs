use crate::api::{ApiClient, ApiResponse, Goal, TreeData, UpdateGoalRequest};
use anyhow::Result;

impl ApiClient {
    pub async fn get_goal_tree(&self, org_id: &str, depth: usize) -> Result<ApiResponse<TreeData>> {
        let path = format!(
            "/api/v2/organizations/{}/objectives/tree?depth={}&include_owner=true",
            org_id, depth
        );

        self.get(&path).await
    }

    pub async fn update_goal(&self, id: &str, req: &UpdateGoalRequest) -> Result<ApiResponse<Goal>> {
        let path = format!("/api/v2/objectives/{}", id);
        self.patch(&path, req).await
    }
}
