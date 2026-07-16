mod activity;
mod api_key;
mod assignment;
mod chat;
mod codex_job;
mod comment;
mod consent;
mod deliverable;
mod desktop_auth;
mod diagnosis;
mod goal;
mod goal_chat;
mod goal_execution;
mod goalreport;
mod inlinemedia;
mod invitation;
mod invoice;
mod issue;
mod kpi;
mod meeting;
mod member;
mod notification;
mod org;
mod personal;
mod referral;
mod search;
mod sharetree;
mod skill;
mod streak;
mod todo_chat;
mod tool;
mod user;

pub use activity::{
    ActivityLogByGoalParams, ActivityLogByMemberParams, ActivityLogSummaryParams,
    GoalActivitySummaryParams,
};
pub use chat::{ChatMessageListParams, ChatRoomListParams, ChatSearchParams};
pub use comment::{ListAllCommentsParams, ListCommentsParams};
pub use goal_chat::GoalChatThreadListParams;
pub use invoice::InvoiceListParams;
pub use issue::{GoalSectionListParams, IssueListParams};
pub use meeting::{HuddleInviteableMembersParams, MinuteListParams};
pub use member::BrowseMembersParams;
pub use notification::ListNotificationsParams;
pub use org::{CreateOrganizationParams, ListAllOrganizationsParams};
pub use search::SearchQueryParams;
pub use user::ListUsersParams;

use anyhow::{Context, Result};
use reqwest::Client;
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderName, HeaderValue};
use reqwest::{Method, RequestBuilder, Response, StatusCode, Url};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

const API_RESOLVE_ENV: &str = "ADDNESS_API_RESOLVE";
const DEFAULT_HTTP_TIMEOUT_SECS: u64 = 120;
const REQUEST_SEND_ATTEMPTS: usize = 3;
/// Long-lived SSE connections (e.g. Codex job event streams) outlive the
/// default per-request timeout, so `get_stream` overrides it with this much
/// larger budget instead of leaving the whole request unbounded.
const EVENT_STREAM_TIMEOUT_SECS: u64 = 1800;

fn http_timeout_from_env_value(value: Option<&str>) -> Duration {
    value
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS))
}

fn configured_http_timeout() -> Duration {
    http_timeout_from_env_value(std::env::var("ADDNESS_HTTP_TIMEOUT_SECS").ok().as_deref())
}

#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    org_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedFetchError {
    pub kind: &'static str,
    pub goal_id: String,
    pub message: String,
}

fn dns_override_for_base_url(base_url: &str) -> Result<Option<(String, Vec<SocketAddr>)>> {
    let override_value = match std::env::var(API_RESOLVE_ENV) {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return Ok(None),
    };
    let url = Url::parse(base_url).context("Invalid API URL")?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Invalid API URL: missing host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("Invalid API URL: missing port"))?;
    let addrs = parse_dns_override_addrs(&override_value, host, port)?;
    if addrs.is_empty() {
        return Ok(None);
    }
    Ok(Some((host.to_string(), addrs)))
}

fn parse_dns_override_addrs(raw: &str, host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    let mut addrs = Vec::new();
    for entry in raw
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let addr_text = match entry.split_once('=') {
            Some((entry_host, value)) if entry_host.trim() == host => value.trim(),
            Some(_) => continue,
            None => entry,
        };
        addrs.push(parse_dns_override_addr(addr_text, port)?);
    }
    Ok(addrs)
}

fn parse_dns_override_addr(raw: &str, port: u16) -> Result<SocketAddr> {
    if let Ok(addr) = raw.parse::<SocketAddr>() {
        return Ok(addr);
    }
    let ip = raw
        .parse::<IpAddr>()
        .with_context(|| format!("Invalid {API_RESOLVE_ENV} address: {raw}"))?;
    Ok(SocketAddr::new(ip, port))
}

fn send_failure_context(url: &str, attempts: usize) -> String {
    format!(
        "Failed to send request to {url} after {attempts} attempt(s). \
         If this is a DNS error in a restricted environment, set \
         {API_RESOLVE_ENV}=<host>=<ip> to temporarily bypass local name resolution."
    )
}
impl ApiClient {
    pub fn new(token: &str, base_url: &str) -> Result<Self> {
        let mut headers = HeaderMap::new();
        let auth_value = format!("Bearer {token}");
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&auth_value).context("Invalid token format")?,
        );

        let mut client_builder = Client::builder()
            .default_headers(headers)
            .user_agent(format!("addness-cli/{}", env!("CARGO_PKG_VERSION")))
            .timeout(configured_http_timeout());

        if let Some((host, addrs)) = dns_override_for_base_url(base_url)? {
            client_builder = client_builder.resolve_to_addrs(&host, &addrs);
        }

        let client = client_builder
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
                if body.contains("AUTH_API_KEY_SCOPE_FORBIDDEN") {
                    Some(
                        "このエンドポイントは現在のAPI Keyのスコープでは呼び出せません。\n\
                         （例: `addness login` で発行されるキーは organization スコープのため、\n\
                         personal スコープ必須のエンドポイントには使えません。\n\
                         `addness api-key create` で作成したキーを `addness configure` で設定してください。）"
                            .to_string(),
                    )
                } else if body.contains("AUTH_CLERK_ONLY") {
                    Some(
                        "このエンドポイントはブラウザ（Clerk）認証専用です。\n\
                         API Keyによる代理実行は禁止されているため、CLIからは呼び出せません。\n\
                         Webアプリから操作してください。"
                            .to_string(),
                    )
                } else if body.contains("ORG_NOT_MEMBER") || body.contains("AUTH_ORG_NOT_MEMBER") {
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

    fn request(
        &self,
        method: Method,
        path: &str,
        include_org_header: bool,
    ) -> Result<(String, RequestBuilder)> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.request(method, &url);

        if include_org_header && let Some(org_id) = &self.org_id {
            req = req.header(
                HeaderName::from_static("x-organization-id"),
                HeaderValue::from_str(org_id).context("Invalid organization ID")?,
            );
        }

        Ok((url, req))
    }

    async fn send(&self, req: RequestBuilder, url: &str) -> Result<Response> {
        let response = Self::send_request(req, url).await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Self::api_error(status, &body));
        }

        Ok(response)
    }

    async fn send_request(req: RequestBuilder, url: &str) -> Result<Response> {
        let retryable_req = req.try_clone();
        let mut first_req = Some(req);

        for attempt in 1..=REQUEST_SEND_ATTEMPTS {
            let current_req = if attempt == 1 {
                first_req
                    .take()
                    .expect("request builder should be available for first send")
            } else if let Some(retryable_req) = &retryable_req {
                retryable_req
                    .try_clone()
                    .expect("request builder clone should remain cloneable")
            } else {
                break;
            };

            match current_req.send().await {
                Ok(response) => return Ok(response),
                Err(err)
                    if Self::should_retry_send_error(&err, attempt, retryable_req.is_some()) =>
                {
                    tokio::time::sleep(Duration::from_millis(150 * attempt as u64)).await;
                }
                Err(err) => {
                    return Err(err).with_context(|| send_failure_context(url, attempt));
                }
            }
        }

        anyhow::bail!("{}", send_failure_context(url, REQUEST_SEND_ATTEMPTS))
    }

    fn should_retry_send_error(
        err: &reqwest::Error,
        attempt: usize,
        request_cloneable: bool,
    ) -> bool {
        request_cloneable
            && attempt < REQUEST_SEND_ATTEMPTS
            && (err.is_connect() || err.is_timeout())
    }

    async fn send_json<T: DeserializeOwned>(&self, req: RequestBuilder, url: &str) -> Result<T> {
        self.send(req, url)
            .await?
            .json::<T>()
            .await
            .with_context(|| format!("Failed to parse response from {url}"))
    }

    async fn send_no_content(&self, req: RequestBuilder, url: &str) -> Result<()> {
        self.send(req, url).await?;
        Ok(())
    }

    /// x-organization-id ヘッダーなしでGETリクエストを発行する。
    /// 組織に依存しないエンドポイント（org list等）で使用。
    pub(super) async fn get_without_org<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (url, req) = self.request(Method::GET, path, false)?;
        self.send_json(req, &url).await
    }

    /// ApiClient::get() は与えられた path をURLパスとして
    /// GET path リクエストをAPIに発行して
    /// レスポンスを返す
    /// mod api 以下に各エンティティに応じて
    /// このラッパをApiClientのメソッドとして実装する
    pub(super) async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (url, req) = self.request(Method::GET, path, true)?;
        self.send_json(req, &url).await
    }

    pub(super) async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let (url, req) = self.request(Method::POST, path, true)?;
        self.send_json(req.json(body), &url).await
    }

    /// x-organization-id ヘッダーなしでPOSTリクエストを発行する。
    /// 組織作成など、まだ対象組織が存在しない操作で使用。
    pub(super) async fn post_without_org<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let (url, req) = self.request(Method::POST, path, false)?;
        self.send_json(req.json(body), &url).await
    }

    /// DELETE with JSON body. Returns no response body (204 No Content).
    pub(super) async fn delete_with_body<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let (url, req) = self.request(Method::DELETE, path, true)?;
        self.send_no_content(req.json(body), &url).await
    }

    pub(super) async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let (url, req) = self.request(Method::PATCH, path, true)?;
        self.send_json(req.json(body), &url).await
    }

    /// POST with JSON body, expects 204 No Content response (no body parsing).
    pub(super) async fn post_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let (url, req) = self.request(Method::POST, path, true)?;
        self.send_no_content(req.json(body), &url).await
    }

    /// POST with no request body, expects JSON response.
    pub(super) async fn post_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (url, req) = self.request(Method::POST, path, true)?;
        self.send_json(req, &url).await
    }

    /// POST with no request body, expects 204 No Content response.
    pub(super) async fn post_empty_no_content(&self, path: &str) -> Result<()> {
        let (url, req) = self.request(Method::POST, path, true)?;
        self.send_no_content(req, &url).await
    }

    /// PATCH with no request body, expects JSON response (used for resolve/unresolve).
    pub(super) async fn patch_empty<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (url, req) = self.request(Method::PATCH, path, true)?;
        self.send_json(req, &url).await
    }

    /// PUT with JSON body and JSON response.
    pub(super) async fn put<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let (url, req) = self.request(Method::PUT, path, true)?;
        self.send_json(req.json(body), &url).await
    }

    /// PUT with JSON body, expects 204 No Content response.
    pub(super) async fn put_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let (url, req) = self.request(Method::PUT, path, true)?;
        self.send_no_content(req.json(body), &url).await
    }

    /// PUT with no request body, expects 204 No Content response.
    pub(super) async fn put_empty_no_content(&self, path: &str) -> Result<()> {
        let (url, req) = self.request(Method::PUT, path, true)?;
        self.send_no_content(req, &url).await
    }

    /// PATCH with JSON body, expects 204 No Content response.
    pub(super) async fn patch_no_content<B: Serialize>(&self, path: &str, body: &B) -> Result<()> {
        let (url, req) = self.request(Method::PATCH, path, true)?;
        self.send_no_content(req.json(body), &url).await
    }

    /// DELETE with no request body, expects 204 No Content response.
    pub(super) async fn delete_no_body(&self, path: &str) -> Result<()> {
        let (url, req) = self.request(Method::DELETE, path, true)?;
        self.send_no_content(req, &url).await
    }

    /// DELETE with no request body, expects a JSON response body (unlike
    /// `delete_no_body`; used by endpoints that return a status payload on
    /// deletion, e.g. the goal activity report schedule).
    pub(super) async fn delete_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let (url, req) = self.request(Method::DELETE, path, true)?;
        self.send_json(req, &url).await
    }

    /// PUT with a raw binary body (e.g. organization logo upload), expects JSON response.
    /// The backend reads the request body directly as the uploaded file, so this
    /// sends the bytes as-is with an explicit `Content-Type`.
    pub(super) async fn put_bytes<T: DeserializeOwned>(
        &self,
        path: &str,
        bytes: Vec<u8>,
        content_type: &str,
    ) -> Result<T> {
        let (url, req) = self.request(Method::PUT, path, true)?;
        let req = req
            .header(reqwest::header::CONTENT_TYPE, content_type.to_string())
            .body(bytes);
        self.send_json(req, &url).await
    }

    /// POST a `multipart/form-data` body directly to our own API (unlike
    /// `upload_attachment`, which posts to a third-party presigned URL).
    /// Used by endpoints that accept a raw file upload alongside auth headers,
    /// e.g. meeting-note audio transcription.
    pub(super) async fn post_multipart<T: DeserializeOwned>(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<T> {
        let (url, req) = self.request(Method::POST, path, true)?;
        self.send_json(req.multipart(form), &url).await
    }

    /// GET a server-sent-events endpoint, returning the raw `Response` for
    /// the caller to consume as a byte stream (e.g. via `bytes_stream()` +
    /// `eventsource_stream::Eventsource`). Overrides the client's default
    /// timeout — SSE connections are kept alive far longer than a normal
    /// request/response round trip.
    pub(super) async fn get_stream(&self, path: &str) -> Result<Response> {
        let (url, req) = self.request(Method::GET, path, true)?;
        let req = req.timeout(Duration::from_secs(EVENT_STREAM_TIMEOUT_SECS));
        self.send(req, &url).await
    }

    /// POST a JSON body to a server-sent-events endpoint, returning the raw
    /// `Response` for the caller to consume as a byte stream. Mirrors
    /// `get_stream`, but for endpoints (e.g. AI goal chat) whose SSE stream
    /// is initiated with a POST + JSON payload rather than a GET.
    pub(super) async fn post_stream<B: Serialize>(&self, path: &str, body: &B) -> Result<Response> {
        let (url, req) = self.request(Method::POST, path, true)?;
        let req = req
            .json(body)
            .timeout(Duration::from_secs(EVENT_STREAM_TIMEOUT_SECS));
        self.send(req, &url).await
    }
}

#[cfg(test)]
mod tests {
    use super::{
        API_RESOLVE_ENV, ApiClient, DEFAULT_HTTP_TIMEOUT_SECS, http_timeout_from_env_value,
        parse_dns_override_addrs, send_failure_context,
    };
    use reqwest::StatusCode;
    use std::net::SocketAddr;
    use std::time::Duration;

    #[test]
    fn http_timeout_uses_default_without_valid_override() {
        assert_eq!(
            http_timeout_from_env_value(None),
            Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS)
        );
        assert_eq!(
            http_timeout_from_env_value(Some("")),
            Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS)
        );
        assert_eq!(
            http_timeout_from_env_value(Some("0")),
            Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS)
        );
        assert_eq!(
            http_timeout_from_env_value(Some("abc")),
            Duration::from_secs(DEFAULT_HTTP_TIMEOUT_SECS)
        );
    }

    #[test]
    fn http_timeout_accepts_positive_seconds_override() {
        assert_eq!(
            http_timeout_from_env_value(Some("15")),
            Duration::from_secs(15)
        );
    }

    #[test]
    fn api_error_keeps_existing_auth_hint() {
        let err = ApiClient::api_error(StatusCode::UNAUTHORIZED, "AUTH_INVALID_API_KEY");
        let message = err.to_string();

        assert!(message.contains("API error (401 Unauthorized): AUTH_INVALID_API_KEY"));
        assert!(message.contains("Run: addness login"));
    }

    #[test]
    fn api_error_hints_api_key_scope_forbidden() {
        let err = ApiClient::api_error(
            StatusCode::FORBIDDEN,
            r#"{"code":"AUTH_API_KEY_SCOPE_FORBIDDEN"}"#,
        );
        let message = err.to_string();

        assert!(message.contains("スコープでは呼び出せません"));
        assert!(message.contains("addness api-key create"));
    }

    #[test]
    fn api_error_hints_clerk_only_endpoints() {
        let err = ApiClient::api_error(StatusCode::FORBIDDEN, r#"{"code":"AUTH_CLERK_ONLY"}"#);
        let message = err.to_string();

        assert!(message.contains("ブラウザ（Clerk）認証専用"));
    }

    #[test]
    fn dns_override_accepts_plain_ip_for_base_host() {
        let addrs = parse_dns_override_addrs("54.248.80.181", "vt.api.addness.com", 443).unwrap();

        assert_eq!(
            addrs,
            vec!["54.248.80.181:443".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn dns_override_filters_host_mapping() {
        let addrs = parse_dns_override_addrs(
            "other.example=10.0.0.1,vt.api.addness.com=54.248.80.181:8443",
            "vt.api.addness.com",
            443,
        )
        .unwrap();

        assert_eq!(
            addrs,
            vec!["54.248.80.181:8443".parse::<SocketAddr>().unwrap()]
        );
    }

    #[test]
    fn dns_override_rejects_invalid_address() {
        let err = parse_dns_override_addrs("not-an-ip", "vt.api.addness.com", 443)
            .unwrap_err()
            .to_string();

        assert!(err.contains("Invalid ADDNESS_API_RESOLVE address"));
    }

    #[test]
    fn send_failure_context_mentions_dns_override_env() {
        let context = send_failure_context("https://vt.api.addness.com/api/v2/objectives", 3);

        assert!(context.contains(API_RESOLVE_ENV));
        assert!(context.contains("after 3 attempt"));
    }
}
