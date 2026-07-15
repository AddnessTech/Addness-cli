use anyhow::{Context, Result};

use crate::api::{
    ActiveHuddlesResponse, ApiClient, ApiResponse, HuddleActive, HuddleInvitationSendRequest,
    HuddleInvitationSendResponse, HuddleInviteableMembersResponse, HuddleRecordingStartRequest,
    HuddleRecordingStartResponse, HuddleRecordingStopResponse, HuddleStatus,
    HuddleTranscriptionProgressResponse, MeetingBotJob, MeetingBotJobCreateRequest,
    MeetingNoteCreateGoalsRequest, MeetingNoteCreateGoalsResponse, MeetingNotePostMinutesRequest,
    MeetingNotePostMinutesResponse, MeetingNoteSuggestGoalsRequest,
    MeetingNoteSuggestGoalsResponse, MeetingNoteSummarizeRequest, MeetingNoteSummarizeResponse,
    MeetingNoteTranscribeResponse, Minute, MinuteCreateRequest, MinuteListResponse,
    MinuteUpdateRequest,
};

/// Query filters for `GET /api/v2/objectives/:id/huddle/inviteable-members`.
#[derive(Default)]
pub struct HuddleInviteableMembersParams<'a> {
    pub page: Option<u32>,
    pub page_size: Option<u32>,
    pub query: Option<&'a str>,
    pub sort_by: Option<&'a str>,
    pub sort_dir: Option<&'a str>,
}

fn huddle_inviteable_members_query_suffix(params: &HuddleInviteableMembersParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(page) = params.page {
            query.append_pair("page", &page.to_string());
        }
        if let Some(page_size) = params.page_size {
            query.append_pair("pageSize", &page_size.to_string());
        }
        if let Some(q) = params.query {
            query.append_pair("query", q);
        }
        if let Some(sort_by) = params.sort_by {
            query.append_pair("sortBy", sort_by);
        }
        if let Some(sort_dir) = params.sort_dir {
            query.append_pair("sortDir", sort_dir);
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

/// Query filters for `GET /api/v2/minutes`.
#[derive(Default)]
pub struct MinuteListParams<'a> {
    pub objective_id: Option<&'a str>,
    pub source_type: Option<&'a str>,
    pub only_unlinked: bool,
}

fn minute_list_query_suffix(params: &MinuteListParams<'_>) -> String {
    let query = {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(objective_id) = params.objective_id {
            query.append_pair("objectiveId", objective_id);
        }
        if let Some(source_type) = params.source_type {
            query.append_pair("sourceType", source_type);
        }
        if params.only_unlinked {
            query.append_pair("onlyUnlinked", "true");
        }
        query.finish()
    };
    if query.is_empty() {
        String::new()
    } else {
        format!("?{query}")
    }
}

impl ApiClient {
    // -- Huddle ----------------------------------------------------------------

    /// GET /api/v2/objectives/:id/huddle
    pub async fn get_huddle_status(&self, objective_id: &str) -> Result<HuddleStatus> {
        let path = format!("/api/v2/objectives/{objective_id}/huddle");
        let resp: ApiResponse<HuddleStatus> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/objectives/:id/huddle/active-subtree
    pub async fn get_huddle_active_subtree(
        &self,
        objective_id: &str,
    ) -> Result<ActiveHuddlesResponse> {
        let path = format!("/api/v2/objectives/{objective_id}/huddle/active-subtree");
        let resp: ApiResponse<ActiveHuddlesResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/objectives/:id/huddle/sessions/:sessionId
    pub async fn get_huddle_session_status(
        &self,
        objective_id: &str,
        session_id: &str,
    ) -> Result<HuddleStatus> {
        let path = format!("/api/v2/objectives/{objective_id}/huddle/sessions/{session_id}");
        let resp: ApiResponse<HuddleStatus> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// POST /api/v2/objectives/:id/huddle/recording/start
    pub async fn start_huddle_recording(
        &self,
        objective_id: &str,
        req: &HuddleRecordingStartRequest,
    ) -> Result<HuddleRecordingStartResponse> {
        let path = format!("/api/v2/objectives/{objective_id}/huddle/recording/start");
        self.post(&path, req).await
    }

    /// POST /api/v2/objectives/:id/huddle/recording/stop
    pub async fn stop_huddle_recording(
        &self,
        objective_id: &str,
    ) -> Result<HuddleRecordingStopResponse> {
        let path = format!("/api/v2/objectives/{objective_id}/huddle/recording/stop");
        self.post_empty(&path).await
    }

    /// GET /api/v2/objectives/:id/huddle/inviteable-members
    pub async fn list_huddle_inviteable_members(
        &self,
        objective_id: &str,
        params: &HuddleInviteableMembersParams<'_>,
    ) -> Result<HuddleInviteableMembersResponse> {
        let path = format!(
            "/api/v2/objectives/{objective_id}/huddle/inviteable-members{}",
            huddle_inviteable_members_query_suffix(params)
        );
        let resp: ApiResponse<HuddleInviteableMembersResponse> = self.get(&path).await?;
        Ok(resp.data)
    }

    /// GET /api/v2/huddle/active — 204 when the caller isn't currently in a huddle.
    pub async fn get_huddle_active(&self) -> Result<Option<HuddleActive>> {
        let (url, req) = self.request(reqwest::Method::GET, "/api/v2/huddle/active", true)?;
        let response = Self::send_request(req, &url).await?;
        if response.status() == reqwest::StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }
        let resp: ApiResponse<HuddleActive> = response
            .json()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))?;
        Ok(Some(resp.data))
    }

    /// GET /api/v2/huddle/transcription-progress
    pub async fn get_huddle_transcription_progress(
        &self,
    ) -> Result<HuddleTranscriptionProgressResponse> {
        let resp: ApiResponse<HuddleTranscriptionProgressResponse> =
            self.get("/api/v2/huddle/transcription-progress").await?;
        Ok(resp.data)
    }

    /// POST /api/v2/huddle/sessions/:sessionId/invitations
    pub async fn send_huddle_invitations(
        &self,
        session_id: &str,
        req: &HuddleInvitationSendRequest,
    ) -> Result<HuddleInvitationSendResponse> {
        let path = format!("/api/v2/huddle/sessions/{session_id}/invitations");
        self.post(&path, req).await
    }

    // -- Meeting Bot（Recall.ai） ------------------------------------------------

    /// GET /api/v1/team/meeting-bot/jobs
    pub async fn list_meeting_bot_jobs(&self) -> Result<Vec<MeetingBotJob>> {
        self.get("/api/v1/team/meeting-bot/jobs").await
    }

    /// GET /api/v1/team/meeting-bot/jobs/:id
    pub async fn get_meeting_bot_job(&self, id: &str) -> Result<MeetingBotJob> {
        let path = format!("/api/v1/team/meeting-bot/jobs/{id}");
        self.get(&path).await
    }

    /// POST /api/v1/team/meeting-bot/jobs
    pub async fn create_meeting_bot_job(
        &self,
        req: &MeetingBotJobCreateRequest,
    ) -> Result<MeetingBotJob> {
        self.post("/api/v1/team/meeting-bot/jobs", req).await
    }

    /// DELETE /api/v1/team/meeting-bot/jobs/:id
    pub async fn delete_meeting_bot_job(&self, id: &str) -> Result<()> {
        let path = format!("/api/v1/team/meeting-bot/jobs/{id}");
        self.delete_no_body(&path).await
    }

    // -- Meeting Note（文字起こし/要約/議事録投稿/ゴール提案・作成） -----------------
    //
    // Unlike most `/api/v2/...` endpoints these are NOT wrapped in
    // `{"data": ...}`: `presentation/handlers/meeting_note/*.go` responds via
    // `Handler.RespondWithJSON`, which is a thin `c.JSON(code, payload)` with
    // no envelope (confirmed against `presentation/handlers/base_handler.go`).

    /// POST /api/v2/meeting-notes/transcribe (multipart `audio` field).
    pub async fn transcribe_meeting_note(
        &self,
        file_bytes: Vec<u8>,
        file_name: &str,
        content_type: &str,
    ) -> Result<MeetingNoteTranscribeResponse> {
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str(content_type)
            .context("Invalid content type for audio upload")?;
        let form = reqwest::multipart::Form::new().part("audio", part);
        self.post_multipart("/api/v2/meeting-notes/transcribe", form)
            .await
    }

    /// POST /api/v2/meeting-notes/summarize
    pub async fn summarize_meeting_note(
        &self,
        req: &MeetingNoteSummarizeRequest,
    ) -> Result<MeetingNoteSummarizeResponse> {
        self.post("/api/v2/meeting-notes/summarize", req).await
    }

    /// POST /api/v2/meeting-notes/post-minutes
    pub async fn post_meeting_note_minutes(
        &self,
        req: &MeetingNotePostMinutesRequest,
    ) -> Result<MeetingNotePostMinutesResponse> {
        self.post("/api/v2/meeting-notes/post-minutes", req).await
    }

    /// POST /api/v2/meeting-notes/suggest-goals
    pub async fn suggest_meeting_note_goals(
        &self,
        req: &MeetingNoteSuggestGoalsRequest,
    ) -> Result<MeetingNoteSuggestGoalsResponse> {
        self.post("/api/v2/meeting-notes/suggest-goals", req).await
    }

    /// POST /api/v2/meeting-notes/create-goals
    pub async fn create_meeting_note_goals(
        &self,
        req: &MeetingNoteCreateGoalsRequest,
    ) -> Result<MeetingNoteCreateGoalsResponse> {
        self.post("/api/v2/meeting-notes/create-goals", req).await
    }

    // -- 議事録（Minutes）CRUD ----------------------------------------------------
    //
    // Same handler package/response helper as Meeting Note above: bare JSON,
    // no `{"data": ...}` envelope.

    /// POST /api/v2/minutes
    pub async fn create_minute(&self, req: &MinuteCreateRequest) -> Result<Minute> {
        self.post("/api/v2/minutes", req).await
    }

    /// GET /api/v2/minutes
    pub async fn list_minutes(&self, params: &MinuteListParams<'_>) -> Result<MinuteListResponse> {
        let path = format!("/api/v2/minutes{}", minute_list_query_suffix(params));
        self.get(&path).await
    }

    /// GET /api/v2/minutes/:id
    pub async fn get_minute(&self, id: &str) -> Result<Minute> {
        let path = format!("/api/v2/minutes/{id}");
        self.get(&path).await
    }

    /// PATCH /api/v2/minutes/:id
    pub async fn update_minute(&self, id: &str, req: &MinuteUpdateRequest) -> Result<Minute> {
        let path = format!("/api/v2/minutes/{id}");
        self.patch(&path, req).await
    }

    /// DELETE /api/v2/minutes/:id
    pub async fn delete_minute(&self, id: &str) -> Result<()> {
        let path = format!("/api/v2/minutes/{id}");
        self.delete_no_body(&path).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        HuddleInviteableMembersParams, MinuteListParams, huddle_inviteable_members_query_suffix,
        minute_list_query_suffix,
    };

    #[test]
    fn huddle_inviteable_members_query_suffix_is_empty_without_params() {
        assert_eq!(
            huddle_inviteable_members_query_suffix(&HuddleInviteableMembersParams::default()),
            ""
        );
    }

    #[test]
    fn huddle_inviteable_members_query_suffix_encodes_all_params() {
        let suffix = huddle_inviteable_members_query_suffix(&HuddleInviteableMembersParams {
            page: Some(2),
            page_size: Some(20),
            query: Some("alice"),
            sort_by: Some("name"),
            sort_dir: Some("asc"),
        });
        assert_eq!(
            suffix,
            "?page=2&pageSize=20&query=alice&sortBy=name&sortDir=asc"
        );
    }

    #[test]
    fn minute_list_query_suffix_is_empty_without_params() {
        assert_eq!(minute_list_query_suffix(&MinuteListParams::default()), "");
    }

    #[test]
    fn minute_list_query_suffix_encodes_all_params() {
        let suffix = minute_list_query_suffix(&MinuteListParams {
            objective_id: Some("obj-1"),
            source_type: Some("zoom"),
            only_unlinked: true,
        });
        assert_eq!(
            suffix,
            "?objectiveId=obj-1&sourceType=zoom&onlyUnlinked=true"
        );
    }
}
