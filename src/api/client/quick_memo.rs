use anyhow::Result;
use serde_json::Value;

use crate::api::{ApiClient, ApiResponse};

impl ApiClient {
    pub async fn list_quick_memos(&self, cursor: Option<&str>, limit: u16) -> Result<Value> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        query.append_pair("limit", &limit.to_string());
        if let Some(cursor) = cursor {
            query.append_pair("cursor", cursor);
        }
        let response: ApiResponse<Value> = self
            .get(&format!("/api/v2/quick-memos?{}", query.finish()))
            .await?;
        Ok(response.data)
    }
}
