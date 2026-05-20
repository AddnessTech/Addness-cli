use crate::api::{
    ApiClient, CreateAssignmentRequest, TransferOwnershipRequest, UpdateAssignmentRequest,
};
use anyhow::Result;

impl ApiClient {
    pub async fn create_assignment(
        &self,
        goal_id: &str,
        member_id: &str,
        role: Option<String>,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/objectives/{goal_id}/assignments");
        let body = CreateAssignmentRequest {
            organization_member_id: member_id.to_string(),
            role,
        };
        self.post(&path, &body).await
    }

    pub async fn update_assignment(
        &self,
        goal_id: &str,
        assignment_id: &str,
        role: Option<String>,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/objectives/{goal_id}/assignments/{assignment_id}");
        let body = UpdateAssignmentRequest { role };
        self.patch(&path, &body).await
    }

    pub async fn delete_assignment(&self, goal_id: &str, assignment_id: &str) -> Result<()> {
        let path = format!("/api/v2/objectives/{goal_id}/assignments/{assignment_id}");
        self.delete_no_body(&path).await
    }

    pub async fn transfer_ownership(
        &self,
        goal_id: &str,
        new_owner_member_id: &str,
        actor_as_editor: bool,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v2/objectives/{goal_id}/transfer-ownership");
        let body = TransferOwnershipRequest {
            new_owner_member_id: new_owner_member_id.to_string(),
            actor_as_editor,
        };
        self.put(&path, &body).await
    }
}
