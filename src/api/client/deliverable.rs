use std::collections::HashMap;

use crate::api::{
    ApiClient, ApiResponse, AttachmentUploadRequest, BatchDeleteDeliverableRequest,
    BatchMoveDeliverableRequest, CreateDeliverableRequest, Deliverable, DeliverableCreateData,
    DeliverableListData, DeliverableType, MoveDeliverableRequest, RenameDeliverableRequest,
    UpdateDeliverableRequest,
};
use anyhow::{Context, Result};

impl ApiClient {
    pub async fn create_link_deliverable(
        &self,
        goal_id: &str,
        url: &str,
        display_name: &str,
    ) -> Result<ApiResponse<Deliverable>> {
        let body = CreateDeliverableRequest {
            node_type: DeliverableType::Link,
            display_name: display_name.to_string(),
            content: None,
            link_url: Some(url.to_string()),
            file: None,
        };
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.post(&path, &body).await
    }

    pub async fn create_document_deliverable(
        &self,
        goal_id: &str,
        display_name: &str,
        content: &str,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let body = CreateDeliverableRequest {
            node_type: DeliverableType::Document,
            display_name: display_name.to_string(),
            content: Some(content.to_string()),
            link_url: None,
            file: None,
        };
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.post(&path, &body).await
    }

    pub async fn create_file_deliverable(
        &self,
        goal_id: &str,
        display_name: &str,
        file_name: &str,
        content_type: &str,
        file_size: i64,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let body = CreateDeliverableRequest {
            node_type: DeliverableType::File,
            display_name: display_name.to_string(),
            content: None,
            link_url: None,
            file: Some(AttachmentUploadRequest {
                file_name: file_name.to_string(),
                content_type: content_type.to_string(),
                file_size,
            }),
        };
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.post(&path, &body).await
    }

    /// S3 presigned POST URL に multipart で実ファイルをアップロードする。
    pub async fn upload_attachment(
        &self,
        url: &str,
        values: &HashMap<String, String>,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<()> {
        let mut form = reqwest::multipart::Form::new();
        for (k, v) in values {
            form = form.text(k.clone(), v.clone());
        }
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str(content_type)
            .context("Invalid content type for upload part")?;
        form = form.part("file", part);

        // S3 への直接POSTなので認証ヘッダ等は付与しない（独立クライアント）
        let resp = reqwest::Client::new()
            .post(url)
            .multipart(form)
            .send()
            .await
            .with_context(|| format!("Failed to upload file to {url}"))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("S3 upload failed ({status}): {body}");
        }
        Ok(())
    }

    pub async fn get_goal_deliverables(
        &self,
        goal_id: &str,
    ) -> Result<ApiResponse<DeliverableListData>> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.get(&path).await
    }

    pub async fn update_deliverable(
        &self,
        goal_id: &str,
        deliverable_id: &str,
        content: &str,
        mentions: Vec<String>,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables/{deliverable_id}");
        let body = UpdateDeliverableRequest {
            content: content.to_string(),
            mentions,
        };
        self.patch(&path, &body).await
    }

    pub async fn rename_deliverable(
        &self,
        goal_id: &str,
        deliverable_id: &str,
        display_name: &str,
    ) -> Result<serde_json::Value> {
        let path =
            format!("/api/v1/team/objectives/{goal_id}/deliverables/{deliverable_id}/rename");
        let body = RenameDeliverableRequest {
            display_name: display_name.to_string(),
        };
        self.patch(&path, &body).await
    }

    pub async fn move_deliverable(
        &self,
        goal_id: &str,
        deliverable_id: &str,
        target_parent_deliverable_id: Option<String>,
        order_no: f64,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables/{deliverable_id}/move");
        let body = MoveDeliverableRequest {
            target_parent_deliverable_id,
            order_no,
        };
        self.patch(&path, &body).await
    }

    pub async fn delete_deliverable(&self, goal_id: &str, deliverable_id: &str) -> Result<()> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables/{deliverable_id}");
        self.delete_no_body(&path).await
    }

    pub async fn batch_move_deliverables(
        &self,
        goal_id: &str,
        node_ids: Vec<String>,
        target_parent_deliverable_id: Option<String>,
    ) -> Result<serde_json::Value> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables/batch_move");
        let body = BatchMoveDeliverableRequest {
            node_ids,
            target_parent_deliverable_id,
        };
        self.post(&path, &body).await
    }

    pub async fn batch_delete_deliverables(
        &self,
        goal_id: &str,
        node_ids: Vec<String>,
    ) -> Result<()> {
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables/batch_delete");
        let body = BatchDeleteDeliverableRequest { node_ids };
        self.post_no_content(&path, &body).await
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
