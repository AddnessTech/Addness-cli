use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Tool CRUD / search / execution — presentation/handlers/ai/tool_handler.go
// (org-scoped, still-live "v1 AI layer" endpoints, admin/internal 対象外)
//
// All responses are wrapped in `{"data": ...}` (confirmed against
// `tool_handler.go`, which calls `c.JSON(code, gin.H{"data": ...})` directly),
// except the 204 No Content response on delete.
// ---------------------------------------------------------------------------

/// `executor` accepted by `POST/PATCH /api/v1/team/organizations/:id/tools[/:toolID]`,
/// confirmed against `domain/models/ai/tool.go` (`ToolExecutorBash/API/Python/Function`)
/// and `application/ai/executors/executor.go` (`ExecutorFactory.GetExecutor` only
/// recognizes these four keys).
#[derive(Clone, Copy, Debug, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutor {
    Bash,
    Api,
    Python,
    Function,
}

/// `type` field of a parameter definition ('string'/'number'/'boolean'/'array'/'object',
/// per `domain/models/ai/tool.go` `ParameterType*` constants).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameterRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub required: bool,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#enum: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParameter {
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default)]
    pub required: bool,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#enum: Option<Vec<String>>,
}

/// `POST /api/v1/team/organizations/:id/tools`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCreateRequest {
    pub name: String,
    pub description: String,
    pub executor: ToolExecutor,
    pub executor_config: Map<String, Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ToolParameterRequest>,
    pub requires_confirmation: bool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_environments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_execution_time: Option<i64>,
    pub is_public: bool,
}

/// `PATCH /api/v1/team/organizations/:id/tools/:toolID` (also reachable via
/// `PUT`; both route to the same handler). Note `executor` itself cannot be
/// changed after creation — only `UpdateToolRequest` fields below are settable.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executor_config: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Vec<ToolParameterRequest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_confirmation: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_environments: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_execution_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_public: Option<bool>,
}

/// `POST /api/v1/team/organizations/:id/tools/:toolID/execute`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecuteRequest {
    pub parameters: Map<String, Value>,
    #[serde(skip_serializing_if = "str::is_empty")]
    pub environment: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub executor: String,
    /// `null` for built-in `system`-type tools (e.g. `system:create_goals`),
    /// confirmed against production data — `ExecutorConfig` has no `omitempty`
    /// on the Go side, so a nil map marshals as JSON `null` rather than `{}`.
    #[serde(default)]
    pub executor_config: Option<Map<String, Value>>,
    #[serde(default)]
    pub parameters: Vec<ToolParameter>,
    #[serde(default)]
    pub requires_confirmation: bool,
    #[serde(default)]
    pub allowed_environments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_execution_time: Option<i64>,
    pub organization_id: String,
    pub creator_id: String,
    #[serde(default)]
    pub is_public: bool,
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
    #[serde(default)]
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// `GET /api/v1/team/organizations/:id/tools`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolListResponse {
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub total: i64,
}

/// `GET /api/v1/team/organizations/:id/tools/search`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchResponse {
    #[serde(default)]
    pub tools: Vec<Tool>,
}

/// `POST /api/v1/team/organizations/:id/tools/:toolID/execute`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecuteResponse {
    #[serde(default)]
    pub execution_id: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub result: Option<Map<String, Value>>,
    #[serde(default)]
    pub execution_time_ms: i64,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub error: String,
}
