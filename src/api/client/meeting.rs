use anyhow::Result;

use crate::api::{
    ActiveHuddlesResponse, ApiClient, ApiResponse, HuddleInvitationSendRequest,
    HuddleInvitationSendResponse, HuddleInviteableMembersResponse, HuddleRecordingStartRequest,
    HuddleRecordingStartResponse, HuddleRecordingStopResponse, HuddleStatus, MeetingBotJob,
    MeetingBotJobCreateRequest,
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

    /// POST /api/v1/team/meeting-bot/jobs/:id/stop
    pub async fn stop_meeting_bot_job(&self, id: &str) -> Result<()> {
        let path = format!("/api/v1/team/meeting-bot/jobs/{id}/stop");
        self.post_no_content(&path, &serde_json::json!({})).await
    }
}

#[cfg(test)]
mod tests {
    use super::{HuddleInviteableMembersParams, huddle_inviteable_members_query_suffix};

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
}
