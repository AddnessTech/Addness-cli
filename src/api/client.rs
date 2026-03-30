use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION};
use reqwest::Client;
use serde::de::DeserializeOwned;

pub struct ApiClient {
    client: Client,
    base_url: String,
    org_id: Option<String>,
}

impl ApiClient {
    pub fn new(token: &str, base_url: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {}", token);
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).context("Invalid token format")?,
        );

        let client = Client::builder()
            .default_headers(headers)
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

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.get(&url);

        if let Some(ref org_id) = self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        let response = req
            .send()
            .await
            .with_context(|| format!("Failed to send request to {}", url))?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            bail!("API error ({}): {}", status, body);
        }

        response
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {}", url))
    }
}
