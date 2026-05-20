use anyhow::Result;

use crate::api::{
    ApiClient, ApiResponse, MembersListData, PinMemberRequest, SetSourceOrganizationRequest,
    UpdateMemberRequest,
};

impl ApiClient {
    pub async fn get_members(&self, org_id: &str) -> Result<ApiResponse<MembersListData>> {
        let path = format!("/api/v2/organizations/{org_id}/members?pageSize=100");
        self.get(&path).await
    }

    pub async fn update_member(&self, member_id: &str, name: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}");
        let body = UpdateMemberRequest {
            name: name.to_string(),
        };
        self.put_no_content(&path, &body).await
    }

    pub async fn pin_member(&self, member_id: &str, pinned: bool) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/pin");
        let body = PinMemberRequest { pinned };
        self.put_no_content(&path, &body).await
    }

    pub async fn delete_member(&self, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}");
        self.delete_no_body(&path).await
    }

    pub async fn assign_admin(&self, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/admin");
        self.put_empty_no_content(&path).await
    }

    pub async fn revoke_admin(&self, member_id: &str) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/admin");
        self.delete_no_body(&path).await
    }

    pub async fn set_member_source_organization(
        &self,
        member_id: &str,
        source_org_id: &str,
    ) -> Result<()> {
        let path = format!("/api/v2/members/{member_id}/source-organization");
        let body = SetSourceOrganizationRequest {
            source_organization_id: source_org_id.to_string(),
        };
        self.patch_no_content(&path, &body).await
    }
}
