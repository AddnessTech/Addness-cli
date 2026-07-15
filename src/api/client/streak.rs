use crate::api::{
    ApiClient, ApiResponse, FreezeResult, PublicStreakResponse, ReviveResult, Streak,
    StreakShareLink, StreakShareStatus,
};
use anyhow::Result;

impl ApiClient {
    pub async fn get_streak(
        &self,
        org_id: &str,
        member_id: &str,
        days: Option<u32>,
    ) -> Result<Streak> {
        let suffix = match days {
            Some(days) => format!("?days={days}"),
            None => String::new(),
        };
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak{suffix}");
        let resp: ApiResponse<Streak> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn get_streak_share_status(
        &self,
        org_id: &str,
        member_id: &str,
    ) -> Result<StreakShareStatus> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak/share");
        let resp: ApiResponse<StreakShareStatus> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn create_streak_share_link(
        &self,
        org_id: &str,
        member_id: &str,
    ) -> Result<StreakShareLink> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak/share");
        let resp: ApiResponse<StreakShareLink> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    pub async fn revoke_streak_share_link(&self, org_id: &str, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak/share");
        self.delete_no_body(&path).await
    }

    pub async fn freeze_streak(&self, org_id: &str, member_id: &str) -> Result<FreezeResult> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak/freeze");
        let resp: ApiResponse<FreezeResult> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    pub async fn unfreeze_streak(&self, org_id: &str, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak/freeze");
        self.delete_no_body(&path).await
    }

    pub async fn revive_streak(&self, org_id: &str, member_id: &str) -> Result<ReviveResult> {
        let path = format!("/api/v2/organizations/{org_id}/members/{member_id}/streak/revive");
        let resp: ApiResponse<ReviveResult> = self.post_empty(&path).await?;
        Ok(resp.data)
    }

    /// 共有トークンによる公開ストリーク取得。認証不要のエンドポイントだが、
    /// 他コマンドと同じ認証済みクライアント経由で呼び出しても問題ない
    /// （サーバー側はこのルートに認証ミドルウェアを適用していない）。
    pub async fn get_public_streak(&self, token: &str) -> Result<PublicStreakResponse> {
        let path = format!("/api/v1/public/streaks/{token}");
        self.get_without_org(&path).await
    }
}
