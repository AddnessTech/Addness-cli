use anyhow::Result;

use crate::api::{ApiClient, ApiResponse, ConsentStatus};

impl ApiClient {
    /// GET /api/v2/me/consents/:consentType
    ///
    /// Requires a `personal`-scope API key (or a Clerk session); see
    /// `RequireApiKeyScope(models.ApiKeyScopePersonal)` on this route in
    /// `presentation/routes/api.go`. Always returns 200 — an unrecorded (or
    /// superseded-version) consent comes back as `agreed: false` rather than
    /// 404, so callers don't need to special-case "not found".
    ///
    /// There is no corresponding `set`/`record` method: `POST
    /// /api/v2/me/consents` is Clerk-only on the backend and rejects API key
    /// auth outright (see `ConsentStatus` doc comment), so the CLI cannot
    /// call it.
    pub async fn get_consent(&self, consent_type: &str) -> Result<ConsentStatus> {
        let path = format!("/api/v2/me/consents/{consent_type}");
        let resp: ApiResponse<ConsentStatus> = self.get(&path).await?;
        Ok(resp.data)
    }
}
