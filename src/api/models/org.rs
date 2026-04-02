use serde::{Deserialize, Serialize};

// GET /api/v1/team/organizations/my_organizations
// Response: { "data": [ { "id": "...", "name": "...", ... } ] }
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
pub struct OrganizationsResponse {
    pub data: Vec<Organization>,
}
