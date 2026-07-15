use serde::{Deserialize, Serialize};

// Personal space API models (internal/personal — "now" document, daily
// entries, agent sessions, projects, markdown/text-patch editing, and the
// personal-organization ensure endpoint used by Chat/Perfect Days billing).
// Backend reference: internal/personal/{handler,usecase}/*.go and
// presentation/handlers/team/personal_organization_handler.go.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalNow {
    pub user_id: String,
    pub body: String,
    pub body_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDailyEntry {
    pub id: String,
    pub user_id: String,
    pub local_date: String,
    pub timezone: String,
    pub body: String,
    pub body_hash: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonalTodayAppendRequest {
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalAgentSession {
    pub id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
    pub title: String,
    pub status: String,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonalAgentSessionCreateRequest {
    pub title: String,
    pub status: String,
    pub body: String,
    #[serde(rename = "objectiveId", skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PersonalAgentSessionUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    #[serde(rename = "objectiveId", skip_serializing_if = "Option::is_none")]
    pub objective_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalProject {
    pub id: String,
    pub user_id: String,
    pub title: String,
    pub content: String,
    pub content_hash: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PersonalProjectCreateRequest {
    pub title: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct PersonalProjectUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Shared request body for `POST /api/v2/personal/text-patch`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PersonalTextPatchRequest {
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    pub start: i64,
    pub end: i64,
    pub text: String,
    #[serde(rename = "baseHash")]
    pub base_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalTextRange {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalDiffLine {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_line: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_line: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalUserVisibleChange {
    #[serde(rename = "type")]
    pub kind: String,
    pub label: String,
    pub path: String,
    pub display_name: String,
    #[serde(rename = "kind")]
    pub change_kind: String,
    pub before: String,
    pub after: String,
    pub before_context: String,
    pub after_context: String,
    pub before_start_line: i64,
    pub after_start_line: i64,
    #[serde(default)]
    pub lines: Vec<PersonalDiffLine>,
    pub range: PersonalTextRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalTextPatchResponse {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub label: String,
    pub dry_run: bool,
    pub changed: bool,
    #[serde(default)]
    pub changes: Vec<PersonalUserVisibleChange>,
    pub base_hash: String,
    pub after_hash: String,
    pub current_length: i64,
    pub after_length: i64,
}

/// Shared request body for the five `POST /api/v2/personal/markdown/*` edit
/// endpoints (replace-section/upsert-section/upsert-list-item/
/// replace-document/append-log-entry). The backend binds all fields onto one
/// struct regardless of which fields a given edit kind actually uses.
#[derive(Debug, Clone, Serialize, Default)]
pub struct PersonalMarkdownEditRequest {
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(rename = "baseHash")]
    pub base_hash: String,
    #[serde(rename = "headingPath", skip_serializing_if = "Vec::is_empty")]
    pub heading_path: Vec<String>,
    #[serde(rename = "contentMarkdown", skip_serializing_if = "Option::is_none")]
    pub content_markdown: Option<String>,
    #[serde(rename = "bodyMarkdown", skip_serializing_if = "Option::is_none")]
    pub body_markdown: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(rename = "itemMarkdown", skip_serializing_if = "Option::is_none")]
    pub item_markdown: Option<String>,
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownRange {
    pub start: i64,
    pub end: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownWarning {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub heading_path: Vec<String>,
    #[serde(default)]
    pub range: Option<PersonalMarkdownRange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownListItem {
    pub id: String,
    #[serde(default)]
    pub section_id: String,
    #[serde(default)]
    pub heading_path: Vec<String>,
    #[serde(default)]
    pub key: String,
    pub text: String,
    pub markdown: String,
    pub range: PersonalMarkdownRange,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownSection {
    pub id: String,
    pub level: i64,
    pub heading: String,
    pub heading_path: Vec<String>,
    pub range: PersonalMarkdownRange,
    pub heading_range: PersonalMarkdownRange,
    pub body_range: PersonalMarkdownRange,
    pub body_markdown: String,
    #[serde(default)]
    pub list_items: Vec<PersonalMarkdownListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownAnalysis {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub sections: Vec<PersonalMarkdownSection>,
    #[serde(default)]
    pub list_items: Vec<PersonalMarkdownListItem>,
    #[serde(default)]
    pub warnings: Vec<PersonalMarkdownWarning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownAnalyzeResponse {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub label: String,
    pub hash: String,
    pub analysis: PersonalMarkdownAnalysis,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownSemanticChange {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub heading_path: Vec<String>,
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalMarkdownEditResponse {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    pub label: String,
    pub dry_run: bool,
    pub changed: bool,
    #[serde(default)]
    pub changes: Vec<PersonalUserVisibleChange>,
    #[serde(default)]
    pub semantic_changes: Vec<PersonalMarkdownSemanticChange>,
    #[serde(default)]
    pub warnings: Vec<PersonalMarkdownWarning>,
    pub base_hash: String,
    pub after_hash: String,
    pub current_length: i64,
    pub after_length: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalResetResponse {
    #[serde(default)]
    pub ok: bool,
}

/// POST /api/v1/team/personal-organization/ensure response. `balance` uses
/// snake_case in the wire format (see PersonalOrganizationHandler in the Go
/// backend), unlike the rest of the v2 `/personal` resources above.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalOrganizationBalance {
    pub balance_micro_usd: i64,
    pub cap_micro_usd: i64,
    pub balance_yen: i64,
    pub cap_yen: i64,
    pub remaining_percentage: f64,
    pub is_special: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonalOrganizationEnsureResponse {
    pub organization_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub balance: Option<PersonalOrganizationBalance>,
}
