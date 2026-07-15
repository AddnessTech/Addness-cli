use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, AttachmentUploadRequest, InlineMediaUploadInitData, InlineMediaViewData,
};

impl ApiClient {
    /// GET /api/v2/inline-media/:id?download=1
    /// Returns a presigned S3 GET URL (JSON), not a redirect/binary body.
    pub async fn view_inline_media_url(&self, id: &str, download: bool) -> Result<String> {
        let path = if download {
            format!("/api/v2/inline-media/{id}?download=1")
        } else {
            format!("/api/v2/inline-media/{id}")
        };
        let resp: ApiResponse<InlineMediaViewData> = self.get(&path).await?;
        Ok(resp.data.url)
    }

    /// POST /api/v2/organizations/:id/objectives/:goalId/inline-media/upload-url
    pub async fn init_inline_media_upload(
        &self,
        org_id: &str,
        goal_id: &str,
        file_name: &str,
        content_type: &str,
        file_size: i64,
    ) -> Result<InlineMediaUploadInitData> {
        let path =
            format!("/api/v2/organizations/{org_id}/objectives/{goal_id}/inline-media/upload-url");
        let body = AttachmentUploadRequest {
            file_name: file_name.to_string(),
            content_type: content_type.to_string(),
            file_size,
        };
        let resp: ApiResponse<InlineMediaUploadInitData> = self.post(&path, &body).await?;
        Ok(resp.data)
    }
}
