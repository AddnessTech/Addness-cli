use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAuthRedeemRequest {
    pub start_token: String,
    pub browser_nonce_hash: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DesktopAuthRedeemResponse {
    pub intent_id: String,
    pub expires_at: String,
    #[serde(default)]
    pub auth_path: Option<String>,
    #[serde(default)]
    pub referral_code: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAuthCompleteRequest {
    pub browser_nonce: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DesktopAuthCompleteResponse {
    pub handoff_id: String,
    pub state: String,
    pub port: String,
}
