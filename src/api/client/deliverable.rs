use crate::api::{
    ApiClient, ApiResponse, Deliverable, DeliverableCreateData, DeliverableListData,
    DeliverableType, RelatedFetchError,
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

const NODES: &str = "/api/v2/drive/nodes";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveNode {
    id: String,
    name: String,
    kind: DeliverableType,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    parent_id: Option<String>,
    #[serde(default)]
    goals: Option<Vec<DriveGoal>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveGoal {
    objective_id: String,
}

#[derive(Deserialize)]
struct DrivePage {
    items: Vec<DriveNode>,
    total: i64,
}

impl DriveNode {
    fn deliverable(self, goal: &str) -> Deliverable {
        let file_name = (self.kind == DeliverableType::File).then(|| self.name.clone());
        let folder = self.kind == DeliverableType::Folder;
        Deliverable {
            id: self.id,
            display_name: self.name,
            node_type: self.kind,
            content: None,
            link_url: self.url,
            file_name,
            objective_id: goal.to_string(),
            is_root: self.parent_id.is_none(),
            parent_deliverable_id: self.parent_id,
            order_no: 0.0,
            depth: 0,
            has_children: folder,
            children_count: 0,
        }
    }
    fn created(self, goal: &str) -> ApiResponse<DeliverableCreateData> {
        let node = self.deliverable(goal);
        ApiResponse {
            data: DeliverableCreateData {
                id: node.id,
                display_name: node.display_name,
                node_type: node.node_type,
                content: node.content,
                link_url: node.link_url,
                file_name: node.file_name,
                objective_id: node.objective_id,
                upload_request: None,
            },
        }
    }
}

/// Links commonly use URLs or owner/repo names as their label. Keep them readable
/// while respecting the backend's display-name contract (255 chars, no /, \\, NUL).
fn link_display_name(name: &str) -> String {
    let normalized: String = name
        .trim()
        .chars()
        .map(|ch| match ch {
            '/' => '／',
            '\\' => '＼',
            '\0' => ' ',
            _ => ch,
        })
        .take(255)
        .collect();
    if normalized.trim().is_empty() {
        "Link".to_string()
    } else {
        normalized
    }
}

impl ApiClient {
    async fn deliverable_folder(&self, goal_id: &str) -> Result<String> {
        let result: Value = self
            .post(
                &format!("{NODES}/goal-folder"),
                &json!({"objectiveId": goal_id}),
            )
            .await?;
        Ok(result["nodeId"]
            .as_str()
            .context("Drive did not return nodeId")?
            .to_string())
    }

    async fn scoped_deliverable(&self, goal_id: &str, id: &str) -> Result<DriveNode> {
        let id = super::issue::encode_path_segment(id);
        let node: DriveNode = self.get(&format!("{NODES}/{id}")).await?;
        if !node
            .goals
            .as_deref()
            .unwrap_or_default()
            .iter()
            .any(|goal| goal.objective_id == goal_id)
        {
            bail!("Drive resource is not linked to goal {goal_id}");
        }
        Ok(node)
    }

    pub async fn create_folder_deliverable(
        &self,
        goal_id: &str,
        display_name: &str,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let parent = self.deliverable_folder(goal_id).await?;
        let node: DriveNode = self
            .post(NODES, &json!({"name":display_name,"parentId":parent}))
            .await?;
        Ok(node.created(goal_id))
    }

    pub async fn create_link_deliverable(
        &self,
        goal_id: &str,
        url: &str,
        display_name: &str,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let parent = self.deliverable_folder(goal_id).await?;
        let node: DriveNode = self
            .post(
                &format!("{NODES}/links"),
                &json!({"name":link_display_name(display_name),"url":url,"parentId":parent}),
            )
            .await?;
        Ok(node.created(goal_id))
    }

    async fn upload_drive_deliverable(
        &self,
        goal_id: &str,
        name: &str,
        content_type: &str,
        bytes: Vec<u8>,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        if bytes.len() > 50 * 1024 * 1024 {
            bail!("Drive upload exceeds 50 MiB");
        }
        let parent = self.deliverable_folder(goal_id).await?;
        let query = form_urlencoded::Serializer::new(String::new())
            .append_pair("parentId", &parent)
            .finish();
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(name.to_string())
            .mime_str(content_type)?;
        let form = reqwest::multipart::Form::new().part("file", part);
        let node: DriveNode = self
            .post_multipart(&format!("{NODES}/files?{query}"), form)
            .await?;
        Ok(node.created(goal_id))
    }

    pub async fn create_document_deliverable(
        &self,
        goal_id: &str,
        display_name: &str,
        content: &str,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let name = if Path::new(display_name).extension().is_none() {
            format!("{display_name}.md")
        } else {
            display_name.to_string()
        };
        self.upload_drive_deliverable(
            goal_id,
            &name,
            "text/markdown; charset=utf-8",
            content.as_bytes().to_vec(),
        )
        .await
    }

    pub async fn create_file_deliverable_from_path(
        &self,
        goal_id: &str,
        path: &Path,
        display_name: Option<&str>,
    ) -> Result<ApiResponse<DeliverableCreateData>> {
        let metadata = std::fs::metadata(path)
            .with_context(|| format!("Could not stat {}", path.display()))?;
        if !metadata.is_file() || metadata.len() > 50 * 1024 * 1024 {
            bail!("Upload must be a regular file of at most 50 MiB");
        }
        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .context("Invalid file name")?;
        let content_type = guess_content_type(path)?;
        let bytes = std::fs::read(path)?;
        self.upload_drive_deliverable(
            goal_id,
            display_name.unwrap_or(filename),
            &content_type,
            bytes,
        )
        .await
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
        let mut deliverables = Vec::new();
        let mut seen = HashSet::new();
        let mut page_number = 1;
        let total = loop {
            let query = form_urlencoded::Serializer::new(String::new())
                .append_pair("objectiveId", goal_id)
                .append_pair("limit", "100")
                .append_pair("page", &page_number.to_string())
                .finish();
            let page: DrivePage = self.get(&format!("{NODES}/search?{query}")).await?;
            if page.total < 0 {
                bail!("Drive returned an invalid total");
            }
            let previous = deliverables.len();
            for node in page.items {
                if !seen.insert(node.id.clone()) {
                    bail!("Drive list changed during pagination; retry the list");
                }
                deliverables.push(node.deliverable(goal_id));
            }
            if deliverables.len() as i64 >= page.total {
                break page.total;
            }
            if deliverables.len() == previous || page_number >= 10_000 {
                bail!("Drive pagination ended before all deliverables were retrieved");
            }
            page_number += 1;
        };
        Ok(ApiResponse {
            data: DeliverableListData {
                total,
                deliverables,
            },
        })
    }

    pub async fn update_deliverable(
        &self,
        goal_id: &str,
        id: &str,
        content: &str,
    ) -> Result<ApiResponse<Deliverable>> {
        if content.len() > 4 * 1024 * 1024 {
            bail!("Drive content exceeds 4 MiB");
        }
        let node = self.scoped_deliverable(goal_id, id).await?;
        let _: Value = self
            .put(
                &format!("{NODES}/{}/content", node.id),
                &json!({"content": content}),
            )
            .await?;
        let mut data = node.deliverable(goal_id);
        data.content = Some(content.to_string());
        Ok(ApiResponse { data })
    }

    pub async fn rename_deliverable(
        &self,
        goal_id: &str,
        id: &str,
        name: &str,
    ) -> Result<ApiResponse<Deliverable>> {
        let node = self.scoped_deliverable(goal_id, id).await?;
        let renamed: DriveNode = self
            .patch(&format!("{NODES}/{}", node.id), &json!({"name": name}))
            .await?;
        Ok(ApiResponse {
            data: renamed.deliverable(goal_id),
        })
    }

    pub async fn move_deliverable(
        &self,
        goal_id: &str,
        id: &str,
        parent: Option<String>,
    ) -> Result<ApiResponse<Deliverable>> {
        let node = self.scoped_deliverable(goal_id, id).await?;
        let parent = match parent {
            Some(parent) => {
                let target = self.scoped_deliverable(goal_id, &parent).await?;
                if target.kind != DeliverableType::Folder {
                    bail!("Destination must be a folder");
                }
                target.id
            }
            None => self.deliverable_folder(goal_id).await?,
        };
        let mut expected = serde_json::Map::new();
        expected.insert(node.id.clone(), json!(node.parent_id));
        let _: Value = self
            .post(
                &format!("{NODES}/move"),
                &json!({"nodeIds":[node.id], "parentId":parent, "expectedParents":expected}),
            )
            .await?;
        Ok(ApiResponse {
            data: self
                .scoped_deliverable(goal_id, &node.id)
                .await?
                .deliverable(goal_id),
        })
    }

    pub async fn delete_deliverable(&self, goal_id: &str, id: &str) -> Result<()> {
        self.batch_delete_deliverables(goal_id, vec![id.to_string()])
            .await
    }

    pub async fn batch_delete_deliverables(&self, goal_id: &str, ids: Vec<String>) -> Result<()> {
        let mut verified = Vec::new();
        for id in ids {
            verified.push(self.scoped_deliverable(goal_id, &id).await?.id);
        }
        self.post_no_content(&format!("{NODES}/trash"), &json!({"nodeIds": verified}))
            .await
    }

    /// 各ゴールの成果物を並行取得してマップで返す
    pub async fn get_deliverables_map(
        &self,
        goal_ids: &[&str],
    ) -> HashMap<String, Vec<Deliverable>> {
        let (map, errors) = self.get_deliverables_map_with_errors(goal_ids).await;
        for error in errors {
            eprintln!(
                "Warning: failed to fetch {} for {}: {}",
                error.kind, error.goal_id, error.message
            );
        }

        map
    }

    /// 各ゴールの成果物を並行取得し、部分失敗を呼び出し側で扱える形で返す。
    pub async fn get_deliverables_map_with_errors(
        &self,
        goal_ids: &[&str],
    ) -> (HashMap<String, Vec<Deliverable>>, Vec<RelatedFetchError>) {
        let futures: Vec<_> = goal_ids
            .iter()
            .map(|g| self.get_goal_deliverables(g))
            .collect();
        let results = futures::future::join_all(futures).await;

        let mut map = HashMap::new();
        let mut errors = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Ok(resp) => {
                    map.insert(goal_ids[i].to_string(), resp.data.deliverables);
                }
                Err(e) => {
                    errors.push(RelatedFetchError {
                        kind: "deliverables",
                        goal_id: goal_ids[i].to_string(),
                        message: e.to_string(),
                    });
                }
            }
        }

        (map, errors)
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

#[cfg(test)]
mod link_name_tests {
    use super::link_display_name;

    #[test]
    fn pr_and_url_labels_obey_backend_display_name_contract() {
        assert_eq!(
            link_display_name("AddnessTech/Addness-cli#118"),
            "AddnessTech／Addness-cli#118"
        );
        assert_eq!(
            link_display_name(" https://example.com/a/b "),
            "https:／／example.com／a／b"
        );
        assert_eq!(
            link_display_name("review\\path\0notes"),
            "review＼path notes"
        );
        assert_eq!(link_display_name("  \0  "), "Link");
        let long = link_display_name(&"日本語".repeat(100));
        assert_eq!(long.chars().count(), 255);
        assert_eq!(link_display_name(&long), long);
    }
}
