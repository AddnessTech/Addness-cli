use anyhow::{Context, Result, bail};
use eventsource_stream::Eventsource;
use futures::{StreamExt, TryStreamExt};
use reqwest::{Method, RequestBuilder, Response};
use serde_json::{Value, json};

use super::{ApiClient, McpSession};

const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;

impl McpSession {
    pub(super) fn request(&self, method: Method) -> Result<RequestBuilder> {
        let (_, mut request) = self.client.request(method, self.path, true)?;
        request = request
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", &self.protocol);
        if let Some(id) = &self.session_id {
            request = request.header("mcp-session-id", id);
        }
        Ok(request)
    }

    async fn send_once(&self, body: &Value) -> Result<Response> {
        // tools/call は非冪等な書き込みも含む。タイムアウトや切断後に再送しない。
        let response = self.request(Method::POST)?.json(body).send().await
            .context("MCP request failed; it was not retried. A tool call may already have completed; check its result before retrying")?;
        if !response.status().is_success() {
            let status = response.status();
            let content_type = response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let bytes = bounded_body(response).await?;
            return Err(ApiClient::api_error(
                status,
                &String::from_utf8_lossy(&bytes),
                content_type.as_deref(),
            )
            .context(format!("MCP request path: {}", self.path)));
        }
        Ok(response)
    }

    pub(super) async fn notify_initialized(&self) -> Result<()> {
        self.send_once(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}))
            .await?;
        Ok(())
    }

    pub(super) async fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let response = self
            .send_once(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        if method == "initialize"
            && let Some(value) = response.headers().get("mcp-session-id")
        {
            let value = value.to_str().context("Invalid MCP session ID")?;
            if value.is_empty() || !value.bytes().all(|c| (0x21..=0x7e).contains(&c)) {
                bail!("Invalid MCP session ID");
            }
            self.session_id = Some(value.to_string());
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        match content_type.as_str() {
            "application/json" => {
                let message = serde_json::from_slice(&bounded_body(response).await?)
                    .context("Invalid MCP JSON response")?;
                decode_response(message, id)?.context("MCP response did not contain a result")
            }
            "text/event-stream" => {
                let mut bytes = 0usize;
                let stream = response.bytes_stream().map(move |chunk| {
                    let chunk =
                        chunk.context("MCP stream disconnected; request was not retried")?;
                    bytes = bytes.saturating_add(chunk.len());
                    if bytes > MAX_RESPONSE_BYTES {
                        bail!("MCP response exceeds 32 MiB");
                    }
                    Ok::<_, anyhow::Error>(chunk)
                });
                let mut events = stream.eventsource();
                while let Some(event) = events.try_next().await? {
                    if event.data.trim().is_empty() {
                        continue;
                    }
                    let message =
                        serde_json::from_str(&event.data).context("Invalid MCP SSE message")?;
                    if let Some(result) = decode_response(message, id)? {
                        return Ok(result);
                    }
                }
                bail!("MCP stream ended without a result; request was not retried")
            }
            _ => bail!("Unexpected MCP response content type: {content_type}"),
        }
    }
}

async fn bounded_body(mut response: Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("MCP response interrupted; request was not retried")?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            bail!("MCP response exceeds 32 MiB");
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn decode_response(message: Value, id: u64) -> Result<Option<Value>> {
    if message["jsonrpc"] != "2.0" {
        bail!("Invalid MCP JSON-RPC version");
    }
    if message.get("method").is_some() {
        if message.get("id").is_some() {
            bail!("MCP server requested an unsupported client capability");
        }
        return Ok(None);
    }
    if message["id"] != id {
        bail!("MCP response ID did not match request");
    }
    if let Some(error) = message.get("error") {
        bail!(
            "MCP error {}: {}",
            error["code"],
            error["message"].as_str().unwrap_or("Unknown error")
        );
    }
    let result = message
        .get("result")
        .context("MCP response is missing result")?;
    Ok(Some(result.clone()))
}
