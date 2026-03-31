use crate::api::{
    ApiClient, ApiResponse, ChildrenData, DeliverableListData, Goal, SearchResponse, TreeData,
    UpdateGoalRequest,
};
use anyhow::Result;

impl ApiClient {
    pub async fn get_goal_tree(&self, org_id: &str, depth: usize) -> Result<ApiResponse<TreeData>> {
        let path = format!(
            "/api/v2/organizations/{}/objectives/tree?depth={}&include_owner=true",
            org_id, depth
        );

        self.get(&path).await
    }

    pub async fn get_goal(&self, goal_id: &str) -> Result<ApiResponse<Goal>> {
        let path = format!("/api/v2/objectives/{goal_id}");
        self.get(&path).await
    }

    pub async fn get_goal_children(
        &self,
        goal_id: &str,
        limit: usize,
        offset: usize,
    ) -> Result<ApiResponse<ChildrenData>> {
        let path = format!(
            "/api/v2/objectives/{goal_id}/children?include_owner=true&limit={limit}&offset={offset}"
        );
        self.get(&path).await
    }

    pub async fn get_goal_subtree(&self, goal_id: &str) -> Result<ApiResponse<TreeData>> {
        let path = format!("/api/v2/objectives/{goal_id}/subtree?include_owner=true");
        self.get(&path).await
    }

    pub async fn get_goal_deliverables(
        &self,
        goal_id: &str,
    ) -> Result<ApiResponse<DeliverableListData>> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.get(&path).await
    }

    pub async fn search_goals(&self, query: &str) -> Result<ApiResponse<SearchResponse>> {
        let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let path = format!("/api/v1/team/objectives/search?title={encoded}&permission=read");
        self.get(&path).await
    }

    pub async fn update_goal(
        &self,
        _org_id: &str,
        goal_id: &str,
        req: &UpdateGoalRequest,
    ) -> Result<ApiResponse<Goal>> {
        let path = format!("/api/v2/objectives/{goal_id}");

        self.patch(&path, req).await
    }
}
