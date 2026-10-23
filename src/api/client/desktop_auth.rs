use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, DesktopAuthCompleteRequest, DesktopAuthCompleteResponse,
    DesktopAuthRedeemRequest, DesktopAuthRedeemResponse,
};

impl ApiClient {
    /// POST /api/v1/public/desktop/auth/start-sessions/redeem
    pub async fn redeem_desktop_auth_start_session(
        &self,
        req: &DesktopAuthRedeemRequest,
    ) -> Result<DesktopAuthRedeemResponse> {
        let resp: ApiResponse<DesktopAuthRedeemResponse> = self
            .post_without_org("/api/v1/public/desktop/auth/start-sessions/redeem", req)
            .await?;
        Ok(resp.data)
    }

    /// POST /api/v1/team/desktop/auth/intents/:id/complete
    pub async fn complete_desktop_auth_intent(
        &self,
        intent_id: &str,
        req: &DesktopAuthCompleteRequest,
    ) -> Result<DesktopAuthCompleteResponse> {
        let path = format!("/api/v1/team/desktop/auth/intents/{intent_id}/complete");
        let resp: ApiResponse<DesktopAuthCompleteResponse> = self.post(&path, req).await?;
        Ok(resp.data)
    }
}
