use crate::api::{
    ApiClient, ApiResponse, User, UserCreateRequest, UserListResponse, UserOrganizationMember,
    UserSetting, UserSettingUpdateRequest, UserUpdateRequest,
};
use anyhow::Result;

#[derive(Default)]
pub struct ListUsersParams<'a> {
    pub name: Option<&'a str>,
    pub email: Option<&'a str>,
    pub limit: Option<u16>,
    pub offset: Option<u64>,
}

impl ApiClient {
    pub async fn get_current_user(&self) -> Result<User> {
        let resp: ApiResponse<User> = self.get("/api/v1/team/users/current").await?;
        Ok(resp.data)
    }

    pub async fn update_user(&self, id: &str, req: &UserUpdateRequest) -> Result<User> {
        let path = format!("/api/v1/team/users/{id}");
        let resp: ApiResponse<User> = self.put(&path, req).await?;
        Ok(resp.data)
    }

    pub async fn get_user_settings(&self) -> Result<UserSetting> {
        let resp: ApiResponse<UserSetting> = self.get("/api/v1/team/user_settings").await?;
        Ok(resp.data)
    }

    pub async fn update_user_settings(
        &self,
        req: &UserSettingUpdateRequest,
    ) -> Result<UserSetting> {
        let resp: ApiResponse<UserSetting> = self.patch("/api/v1/team/user_settings", req).await?;
        Ok(resp.data)
    }

    pub async fn list_user_organization_memberships(&self) -> Result<Vec<UserOrganizationMember>> {
        let resp: ApiResponse<Vec<UserOrganizationMember>> =
            self.get("/api/v1/team/organization_members").await?;
        Ok(resp.data)
    }

    pub async fn list_users(&self, params: ListUsersParams<'_>) -> Result<UserListResponse> {
        // Serializer は非Sendなので、ブロック内で文字列に確定させて drop する（notification.rsの流儀に合わせる）。
        let query = {
            let mut query = form_urlencoded::Serializer::new(String::new());
            if let Some(name) = params.name {
                query.append_pair("name", name);
            }
            if let Some(email) = params.email {
                query.append_pair("email", email);
            }
            if let Some(limit) = params.limit {
                query.append_pair("limit", &limit.to_string());
            }
            if let Some(offset) = params.offset {
                query.append_pair("offset", &offset.to_string());
            }
            query.finish()
        };
        let suffix = if query.is_empty() {
            String::new()
        } else {
            format!("?{query}")
        };
        let path = format!("/api/v1/team/users{suffix}");
        self.get(&path).await
    }

    pub async fn get_user(&self, id: &str) -> Result<User> {
        let path = format!("/api/v1/team/users/{id}");
        let resp: ApiResponse<User> = self.get(&path).await?;
        Ok(resp.data)
    }

    pub async fn create_user(&self, req: &UserCreateRequest) -> Result<User> {
        let resp: ApiResponse<User> = self.post("/api/v1/team/users", req).await?;
        Ok(resp.data)
    }

    pub async fn delete_user(&self, id: &str) -> Result<()> {
        let path = format!("/api/v1/team/users/{id}");
        self.delete_no_body(&path).await
    }
}
