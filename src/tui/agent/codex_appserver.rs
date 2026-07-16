//! Codex `app-server`（JSON-RPC 2.0 / 改行区切り JSON / stdio）常駐クライアント。
//!
//! ワンショット（`codex exec --json`、1 ターン 1 プロセス）に対し、常駐モードは 1 プロセスで
//! 多ターンを回す。ここには TUI 状態を持たない純粋関数と、`Child` を包む薄いラッパのみを置く:
//! - stdin へ書き込む専用 writer スレッド + mpsc（不正 JSON でも codex は死なないが、必ず
//!   `serde_json` でシリアライズしたものだけを書く）
//! - JSON-RPC リクエスト/通知の生成（initialize / thread.start / turn.start / interrupt /
//!   settings.update / 承認応答）
//! - stdout に来る JSON-RPC メッセージ（レスポンス / サーバ発リクエスト（承認）/ 通知）のパース
//!
//! TUI 側の状態更新は `agent/mod.rs`（`CodexPane`）が行う。

use std::io::Write;
use std::process::{Child, ExitStatus};
use std::sync::mpsc::{self, Sender};

use anyhow::{Context, Result};
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// スレッド設定（thread/start・thread/resume・turn/start へ反映する CodexExecSettings 由来の値）
// ---------------------------------------------------------------------------

/// thread/start・thread/resume に渡すスレッド初期設定。すべて任意（None は codex の既定に従う）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct ThreadConfig {
    pub(super) cwd: Option<String>,
    pub(super) model: Option<String>,
    /// approvalPolicy（untrusted / on-request / on-failure / never）。
    pub(super) approval_policy: Option<String>,
    /// sandbox（read-only / workspace-write / danger-full-access）。
    pub(super) sandbox: Option<String>,
    pub(super) developer_instructions: Option<String>,
}

impl ThreadConfig {
    fn apply(&self, params: &mut serde_json::Map<String, Value>) {
        if let Some(cwd) = &self.cwd {
            params.insert("cwd".to_string(), json!(cwd));
        }
        if let Some(model) = &self.model {
            params.insert("model".to_string(), json!(model));
        }
        if let Some(approval) = &self.approval_policy {
            params.insert("approvalPolicy".to_string(), json!(approval));
        }
        if let Some(sandbox) = &self.sandbox {
            params.insert("sandbox".to_string(), json!(sandbox));
        }
        if let Some(instructions) = &self.developer_instructions {
            params.insert("developerInstructions".to_string(), json!(instructions));
        }
    }
}

/// thread/settings/update に渡す実行時オーバーライド。指定した項目だけ送る。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct SettingsUpdate {
    pub(super) model: Option<String>,
    pub(super) effort: Option<String>,
    pub(super) approval_policy: Option<String>,
    /// sandbox（read-only / workspace-write / danger-full-access）。SandboxPolicy へ変換して送る。
    pub(super) sandbox: Option<String>,
}

impl SettingsUpdate {
    fn is_empty(&self) -> bool {
        self.model.is_none()
            && self.effort.is_none()
            && self.approval_policy.is_none()
            && self.sandbox.is_none()
    }
}

/// sandbox ラベル（read-only 等）を app-server の SandboxPolicy オブジェクトへ変換する。
fn sandbox_policy(label: &str) -> Option<Value> {
    match label {
        "read-only" => Some(json!({"type": "readOnly"})),
        "workspace-write" => Some(json!({"type": "workspaceWrite"})),
        "danger-full-access" => Some(json!({"type": "dangerFullAccess"})),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 送信メッセージ生成（純粋関数・serde_json 経由でのみ stdin へ書く）
// ---------------------------------------------------------------------------

/// initialize リクエスト。experimentalApi を有効化し、雑多な通知を optOut で抑制する。
pub(super) fn initialize_request(id: u64, name: &str, version: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "clientInfo": {"name": name, "title": "Addness TUI", "version": version},
            "capabilities": {
                "experimentalApi": true,
                "requestAttestation": false,
                "optOutNotificationMethods": [
                    "mcpServer/startupStatus/updated",
                    "account/rateLimits/updated"
                ]
            }
        }
    })
}

/// initialized 通知（params 無し）。initialize 応答の後に 1 回送る。
pub(super) fn initialized_notification() -> Value {
    json!({"jsonrpc": "2.0", "method": "initialized"})
}

/// thread/start リクエスト。
pub(super) fn thread_start_request(id: u64, config: &ThreadConfig) -> Value {
    let mut params = serde_json::Map::new();
    config.apply(&mut params);
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "thread/start",
        "params": Value::Object(params)
    })
}

/// thread/resume リクエスト（既存 thread_id を別プロセスから継続）。
pub(super) fn thread_resume_request(id: u64, thread_id: &str, config: &ThreadConfig) -> Value {
    let mut params = serde_json::Map::new();
    params.insert("threadId".to_string(), json!(thread_id));
    config.apply(&mut params);
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "thread/resume",
        "params": Value::Object(params)
    })
}

/// turn/start リクエスト。effort はここでオーバーライドする（thread/start には effort が無い）。
/// `image_paths` は LocalImageUserInput（`{"type":"localImage","path":...}`）として input に追加する。
pub(super) fn turn_start_request(
    id: u64,
    thread_id: &str,
    text: &str,
    effort: Option<&str>,
    image_paths: &[String],
) -> Value {
    let mut input = vec![json!({"type": "text", "text": text, "text_elements": []})];
    for path in image_paths {
        input.push(json!({"type": "localImage", "path": path}));
    }
    let mut params = serde_json::Map::new();
    params.insert("threadId".to_string(), json!(thread_id));
    params.insert("input".to_string(), Value::Array(input));
    if let Some(effort) = effort {
        params.insert("effort".to_string(), json!(effort));
    }
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "turn/start",
        "params": Value::Object(params)
    })
}

/// turn/interrupt リクエスト（グレースフル中断、継続可能）。
pub(super) fn turn_interrupt_request(id: u64, thread_id: &str, turn_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "turn/interrupt",
        "params": {"threadId": thread_id, "turnId": turn_id}
    })
}

/// thread/settings/update リクエスト。指定した項目だけ送る。空なら None。
pub(super) fn settings_update_request(
    id: u64,
    thread_id: &str,
    update: &SettingsUpdate,
) -> Option<Value> {
    if update.is_empty() {
        return None;
    }
    let mut params = serde_json::Map::new();
    params.insert("threadId".to_string(), json!(thread_id));
    if let Some(model) = &update.model {
        params.insert("model".to_string(), json!(model));
    }
    if let Some(effort) = &update.effort {
        params.insert("effort".to_string(), json!(effort));
    }
    if let Some(approval) = &update.approval_policy {
        params.insert("approvalPolicy".to_string(), json!(approval));
    }
    if let Some(sandbox) = &update.sandbox
        && let Some(policy) = sandbox_policy(sandbox)
    {
        params.insert("sandboxPolicy".to_string(), policy);
    }
    Some(json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "thread/settings/update",
        "params": Value::Object(params)
    }))
}

/// commandExecution / fileChange の承認応答（result に decision を返す）。
pub(super) fn approval_result(id: &Value, decision: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": {"decision": decision}})
}

/// permissions/requestApproval への応答。granted は付与するプロファイル、scope は turn / session。
pub(super) fn permissions_result(id: &Value, granted: Value, scope: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {"permissions": granted, "scope": scope}
    })
}

/// 未対応のサーバ発リクエストへ返す JSON-RPC エラー応答（プロセスを待たせない）。
pub(super) fn error_response(id: &Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

// ---------------------------------------------------------------------------
// 受信パース
// ---------------------------------------------------------------------------

/// JSON-RPC エラー（-32600 等）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct JsonRpcError {
    pub(super) code: i64,
    pub(super) message: String,
}

/// 承認要求の種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApprovalKind {
    Command,
    FileChange,
    Permissions,
}

/// サーバ発の承認リクエスト。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ApprovalRequest {
    /// エコーバックする JSON-RPC id（整数 or 文字列。そのまま Value で保持）。
    pub(super) id: Value,
    pub(super) kind: ApprovalKind,
    pub(super) item_id: Option<String>,
    /// バナー表示用の要約（コマンド文字列やファイルパス）。
    pub(super) summary: String,
    /// 提示可能な決定肢（accept / acceptForSession / decline 等）。
    pub(super) available_decisions: Vec<String>,
    /// permissions リクエストで要求されたプロファイル（accept 時にエコーバック）。
    pub(super) requested_permissions: Option<Value>,
}

impl ApprovalRequest {
    /// availableDecisions に acceptForSession（またはセッション付与 scope）が含まれるか。
    pub(super) fn allows_session(&self) -> bool {
        match self.kind {
            ApprovalKind::Permissions => true,
            _ => self
                .available_decisions
                .iter()
                .any(|d| d == "acceptForSession"),
        }
    }
}

/// ThreadItem（item/started・item/completed の item）の意味づけ。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ThreadItemKind {
    CommandExecution {
        command: String,
        exit_code: Option<i64>,
        duration_ms: Option<i64>,
        status: Option<String>,
    },
    AgentMessage {
        text: String,
        phase: Option<String>,
    },
    Reasoning {
        summary: Vec<String>,
        content: Vec<String>,
    },
    FileChange {
        changes: Vec<FileChangeDetail>,
        status: Option<String>,
    },
    McpToolCall {
        server: String,
        tool: String,
        status: Option<String>,
    },
    /// 上記以外（webSearch 等）。type 名を保持。
    Other(String),
}

/// fileChange の変更 1 件（パス・種別・unified diff 文字列）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct FileChangeDetail {
    pub(super) path: String,
    /// "add" / "delete" / "update"（PatchChangeKind.type）。
    pub(super) change_type: String,
    /// unified diff 文字列（空のこともある）。
    pub(super) diff: String,
}

/// ThreadItem の共通情報。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ThreadItemInfo {
    pub(super) id: String,
    pub(super) kind: ThreadItemKind,
}

/// トークン使用量（保存用）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TokenUsageInfo {
    pub(super) total_tokens: Option<u64>,
    pub(super) last_total_tokens: Option<u64>,
    pub(super) model_context_window: Option<u64>,
}

/// サーバから届く通知の意味づけ。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Notification {
    TurnStarted,
    /// turn/completed（status: completed / interrupted / failed）。
    TurnCompleted {
        status: String,
    },
    AgentMessageDelta {
        delta: String,
    },
    ItemStarted(ThreadItemInfo),
    ItemCompleted(ThreadItemInfo),
    /// item/commandExecution/outputDelta（実行中コマンドの stdout ストリーミング）。
    CommandOutputDelta {
        item_id: String,
        delta: String,
    },
    ReasoningDelta {
        text: String,
    },
    TokenUsage(TokenUsageInfo),
    Error {
        message: String,
    },
    /// 表示対象外の通知。
    Ignored,
}

/// stdout に来る JSON-RPC メッセージ。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ServerMessage {
    /// 自前リクエストへの応答。
    Response {
        id: u64,
        result: Option<Value>,
        error: Option<JsonRpcError>,
    },
    /// サーバ発の承認リクエスト。
    Approval(ApprovalRequest),
    /// 承認以外の未対応サーバ発リクエスト（エラー応答を返して継続する）。
    UnhandledRequest { id: Value, method: String },
    /// 通知。
    Notification(Notification),
}

/// JSON-RPC メッセージらしい形かを判定する（常駐経路への振り分け用の純粋関数）。
///
/// codex 0.142.5 の app-server は応答・通知に `"jsonrpc":"2.0"` を付けないため、
/// `jsonrpc` キーの有無だけでは判定できない。判定基準:
/// - `jsonrpc` キーがある
/// - または `method` が文字列
/// - または（`id` があり、かつ `result` か `error` を持つ）
///
/// ワンショット codex exec の snake_case イベント（`{"type":"thread.started",...}` のような
/// `type` 持ち・`method`/`result`/`error` なし）は false になる。
pub(super) fn looks_like_jsonrpc(value: &Value) -> bool {
    value.get("jsonrpc").is_some()
        || value.get("method").and_then(Value::as_str).is_some()
        || (value.get("id").is_some()
            && (value.get("result").is_some() || value.get("error").is_some()))
}

/// 1 行の JSON-RPC メッセージをパースする。JSON-RPC でなければ None。
pub(super) fn parse_message(value: &Value) -> Option<ServerMessage> {
    // codex 0.142.5 の app-server は `"jsonrpc":"2.0"` を省略する。無い場合は許容し、
    // 明示されている場合のみ "2.0" 以外を弾く。
    if let Some(version) = value.get("jsonrpc")
        && version.as_str() != Some("2.0")
    {
        return None;
    }
    let has_id = value.get("id").is_some();
    let method = value.get("method").and_then(Value::as_str);

    match (method, has_id) {
        // サーバ発リクエスト（method + id）。
        (Some(method), true) => {
            let id = value.get("id").cloned().unwrap_or(Value::Null);
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            Some(parse_server_request(id, method, &params))
        }
        // 通知（method のみ）。
        (Some(method), false) => {
            let params = value.get("params").cloned().unwrap_or(Value::Null);
            Some(ServerMessage::Notification(parse_notification(
                method, &params,
            )))
        }
        // 応答（id のみ）。
        (None, true) => {
            // クライアント id は整数で採番している。
            let id = value.get("id").and_then(Value::as_u64)?;
            if let Some(error) = value.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(0);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("JSON-RPC error")
                    .to_string();
                Some(ServerMessage::Response {
                    id,
                    result: None,
                    error: Some(JsonRpcError { code, message }),
                })
            } else {
                Some(ServerMessage::Response {
                    id,
                    result: value.get("result").cloned(),
                    error: None,
                })
            }
        }
        (None, false) => None,
    }
}

fn parse_server_request(id: Value, method: &str, params: &Value) -> ServerMessage {
    let kind = match method {
        "item/commandExecution/requestApproval" => Some(ApprovalKind::Command),
        "item/fileChange/requestApproval" => Some(ApprovalKind::FileChange),
        "item/permissions/requestApproval" => Some(ApprovalKind::Permissions),
        _ => None,
    };
    let Some(kind) = kind else {
        return ServerMessage::UnhandledRequest {
            id,
            method: method.to_string(),
        };
    };

    let item_id = params
        .get("itemId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let available_decisions = params
        .get("availableDecisions")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(decision_label).collect())
        .unwrap_or_default();
    let summary = match kind {
        ApprovalKind::Command => {
            super::command_text(params).unwrap_or_else(|| "(コマンド不明)".to_string())
        }
        ApprovalKind::FileChange => params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("ファイル変更")
            .to_string(),
        ApprovalKind::Permissions => params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("権限昇格")
            .to_string(),
    };
    let requested_permissions = if kind == ApprovalKind::Permissions {
        params.get("permissions").cloned()
    } else {
        None
    };

    ServerMessage::Approval(ApprovalRequest {
        id,
        kind,
        item_id,
        summary,
        available_decisions,
        requested_permissions,
    })
}

/// availableDecisions の要素（文字列 or `{acceptWithExecpolicyAmendment:...}` 等）をラベル化する。
fn decision_label(value: &Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    // オブジェクト型決定肢（例: acceptWithExecpolicyAmendment）はキー名で表す。
    value
        .as_object()
        .and_then(|obj| obj.keys().next())
        .map(|k| k.to_string())
}

fn parse_notification(method: &str, params: &Value) -> Notification {
    match method {
        "turn/started" => Notification::TurnStarted,
        "turn/completed" => {
            let status = params
                .get("turn")
                .and_then(|t| t.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("completed")
                .to_string();
            Notification::TurnCompleted { status }
        }
        "item/agentMessage/delta" => {
            let delta = params
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Notification::AgentMessageDelta { delta }
        }
        "item/commandExecution/outputDelta" => {
            let item_id = params
                .get("itemId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let delta = params
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Notification::CommandOutputDelta { item_id, delta }
        }
        "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
            let text = params
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Notification::ReasoningDelta { text }
        }
        "item/started" => match parse_thread_item(params.get("item")) {
            Some(item) => Notification::ItemStarted(item),
            None => Notification::Ignored,
        },
        "item/completed" => match parse_thread_item(params.get("item")) {
            Some(item) => Notification::ItemCompleted(item),
            None => Notification::Ignored,
        },
        "thread/tokenUsage/updated" => {
            Notification::TokenUsage(parse_token_usage(params.get("tokenUsage")))
        }
        "error" => {
            let message = params
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| params.get("error").and_then(Value::as_str))
                .unwrap_or("Codex エラー")
                .to_string();
            Notification::Error { message }
        }
        _ => Notification::Ignored,
    }
}

fn parse_thread_item(item: Option<&Value>) -> Option<ThreadItemInfo> {
    let item = item?;
    let id = item.get("id").and_then(Value::as_str)?.to_string();
    let item_type = item.get("type").and_then(Value::as_str)?;
    let kind = match item_type {
        "commandExecution" => ThreadItemKind::CommandExecution {
            command: super::command_text(item).unwrap_or_default(),
            exit_code: item.get("exitCode").and_then(Value::as_i64),
            duration_ms: item.get("durationMs").and_then(Value::as_i64),
            status: item
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "agentMessage" => ThreadItemKind::AgentMessage {
            text: item
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            phase: item
                .get("phase")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "reasoning" => ThreadItemKind::Reasoning {
            summary: string_array(item.get("summary")),
            content: string_array(item.get("content")),
        },
        "fileChange" => ThreadItemKind::FileChange {
            changes: item
                .get("changes")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|c| {
                            let path = c.get("path").and_then(Value::as_str)?.to_string();
                            let change_type = c
                                .get("kind")
                                .and_then(|k| k.get("type"))
                                .and_then(Value::as_str)
                                .unwrap_or("update")
                                .to_string();
                            let diff = c
                                .get("diff")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            Some(FileChangeDetail {
                                path,
                                change_type,
                                diff,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            status: item
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        "mcpToolCall" => ThreadItemKind::McpToolCall {
            server: item
                .get("server")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            tool: item
                .get("tool")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            status: item
                .get("status")
                .and_then(Value::as_str)
                .map(str::to_string),
        },
        other => ThreadItemKind::Other(other.to_string()),
    };
    Some(ThreadItemInfo { id, kind })
}

fn string_array(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn parse_token_usage(usage: Option<&Value>) -> TokenUsageInfo {
    let Some(usage) = usage else {
        return TokenUsageInfo::default();
    };
    let total_tokens = usage
        .get("total")
        .and_then(|t| t.get("totalTokens"))
        .and_then(Value::as_u64);
    let last_total_tokens = usage
        .get("last")
        .and_then(|t| t.get("totalTokens"))
        .and_then(Value::as_u64);
    let model_context_window = usage.get("modelContextWindow").and_then(Value::as_u64);
    TokenUsageInfo {
        total_tokens,
        last_total_tokens,
        model_context_window,
    }
}

/// thread/start・thread/resume 応答から thread.id を取り出す。
pub(super) fn thread_id_from_response(result: &Value) -> Option<String> {
    result
        .get("thread")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// turn/start 応答から turn.id を取り出す。
pub(super) fn turn_id_from_response(result: &Value) -> Option<String> {
    result
        .get("turn")
        .and_then(|t| t.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// 常駐プロセスの薄いラッパ（Child を保持し、stdin は writer スレッドが専有）
// ---------------------------------------------------------------------------

/// writer スレッドへの指示。
enum WriterMsg {
    /// 1 行（改行なし）の JSON を書く。writer が改行付きで flush する。
    Line(String),
    /// stdin をクローズしてグレースフル終了させる。
    Close,
}

/// グレースフルクローズを始めた理由（終了検知時のログ文言の出し分けに使う）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CloseReason {
    /// 無操作が続いたためのアイドル回収。
    Idle,
    /// 設定変更（config へ戻すなど）を反映するための再起動。
    SettingsRestart,
}

/// 常駐 codex app-server プロセスのクライアント。`Child` を所有し、stdin は writer スレッドへ委譲する。
/// stdout/stderr は呼び出し側（`CodexPane`）が `spawn_line_reader` で読む。
pub(super) struct AppServerClient {
    child: Child,
    writer_tx: Sender<WriterMsg>,
    next_request_id: u64,
    /// stdin クローズ済み（終了待ち）なら、その理由。
    pub(super) closing: Option<CloseReason>,
}

impl AppServerClient {
    /// spawn 済みの `Child`（stdout/stderr は取得済みで良い）から常駐クライアントを作る。
    /// stdin を writer スレッドへ移す。
    pub(super) fn new(mut child: Child) -> Result<Self> {
        let mut stdin = child
            .stdin
            .take()
            .context("codex app-server 常駐プロセス stdin の取得に失敗しました")?;
        let (writer_tx, writer_rx) = mpsc::channel::<WriterMsg>();
        std::thread::spawn(move || {
            for msg in writer_rx {
                match msg {
                    WriterMsg::Line(line) => {
                        if stdin.write_all(line.as_bytes()).is_err()
                            || stdin.write_all(b"\n").is_err()
                            || stdin.flush().is_err()
                        {
                            break;
                        }
                    }
                    WriterMsg::Close => break,
                }
            }
            // ループを抜けると stdin がドロップされ、パイプがクローズされる。
        });
        Ok(Self {
            child,
            writer_tx,
            next_request_id: 1,
            closing: None,
        })
    }

    /// 次の JSON-RPC リクエスト id を採番する。
    pub(super) fn next_id(&mut self) -> u64 {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        id
    }

    /// serde_json でシリアライズして writer へ渡す。失敗（シリアライズ or writer 切断）は `false`。
    pub(super) fn send_value(&self, value: &Value) -> bool {
        match serde_json::to_string(value) {
            Ok(line) => self.writer_tx.send(WriterMsg::Line(line)).is_ok(),
            Err(_) => false,
        }
    }

    /// stdin をクローズしてグレースフル終了を開始する（アイドル回収・設定変更フォールバック用）。
    pub(super) fn begin_close(&mut self, reason: CloseReason) {
        self.closing = Some(reason);
        let _ = self.writer_tx.send(WriterMsg::Close);
    }

    /// 終了検知（ノンブロッキング）。
    pub(super) fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// 強制終了して待ち受ける（ゾンビ化防止）。
    pub(super) fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    /// テスト用: 実 codex を起動せず、stdin を捨てるだけの生存プロセス（`cat`）で
    /// 常駐クライアントを作る。常駐固有の状態遷移を検証するために使う。
    /// 生成できない環境（`cat` 不在）では None。
    #[cfg(test)]
    pub(super) fn spawn_dummy() -> Option<Self> {
        let child = std::process::Command::new("cat")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        Self::new(child).ok()
    }

    /// テスト用: 子プロセスの PID。
    #[cfg(test)]
    pub(super) fn child_pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for AppServerClient {
    /// パニック/drop 時の孤児化を防ぐ。closing（stdin クローズ済みで終了待ち）でも
    /// 最終的に kill + wait して確実にゾンビ化を避ける。
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_request_shape() {
        let v = initialize_request(1, "addness-tui", "0.6.0");
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["method"], "initialize");
        assert_eq!(v["params"]["clientInfo"]["name"], "addness-tui");
        assert_eq!(v["params"]["clientInfo"]["version"], "0.6.0");
        assert_eq!(v["params"]["capabilities"]["experimentalApi"], true);
        assert!(v["params"]["capabilities"]["optOutNotificationMethods"].is_array());
    }

    #[test]
    fn initialized_notification_has_no_id() {
        let v = initialized_notification();
        assert_eq!(v["method"], "initialized");
        assert!(v.get("id").is_none());
    }

    #[test]
    fn thread_start_and_resume_apply_config() {
        let config = ThreadConfig {
            cwd: Some("/repo".to_string()),
            model: Some("gpt-5".to_string()),
            approval_policy: Some("untrusted".to_string()),
            sandbox: Some("workspace-write".to_string()),
            developer_instructions: Some("do the thing".to_string()),
        };
        let start = thread_start_request(2, &config);
        assert_eq!(start["method"], "thread/start");
        assert_eq!(start["params"]["cwd"], "/repo");
        assert_eq!(start["params"]["model"], "gpt-5");
        assert_eq!(start["params"]["approvalPolicy"], "untrusted");
        assert_eq!(start["params"]["sandbox"], "workspace-write");
        assert_eq!(start["params"]["developerInstructions"], "do the thing");

        let resume = thread_resume_request(3, "019f-thread", &config);
        assert_eq!(resume["method"], "thread/resume");
        assert_eq!(resume["params"]["threadId"], "019f-thread");
        assert_eq!(resume["params"]["model"], "gpt-5");
    }

    #[test]
    fn thread_start_omits_unset_fields() {
        let start = thread_start_request(2, &ThreadConfig::default());
        assert!(start["params"].get("model").is_none());
        assert!(start["params"].get("cwd").is_none());
        assert!(start["params"].as_object().unwrap().is_empty());
    }

    #[test]
    fn turn_start_request_shape() {
        let v = turn_start_request(4, "th-1", "1+1は?", Some("high"), &[]);
        assert_eq!(v["method"], "turn/start");
        assert_eq!(v["params"]["threadId"], "th-1");
        assert_eq!(v["params"]["input"][0]["type"], "text");
        assert_eq!(v["params"]["input"][0]["text"], "1+1は?");
        assert_eq!(v["params"]["effort"], "high");
    }

    #[test]
    fn turn_start_omits_effort_when_none() {
        let v = turn_start_request(4, "th-1", "hi", None, &[]);
        assert!(v["params"].get("effort").is_none());
    }

    #[test]
    fn turn_start_appends_local_images() {
        let images = vec!["/tmp/a.png".to_string(), "/tmp/b.png".to_string()];
        let v = turn_start_request(4, "th-1", "見て", None, &images);
        let input = v["params"]["input"].as_array().expect("input array");
        assert_eq!(input.len(), 3);
        assert_eq!(input[0]["type"], "text");
        assert_eq!(input[1]["type"], "localImage");
        assert_eq!(input[1]["path"], "/tmp/a.png");
        assert_eq!(input[2]["type"], "localImage");
        assert_eq!(input[2]["path"], "/tmp/b.png");
    }

    #[test]
    fn interrupt_request_shape() {
        let v = turn_interrupt_request(5, "th-1", "turn-9");
        assert_eq!(v["method"], "turn/interrupt");
        assert_eq!(v["params"]["threadId"], "th-1");
        assert_eq!(v["params"]["turnId"], "turn-9");
    }

    #[test]
    fn settings_update_builds_only_set_fields() {
        let update = SettingsUpdate {
            model: Some("gpt-5.5".to_string()),
            effort: None,
            approval_policy: None,
            sandbox: Some("read-only".to_string()),
        };
        let v = settings_update_request(6, "th-1", &update).expect("some");
        assert_eq!(v["method"], "thread/settings/update");
        assert_eq!(v["params"]["model"], "gpt-5.5");
        assert!(v["params"].get("effort").is_none());
        assert_eq!(v["params"]["sandboxPolicy"]["type"], "readOnly");
    }

    #[test]
    fn settings_update_empty_returns_none() {
        assert!(settings_update_request(6, "th-1", &SettingsUpdate::default()).is_none());
    }

    #[test]
    fn approval_and_error_responses() {
        let id = json!(0);
        let ok = approval_result(&id, "accept");
        assert_eq!(ok["id"], 0);
        assert_eq!(ok["result"]["decision"], "accept");

        let err = error_response(&json!(2), -32601, "method not found");
        assert_eq!(err["error"]["code"], -32601);
        assert_eq!(err["error"]["message"], "method not found");

        let perm = permissions_result(&json!("srv-1"), json!({}), "turn");
        assert_eq!(perm["id"], "srv-1");
        assert_eq!(perm["result"]["scope"], "turn");
    }

    #[test]
    fn parse_response_result_and_error() {
        let ok = json!({"jsonrpc": "2.0", "id": 2, "result": {"thread": {"id": "abc"}}});
        match parse_message(&ok) {
            Some(ServerMessage::Response { id, result, error }) => {
                assert_eq!(id, 2);
                assert!(error.is_none());
                assert_eq!(
                    thread_id_from_response(&result.unwrap()).as_deref(),
                    Some("abc")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        let err = json!({"jsonrpc": "2.0", "id": 3, "error": {"code": -32600, "message": "bad"}});
        match parse_message(&err) {
            Some(ServerMessage::Response {
                id, error: Some(e), ..
            }) => {
                assert_eq!(id, 3);
                assert_eq!(e.code, -32600);
                assert_eq!(e.message, "bad");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_command_approval_request() {
        let v = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "item/commandExecution/requestApproval",
            "params": {
                "itemId": "call_1",
                "command": "/bin/zsh -lc 'ls'",
                "availableDecisions": ["accept", {"acceptWithExecpolicyAmendment": {}}, "decline"]
            }
        });
        match parse_message(&v) {
            Some(ServerMessage::Approval(req)) => {
                assert_eq!(req.kind, ApprovalKind::Command);
                assert_eq!(req.id, json!(0));
                assert_eq!(req.item_id.as_deref(), Some("call_1"));
                assert_eq!(req.summary, "ls");
                assert_eq!(
                    req.available_decisions,
                    vec![
                        "accept".to_string(),
                        "acceptWithExecpolicyAmendment".to_string(),
                        "decline".to_string()
                    ]
                );
                assert!(!req.allows_session());
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_command_approval_request_with_args() {
        let v = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "item/commandExecution/requestApproval",
            "params": {
                "itemId": "call_1",
                "command": "/bin/zsh",
                "args": ["-lc", "cargo test"],
                "availableDecisions": ["accept", "decline"]
            }
        });
        match parse_message(&v) {
            Some(ServerMessage::Approval(req)) => {
                assert_eq!(req.kind, ApprovalKind::Command);
                assert_eq!(req.summary, "cargo test");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_permissions_request_allows_session() {
        let v = json!({
            "jsonrpc": "2.0",
            "id": "srv-2",
            "method": "item/permissions/requestApproval",
            "params": {"itemId": "call_2", "reason": "書き込み権限", "permissions": {"network": {"enabled": true}}}
        });
        match parse_message(&v) {
            Some(ServerMessage::Approval(req)) => {
                assert_eq!(req.kind, ApprovalKind::Permissions);
                assert_eq!(req.id, json!("srv-2"));
                assert!(req.allows_session());
                assert_eq!(
                    req.requested_permissions,
                    Some(json!({"network": {"enabled": true}}))
                );
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_unhandled_server_request() {
        let v = json!({"jsonrpc": "2.0", "id": 7, "method": "currentTime/read", "params": {}});
        match parse_message(&v) {
            Some(ServerMessage::UnhandledRequest { id, method }) => {
                assert_eq!(id, json!(7));
                assert_eq!(method, "currentTime/read");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_turn_completed_and_delta_notifications() {
        let done = json!({
            "jsonrpc": "2.0",
            "method": "turn/completed",
            "params": {"threadId": "t", "turn": {"id": "x", "status": "interrupted", "items": []}}
        });
        assert_eq!(
            parse_message(&done),
            Some(ServerMessage::Notification(Notification::TurnCompleted {
                status: "interrupted".to_string()
            }))
        );

        let delta = json!({
            "jsonrpc": "2.0",
            "method": "item/agentMessage/delta",
            "params": {"itemId": "i", "delta": "こんにちは"}
        });
        assert_eq!(
            parse_message(&delta),
            Some(ServerMessage::Notification(
                Notification::AgentMessageDelta {
                    delta: "こんにちは".to_string()
                }
            ))
        );
    }

    #[test]
    fn parse_output_delta_notification() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "item/commandExecution/outputDelta",
            "params": {"itemId": "call_1", "delta": "line2\n"}
        });
        assert_eq!(
            parse_message(&v),
            Some(ServerMessage::Notification(
                Notification::CommandOutputDelta {
                    item_id: "call_1".to_string(),
                    delta: "line2\n".to_string()
                }
            ))
        );
    }

    #[test]
    fn parse_command_execution_item() {
        let started = json!({
            "jsonrpc": "2.0",
            "method": "item/started",
            "params": {"item": {"id": "call_1", "type": "commandExecution", "command": "cargo build"}}
        });
        match parse_message(&started) {
            Some(ServerMessage::Notification(Notification::ItemStarted(item))) => {
                assert_eq!(item.id, "call_1");
                match item.kind {
                    ThreadItemKind::CommandExecution {
                        command, exit_code, ..
                    } => {
                        assert_eq!(command, "cargo build");
                        assert!(exit_code.is_none());
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            other => panic!("unexpected {other:?}"),
        }

        let started_with_args = json!({
            "jsonrpc": "2.0",
            "method": "item/started",
            "params": {"item": {"id": "call_2", "type": "commandExecution", "command": "/bin/zsh", "args": ["-lc", "cargo test"]}}
        });
        match parse_message(&started_with_args) {
            Some(ServerMessage::Notification(Notification::ItemStarted(item))) => {
                assert_eq!(item.id, "call_2");
                match item.kind {
                    ThreadItemKind::CommandExecution { command, .. } => {
                        assert_eq!(command, "cargo test");
                    }
                    other => panic!("unexpected {other:?}"),
                }
            }
            other => panic!("unexpected {other:?}"),
        }

        let completed = json!({
            "jsonrpc": "2.0",
            "method": "item/completed",
            "params": {"item": {"id": "call_1", "type": "commandExecution", "command": "cargo build", "exitCode": 0, "durationMs": 1200, "status": "completed"}}
        });
        match parse_message(&completed) {
            Some(ServerMessage::Notification(Notification::ItemCompleted(item))) => match item.kind
            {
                ThreadItemKind::CommandExecution {
                    exit_code,
                    duration_ms,
                    status,
                    ..
                } => {
                    assert_eq!(exit_code, Some(0));
                    assert_eq!(duration_ms, Some(1200));
                    assert_eq!(status.as_deref(), Some("completed"));
                }
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_agent_message_and_file_change_items() {
        let msg = json!({
            "jsonrpc": "2.0",
            "method": "item/completed",
            "params": {"item": {"id": "m1", "type": "agentMessage", "text": "答えは2", "phase": "final_answer"}}
        });
        match parse_message(&msg) {
            Some(ServerMessage::Notification(Notification::ItemCompleted(item))) => match item.kind
            {
                ThreadItemKind::AgentMessage { text, phase } => {
                    assert_eq!(text, "答えは2");
                    assert_eq!(phase.as_deref(), Some("final_answer"));
                }
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }

        let fc = json!({
            "jsonrpc": "2.0",
            "method": "item/completed",
            "params": {"item": {"id": "f1", "type": "fileChange", "status": "completed", "changes": [{"path": "src/a.rs", "kind": {"type": "update"}, "diff": "@@\n-old\n+new\n"}]}}
        });
        match parse_message(&fc) {
            Some(ServerMessage::Notification(Notification::ItemCompleted(item))) => match item.kind
            {
                ThreadItemKind::FileChange { changes, .. } => {
                    assert_eq!(changes.len(), 1);
                    assert_eq!(changes[0].path, "src/a.rs");
                    assert_eq!(changes[0].change_type, "update");
                    assert_eq!(changes[0].diff, "@@\n-old\n+new\n");
                }
                other => panic!("unexpected {other:?}"),
            },
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_token_usage_notification() {
        let v = json!({
            "jsonrpc": "2.0",
            "method": "thread/tokenUsage/updated",
            "params": {"tokenUsage": {"total": {"totalTokens": 12711}, "last": {"totalTokens": 42}, "modelContextWindow": 258400}}
        });
        match parse_message(&v) {
            Some(ServerMessage::Notification(Notification::TokenUsage(usage))) => {
                assert_eq!(usage.total_tokens, Some(12711));
                assert_eq!(usage.last_total_tokens, Some(42));
                assert_eq!(usage.model_context_window, Some(258400));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn non_jsonrpc_value_is_none() {
        // ワンショット codex の snake_case イベントは JSON-RPC ではない。
        let v = json!({"type": "thread.started", "thread_id": "abc"});
        assert_eq!(parse_message(&v), None);
    }

    #[test]
    fn parse_message_without_jsonrpc_field() {
        // codex 0.142.5 は応答・通知・サーバ発リクエストに "jsonrpc":"2.0" を付けない。
        // 応答（id + result）。
        let resp = json!({"id": 1, "result": {"thread": {"id": "abc"}}});
        match parse_message(&resp) {
            Some(ServerMessage::Response { id, result, error }) => {
                assert_eq!(id, 1);
                assert!(error.is_none());
                assert_eq!(
                    thread_id_from_response(&result.unwrap()).as_deref(),
                    Some("abc")
                );
            }
            other => panic!("unexpected {other:?}"),
        }
        // 通知（method のみ）。
        let notif = json!({"method": "turn/started", "params": {"threadId": "t"}});
        assert_eq!(
            parse_message(&notif),
            Some(ServerMessage::Notification(Notification::TurnStarted))
        );
        // サーバ発リクエスト（method + id、承認要求）。
        let approval = json!({
            "id": 0,
            "method": "item/commandExecution/requestApproval",
            "params": {"itemId": "call_1", "command": "ls", "availableDecisions": ["accept", "decline"]}
        });
        match parse_message(&approval) {
            Some(ServerMessage::Approval(req)) => {
                assert_eq!(req.kind, ApprovalKind::Command);
                assert_eq!(req.id, json!(0));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_message_rejects_wrong_jsonrpc_version() {
        // jsonrpc が明示されているのに "2.0" でない場合は弾く。
        let v = json!({"jsonrpc": "1.0", "id": 1, "result": {}});
        assert_eq!(parse_message(&v), None);
    }

    #[test]
    fn looks_like_jsonrpc_detects_shapes() {
        // jsonrpc あり。
        assert!(looks_like_jsonrpc(&json!({"jsonrpc": "2.0", "id": 1})));
        // jsonrpc 無し・method 文字列。
        assert!(looks_like_jsonrpc(&json!({"method": "turn/started"})));
        // jsonrpc 無し・id + result。
        assert!(looks_like_jsonrpc(&json!({"id": 1, "result": {}})));
        // jsonrpc 無し・id + error。
        assert!(looks_like_jsonrpc(
            &json!({"id": 1, "error": {"code": -1, "message": "x"}})
        ));
        // ワンショット exec の snake_case イベント（type 持ち・method/result/error なし）は false。
        assert!(!looks_like_jsonrpc(
            &json!({"type": "thread.started", "thread_id": "abc"})
        ));
        assert!(!looks_like_jsonrpc(
            &json!({"type": "item.completed", "item": {"id": "x"}})
        ));
        // id だけ（result/error なし）も JSON-RPC とは見なさない。
        assert!(!looks_like_jsonrpc(&json!({"id": 1})));
    }

    #[test]
    fn drop_kills_child_process() {
        let Some(client) = AppServerClient::spawn_dummy() else {
            return; // `cat` 不在環境ではスキップ。
        };
        let pid = client.child_pid();
        drop(client);
        // Drop 内で wait() 済みなので同期的に reap されている。
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(!alive, "drop 後も子プロセスが生存している");
    }

    // -----------------------------------------------------------------------
    // 上流プローブ（#[ignore]・実 codex バイナリ相手）
    //
    // 通常の `cargo test` では走らない。CI の upstream-sync が新バージョンの実
    // バイナリをインストールした上で `--ignored` 付きで実行し、チェンジログに
    // 現れない統合サーフェスの破壊的変更（例: 0.142.5 の `jsonrpc` フィールド欠落）を
    // 実測検知する。ローカル実行: `cargo test upstream_probe_ -- --ignored`。
    // バイナリは `ADDNESS_PROBE_CODEX_BIN`（未設定時 `codex`）で差し替え可能。
    // -----------------------------------------------------------------------

    /// プローブ対象の codex バイナリ。`ADDNESS_PROBE_CODEX_BIN` 優先、無ければ `codex`。
    fn probe_codex_bin() -> String {
        std::env::var("ADDNESS_PROBE_CODEX_BIN").unwrap_or_else(|_| "codex".to_string())
    }

    /// パニック時も子プロセスを確実に kill + wait するためのガード。
    struct ProbeChildGuard(std::process::Child);
    impl Drop for ProbeChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// 実 `codex app-server` を spawn して initialize ハンドシェイクを実測する。
    /// stdout の読み取りは別スレッド + mpsc + recv_timeout で行い、本スレッドを
    /// ハングさせない。応答（`Response { id: 1, .. }`）が来るまで通知行を読み飛ばす。
    #[test]
    #[ignore = "実 codex バイナリが必要（CI の upstream-sync が --ignored で実行）"]
    fn upstream_probe_codex_appserver_handshake() {
        use std::io::{BufRead, BufReader, Write};

        let bin = probe_codex_bin();
        let child = std::process::Command::new(&bin)
            .arg("app-server")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("codex app-server の起動に失敗しました（bin={bin}）: {e}"));
        let mut child = ProbeChildGuard(child);

        let mut stdin = child.0.stdin.take().expect("app-server stdin");
        let stdout = child.0.stdout.take().expect("app-server stdout");

        // stdout を別スレッドで 1 行ずつ読み、mpsc へ流す（本スレッドをハングさせない）。
        let (tx, rx) = mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        if tx.send(line.clone()).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });

        // initialize リクエストを stdin へ送る（auth 不要な範囲に留める）。
        let req = initialize_request(1, "addness-probe", env!("CARGO_PKG_VERSION"));
        let payload = serde_json::to_string(&req).expect("initialize をシリアライズできません");
        (|| -> std::io::Result<()> {
            stdin.write_all(payload.as_bytes())?;
            stdin.write_all(b"\n")?;
            stdin.flush()
        })()
        .expect("initialize リクエストの送信に失敗しました");

        // 応答行を最大10秒待つ。通知（method 持ち）が先行しうるので最大10行読み飛ばす。
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut got_response = false;
        for _ in 0..10 {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            let Ok(line) = rx.recv_timeout(remaining) else {
                break; // タイムアウト or 送信側切断
            };
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let value: Value = serde_json::from_str(trimmed).unwrap_or_else(|e| {
                panic!("app-server 出力が JSON ではありません: {trimmed:?}（{e}）")
            });
            assert!(
                looks_like_jsonrpc(&value),
                "app-server 出力が JSON-RPC 形に見えません: {trimmed}"
            );
            match parse_message(&value) {
                Some(ServerMessage::Response { id: 1, error, .. }) => {
                    assert!(
                        error.is_none(),
                        "initialize が JSON-RPC エラーを返しました: {error:?}"
                    );
                    got_response = true;
                    break;
                }
                // 通知・サーバ発リクエストなどは Response が来るまで読み飛ばす。
                _ => continue,
            }
        }

        drop(stdin); // stdin をクローズ（reader スレッドは kill 後の EOF で終了する）。
        assert!(
            got_response,
            "10秒以内に initialize の Response(id=1) を受信できませんでした（bin={bin}）"
        );
        // child は ProbeChildGuard の Drop で kill + wait される。
    }

    /// 実 `codex exec --help` / `codex --help` に、本リポジトリがワンショット/常駐で
    /// 渡すサブコマンド・フラグが存在することを確認する（`codex.rs` の引数ビルダー由来）。
    #[test]
    #[ignore = "実 codex バイナリが必要（CI の upstream-sync が --ignored で実行）"]
    fn upstream_probe_codex_cli_flags() {
        let bin = probe_codex_bin();
        let exec_help = probe_help(&bin, &["exec", "--help"]);
        let root_help = probe_help(&bin, &["--help"]);
        let combined = format!("{exec_help}\n{root_help}");

        // codex_exec_args / codex_exec_resume_args / push_global_exec_settings /
        // push_optional_exec_settings が組み立てるサブコマンド・フラグ。
        const REQUIRED: &[&str] = &[
            "exec",                                       // exec サブコマンド
            "resume",                                     // exec resume サブコマンド
            "--json",                                     // イベントを JSON Lines で受信
            "--model",                                    // -m モデル指定
            "--sandbox",                                  // -s サンドボックス
            "--image",                                    // -i 画像入力
            "--color",                                    // カラー出力
            "--cd",                                       // -C 作業ディレクトリ
            "--add-dir",                                  // 書込許可ディレクトリ追加
            "--config",              // -c key=value（developer_instructions 等）
            "--ask-for-approval",    // -a 承認ポリシー
            "--search",              // Web 検索
            "--skip-git-repo-check", // git リポジトリ外での実行
            "--ignore-user-config",  // ユーザ設定を無視
            "--dangerously-bypass-approvals-and-sandbox", // 承認/サンドボックス全バイパス
        ];
        let missing: Vec<&str> = REQUIRED
            .iter()
            .copied()
            .filter(|flag| !combined.contains(flag))
            .collect();
        assert!(
            missing.is_empty(),
            "codex help に存在しないフラグ/サブコマンド: {missing:?}（bin={bin}）"
        );
    }

    /// 指定バイナリを与えた引数で実行し、stdout+stderr を連結して返す（help 取得用）。
    fn probe_help(bin: &str, args: &[&str]) -> String {
        let output = std::process::Command::new(bin)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("{bin} {args:?} の実行に失敗しました: {e}"));
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        text
    }
}
