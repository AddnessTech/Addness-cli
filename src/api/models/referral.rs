use serde::{Deserialize, Serialize};

// Referral API models (internal/referral).
//
// The referral handlers respond with raw JSON (no `{"data": ...}` envelope,
// unlike most other internal/* modules — see `h.JSONRaw` in the backend).
// Backend reference: internal/referral/dto/referral_res.go.

#[derive(Debug, Clone, Serialize)]
pub struct ReferralLinkRequest {
    pub channel: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralLink {
    pub id: String,
    #[serde(rename = "referralCode")]
    pub referral_code: String,
    #[serde(rename = "shareUrl")]
    pub share_url: String,
    pub channel: String,
    #[serde(rename = "snapshotPlanTokenLimit")]
    pub snapshot_plan_token_limit: i64,
    #[serde(rename = "expiresAt", skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralConversion {
    pub id: String,
    #[serde(rename = "referralCode", default)]
    pub referral_code: String,
    #[serde(default)]
    pub status: String,
    #[serde(rename = "createdAt", default)]
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralSummary {
    #[serde(rename = "totalLinks", default)]
    pub total_links: i64,
    #[serde(rename = "totalConversions", default)]
    pub total_conversions: i64,
    #[serde(rename = "totalRewards", default)]
    pub total_rewards: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralMyList {
    pub summary: ReferralSummary,
    #[serde(default)]
    pub items: Vec<ReferralConversion>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReferralConvertRequest {
    #[serde(rename = "referralCode")]
    pub referral_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReferralConvertResult {
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}
