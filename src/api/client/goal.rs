use crate::api::{
    ApiClient, ApiResponse, ChangeParentRequest, CreateGoalRequest, Goal, GoalChildrenData,
    GoalSearchResponse, GoalTreeData, ObjectiveIdsRequest, ShareLinkResponse, UpdateGoalRequest,
};
use anyhow::Result;

impl ApiClient {
    pub async fn get_goal_tree(
        &self,
        org_id: &str,
        depth: usize,
    ) -> Result<ApiResponse<GoalTreeData>> {
        let path = format!(
            "/api/v2/organizations/{}/objectives/tree?depth={}&include_owner=true",
            org_id, depth
        );

        self.get(&path).await
    }

    pub async fn get_goal_tree_with_completed(
        &self,
        org_id: &str,
        depth: usize,
    ) -> Result<ApiResponse<GoalTreeData>> {
        let path = format!(
            "/api/v2/organizations/{}/objectives/tree?depth={}&include_owner=true&include_completed=true",
            org_id, depth
        );

        // `include_completed=true` を付けるとサーバーが 5xx を返す不具合があるため、
        // 失敗時は完了ゴールを除いた通常ツリーへフォールバックし、一覧が空に
        // ならないようにする。サーバー復旧後は自動的に完了ゴールも再表示される。
        // 通常ツリーも失敗した場合は本来のエラーを返す（認証切れ等の握り潰し回避）。
        match self.get(&path).await {
            Ok(resp) => Ok(resp),
            Err(primary_err) => self
                .get_goal_tree(org_id, depth)
                .await
                .map_err(|_| primary_err),
        }
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
    ) -> Result<ApiResponse<GoalChildrenData>> {
        let path = format!(
            "/api/v2/objectives/{goal_id}/children?include_owner=true&limit={limit}&offset={offset}"
        );
        self.get(&path).await
    }

    pub async fn get_goal_subtree(&self, goal_id: &str) -> Result<ApiResponse<GoalTreeData>> {
        let path = format!("/api/v2/objectives/{goal_id}/subtree?include_owner=true");
        self.get(&path).await
    }

    pub async fn search_goals(&self, query: &str) -> Result<ApiResponse<GoalSearchResponse>> {
        let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
        let path = format!("/api/v2/objectives/search?title={encoded}&permission=read");
        self.get(&path).await
    }

    pub async fn create_goal(&self, req: &CreateGoalRequest) -> Result<ApiResponse<Goal>> {
        self.post("/api/v2/objectives", req).await
    }

    pub async fn update_goal(
        &self,
        goal_id: &str,
        req: &UpdateGoalRequest,
    ) -> Result<ApiResponse<Goal>> {
        let path = format!("/api/v2/objectives/{goal_id}");
        self.patch(&path, req).await
    }

    pub async fn delete_goal(&self, goal_id: &str) -> Result<()> {
        let body = serde_json::json!({ "objectiveIds": [goal_id] });
        self.delete_with_body("/api/v2/objectives/delete", &body)
            .await
    }

    pub async fn archive_goals(&self, goal_ids: Vec<String>) -> Result<()> {
        let body = ObjectiveIdsRequest {
            objective_ids: goal_ids,
        };
        self.post_no_content("/api/v2/objectives/archive", &body)
            .await
    }

    pub async fn unarchive_goals(&self, goal_ids: Vec<String>) -> Result<()> {
        let body = ObjectiveIdsRequest {
            objective_ids: goal_ids,
        };
        self.post_no_content("/api/v2/objectives/unarchive", &body)
            .await
    }

    pub async fn restore_goals(&self, goal_ids: Vec<String>) -> Result<()> {
        let body = ObjectiveIdsRequest {
            objective_ids: goal_ids,
        };
        self.post_no_content("/api/v2/objectives/restore", &body)
            .await
    }

    pub async fn change_goal_parent(
        &self,
        goal_id: &str,
        new_parent_id: Option<String>,
    ) -> Result<ApiResponse<Goal>> {
        let path = format!("/api/v2/objectives/{goal_id}/parent");
        let body = ChangeParentRequest { new_parent_id };
        self.post(&path, &body).await
    }

    pub async fn create_share_link(&self, goal_id: &str) -> Result<ShareLinkResponse> {
        let path = format!("/api/v2/objectives/{goal_id}/share");
        let response: ApiResponse<ShareLinkResponse> = self.post_empty(&path).await?;
        Ok(response.data)
    }

    pub async fn revoke_share_link(&self, goal_id: &str) -> Result<()> {
        let path = format!("/api/v2/objectives/{goal_id}/share");
        self.delete_no_body(&path).await
    }
}
