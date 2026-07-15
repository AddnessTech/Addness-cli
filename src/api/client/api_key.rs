use anyhow::Result;

use crate::api::{
    ApiClient, ApiKey, ApiKeyCreateRequest, ApiKeyCreated, ApiKeyRevokeResponse, ApiResponse,
};

impl ApiClient {
    /// GET /api/v1/team/api-keys
    pub async fn list_api_keys(&self) -> Result<Vec<ApiKey>> {
        let resp: ApiResponse<Vec<ApiKey>> = self.get("/api/v1/team/api-keys").await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/api-keys (201 Created). `resp.key` is the plaintext
    /// secret — the backend returns it only on this call.
    pub async fn create_api_key(&self, req: &ApiKeyCreateRequest) -> Result<ApiKeyCreated> {
        let resp: ApiResponse<ApiKeyCreated> = self.post("/api/v1/team/api-keys", req).await?;
        Ok(resp.data)
    }

    /// DELETE /api/v1/team/api-keys/:id (200 OK with a JSON `{"message": ...}`
    /// body, not 204 — uses `delete_json` rather than `delete_no_body`).
    pub async fn revoke_api_key(&self, key_id: &str) -> Result<ApiKeyRevokeResponse> {
        let path = format!("/api/v1/team/api-keys/{key_id}");
        let resp: ApiResponse<ApiKeyRevokeResponse> = self.delete_json(&path).await?;
        Ok(resp.data)
    }
}
