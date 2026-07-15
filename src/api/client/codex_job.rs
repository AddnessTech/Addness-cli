use anyhow::Result;
use futures::TryStreamExt;

use crate::api::{
    ApiClient, ApiResponse, CodexJob, CodexJobCreateRequest, CodexJobInputRequest,
    CodexJobListResponse,
};

/// Build the query-string suffix for `limit`/`from_seq`/`tail` pairs, sharing
/// the same encoding style as `skill.rs`/`personal.rs`.
fn query_suffix(pairs: &[(&str, Option<&str>)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    let mut any = false;
    for (key, value) in pairs {
        if let Some(value) = value {
            serializer.append_pair(key, value);
            any = true;
        }
    }
    if !any {
        return String::new();
    }
    format!("?{}", serializer.finish())
}

// v1 (`/api/v1/codex/jobs`) と v2 (`/api/v2/codex/jobs`) は同一Handlerを共有し、
// v2にのみDELETE（論理削除）が追加登録されている完全上位互換のため、v2のみを
// 実装する（`src/api/models/codex_job.rs` 冒頭のメモ参照）。
impl ApiClient {
    /// GET /api/v2/codex/jobs?limit=
    /// (limit: default 50, max 100; cursor pagination is not offered)
    pub async fn list_codex_jobs(&self, limit: Option<u32>) -> Result<CodexJobListResponse> {
        let limit_str = limit.map(|l| l.to_string());
        let path = format!(
            "/api/v2/codex/jobs{}",
            query_suffix(&[("limit", limit_str.as_deref())])
        );
        let resp: ApiResponse<CodexJobListResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/codex/jobs/:id
    pub async fn get_codex_job(&self, job_id: &str) -> Result<CodexJob> {
        let path = format!("/api/v2/codex/jobs/{job_id}");
        let resp: ApiResponse<CodexJob> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/codex/jobs (201 Created)
    pub async fn create_codex_job(&self, req: &CodexJobCreateRequest) -> Result<CodexJob> {
        let resp: ApiResponse<CodexJob> = self.post("/api/v2/codex/jobs", req).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/codex/jobs/:id/input (202 Accepted, no body)
    pub async fn send_codex_job_input(&self, job_id: &str, prompt: &str) -> Result<()> {
        let path = format!("/api/v2/codex/jobs/{job_id}/input");
        let req = CodexJobInputRequest {
            prompt: prompt.to_string(),
        };
        self.post_no_content(&path, &req).await
    }

    /// POST /api/v2/codex/jobs/:id/resume (202 Accepted)
    pub async fn resume_codex_job(&self, job_id: &str) -> Result<CodexJob> {
        let path = format!("/api/v2/codex/jobs/{job_id}/resume");
        let resp: ApiResponse<CodexJob> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/codex/jobs/:id/close (202 Accepted, no body)
    pub async fn close_codex_job(&self, job_id: &str) -> Result<()> {
        let path = format!("/api/v2/codex/jobs/{job_id}/close");
        self.post_empty_no_content(&path).await
    }

    /// POST /api/v2/codex/jobs/:id/cancel (202 Accepted, no body)
    pub async fn cancel_codex_job(&self, job_id: &str) -> Result<()> {
        let path = format!("/api/v2/codex/jobs/{job_id}/cancel");
        self.post_empty_no_content(&path).await
    }

    /// DELETE /api/v2/codex/jobs/:id (204 No Content; soft delete, v2 only)
    pub async fn delete_codex_job(&self, job_id: &str) -> Result<()> {
        let path = format!("/api/v2/codex/jobs/{job_id}");
        self.delete_no_body(&path).await
    }

    /// GET /api/v2/codex/jobs/:id/events?from_seq=&tail= (SSE)
    ///
    /// Streams job events to `on_event(event_type, data_json)` until the
    /// server closes the stream (the backend terminates it once the job
    /// reaches a terminal status or a `done` event is emitted). `from_seq`
    /// resumes after a known sequence number; `tail` replays only the last N
    /// stored events (ignored when `from_seq` is set; server cap 2000).
    pub async fn stream_codex_job_events<F>(
        &self,
        job_id: &str,
        from_seq: Option<u64>,
        tail: Option<u64>,
        mut on_event: F,
    ) -> Result<()>
    where
        F: FnMut(&str, &str) -> Result<()>,
    {
        use eventsource_stream::Eventsource;

        let from_seq_str = from_seq.map(|v| v.to_string());
        let tail_str = tail.map(|v| v.to_string());
        let path = format!(
            "/api/v2/codex/jobs/{job_id}/events{}",
            query_suffix(&[
                ("from_seq", from_seq_str.as_deref()),
                ("tail", tail_str.as_deref())
            ])
        );
        let response = self.get_stream(&path).await?;
        let mut events = response.bytes_stream().eventsource();
        while let Some(event) = events.try_next().await? {
            on_event(&event.event, &event.data)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::query_suffix;

    #[test]
    fn query_suffix_is_empty_without_any_value() {
        assert_eq!(query_suffix(&[("from_seq", None), ("tail", None)]), "");
    }

    #[test]
    fn query_suffix_encodes_present_values_only() {
        assert_eq!(query_suffix(&[("limit", Some("10"))]), "?limit=10");
        assert_eq!(
            query_suffix(&[("from_seq", Some("42")), ("tail", Some("100"))]),
            "?from_seq=42&tail=100"
        );
    }
}
