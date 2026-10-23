use crate::api::{
    ApiClient, ApiResponse, User, UserSetting, UserSettingUpdateRequest, UserUpdateRequest,
};
use anyhow::Result;

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

    pub async fn get_user(&self, id: &str) -> Result<User> {
        let path = format!("/api/v1/team/users/{id}");
        let resp: ApiResponse<User> = self.get(&path).await?;
        Ok(resp.data)
    }
}
