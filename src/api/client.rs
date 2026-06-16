mod assignment;
mod comment;
mod deliverable;
mod goal;
mod goal_execution;
mod invitation;
mod kpi;
mod member;
mod org;

pub use comment::ListCommentsParams;
pub use org::CreateOrganizationParams;

use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub struct ApiClient {
    client: Client,
    base_url: String,
    org_id: Option<String>,
}

impl ApiClient {
    pub fn new(token: &str, base_url: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {token}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).context("Invalid token format")?,
        );

        let client = Client::builder()
            .default_headers(headers)
            .user_agent(format!("addness-cli/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .context("Failed to create HTTP client")?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            org_id: None,
        })
    }

    pub fn with_org_id(mut self, org_id: Option<String>) -> Self {
        self.org_id = org_id;
        self
    }

    pub fn set_org_id(&mut self, org_id: Option<String>) {
        self.org_id = org_id;
    }

    fn api_error(status: StatusCode, body: &str) -> anyhow::Error {
        let hint = Self::error_hint(status, body);
        match hint {
            Some(h) => anyhow::anyhow!("API error ({status}): {body}\n\nHint: {h}"),
            None => anyhow::anyhow!("API error ({status}): {body}"),
        }
    }

    fn error_hint(status: StatusCode, body: &str) -> Option<String> {
        match status {
            // 401 Unauthorized
            StatusCode::UNAUTHORIZED => {
                if body.contains("AUTH_INVALID_API_KEY") {
                    Some(
                        "API Keyが無効です。再ログインしてください。\n\
                         Run: addness login"
                            .to_string(),
                    )
                } else if body.contains("AUTH_USER_NOT_FOUND") {
                    Some(
                        "API Keyに紐づくユーザーが見つかりません。再ログインしてください。\n\
                         Run: addness login"
                            .to_string(),
                    )
                } else {
                    Some(
                        "認証に失敗しました。再ログインしてください。\n\
                         Run: addness login"
                            .to_string(),
                    )
                }
            }
            // 403 Forbidden
            StatusCode::FORBIDDEN => {
                if body.contains("ORG_NOT_MEMBER") || body.contains("AUTH_ORG_NOT_MEMBER") {
                    Some(
                        "指定された組織に所属していません。\n\
                         Run `addness org list` で所属組織を確認し、\n\
                         `addness org switch <id>` で切り替えてください。"
                            .to_string(),
                    )
                } else if body.contains("objective.update") {
                    Some(
                        "親ゴールに対する編集権限（OWNER または EDITOR）が必要です。\n\
                         自分がOWNER/EDITORとしてアサインされているゴールの配下にのみ作成・更新できます。"
                            .to_string(),
                    )
                } else if body.contains("objective.delete") {
                    Some(
                        "このゴールの削除権限がありません。\n\
                         OWNER ロールが必要です。"
                            .to_string(),
                    )
                } else if body.contains("objective.create") {
                    Some(
                        "このゴールへのアサイン変更権限がありません。\n\
                         OWNER または EDITOR ロールが必要です。"
                            .to_string(),
                    )
                } else if body.contains("ルート目標は削除できません") {
                    Some("ルート目標（組織の最上位ゴール）は削除できません。".to_string())
                } else {
                    Some(
                        "この操作を行う権限がありません。\n\
                         対象ゴールに OWNER または EDITOR としてアサインされているか確認してください。"
                            .to_string(),
                    )
                }
            }
            // 404 Not Found
            StatusCode::NOT_FOUND => {
                if body.contains("目標が見つかりません") {
                    Some(
                        "指定されたゴールが見つかりません。IDが正しいか確認してください。\n\
                         Run: addness goal search <keyword>"
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            // 400 Bad Request
            StatusCode::BAD_REQUEST => {
                if body.contains("タイトルは必須") {
                    Some("タイトルは必須です。--title を指定してください。".to_string())
                } else if body.contains("タイトルは128文字以内") {
                    Some("タイトルは128文字以内にしてください。".to_string())
                } else if body.contains("説明は10000文字以内") {
                    Some("説明は10000文字以内にしてください。".to_string())
                } else if body.contains("無効なステータス") {
                    Some(
                        "無効なステータスです。使用可能: NOT_STARTED, IN_PROGRESS, COMPLETED, CANCELLED"
                            .to_string(),
                    )
                } else if body.contains("ツリーの深さが制限") {
                    Some("ゴールの階層が深すぎます。ツリー構造を見直してください。".to_string())
                } else {
                    None
                }
            }
            // 409 Conflict
            StatusCode::CONFLICT => {
                if body.contains("すでにルート目標が存在") {
                    Some(
                        "この組織にはすでにルート目標があります。\n\
                         サブゴールを作成するには --parent <GOAL_ID> を指定してください。"
                            .to_string(),
                    )
                } else {
                    None
                }
            }
            // 429 Too Many Requests
            StatusCode::TOO_MANY_REQUESTS => Some(
                "リクエスト数の上限に達しました。しばらく待ってから再試行してください。"
                    .to_string(),
            ),
            // 5xx Server Errors
            s if s.is_server_error() => Some(
                "サーバーエラーが発生しました。しばらく待ってから再試行してください。".to_string(),
            ),
            _ => None,
        }
    }

    /// x-organization-id ヘッダーなしでGETリクエストを発行する。
    /// 組織に依存しないエンドポイント（org list等）で使用。
    pub(super) async fn get_without_org<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// ApiClient::get() は与えられた path をURLパスとして
    /// GET path リクエストをAPIに発行して
    /// レスポンスを返す
    /// mod api 以下に各エンティティに応じて
    /// このラッパをApiClientのメソッドとして実装する
    pub(super) async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.get(&url);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    pub(super) async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// x-organization-id ヘッダーなしでPOSTリクエストを発行する。
    /// 組織作成など、まだ対象組織が存在しない操作で使用。
    pub(super) async fn post_without_org<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .client
            .post(&url)
            .json(body)
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// DELETE with JSON body. Returns no response body (204 No Content).
    pub(super) async fn delete_with_body<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.delete(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        Ok(())
    }

    pub(super) async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.patch(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// POST with JSON body, expects 204 No Content response (no body parsing).
    pub(super) async fn post_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }
        Ok(())
    }

    /// POST with no request body, expects JSON response.
    pub(super) async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// PATCH with no request body, expects JSON response (used for resolve/unresolve).
    pub(super) async fn patch_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.patch(&url);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// PUT with JSON body and JSON response.
    pub(super) async fn put<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.put(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    /// PUT with JSON body, expects 204 No Content response.
    pub(super) async fn put_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.put(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }
        Ok(())
    }

    /// PUT with no request body, expects 204 No Content response.
    pub(super) async fn put_empty_no_content(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.put(&url);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }
        Ok(())
    }

    /// PATCH with JSON body, expects 204 No Content response.
    pub(super) async fn patch_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.patch(&url).json(body);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }
        Ok(())
    }

    /// DELETE with no request body, expects 204 No Content response.
    pub(super) async fn delete_no_body(&self, path: &str) -> Result<()> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.delete(&url);

        if let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {url}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }
        Ok(())
    }
}
