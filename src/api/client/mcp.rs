//! Addness の公開 MCP カタログを CLI からそのまま利用する。
#[cfg(test)]
pub(super) mod tests;
mod transport;

use std::collections::HashSet;

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde_json::{Value, json};

use super::ApiClient;

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum McpScope {
    #[default]
    Organization,
    Personal,
    Copilot,
    Openai,
    Admin,
    Support,
}

impl McpScope {
    fn path(self) -> &'static str {
        match self {
            Self::Organization => "/mcp",
            Self::Personal => "/mcp/personal",
            Self::Copilot => "/mcp/copilot",
            Self::Openai => "/mcp/openai",
            Self::Admin => "/mcp/admin",
            Self::Support => "/mcp/support",
        }
    }
}

pub struct McpSession {
    client: ApiClient,
    path: &'static str,
    protocol: String,
    session_id: Option<String>,
    next_id: u64,
    pub information: Value,
}

impl ApiClient {
    pub async fn connect_mcp(&self, scope: McpScope) -> Result<McpSession> {
        let mut session = McpSession {
            client: self.clone(),
            path: scope.path(),
            protocol: "2025-06-18".to_string(),
            session_id: None,
            next_id: 1,
            information: Value::Null,
        };
        // 個人・運営入口へ、保存された組織を暗黙に送らない。
        if matches!(
            scope,
            McpScope::Personal | McpScope::Admin | McpScope::Support
        ) {
            session.client.org_id = None;
        }
        let information = session
            .rpc(
                "initialize",
                json!({
                    "protocolVersion": session.protocol,
                    "capabilities": {},
                    "clientInfo": {"name": "addness-cli", "version": env!("CARGO_PKG_VERSION")}
                }),
            )
            .await?;
        let protocol = information["protocolVersion"]
            .as_str()
            .context("MCP initialize did not return protocolVersion")?;
        if !matches!(protocol, "2025-03-26" | "2025-06-18" | "2025-11-25") {
            bail!("Unsupported MCP protocol version: {protocol}");
        }
        session.protocol = protocol.to_string();
        session.information = information;
        session.notify_initialized().await?;
        Ok(session)
    }
}

impl McpSession {
    /// tools/list の全ページを取得する。ツール名やスキーマは固定しない。
    pub async fn list_tools(&mut self) -> Result<Value> {
        let mut tools = Vec::new();
        let mut cursors = HashSet::new();
        let mut params = json!({});
        let mut metadata = None;
        loop {
            let page = self.rpc("tools/list", params).await?;
            tools.extend(
                page["tools"]
                    .as_array()
                    .context("Invalid MCP tools/list response")?
                    .iter()
                    .cloned(),
            );
            if metadata.is_none() {
                metadata = page.get("_meta").cloned();
            }
            let cursor = match page.get("nextCursor") {
                None | Some(Value::Null) => break,
                Some(Value::String(cursor)) if cursor.is_empty() => break,
                Some(Value::String(cursor)) => cursor.clone(),
                _ => bail!("Invalid MCP tools/list cursor"),
            };
            if !cursors.insert(cursor.clone()) || cursors.len() > 1_000 {
                bail!("MCP tools/list pagination did not advance");
            }
            params = json!({"cursor": cursor});
        }
        let mut result = json!({"tools": tools});
        if let Some(metadata) = metadata {
            result["_meta"] = metadata;
        }
        Ok(result)
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value> {
        if name.trim().is_empty() || !arguments.is_object() {
            bail!("A tool name and a JSON object of arguments are required");
        }
        self.rpc("tools/call", json!({"name": name, "arguments": arguments}))
            .await
    }

    pub async fn close(&self) {
        if self.session_id.is_some()
            && let Ok(request) = self.request(reqwest::Method::DELETE)
        {
            // セッションの終了失敗を理由に、成功した書き込みを失敗扱いにしない。
            let _ = request.send().await;
        }
    }
}
