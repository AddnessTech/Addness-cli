use serde::{Deserialize, Serialize};

use super::AttachmentUploadResponse;

// Inline media API models (internal/inlinemedia — editor paste/drop images
// and videos). The upload-init request/response shapes are identical to the
// deliverable file-upload flow (S3 presigned POST url + form values), so
// this reuses `AttachmentUploadRequest`/`AttachmentUploadResponse` and
// `ApiClient::upload_attachment` rather than redefining them.
// Backend reference: internal/inlinemedia/handler/endpoints/*.go.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineMediaViewData {
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineMediaUploadInitData {
    pub inline_media_id: String,
    pub view_url: String,
    pub file_type: String,
    pub upload: AttachmentUploadResponse,
}
