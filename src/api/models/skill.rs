use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Skill / Skill Resource / Skill Refinement
// — presentation/handlers/ai/{skill_handler,skill_resource_handler}.go
//   (org-scoped, still-live "v1 AI layer" endpoints, admin/internal 対象外)
//
// All responses are wrapped in `{"data": ...}` (confirmed against
// `skill_handler.go` / `skill_resource_handler.go`, which call
// `c.JSON(code, gin.H{"data": ...})` directly rather than a shared
// response helper), except the 204 No Content responses on delete/reject.
// ---------------------------------------------------------------------------

/// `POST /api/v1/team/organizations/:id/skills`
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillCreateRequest {
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub thinking_steps: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub preferred_tools: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_execution_order: Option<String>,
    pub requires_confirmation: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Map<String, Value>>,
}

/// `PATCH /api/v1/team/organizations/:id/skills/:skillID` (also reachable via
/// `PUT`; both route to the same handler — all fields are optional partial
/// updates).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_steps: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_tools: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_execution_order: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_confirmation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    #[serde(default)]
    pub is_general: bool,
    #[serde(default)]
    pub thinking_steps: Vec<String>,
    #[serde(default)]
    pub preferred_tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_execution_order: Option<String>,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub examples: Option<Map<String, Value>>,
    pub organization_id: String,
    pub creator_id: String,
    #[serde(default)]
    pub usage_count: i64,
    #[serde(default)]
    pub success_count: i64,
    #[serde(default)]
    pub failure_count: i64,
    #[serde(default)]
    pub success_rate: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refined_at: Option<String>,
    #[serde(default)]
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// `GET /api/v1/team/organizations/:id/skills` and
/// `GET /api/v1/team/organizations/:id/general-skills`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillListResponse {
    #[serde(default)]
    pub skills: Vec<Skill>,
    #[serde(default)]
    pub total: i64,
}

/// `GET /api/v1/team/organizations/:id/skills/search`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillSearchResponse {
    #[serde(default)]
    pub skills: Vec<Skill>,
}

/// `GET /api/v1/team/organizations/:id/skills/:skillID/performance`
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillPerformance {
    pub skill_id: String,
    pub skill_name: String,
    #[serde(default)]
    pub usage_count: i64,
    #[serde(default)]
    pub success_count: i64,
    #[serde(default)]
    pub failure_count: i64,
    #[serde(default)]
    pub success_rate: f64,
    #[serde(default)]
    pub version: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refined_at: Option<String>,
}

/// `POST /api/v1/team/organizations/:id/skills/:skillID/resources`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillResourceCreateRequest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    pub content: String,
}

/// `PUT /api/v1/team/organizations/:id/skills/:skillID/resources/:resourceID`
/// (there is no PATCH route for resource updates — confirmed against
/// `presentation/routes/api.go`, only `putJSON` is registered).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillResourceUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillResource {
    pub id: String,
    pub skill_id: String,
    pub name: String,
    pub content_type: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

/// List item shape returned by `ListResources` (omits `content` — lightweight).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillResourceListItem {
    pub id: String,
    pub skill_id: String,
    pub name: String,
    pub content_type: String,
    pub created_at: String,
    pub updated_at: String,
}
