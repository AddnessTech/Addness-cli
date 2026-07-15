use serde::{Deserialize, Serialize};

// GET /api/v1/team/users/current
// GET /api/v1/team/users/:id
// PUT /api/v1/team/users/:id
// POST /api/v1/team/users
// Response: { "data": UserResource }
//
// バックエンドの都合上 `email` は常に空文字で返る（バックエンドの既知の仕様）。
// レスポンス契約の忠実性のためフィールドは残す。
// ネストされた `avatar` オブジェクトは意図的にモデル化しない
// （activity.rs / streak.rs と同様、未知フィールドはserdeが無視する）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub last_signed_in_at: Option<String>,
    #[serde(default)]
    pub gender: String,
    #[serde(default)]
    pub date_of_birth: Option<String>,
    #[serde(default)]
    pub discarded_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// PUT /api/v1/team/users/:id
//
// `dateOfBirth` はサーバー側で `*string`。キー省略/JSON nullは「変更しない」、
// 空文字 `""` は「NULLにクリア」を意味する。`Some(None)` は使わず、
// 代わりに `Option<String>` のまま空文字を送ることでクリアを表現する
// （goal.rsの `due_date: Option<Option<String>>` パターンとは異なり、
// バックエンドが空文字とnullを区別してハンドルするため、単純な `Option<String>` で足りる）。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_of_birth: Option<String>,
}

// POST /api/v1/team/users
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserCreateRequest {
    pub name: String,
    pub email: String,
}

// GET /api/v1/team/users?name=&email=&limit=&offset=
// Response: { "data": [UserResource, ...], "pagination": {...} }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserListResponse {
    #[serde(default)]
    pub data: Vec<User>,
    pub pagination: UserListPagination,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserListPagination {
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

// GET /api/v1/team/user_settings
// PATCH /api/v1/team/user_settings
// Response: { "data": UserSettingResource }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSetting {
    pub id: String,
    pub user_id: String,
    #[serde(default)]
    pub calendar_organization_member_id: Option<String>,
    #[serde(default)]
    pub receive_calendar_events: bool,
    #[serde(default)]
    pub goal_decompose_enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}

// PATCH /api/v1/team/user_settings
//
// `calendarOrganizationMemberId` を現状nullに戻す手段はない
// （このエンドポイントでは提供されていないため、クリアフラグは実装しない）。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettingUpdateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receive_calendar_events: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub calendar_organization_member_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal_decompose_enabled: Option<bool>,
}

// GET /api/v1/team/organization_members
// Response (not paginated): { "data": [UserOrganizationMemberResource, ...] }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrganizationBasic {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub is_my_organization: bool,
    #[serde(default)]
    pub logo_url: String,
    #[serde(default)]
    pub plan_type: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserOrganizationMember {
    pub id: String,
    pub organization_id: String,
    pub organization: OrganizationBasic,
    pub user_id: String,
    pub name: String,
    #[serde(default)]
    pub avatar_url: String,
    pub created_at: String,
    pub updated_at: String,
}
