use std::collections::HashMap;

use crate::api::{
    ApiClient, ApiResponse, Comment, Deliverable, DeliverableListData, Goal, GoalChildrenData,
    GoalTreeData, SearchResponse, UpdateGoalRequest,
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

    /// 各ゴールの成果物を並行取得してマップで返す
    pub async fn get_deliverables_map(
        &self,
        goal_ids: &[&str],
    ) -> HashMap<String, Vec<Deliverable>> {
        let futures: Vec<_> = goal_ids
            .iter()
            .map(|g| self.get_goal_deliverables(g))
            .collect();
        let results = futures::future::join_all(futures).await;

        let mut map = HashMap::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(resp) => {
                    map.insert(goal_ids[i].to_string(), resp.data.deliverables);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: failed to fetch deliverables for {}: {e}",
                        goal_ids[i]
                    );
                }
            }
        }

        map
    }

    /// 各ゴールのコメントを並行取得してマップで返す
    pub async fn get_comments_map(&self, goal_ids: &[&str]) -> HashMap<String, Vec<Comment>> {
        let futures: Vec<_> = goal_ids.iter().map(|g| self.list_comments(g)).collect();
        let results = futures::future::join_all(futures).await;

        let mut map = HashMap::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(resp) => {
                    map.insert(goal_ids[i].to_string(), resp.comments);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: failed to fetch deliverables for {}: {e}",
                        goal_ids[i]
                    );
                }
            }
        }

        map
    }
}
