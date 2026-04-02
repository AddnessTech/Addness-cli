use std::collections::HashMap;

use crate::api::{ApiClient, ApiResponse, Deliverable, DeliverableListData};
use anyhow::Result;

impl ApiClient {
    pub async fn get_goal_deliverables(
        &self,
        goal_id: &str,
    ) -> Result<ApiResponse<DeliverableListData>> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.get(&path).await
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
}
