use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

// Diagnosis result API models (internal/diagnosis).
//
// Unlike most v2 resources this module's wire format is snake_case (not
// camelCase), so field names below map 1:1 without `rename_all`.
// Backend reference: internal/diagnosis/handler/endpoints/*.go.

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosisSaveRequest {
    pub diagnosis_kind: String,
    pub schema_version: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisSaveData {
    pub id: String,
    pub diagnosis_kind: String,
    pub schema_version: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResultSummary {
    pub id: String,
    pub diagnosis_kind: String,
    #[serde(default)]
    pub schema_version: String,
    #[serde(default)]
    pub result: Value,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisMyResultsData {
    #[serde(default)]
    pub results: Vec<DiagnosisResultSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisResultByKindData {
    pub result: DiagnosisResultSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisStatsBucket {
    pub type_code: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisStats {
    pub diagnosis_kind: String,
    pub total: i64,
    #[serde(default)]
    pub distribution: Vec<DiagnosisStatsBucket>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosisVisibilityRequest {
    pub visibilities: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisVisibility {
    #[serde(default)]
    pub default_public: bool,
    #[serde(default)]
    pub visibilities: HashMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisMemberResult {
    pub diagnosis_kind: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisMemberProfile {
    pub member_id: String,
    #[serde(default)]
    pub results: Vec<DiagnosisMemberResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosisMemberProfilesData {
    #[serde(default)]
    pub profiles: Vec<DiagnosisMemberProfile>,
}
