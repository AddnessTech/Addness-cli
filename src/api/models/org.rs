use serde::{Deserialize, Serialize};

// GET /api/v2/organizations/me
// Response: { "data": { "organizations": [ { "id": "...", "name": "...", ... } ] }, "message": "success" }
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub plan_type: Option<String>,
    #[serde(default)]
    pub context_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationsData {
    pub organizations: Vec<Organization>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationsResponse {
    pub data: OrganizationsData,
}

// POST /api/v1/team/organizations
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOrganizationRequest {
    pub name: String,
    /// PERSONAL or BUSINESS. Backend requires this field.
    #[serde(rename = "type")]
    pub organization_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_type: Option<String>,
    /// Required when organization_type is BUSINESS. One of SOLO, 2_5, 6_20, 21_50, 50_PLUS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_scale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub industry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_timezone: Option<String>,
}

// PUT/PATCH /api/v2/organizations/:id
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOrganizationRequest {
    pub name: String,
}

// PATCH /api/v2/organizations/:id/context
#[derive(Debug, Serialize)]
pub struct UpdateContextRequest {
    pub context_text: String,
}
