use serde::{Deserialize, Serialize};

// User consent state (internal/userconsent). Currently gates a user's consent
// to admins viewing their own DM/group chat messages ("telecommunications
// secrecy" disclosure under Japanese law), tracked per consent type per user
// (not per chat room, and independent of organization). Backend reference:
// internal/userconsent/{handler,usecase}/*.go,
// internal/userconsent/dto/payload.go `ConsentRes`.
//
// Only GET is exposed via the CLI. `POST /api/v2/me/consents` (record/update)
// is intentionally Clerk-only on the backend — API-key-authenticated
// recording of legal consent on a user's behalf is forbidden by design (see
// presentation/routes/api_consent_route_policy_test.go
// `TestPostConsentsRouteRequiresClerkAuth`), so there is no way for the CLI's
// API-key auth to call it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConsentStatus {
    pub consent_type: String,
    /// `false` (with the requested `consent_type` echoed back) when the user
    /// has never recorded consent, or recorded it against a since-superseded
    /// version — the backend always returns 200, never 404, for this case.
    pub agreed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agreed_at: Option<String>,
    /// Empty when unagreed; otherwise the (possibly superseded) version the
    /// user actually agreed to, kept for audit purposes.
    #[serde(default)]
    pub version: String,
}
