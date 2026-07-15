use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, ShareTreeCloneData, ShareTreeCloneRequest, ShareTreeCreateData,
    ShareTreeCreateRequest, ShareTreeListMineData, ShareTreeSummary, SharedGoalTreePublic,
};

impl ApiClient {
    /// POST /api/v2/share-trees
    /// Requires the `x-organization-id` header (org context of the source
    /// goal); no organization id appears in the path.
    pub async fn create_share_tree(
        &self,
        source_objective_id: &str,
        selected_objective_ids: Vec<String>,
    ) -> Result<ShareTreeCreateData> {
        let body = ShareTreeCreateRequest {
            source_objective_id: source_objective_id.to_string(),
            selected_objective_ids,
        };
        let resp: ApiResponse<ShareTreeCreateData> =
            self.post("/api/v2/share-trees", &body).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v2/share-trees/:id (204)
    pub async fn revoke_share_tree(&self, id: &str) -> Result<()> {
        let path = format!("/api/v2/share-trees/{id}");
        self.delete_no_body(&path).await
    }

    /// GET /api/v2/share-trees/mine
    pub async fn list_my_share_trees(&self) -> Result<Vec<ShareTreeSummary>> {
        let resp: ApiResponse<ShareTreeListMineData> = self.get("/api/v2/share-trees/mine").await?;
        Ok(resp.data.items)
    }

    /// POST /api/v2/share-trees/clones
    pub async fn clone_share_tree(
        &self,
        public_id: &str,
        parent_objective_id: Option<String>,
    ) -> Result<ShareTreeCloneData> {
        let body = ShareTreeCloneRequest {
            public_id: public_id.to_string(),
            parent_objective_id,
        };
        let resp: ApiResponse<ShareTreeCloneData> =
            self.post("/api/v2/share-trees/clones", &body).await?;
        Ok(resp.data)
    }

    /// GET /api/v1/public/goal-trees/:publicId (no auth required)
    pub async fn get_public_shared_goal_tree(
        &self,
        public_id: &str,
    ) -> Result<SharedGoalTreePublic> {
        let path = format!("/api/v1/public/goal-trees/{public_id}");
        let resp: ApiResponse<SharedGoalTreePublic> = self.get_without_org(&path).await?;
        Ok(resp.data)
    }
}
