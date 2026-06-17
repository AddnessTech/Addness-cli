use std::{collections::HashMap, path::Path};

use crate::api::{
    ApiClient, ApiResponse, AttachmentUploadRequest, BatchDeleteDeliverableRequest,
    BatchMoveDeliverableRequest, CreateDeliverableRequest, Deliverable, DeliverableCreateData,
    DeliverableListData, DeliverableType, MoveDeliverableRequest, RenameDeliverableRequest,
    UpdateDeliverableRequest,
};
use anyhow::{Context, Result};

impl ApiClient {
    pub async fn create_folder_deliverable(
        &self,
        goal_id: &str,
        display_name: &str,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let body = CreateDeliverableRequest {
            node_type: DeliverableType::Folder,
            display_name: display_name.to_string(),
            content: None,
            link_url: None,
            file: None,
        };
        let path = format!("/api/v1/team/objectives/{goal_id}/deliverables");
        self.post(&path, &body).await
    }

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

    pub async fn create_file_deliverable_from_path(
        &self,
        goal_id: &str,
        path: &Path,
        display_name: Option<&str>,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Failed to stat file {}", path.display()))?;
        if !metadata.is_file() {
            anyhow::bail!("{} is not a regular file", path.display());
        }

        let file_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(String::from)
            .ok_or_else(|| anyhow::anyhow!("Cannot derive file name from {}", path.display()))?;
        let display = display_name.unwrap_or(&file_name);
        let content_type = guess_content_type(path)?;

        let resp = self
            .create_file_deliverable(
                goal_id,
                display,
                &file_name,
                &content_type,
                metadata.len() as i64,
            )
            .await?;

        let upload = resp.data.upload_request.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Server did not return an upload URL for the file deliverable")
        })?;
        let bytes = std::fs::read(path)
            .with_context(|| format!("Failed to read file {}", path.display()))?;

        if let Err(upload_err) = self
            .upload_attachment(
                &upload.url,
                &upload.values,
                bytes,
                &file_name,
                &content_type,
            )
            .await
        {
            // アップロード失敗時はサーバ側に空のdeliverableが残るため削除する。
            // 削除にも失敗した場合は孤立IDをユーザーに案内する。
            if let Err(cleanup_err) = self.delete_deliverable(goal_id, &resp.data.id).await {
                anyhow::bail!(
                    "{upload_err}\n\nNote: failed to remove the placeholder deliverable \
                     (id={}): {cleanup_err}. You may want to delete it from the web UI.",
                    resp.data.id
                );
            }
            return Err(upload_err.context("Failed to upload file deliverable"));
        }

        Ok(resp)
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
    ) -> Result<ApiResponse<Deliverable>> {
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
    ) -> Result<ApiResponse<Deliverable>> {
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
    ) -> Result<ApiResponse<Deliverable>> {
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
    ) -> Result<ApiResponse<DeliverableListData>> {
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

fn guess_content_type(path: &Path) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    let ct = match ext.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("webm") => "video/webm",
        Some("pdf") => "application/pdf",
        Some("csv") => "text/csv",
        Some("txt") => "text/plain",
        Some("md" | "markdown") => "text/markdown",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("ppt") => "application/vnd.ms-powerpoint",
        Some("pptx") => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => anyhow::bail!(
            "Unsupported file extension: {}. Supported: jpg/jpeg/png/gif/webp/mp4/mov/webm/pdf/csv/txt/md/doc/docx/xls/xlsx/ppt/pptx",
            path.display()
        ),
    };
    Ok(ct.to_string())
}
