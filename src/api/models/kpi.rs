use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Kpi {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub objective_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub target_value: Option<i32>,
    #[serde(default)]
    pub actual_value: Option<i32>,
}

// POST /api/v2/objectives/:id/kpis
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateKpiRequest {
    pub title: String,
    pub unit: String,
    pub target_value: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<i32>,
}

// PATCH /api/v2/objective-kpis/:id
#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateKpiRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_value: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_value: Option<i32>,
}
