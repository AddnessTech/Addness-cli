use anyhow::Result;

use crate::api::{
    ApiClient, ReferralConvertRequest, ReferralConvertResult, ReferralLink, ReferralLinkRequest,
    ReferralMyList,
};

impl ApiClient {
    /// POST /api/v1/team/referrals/links
    /// Response is not envelope-wrapped (raw JSON on the wire).
    pub async fn create_referral_link(&self, channel: &str) -> Result<ReferralLink> {
        let body = ReferralLinkRequest {
            channel: channel.to_string(),
        };
        self.post("/api/v1/team/referrals/links", &body).await
    }

    /// GET /api/v1/team/referrals/me?limit=..&offset=..
    pub async fn list_my_referrals(
        &self,
        limit: Option<u16>,
        offset: Option<u16>,
    ) -> Result<ReferralMyList> {
        let mut query = form_urlencoded::Serializer::new(String::new());
        if let Some(limit) = limit {
            query.append_pair("limit", &limit.to_string());
        }
        if let Some(offset) = offset {
            query.append_pair("offset", &offset.to_string());
        }
        let suffix = query.finish();
        let path = if suffix.is_empty() {
            "/api/v1/team/referrals/me".to_string()
        } else {
            format!("/api/v1/team/referrals/me?{suffix}")
        };
        self.get(&path).await
    }

    /// POST /api/v1/team/referrals/conversions
    /// `signupUserId`/`signupIp` are ignored by the backend (overwritten
    /// server-side to the caller's own identity/IP), so only the referral
    /// code needs to be supplied here.
    pub async fn convert_referral_signup(
        &self,
        referral_code: &str,
    ) -> Result<ReferralConvertResult> {
        let body = ReferralConvertRequest {
            referral_code: referral_code.to_string(),
        };
        self.post("/api/v1/team/referrals/conversions", &body).await
    }
}
