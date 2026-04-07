mod comment;
mod deliverable;
mod goal;
mod member;
mod org;

use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::StatusCode;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::multipart;
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

    fn api_error(status: StatusCode, body: &str) -> anyhow::Error {
        if status == StatusCode::FORBIDDEN && body.contains("ORG_NOT_MEMBER") {
            return anyhow::anyhow!(
                "API error ({status}): {body}\n\n\
                 Hint: Your current organization may be invalid.\n\
                 Run `addness org list` to see available organizations,\n\
                 then `addness org switch <id>` to switch."
            );
        }
        anyhow::anyhow!("API error ({status}): {body}")
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

    pub(super) async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: multipart::Form,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.post(&url).multipart(form);

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
}
