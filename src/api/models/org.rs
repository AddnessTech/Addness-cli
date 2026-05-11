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
