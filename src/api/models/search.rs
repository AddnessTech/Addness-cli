use serde::{Deserialize, Serialize};
use serde_json::Value;

// Unified search API models (GET /api/v2/search).
//
// The response is not envelope-wrapped (no `{"data": ...}` wrapper) — the
// wire body is the raw `{"items": [...], "hasMore": bool}` object.
// Each item's `data` shape depends on `type` (objective/comment/member), so
// it is surfaced as raw JSON rather than a fully-typed union, mirroring how
// `preview_issue_messages` handles polymorphic goal-issue payloads.
// Backend reference: application/resources/search_resource.go.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponse {
    #[serde(default)]
    pub items: Vec<SearchResultItem>,
    #[serde(default)]
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    #[serde(rename = "type")]
    pub kind: String,
    pub data: Value,
}
