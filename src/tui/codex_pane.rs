//! TUI 内で `codex exec --json` を起動し、JSONL イベントを Addness 側の
//! 会話履歴として描画するためのモジュール。
//!
//! Codex の対話型 TUI は使わず、入力・履歴・スクロール・イベント表示を
//! Addness 側で持つ。各ユーザー入力ごとに `codex exec --json` を 1 ターン実行し、
//! 2 ターン目以降は `codex exec resume <thread_id> --json` で同じ Codex セッションを
//! 継続する。

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

use anyhow::{Context, Result};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub const CODEX_LOG_PREFIX_WIDTH: usize = 7;

/// スラッシュコマンドのパレット候補（コマンド名, 1 行説明）。
/// `handle_local_slash_command` が受け付ける主要コマンドの正本。よく使う
/// Addness 連携・セッション操作を上に、codex 委譲・設定系を下に並べる。
/// 表示はコマンド名の先頭一致で絞り込む。
const SLASH_COMMANDS: &[(&str, &str)] = &[
    ("/goal", "Goal モードを開始 / 更新"),
    ("/work", "子ゴールの作業単位へ進む"),
    ("/organize", "子ゴールに分解する"),
    ("/remember", "メモを Addness の body へ保存"),
    ("/handoff", "会話コンテキストを Addness へ保存"),
    ("/exec", "Goal モードを通さず直接送信"),
    ("/init", "AGENTS.md を作成 / 更新"),
    ("/plan", "実装前の計画を依頼"),
    ("/compact", "会話を圧縮する"),
    ("/skills", "ローカル skill 一覧 / 使用依頼"),
    ("/new", "新しいセッションを開始"),
    ("/clear", "表示ログをクリア"),
    ("/stop", "実行中のターンを中断"),
    ("/sessions", "セッション候補を番号付きで表示"),
    ("/resume", "作業メモ・決定ログから再開"),
    ("/resume-last", "最新セッションを継続"),
    ("/resume-session", "番号 / id のセッションを継続"),
    ("/fork", "セッションを fork"),
    ("/fork-last", "最新セッションを fork"),
    ("/fork-session", "番号 / id のセッションを fork"),
    ("/rename", "現在のセッション名を変更"),
    ("/archive", "セッションをアーカイブ"),
    ("/history", "セッション履歴を表示"),
    ("/turn", "指定 turn を展開 / 格納"),
    ("/diff", "ファイル編集の diff を表示"),
    ("/ps", "実行中 turn / 予約を表示"),
    ("/btw", "最後の応答を Markdown で表示"),
    ("/model", "次ターンのモデルを切替 / 指定"),
    ("/reasoning", "推論強度を切替 / 指定"),
    ("/approval", "承認モードを切替"),
    ("/sandbox", "sandbox を切替 / 指定"),
    ("/permissions", "承認 / sandbox 権限を設定"),
    ("/personality", "通信スタイルを切替"),
    ("/settings", "モデル・推論・承認・sandbox 設定を表示"),
    ("/cd", "次セッションの作業ルートを変更"),
    ("/add-dir", "書込許可ディレクトリを追加"),
    ("/image", "画像を添付する"),
    ("/attachments", "添付の一覧・追加・クリア"),
    ("/theme", "テーマを設定"),
    ("/vim", "Vim composer を切替"),
    ("/profile", "codex profile を適用"),
    ("/search", "web search を切替"),
    ("/config", "codex config override を追加"),
    ("/mcp", "MCP の一覧・管理"),
    ("/review", "codex review を実行"),
    ("/apply", "codex apply <task_id> を実行"),
    ("/cloud", "Codex Cloud task を操作"),
    ("/apps", "Desktop / app-server / remote 入口"),
    ("/import", "他ツール設定を検出 / AGENTS 化"),
    ("/hooks", "hook 設定の override を表示 / 設定"),
    ("/codex", "任意の codex サブコマンドを実行"),
    ("/login", "ログイン状態を確認"),
    ("/logout", "ログアウトする"),
    ("/status", "現在の状態を表示"),
    ("/usage", "token 使用量を表示"),
    ("/help", "スラッシュコマンド一覧を表示"),
];

const CODEX_SESSION_HISTORY_DIR: &str = "codex-sessions";
const CODEX_SESSION_HISTORY_MAX_LOG_LINES: usize = 5_000;
const CODEX_SESSION_HISTORY_MAX_RECORDS: usize = 20_000;
const CODEX_SESSION_HISTORY_MAX_BYTES: u64 = 20 * 1024 * 1024;

/// codex 実行ファイルのパスを解決する。
/// 環境変数 `ADDNESS_CODEX_BIN` を最優先で見て、無ければ PATH 上を探す。
/// 見つからなければ `None`。
pub fn codex_path() -> Option<PathBuf> {
    // 明示指定（別パスにインストールした場合や検証用の上書き）を最優先。
    if let Some(bin) = std::env::var_os("ADDNESS_CODEX_BIN") {
        let cand = PathBuf::from(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join("codex");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// プロンプトを子プロセスへ渡す方式。backend ごとに異なる。
/// codex は stdin へ書き込み、引数末尾に sentinel `-` を置く（`Stdin`）。
/// 将来の backend（例: `claude -p "<prompt>"`）は引数として渡す（`Arg`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptDelivery {
    /// stdin へ書き込む（codex exec 方式）。
    Stdin,
    /// コマンドライン引数として渡す。まだ実装 backend が無いため未構築。
    #[allow(dead_code)]
    Arg,
}

/// backend 非依存に正規化したターンライフサイクルイベント。
///
/// codex の `thread.started` / `turn.*` / `error` といった型名の違いを
/// この共通形へマップし、`CodexPane` 側は型名文字列ではなくこの enum で
/// 分岐する。ツール実行やアシスタント本文など表層に近いイベントは、当面
/// `handle_generic_json_event` 側で扱うため `Other` に集約する。
enum AgentEvent {
    /// セッション開始（codex: `thread.started`）。値はセッション/スレッド ID。
    SessionStarted(Option<String>),
    /// ターン開始（codex: `turn.started`）。
    TurnStarted,
    /// ターン完了（codex: `turn.completed` / `turn.finished`）。
    TurnCompleted,
    /// ターン失敗（codex: `turn.failed`）。値は表示用メッセージ。
    TurnFailed(String),
    /// エラー通知（codex: `error`）。値は表示用メッセージ。
    Error(String),
    /// 上記以外。ツール実行・本文など、backend 共通の表層処理へ委ねる。
    Other,
}

/// TUI から起動できるコーディングエージェントの継ぎ目。
///
/// 同一 TUI から複数のエージェント（現状 codex、将来 Claude Code 等）を
/// 呼べるようにするための抽象。CLI 引数生成・バイナリ解決・プロンプト受け渡し
/// 方式など backend 固有の差分をこの trait に閉じ込め、`CodexPane` 側は
/// backend 非依存のロジックだけを持つ形へ寄せていく。
///
/// 現状は `CodexBackend` のみが実装する。
///
/// 設定型は関連型 `Settings` に閉じ込め、ライフサイクルイベントの型名解釈
/// （`parse_lifecycle_event` / `session_id_from_event`）を backend 側へ寄せる。
/// バイナリ解決（`resolve_bin`）・resume 可否・ツール/本文イベントの正規化は、
/// 呼び出し側（app.rs）の抽象化と Claude backend の追加に合わせ後続で加える。
trait AgentBackend {
    /// backend 固有の実行設定型。
    type Settings;

    /// 通常ターンの CLI 引数を生成する（新規 / resume を `session` で分岐）。
    fn turn_args(&self, session: Option<&str>, cwd: &str, settings: &Self::Settings)
    -> Vec<String>;

    /// プロンプトの受け渡し方式。
    fn prompt_delivery(&self) -> PromptDelivery;

    /// JSON イベント 1 件を backend 非依存のライフサイクルイベントへ正規化する。
    /// ツール/本文など表層イベントは `AgentEvent::Other` に集約する。
    fn parse_lifecycle_event(&self, value: &Value) -> AgentEvent;

    /// セッション開始イベントからセッション/スレッド ID を抽出する。
    fn session_id_from_event(&self, value: &Value) -> Option<String>;

    /// スラッシュコマンドのヘルプ本文。
    fn help_text(&self) -> &'static str;
}

/// codex CLI（`codex exec --json`）を駆動する backend。
struct CodexBackend;

impl AgentBackend for CodexBackend {
    type Settings = CodexExecSettings;

    fn turn_args(
        &self,
        session: Option<&str>,
        cwd: &str,
        settings: &Self::Settings,
    ) -> Vec<String> {
        codex_exec_args(session, cwd, settings)
    }

    fn prompt_delivery(&self) -> PromptDelivery {
        // codex exec は stdin から読み、引数末尾の `-` と対になる。
        PromptDelivery::Stdin
    }

    fn session_id_from_event(&self, value: &Value) -> Option<String> {
        string_at_any(value, &["thread_id", "threadId", "id"])
    }

    fn parse_lifecycle_event(&self, value: &Value) -> AgentEvent {
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("event");
        match event_type {
            "thread.started" => AgentEvent::SessionStarted(self.session_id_from_event(value)),
            "turn.started" => AgentEvent::TurnStarted,
            "turn.completed" | "turn.finished" => AgentEvent::TurnCompleted,
            "turn.failed" => {
                let message = nested_error_message(value)
                    .or_else(|| first_text_field(value))
                    .unwrap_or_else(|| "Codex ターンが失敗しました".to_string());
                AgentEvent::TurnFailed(message)
            }
            "error" => {
                let message = nested_error_message(value)
                    .or_else(|| first_text_field(value))
                    .unwrap_or_else(|| "Codex エラー".to_string());
                AgentEvent::Error(message)
            }
            _ => AgentEvent::Other,
        }
    }

    fn help_text(&self) -> &'static str {
        slash_help_text()
    }
}

/// 作業ディレクトリの `git diff --stat`（HEAD 比較）を取得する。
/// 還流コメントのプリフィルに使う。取得できなければ空文字。
pub fn git_diff_stat(cwd: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["diff", "--stat", "HEAD"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

pub fn git_branch_label(cwd: &Path) -> String {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .current_dir(cwd)
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let branch = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if branch.is_empty() {
                "(detached)".to_string()
            } else {
                branch
            }
        }
        _ => "(unknown)".to_string(),
    }
}

pub fn git_diff_preview(cwd: &Path) -> String {
    let stat = git_diff_stat(cwd);
    let output = std::process::Command::new("git")
        .args(["diff", "--color=never", "--unified=3", "HEAD"])
        .current_dir(cwd)
        .output();
    let diff = match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            return format!("git diff failed: {stderr}");
        }
        Err(e) => return format!("git diff failed: {e}"),
    };
    if stat.trim().is_empty() && diff.trim().is_empty() {
        return "差分はありません。".to_string();
    }

    let mut lines = Vec::new();
    if !stat.trim().is_empty() {
        lines.push("## git diff --stat HEAD".to_string());
        lines.extend(stat.lines().map(str::to_string));
        lines.push(String::new());
    }
    lines.push("## git diff --unified=3 HEAD".to_string());
    let max_diff_lines = 400usize;
    let mut omitted = 0usize;
    for (idx, line) in diff.lines().enumerate() {
        if idx < max_diff_lines {
            lines.push(line.to_string());
        } else {
            omitted += 1;
        }
    }
    if omitted > 0 {
        lines.push(format!("... {omitted} diff lines omitted"));
    }
    lines.join("\n")
}

/// Addness 独自 UI に表示する Codex ログ行の種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodexLogKind {
    User,
    Assistant,
    Tool,
    Turn,
    System,
    Error,
    Event,
}

/// Addness 独自 UI に表示する Codex ログ行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CodexLogLine {
    pub kind: CodexLogKind,
    pub text: String,
}

impl CodexLogLine {
    fn new(kind: CodexLogKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            text: text.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLogFilter {
    All,
    Conversation,
    Tools,
    Errors,
}

impl CodexLogFilter {
    fn next(self) -> Self {
        match self {
            Self::Conversation => Self::Tools,
            Self::Tools => Self::Errors,
            Self::Errors => Self::All,
            Self::All => Self::Conversation,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Conversation => "会話",
            Self::Tools => "実行",
            Self::Errors => "失敗",
            Self::All => "全部",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexModelChoice {
    Config,
    Gpt55,
    Gpt5,
    O3,
}

impl CodexModelChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Gpt55,
            Self::Gpt55 => Self::Gpt5,
            Self::Gpt5 => Self::O3,
            Self::O3 => Self::Config,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Gpt55 => "gpt-5.5",
            Self::Gpt5 => "gpt-5",
            Self::O3 => "o3",
        }
    }

    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Gpt55 => Some("gpt-5.5"),
            Self::Gpt5 => Some("gpt-5"),
            Self::O3 => Some("o3"),
        }
    }
}

fn parse_builtin_model_choice(value: &str) -> Option<CodexModelChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(CodexModelChoice::Config),
        "gpt-5.5" | "gpt5.5" | "gpt55" => Some(CodexModelChoice::Gpt55),
        "gpt-5" | "gpt5" => Some(CodexModelChoice::Gpt5),
        "o3" => Some(CodexModelChoice::O3),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexReasoningChoice {
    Config,
    Low,
    Medium,
    High,
    XHigh,
}

impl CodexReasoningChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::XHigh,
            Self::XHigh => Self::Config,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    fn config_value(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::XHigh => Some("xhigh"),
        }
    }
}

fn parse_reasoning_choice(value: &str) -> Option<CodexReasoningChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(CodexReasoningChoice::Config),
        "low" => Some(CodexReasoningChoice::Low),
        "medium" | "med" => Some(CodexReasoningChoice::Medium),
        "high" => Some(CodexReasoningChoice::High),
        "xhigh" | "extra-high" | "extra_high" => Some(CodexReasoningChoice::XHigh),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexApprovalChoice {
    Config,
    Untrusted,
    OnRequest,
    OnFailure,
    Never,
}

impl CodexApprovalChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Untrusted,
            Self::Untrusted => Self::OnRequest,
            Self::OnRequest => Self::OnFailure,
            Self::OnFailure => Self::Never,
            Self::Never => Self::Config,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Untrusted => "untrusted",
            Self::OnRequest => "on-request",
            Self::OnFailure => "on-failure",
            Self::Never => "never",
        }
    }

    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Untrusted => Some("untrusted"),
            Self::OnRequest => Some("on-request"),
            Self::OnFailure => Some("on-failure"),
            Self::Never => Some("never"),
        }
    }
}

fn parse_approval_choice(value: &str) -> Option<CodexApprovalChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(CodexApprovalChoice::Config),
        "untrusted" => Some(CodexApprovalChoice::Untrusted),
        "on-request" | "onrequest" => Some(CodexApprovalChoice::OnRequest),
        "on-failure" | "onfailure" => Some(CodexApprovalChoice::OnFailure),
        "never" => Some(CodexApprovalChoice::Never),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexSandboxChoice {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

impl CodexSandboxChoice {
    fn next(self) -> Self {
        match self {
            Self::ReadOnly => Self::WorkspaceWrite,
            Self::WorkspaceWrite => Self::DangerFullAccess,
            Self::DangerFullAccess => Self::ReadOnly,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::WorkspaceWrite => "workspace-write",
            Self::DangerFullAccess => "danger-full-access",
        }
    }

    fn cli_arg(self) -> &'static str {
        self.label()
    }
}

fn parse_sandbox_choice(value: &str) -> Option<CodexSandboxChoice> {
    match value.to_ascii_lowercase().as_str() {
        "read-only" | "readonly" => Some(CodexSandboxChoice::ReadOnly),
        "workspace-write" | "workspace" | "workspacewrite" => {
            Some(CodexSandboxChoice::WorkspaceWrite)
        }
        "danger-full-access" | "danger" | "full" | "dangerfullaccess" => {
            Some(CodexSandboxChoice::DangerFullAccess)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexLocalProviderChoice {
    Config,
    LmStudio,
    Ollama,
}

impl CodexLocalProviderChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::LmStudio,
            Self::LmStudio => Self::Ollama,
            Self::Ollama => Self::Config,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::LmStudio => "lmstudio",
            Self::Ollama => "ollama",
        }
    }

    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::LmStudio => Some("lmstudio"),
            Self::Ollama => Some("ollama"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexColorChoice {
    Never,
    Auto,
    Always,
}

impl CodexColorChoice {
    fn next(self) -> Self {
        match self {
            Self::Never => Self::Auto,
            Self::Auto => Self::Always,
            Self::Always => Self::Never,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Auto => "auto",
            Self::Always => "always",
        }
    }
}

fn parse_color_choice(value: &str) -> Option<CodexColorChoice> {
    match value.to_ascii_lowercase().as_str() {
        "never" | "off" | "none" => Some(CodexColorChoice::Never),
        "auto" | "default" => Some(CodexColorChoice::Auto),
        "always" | "on" => Some(CodexColorChoice::Always),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexExecSettings {
    model: CodexModelChoice,
    model_override: Option<String>,
    reasoning: CodexReasoningChoice,
    approval: CodexApprovalChoice,
    sandbox: CodexSandboxChoice,
    web_search: bool,
    oss: bool,
    remote_addr: Option<String>,
    remote_auth_token_env: Option<String>,
    no_alt_screen: bool,
    local_provider: CodexLocalProviderChoice,
    profile: Option<String>,
    additional_dirs: Vec<String>,
    image_paths: Vec<String>,
    config_overrides: Vec<String>,
    enabled_features: Vec<String>,
    disabled_features: Vec<String>,
    strict_config: bool,
    ignore_user_config: bool,
    ignore_rules: bool,
    skip_git_repo_check: bool,
    ephemeral: bool,
    bypass_approvals_and_sandbox: bool,
    bypass_hook_trust: bool,
    color: CodexColorChoice,
    output_schema: Option<String>,
    output_last_message: Option<String>,
}

const ADDNESS_MEMORY_CONFIG_OVERRIDES: [&str; 2] = [
    "memories.use_memories=false",
    "memories.generate_memories=false",
];

fn default_addness_memory_config_overrides() -> Vec<String> {
    ADDNESS_MEMORY_CONFIG_OVERRIDES
        .iter()
        .map(|value| (*value).to_string())
        .collect()
}

impl Default for CodexExecSettings {
    fn default() -> Self {
        Self {
            model: CodexModelChoice::Config,
            model_override: None,
            reasoning: CodexReasoningChoice::Config,
            approval: CodexApprovalChoice::Config,
            sandbox: CodexSandboxChoice::WorkspaceWrite,
            web_search: false,
            oss: false,
            remote_addr: None,
            remote_auth_token_env: None,
            no_alt_screen: false,
            local_provider: CodexLocalProviderChoice::Config,
            profile: None,
            additional_dirs: Vec::new(),
            image_paths: Vec::new(),
            config_overrides: default_addness_memory_config_overrides(),
            enabled_features: Vec::new(),
            disabled_features: Vec::new(),
            strict_config: false,
            ignore_user_config: false,
            ignore_rules: false,
            skip_git_repo_check: false,
            ephemeral: false,
            bypass_approvals_and_sandbox: false,
            bypass_hook_trust: false,
            color: CodexColorChoice::Never,
            output_schema: None,
            output_last_message: None,
        }
    }
}

impl CodexExecSettings {
    pub fn label(&self) -> String {
        let approval = if self.bypass_approvals_and_sandbox {
            "bypass-all"
        } else {
            self.approval.label()
        };
        let mut parts = vec![
            format!(
                "model:{}",
                self.model_override
                    .as_deref()
                    .unwrap_or_else(|| self.model.label())
            ),
            format!("effort:{}", self.reasoning.label()),
            format!("approval:{approval}"),
            format!("sandbox:{}", self.sandbox.label()),
        ];
        if self.web_search {
            parts.push("search:on".to_string());
        }
        if self.oss {
            parts.push("oss:on".to_string());
        }
        if let Some(remote) = &self.remote_addr {
            parts.push(format!("remote:{remote}"));
        }
        if let Some(env) = &self.remote_auth_token_env {
            parts.push(format!("remote-auth-env:{env}"));
        }
        if self.no_alt_screen {
            parts.push("no-alt-screen".to_string());
        }
        if self.local_provider != CodexLocalProviderChoice::Config {
            parts.push(format!("provider:{}", self.local_provider.label()));
        }
        if let Some(profile) = &self.profile {
            parts.push(format!("profile:{profile}"));
        }
        if !self.additional_dirs.is_empty() {
            parts.push(format!("add-dir:{}", self.additional_dirs.len()));
        }
        if !self.image_paths.is_empty() {
            parts.push(format!("image:{}", self.image_paths.len()));
        }
        if !self.config_overrides.is_empty() {
            parts.push(format!("config:{}", self.config_overrides.len()));
        }
        if !self.enabled_features.is_empty() {
            parts.push(format!("enable:{}", self.enabled_features.len()));
        }
        if !self.disabled_features.is_empty() {
            parts.push(format!("disable:{}", self.disabled_features.len()));
        }
        if self.strict_config {
            parts.push("strict-config".to_string());
        }
        if self.bypass_hook_trust {
            parts.push("bypass-hook-trust".to_string());
        }
        if self.color != CodexColorChoice::Never {
            parts.push(format!("color:{}", self.color.label()));
        }
        if self.output_schema.is_some() {
            parts.push("output-schema".to_string());
        }
        if self.output_last_message.is_some() {
            parts.push("output-last-message".to_string());
        }
        let flags = [
            (self.ignore_user_config, "ignore-user-config"),
            (self.ignore_rules, "ignore-rules"),
            (self.skip_git_repo_check, "skip-git-check"),
            (self.ephemeral, "ephemeral"),
        ]
        .into_iter()
        .filter_map(|(enabled, label)| enabled.then_some(label))
        .collect::<Vec<_>>();
        if !flags.is_empty() {
            parts.push(format!("flags:{}", flags.join(",")));
        }
        parts.join(" ")
    }

    fn memory_mode_label(&self) -> String {
        let use_memories = self.config_override_value_for("memories.use_memories");
        let generate = self.config_override_value_for("memories.generate_memories");
        match (use_memories, generate) {
            (Some("false"), Some("false")) => "Addness DB / Codex memory off".to_string(),
            (Some("true"), Some("true")) => "Codex global memory on".to_string(),
            (Some(use_memories), Some(generate)) => {
                format!("Codex memory use={use_memories} generate={generate}")
            }
            (Some(use_memories), None) => {
                format!("Codex memory use={use_memories} generate=config")
            }
            (None, Some(generate)) => {
                format!("Codex memory use=config generate={generate}")
            }
            (None, None) => "Codex memory config".to_string(),
        }
    }

    fn memory_mode_is_addness_safe(&self) -> bool {
        self.config_override_value_for("memories.use_memories") == Some("false")
            && self.config_override_value_for("memories.generate_memories") == Some("false")
    }

    fn config_override_value_for(&self, key: &str) -> Option<&str> {
        self.config_overrides
            .iter()
            .find_map(|entry| config_override_value(entry, key))
    }

    fn cycle_model(&mut self) -> &'static str {
        self.model = self.model.next();
        self.model.label()
    }

    fn model_cli_arg(&self) -> Option<&str> {
        self.model_override
            .as_deref()
            .or_else(|| self.model.cli_arg())
    }

    fn cycle_reasoning(&mut self) -> &'static str {
        self.reasoning = self.reasoning.next();
        self.reasoning.label()
    }

    fn cycle_approval(&mut self) -> &'static str {
        self.approval = self.approval.next();
        self.approval.label()
    }

    fn cycle_sandbox(&mut self) -> &'static str {
        self.sandbox = self.sandbox.next();
        self.sandbox.label()
    }

    fn toggle_web_search(&mut self) -> bool {
        self.web_search = !self.web_search;
        self.web_search
    }

    fn toggle_oss(&mut self) -> bool {
        self.oss = !self.oss;
        self.oss
    }

    fn cycle_local_provider(&mut self) -> &'static str {
        self.local_provider = self.local_provider.next();
        self.local_provider.label()
    }

    fn cycle_color(&mut self) -> &'static str {
        self.color = self.color.next();
        self.color.label()
    }
}

fn matches_log_filter(kind: CodexLogKind, filter: CodexLogFilter) -> bool {
    match filter {
        CodexLogFilter::Conversation => {
            matches!(
                kind,
                CodexLogKind::User
                    | CodexLogKind::Assistant
                    | CodexLogKind::Turn
                    | CodexLogKind::System
                    | CodexLogKind::Error
            )
        }
        CodexLogFilter::Tools => {
            matches!(kind, CodexLogKind::Tool | CodexLogKind::Event)
        }
        CodexLogFilter::Errors => matches!(kind, CodexLogKind::Error),
        CodexLogFilter::All => true,
    }
}

fn on_off(enabled: bool) -> &'static str {
    if enabled { "on" } else { "off" }
}

fn config_override_key(entry: &str) -> &str {
    entry
        .split_once('=')
        .map(|(key, _)| key.trim())
        .unwrap_or_else(|| entry.trim())
}

fn config_override_value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let (entry_key, value) = entry.split_once('=')?;
    if entry_key.trim() == key {
        Some(value.trim())
    } else {
        None
    }
}

fn toml_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

fn toml_string_array(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| toml_string(value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{items}]")
}

fn parse_bool_choice(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

fn memory_override_enables_global_memory(key: &str, raw_value: &str) -> bool {
    let key = key.trim();
    let value = raw_value.trim().trim_matches('"').trim_matches('\'');
    match key {
        "memories.use_memories" | "memories.generate_memories" => {
            parse_bool_choice(value) == Some(true)
        }
        "memories" => {
            let compact = raw_value
                .to_ascii_lowercase()
                .chars()
                .filter(|ch| !ch.is_whitespace())
                .collect::<String>();
            compact.contains("use_memories=true") || compact.contains("generate_memories=true")
        }
        _ => false,
    }
}

fn parse_statusline_items(input: &str) -> Vec<String> {
    input
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}

fn numbered_or_none(items: &[String]) -> Vec<String> {
    if items.is_empty() {
        return vec!["未設定".to_string()];
    }
    items
        .iter()
        .enumerate()
        .map(|(idx, item)| format!("{}. {item}", idx + 1))
        .collect()
}

fn parse_one_based_index(input: &str) -> Option<usize> {
    input.trim().parse::<usize>().ok()?.checked_sub(1)
}

fn split_codex_command_args(input: &str) -> Result<Vec<String>> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars().peekable();
    let mut quote = None;
    let mut escaped = false;
    let mut has_token = false;

    while let Some(ch) = chars.next() {
        if escaped {
            current.push(ch);
            has_token = true;
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            has_token = true;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            } else {
                current.push(ch);
                has_token = true;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                has_token = true;
            }
            c if c.is_whitespace() => {
                if has_token {
                    args.push(std::mem::take(&mut current));
                    has_token = false;
                }
                while chars.peek().is_some_and(|next| next.is_whitespace()) {
                    chars.next();
                }
            }
            _ => {
                current.push(ch);
                has_token = true;
            }
        }
    }

    if escaped {
        current.push('\\');
    }
    if quote.is_some() {
        anyhow::bail!("引用符が閉じられていません");
    }
    if has_token {
        args.push(current);
    }
    Ok(args)
}

fn codex_named_subcommand_args(name: &str, raw_args: &str) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    match name {
        "doctor" => {
            let mut args = vec!["doctor".to_string()];
            args.append(&mut parsed);
            Ok(args)
        }
        "features" => {
            let mut args = vec!["features".to_string()];
            if parsed.is_empty() {
                args.push("list".to_string());
            } else {
                args.append(&mut parsed);
            }
            Ok(args)
        }
        "mcp" => codex_command_with_default("mcp", "list", parsed),
        "plugin" => codex_command_with_default("plugin", "list", parsed),
        "cloud" => codex_command_with_default("cloud", "list", parsed),
        "debug" => codex_command_with_default("debug", "models", parsed),
        "login" => codex_command_with_default("login", "status", parsed),
        "help" => codex_command_with_args("help", parsed),
        "version" => Ok(vec!["--version".to_string()]),
        "logout" | "update" | "app" | "completion" => codex_command_with_args(name, parsed),
        "sandbox" | "mcp-server" | "exec-server" => codex_command_with_help_default(name, parsed),
        "app-server" => {
            let default = vec![
                "app-server".to_string(),
                "daemon".to_string(),
                "version".to_string(),
            ];
            codex_command_with_vec_default(default, parsed)
        }
        "remote-control" => codex_command_with_help_default("remote-control", parsed),
        "review" => {
            let mut args = vec!["review".to_string()];
            args.append(&mut parsed);
            Ok(args)
        }
        "exec-review" => {
            let mut args = vec![
                "exec".to_string(),
                "review".to_string(),
                "--json".to_string(),
            ];
            args.append(&mut parsed);
            Ok(args)
        }
        "apply" => {
            if parsed.is_empty() {
                anyhow::bail!("apply には task id を指定してください");
            }
            let mut args = vec!["apply".to_string()];
            args.append(&mut parsed);
            Ok(args)
        }
        _ => anyhow::bail!("unsupported codex subcommand alias: {name}"),
    }
}

fn codex_command_with_args(name: &str, mut parsed: Vec<String>) -> Result<Vec<String>> {
    let mut args = vec![name.to_string()];
    args.append(&mut parsed);
    Ok(args)
}

fn codex_command_with_default(
    name: &str,
    default_subcommand: &str,
    mut parsed: Vec<String>,
) -> Result<Vec<String>> {
    let mut args = vec![name.to_string()];
    if parsed.is_empty() {
        args.push(default_subcommand.to_string());
    } else {
        args.append(&mut parsed);
    }
    Ok(args)
}

fn codex_command_with_help_default(name: &str, mut parsed: Vec<String>) -> Result<Vec<String>> {
    let mut args = vec![name.to_string()];
    if parsed.is_empty() {
        args.push("--help".to_string());
    } else {
        args.append(&mut parsed);
    }
    Ok(args)
}

fn codex_command_with_vec_default(
    default_args: Vec<String>,
    mut parsed: Vec<String>,
) -> Result<Vec<String>> {
    if parsed.is_empty() {
        Ok(default_args)
    } else {
        let mut args = vec![default_args[0].clone()];
        args.append(&mut parsed);
        Ok(args)
    }
}

fn codex_named_subcommand_args_with_settings(
    mut args: Vec<String>,
    settings: &CodexExecSettings,
) -> Vec<String> {
    if matches!(args.first().map(String::as_str), Some("--version" | "help")) {
        return args;
    }
    let mut out = Vec::new();
    if let Some(remote) = &settings.remote_addr {
        out.push("--remote".to_string());
        out.push(remote.clone());
    }
    if let Some(env) = &settings.remote_auth_token_env {
        out.push("--remote-auth-token-env".to_string());
        out.push(env.clone());
    }
    if settings.strict_config {
        out.push("--strict-config".to_string());
    }
    for config in &settings.config_overrides {
        out.push("-c".to_string());
        out.push(config.clone());
    }
    for feature in &settings.enabled_features {
        out.push("--enable".to_string());
        out.push(feature.clone());
    }
    for feature in &settings.disabled_features {
        out.push("--disable".to_string());
        out.push(feature.clone());
    }
    if codex_command_needs_addness_developer_instructions(&args) {
        push_addness_developer_instructions(&mut out);
    }
    out.append(&mut args);
    out
}

fn codex_command_needs_addness_developer_instructions(args: &[String]) -> bool {
    matches!(
        codex_command_name(args),
        Some("exec" | "review" | "resume" | "fork")
    )
}

fn codex_command_name(args: &[String]) -> Option<&str> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--" {
            return args.get(index + 1).map(String::as_str);
        }
        if !arg.starts_with('-') || arg == "-" {
            return Some(arg);
        }
        index += 1;
        if codex_global_option_takes_value(arg) {
            index += 1;
        }
    }
    None
}

fn codex_global_option_takes_value(arg: &str) -> bool {
    matches!(
        arg,
        "-a" | "--ask-for-approval"
            | "-s"
            | "--sandbox"
            | "-m"
            | "--model"
            | "-p"
            | "--profile"
            | "-C"
            | "--cd"
            | "-c"
            | "--config"
            | "-i"
            | "--image"
            | "-o"
            | "--output-last-message"
            | "--remote"
            | "--remote-auth-token-env"
            | "--local-provider"
            | "--add-dir"
            | "--enable"
            | "--disable"
            | "--output-schema"
    )
}

fn codex_command_category(args: &[String]) -> &'static str {
    match codex_command_name(args) {
        Some("exec") => "agent",
        Some("review") | Some("apply") | Some("sandbox") => "workspace",
        Some("resume" | "fork" | "archive" | "delete" | "unarchive") => "session",
        Some("login" | "logout") => "auth",
        Some("mcp" | "plugin" | "features" | "debug" | "doctor" | "completion" | "update") => {
            "config"
        }
        Some("cloud") => "cloud",
        Some("app" | "app-server" | "remote-control" | "mcp-server" | "exec-server") => "server",
        _ => "codex",
    }
}

fn command_preview(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.starts_with("developer_instructions=") {
                return "developer_instructions=<Addness DB>".to_string();
            }
            if arg.chars().any(char::is_whitespace) {
                format!("{arg:?}")
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_first_arg(input: &str) -> Option<(String, String)> {
    let mut args = split_codex_command_args(input).ok()?;
    if args.is_empty() {
        return None;
    }
    let first = args.remove(0);
    Some((first, args.join(" ")))
}

fn looks_like_uuid(value: &str) -> bool {
    let parts = value.split('-').collect::<Vec<_>>();
    let lengths = [8usize, 4, 4, 4, 12];
    parts.len() == lengths.len()
        && parts
            .iter()
            .zip(lengths)
            .all(|(part, len)| part.len() == len && part.chars().all(|ch| ch.is_ascii_hexdigit()))
}

fn codex_home_dir() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
}

fn load_codex_session_candidates(limit: usize) -> Result<Vec<CodexSessionCandidate>> {
    let Some(home) = codex_home_dir() else {
        return Ok(Vec::new());
    };
    load_codex_session_candidates_from(&home, limit)
}

fn codex_skill_roots(cwd: &str) -> Vec<PathBuf> {
    let mut roots = vec![
        Path::new(cwd).join(".codex").join("skills"),
        Path::new(cwd).join(".agents").join("skills"),
    ];
    if let Some(home) = codex_home_dir() {
        roots.push(home.join("skills"));
    }
    roots
}

fn collect_skill_names_from_roots(roots: &[PathBuf]) -> Vec<String> {
    let mut skills = BTreeSet::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || !path.join("SKILL.md").is_file() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            skills.insert(format!("{name} ({})", compact_home_path(&path)));
        }
    }
    skills.into_iter().collect()
}

fn load_codex_session_candidates_from(
    codex_home: &Path,
    limit: usize,
) -> Result<Vec<CodexSessionCandidate>> {
    let mut sessions = read_codex_session_index(codex_home)?;
    let sessions_dir = codex_home.join("sessions");
    if sessions_dir.is_dir() {
        read_codex_session_meta_files(&sessions_dir, &mut sessions)?;
    }
    let mut values = sessions.into_values().collect::<Vec<_>>();
    values.sort_by(|a, b| {
        b.updated_at
            .cmp(&a.updated_at)
            .then_with(|| a.title.cmp(&b.title))
    });
    values.truncate(limit);
    Ok(values)
}

fn append_codex_session_rename(session_id: &str, title: &str) -> Result<()> {
    let Some(home) = codex_home_dir() else {
        anyhow::bail!("Codex home を解決できません");
    };
    append_codex_session_rename_to(&home, session_id, title)
}

fn append_codex_session_rename_to(codex_home: &Path, session_id: &str, title: &str) -> Result<()> {
    fs::create_dir_all(codex_home)?;
    let path = codex_home.join("session_index.jsonl");
    let record = serde_json::json!({
        "id": session_id,
        "thread_name": title,
        "updated_at": chrono::Utc::now().to_rfc3339(),
    });
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(())
}

fn read_codex_session_index(codex_home: &Path) -> Result<HashMap<String, CodexSessionCandidate>> {
    let path = codex_home.join("session_index.jsonl");
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(e.into()),
    };
    let mut sessions = HashMap::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(record) = serde_json::from_str::<CodexSessionIndexRecord>(&line) {
            let title = record
                .thread_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "untitled".to_string());
            sessions.insert(
                record.id.clone(),
                CodexSessionCandidate {
                    id: record.id,
                    title,
                    updated_at: record.updated_at.unwrap_or_default(),
                    cwd: None,
                },
            );
        }
    }
    Ok(sessions)
}

fn read_codex_session_meta_files(
    dir: &Path,
    sessions: &mut HashMap<String, CodexSessionCandidate>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            read_codex_session_meta_files(&path, sessions)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(candidate) = read_codex_session_meta_file(&path)? else {
            continue;
        };
        sessions
            .entry(candidate.id.clone())
            .and_modify(|existing| {
                if existing.title == "untitled" && candidate.title != "untitled" {
                    existing.title = candidate.title.clone();
                }
                if existing.updated_at.is_empty() {
                    existing.updated_at = candidate.updated_at.clone();
                }
                if existing.cwd.is_none() {
                    existing.cwd = candidate.cwd.clone();
                }
            })
            .or_insert(candidate);
    }
    Ok(())
}

fn read_codex_session_meta_file(path: &Path) -> Result<Option<CodexSessionCandidate>> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let Some(line) = lines.next().transpose()? else {
        return Ok(None);
    };
    Ok(parse_codex_session_meta_line(&line))
}

fn parse_codex_session_meta_line(line: &str) -> Option<CodexSessionCandidate> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = value.get("payload")?;
    let id = payload.get("id").and_then(Value::as_str)?.to_string();
    let updated_at = payload
        .get("timestamp")
        .or_else(|| value.get("timestamp"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let cwd = payload
        .get("cwd")
        .and_then(Value::as_str)
        .map(str::to_string);
    let title = payload
        .get("thread_name")
        .or_else(|| payload.get("title"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("untitled")
        .to_string();
    Some(CodexSessionCandidate {
        id,
        title,
        updated_at,
        cwd,
    })
}

fn session_list_line(index: usize, session: &CodexSessionCandidate) -> String {
    let short_id = short_session_id(&session.id);
    let cwd = session
        .cwd
        .as_deref()
        .map(|cwd| format!("  {}", compact_home_path(Path::new(cwd))))
        .unwrap_or_default();
    format!(
        "{index}. {short_id}  {}  {}{cwd}",
        session.updated_at, session.title
    )
}

fn short_session_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

/// エージェントプロセスの stdout/stderr を 1 行単位で運ぶ、backend 非依存の
/// 行ストリームイベント。codex 固有情報は含まない。
enum AgentProcessEvent {
    Stdout(String),
    Stderr(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct QueuedPrompt {
    submitted: String,
    display_prompt: String,
    apply_goal_mode: bool,
    active_work: Option<ActiveWorkPackage>,
}

impl QueuedPrompt {
    fn user(submitted: String) -> Self {
        Self {
            display_prompt: submitted.clone(),
            submitted,
            apply_goal_mode: true,
            active_work: None,
        }
    }

    fn direct(submitted: String, display_prompt: String) -> Self {
        Self {
            submitted,
            display_prompt,
            apply_goal_mode: false,
            active_work: None,
        }
    }

    fn direct_work(
        submitted: String,
        display_prompt: String,
        active_work: ActiveWorkPackage,
    ) -> Self {
        Self {
            submitted,
            display_prompt,
            apply_goal_mode: false,
            active_work: Some(active_work),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "record", rename_all = "snake_case")]
enum CodexSessionRecord {
    Log {
        kind: CodexLogKind,
        text: String,
    },
    UpdateTurn {
        turn: usize,
        text: String,
    },
    AssistantDelta {
        text: String,
    },
    GoalMode {
        objective: Option<String>,
        paused: bool,
    },
    RawEvent {
        stream: String,
        line: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct CodexGoalMode {
    objective: Option<String>,
    paused: bool,
}

impl CodexGoalMode {
    fn is_active(&self) -> bool {
        self.objective.is_some() && !self.paused
    }

    fn label(&self) -> Option<String> {
        self.objective.as_ref().map(|objective| {
            if self.paused {
                format!("paused: {objective}")
            } else {
                format!("active: {objective}")
            }
        })
    }
}

struct LoadedCodexSession {
    log: Vec<CodexLogLine>,
    record_count: usize,
    goal_mode: CodexGoalMode,
}

struct CodexPaneSpawnOptions<'a> {
    codex_bin: &'a Path,
    cwd: &'a Path,
    addness_bin: &'a str,
    goal_id: String,
    goal_title: String,
    dod: String,
    status_label: String,
    session_log_path: Option<PathBuf>,
}

/// 子ゴール 1 件の表示用情報。
pub struct ChildGoal {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: &'static str,
    pub status_label: String,
    pub is_completed: bool,
    /// 新着ハイライトの有効期限（None=通常表示）。
    pub new_until: Option<Instant>,
}

/// Addness 側から取得した子ゴール更新情報。
pub struct ChildGoalUpdate {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub icon: &'static str,
    pub status_label: String,
    pub is_completed: bool,
}

/// TUI で選択中の実装ワークパッケージ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWorkPackage {
    pub id: String,
    pub title: String,
    pub ordinal: usize,
}

/// ホスト側 TUI が端末へ通知すべき Codex イベント。
pub struct TerminalNotice {
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexRunState {
    InputWaiting,
    Thinking,
    CommandRunning,
    Confirming,
    Completed,
}

impl CodexRunState {
    pub fn label(self) -> &'static str {
        match self {
            Self::InputWaiting => "入力待ち",
            Self::Thinking => "考え中",
            Self::CommandRunning => "コマンド実行中",
            Self::Confirming => "確認待ち",
            Self::Completed => "完了",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexDecisionKind {
    Approval,
    Permission,
    Dangerous,
    YesNo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexDecisionBanner {
    pub kind: CodexDecisionKind,
    pub message: String,
    pub accept_key: char,
    pub accept_label: &'static str,
    pub deny_key: char,
    pub deny_label: &'static str,
}

impl CodexDecisionBanner {
    fn new(kind: CodexDecisionKind, message: String) -> Self {
        match kind {
            CodexDecisionKind::Approval => Self {
                kind,
                message,
                accept_key: 'a',
                accept_label: "承認",
                deny_key: 'd',
                deny_label: "拒否",
            },
            CodexDecisionKind::Permission | CodexDecisionKind::Dangerous => Self {
                kind,
                message,
                accept_key: 'a',
                accept_label: "許可",
                deny_key: 'd',
                deny_label: "拒否",
            },
            CodexDecisionKind::YesNo => Self {
                kind,
                message,
                accept_key: 'y',
                accept_label: "Yes",
                deny_key: 'n',
                deny_label: "No",
            },
        }
    }

    fn response_for_key(&self, ch: char) -> Option<CodexDecisionResponse> {
        if ch == self.accept_key || ch == 'y' {
            Some(CodexDecisionResponse::Accept)
        } else if ch == self.deny_key || ch == 'n' {
            Some(CodexDecisionResponse::Deny)
        } else if self
            .always_choice()
            .is_some_and(|(key, _)| ch == key.to_ascii_lowercase())
        {
            Some(CodexDecisionResponse::Always)
        } else {
            None
        }
    }

    fn label_for_response(&self, response: CodexDecisionResponse) -> &'static str {
        match response {
            CodexDecisionResponse::Accept => self.accept_label,
            CodexDecisionResponse::Deny => self.deny_label,
            CodexDecisionResponse::Always => self
                .always_choice()
                .map(|(_, label)| label)
                .unwrap_or(self.accept_label),
        }
    }

    pub fn always_choice(&self) -> Option<(char, &'static str)> {
        match self.kind {
            CodexDecisionKind::Approval | CodexDecisionKind::Permission => {
                Some(('l', "これからずっと許可"))
            }
            CodexDecisionKind::Dangerous | CodexDecisionKind::YesNo => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexDecisionResponse {
    Accept,
    Deny,
    Always,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CodexWorkSummary {
    pub implemented: Vec<String>,
    pub checks: Vec<String>,
    pub remaining: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedTurnBodyRecord {
    pub turn: usize,
    pub prompt: Option<String>,
    pub summary: CodexWorkSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexSessionCandidate {
    id: String,
    title: String,
    updated_at: String,
    cwd: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexTurnPickerItem {
    pub turn: usize,
    pub title: String,
    pub collapsed: bool,
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CodexTurnPicker {
    selected_turn: usize,
}

#[derive(Debug, Deserialize)]
struct CodexSessionIndexRecord {
    id: String,
    #[serde(default)]
    thread_name: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

/// 埋め込み codex セッションの状態。
pub struct CodexPane {
    /// このペインを駆動するエージェント backend。CLI 引数生成・バイナリ解決・
    /// プロンプト受け渡し方式など backend 固有の差分をここへ委譲する。
    backend: CodexBackend,
    codex_bin: PathBuf,
    addness_bin: String,
    child: Option<Child>,
    tx: Sender<AgentProcessEvent>,
    rx: Receiver<AgentProcessEvent>,
    /// Codex プロセスが終了済みか。通常のターン完了では true にせず、
    /// ユーザーが `/exit` した場合やペインを閉じる場合にだけ終了扱いにする。
    pub finished: bool,
    /// 現在の `codex exec` ターンが実行中か。
    turn_running: bool,
    rows: u16,
    cols: u16,
    /// 還流先となる対象ゴールの ID。
    pub goal_id: String,
    /// codex 起動元として選択された親ゴール。Codex が必要に応じて子ゴールを作る時の文脈に使う。
    pub parent_goal_id: String,
    pub parent_goal_title: String,
    /// 契約ペイン表示用に保持する対象ゴールのタイトルと DoD。
    pub goal_title: String,
    pub dod: String,
    /// TUIが起動時または子ゴール切替時に取得済みのbody抜粋。
    addness_body_excerpt: Option<String>,
    /// codex が参照しているローカルの作業ディレクトリ（cwd）。
    pub cwd: String,
    /// 対象ゴールの現在ステータス表示（例: "進行中"）。
    pub status_label: String,
    /// DoD を行単位に分割した項目（契約ペインのチェックリスト用）。
    pub dod_items: Vec<String>,
    /// 各 DoD 項目の達成判定。None=未判定 / Some(true)=達成 / Some(false)=未達。
    pub dod_checks: Vec<Option<bool>>,
    /// DoD 自動判定（codex exec）を実行中か。
    pub assessing: bool,
    /// 子ゴール数・コメント数（変化検知で更新ログに反映する。未取得は None）。
    pub child_count: Option<usize>,
    pub comment_count: Option<usize>,
    pub deliverable_count: Option<usize>,
    /// PR / Release / tag など、作業の成果物トレースとして見せるリンク名。
    pub trace_links: Vec<String>,
    /// 子ゴールのライブリスト（新着は new_until までハイライト）。
    pub children: Vec<ChildGoal>,
    /// `/work` で選択した現在の実装ワークパッケージ。
    pub active_work_package: Option<ActiveWorkPackage>,
    /// codex が直近に実行した addness 操作の表示ラベル（参照/書込中インジケータ）。
    pub action: Option<String>,
    /// codex が現在実行中として報告したコマンド。
    current_command: Option<String>,
    current_command_started_at: Option<Instant>,
    /// `codex exec` 通常ターン以外のサブコマンドを実行中ならその表示名。
    child_process_label: Option<String>,
    /// サブコマンドの非JSON stdout/stderr。完了時にまとめてToolログへ出す。
    child_process_output: Vec<String>,
    child_process_error_output: Vec<String>,
    /// Addness をメモリとして使っているかを左ペインで示すための最終読込/書込時刻。
    pub last_addness_read_at: Option<Instant>,
    pub last_addness_write_at: Option<Instant>,
    pub last_addness_read_label: Option<String>,
    pub last_addness_write_label: Option<String>,
    /// ステータス・DoD が変化した時刻（変化行を数秒ハイライトするのに使う）。
    pub status_changed_at: Option<Instant>,
    pub dod_changed_at: Option<Instant>,
    /// codex ログのスクロールバック位置（0=最新、増えるほど過去）。
    pub scrollback: usize,
    /// 直近の描画で計算した表示行数ベースの最大スクロールバック。
    rendered_history_max_scrollback: Option<usize>,
    /// Addness 側の更新ログ（codex の書き込みやステータス変化を可視化）。新しいものほど末尾。
    pub activity: Vec<String>,
    /// codex セッション終了時の Addness body 自動記録を試行済みか。
    pub auto_record_attempted: bool,
    /// 最初の実依頼の body 自動記録を済み（再試行しない）とするフラグ。
    body_record_done: bool,
    input_state: CodexInputState,
    /// スラッシュコマンドパレットで選択中の候補インデックス。入力が変わると 0 に戻す。
    slash_palette_selected: usize,
    queued_prompts: VecDeque<QueuedPrompt>,
    /// `codex exec --json` が返した Codex thread id。2ターン目以降の resume に使う。
    thread_id: Option<String>,
    /// いま実行中のターンに対応するユーザー入力。turn.started の見出しに使う。
    current_turn_prompt: Option<String>,
    /// 承認リトライ時に再実行する、Addness文脈注入前の実プロンプト。
    current_turn_retry_prompt: Option<String>,
    /// Codex の turn 番号。UI上の区切りに使う。
    turn_count: usize,
    /// Addness body の自動メモへ反映済みの完了 turn 番号。
    turn_body_recorded_count: usize,
    /// 完了時点で固定した Addness body 自動メモ用の turn レコード。
    pending_turn_body_records: VecDeque<CompletedTurnBodyRecord>,
    /// 終了/中断時の自動メモで使う、直近の実行済み turn レコード。
    last_completed_turn_body_record: Option<CompletedTurnBodyRecord>,
    /// Codex が最後に報告した token/context 使用量。`/usage` で表示する。
    last_token_usage_label: Option<String>,
    collapsed_turns: BTreeSet<usize>,
    /// Addness 側で保持する会話・イベント履歴。
    log: Vec<CodexLogLine>,
    /// Addness 側で永続化する codex exec JSONL/UI 履歴。
    session_log_path: Option<PathBuf>,
    /// セッションログファイル内の概算レコード数（trim 判断とUI表示用）。
    session_record_count: usize,
    /// 起動時に復元した表示用ログ件数。
    loaded_history_count: usize,
    /// ストリーミング中の assistant 行。delta イベントが来た場合はこの行を伸ばす。
    streaming_assistant_index: Option<usize>,
    /// 端末通知待ちイベント。
    pending_notices: VecDeque<TerminalNotice>,
    /// `/sessions` で最後に表示した Codex セッション候補。番号指定操作で使う。
    indexed_sessions: Vec<CodexSessionCandidate>,
    /// 直近の exec 子プロセス終了イベントを JSON 側で受け取ったか。
    turn_finished_by_event: bool,
    /// Codex 履歴の表示フィルタ。
    log_filter: CodexLogFilter,
    /// Codex 履歴検索クエリ。
    search_query: String,
    /// 検索入力中か。
    search_editing: bool,
    /// 次回 `codex exec` 起動時に使う設定。
    exec_settings: CodexExecSettings,
    /// 現在turnを承認後に再実行する場合だけ使う一時的な approval override。
    one_shot_approval: Option<CodexApprovalChoice>,
    /// 作業ツリーの diff 表示。Some の間はログ領域を diff ビューとして使う。
    diff_view: Option<String>,
    /// 格納済み turn を明示的に選んで展開・格納するためのパネル。
    turn_picker: Option<CodexTurnPicker>,
    /// Addness 独自UI上で `codex exec` に注入する永続目標。
    goal_mode: CodexGoalMode,
    pending_decision: Option<CodexDecisionBanner>,
}

impl CodexPane {
    /// codex exec JSON セッションを開始する。
    ///
    /// 起動時点では Codex プロセスを走らせず、Addness 側の入力欄で最初の依頼を待つ。
    /// 最初の Enter で `codex exec --json` を起動し、以降は `codex exec resume` を使う。
    pub fn spawn(
        codex_bin: &Path,
        cwd: &Path,
        addness_bin: &str,
        goal_id: String,
        goal_title: String,
        dod: String,
        status_label: String,
    ) -> Result<Self> {
        Self::spawn_inner(CodexPaneSpawnOptions {
            codex_bin,
            cwd,
            addness_bin,
            session_log_path: codex_session_log_path(&goal_id),
            goal_id,
            goal_title,
            dod,
            status_label,
        })
    }

    fn spawn_inner(options: CodexPaneSpawnOptions<'_>) -> Result<Self> {
        let CodexPaneSpawnOptions {
            codex_bin,
            cwd,
            addness_bin,
            goal_id,
            goal_title,
            dod,
            status_label,
            session_log_path,
        } = options;
        let dod_items = split_dod_items(&dod);
        let dod_checks = vec![None; dod_items.len()];
        let (tx, rx) = mpsc::channel::<AgentProcessEvent>();
        let loaded = session_log_path
            .as_deref()
            .map(load_codex_session)
            .transpose()
            .unwrap_or(None)
            .unwrap_or_else(|| LoadedCodexSession {
                log: Vec::new(),
                record_count: 0,
                goal_mode: CodexGoalMode::default(),
            });
        let goal_mode = loaded.goal_mode.clone();
        let loaded_history_count = loaded.log.len();
        let turn_count = loaded
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Turn)
            .count();
        let collapsed_turns = (1..turn_count).collect::<BTreeSet<_>>();

        let mut pane = Self {
            backend: CodexBackend,
            codex_bin: codex_bin.to_path_buf(),
            addness_bin: addness_bin.to_string(),
            child: None,
            tx,
            rx,
            finished: false,
            turn_running: false,
            rows: 24,
            cols: 80,
            parent_goal_id: goal_id.clone(),
            parent_goal_title: goal_title.clone(),
            goal_id,
            goal_title,
            addness_body_excerpt: None,
            cwd: cwd.display().to_string(),
            status_label,
            dod_items,
            dod_checks,
            assessing: false,
            child_count: None,
            comment_count: None,
            deliverable_count: None,
            trace_links: Vec::new(),
            children: Vec::new(),
            active_work_package: None,
            action: None,
            current_command: None,
            current_command_started_at: None,
            child_process_label: None,
            child_process_output: Vec::new(),
            child_process_error_output: Vec::new(),
            last_addness_read_at: None,
            last_addness_write_at: None,
            last_addness_read_label: None,
            last_addness_write_label: None,
            status_changed_at: None,
            dod_changed_at: None,
            scrollback: 0,
            rendered_history_max_scrollback: None,
            activity: Vec::new(),
            auto_record_attempted: false,
            body_record_done: false,
            input_state: CodexInputState::default(),
            slash_palette_selected: 0,
            queued_prompts: VecDeque::new(),
            thread_id: None,
            current_turn_prompt: None,
            current_turn_retry_prompt: None,
            turn_count,
            turn_body_recorded_count: turn_count,
            pending_turn_body_records: VecDeque::new(),
            last_completed_turn_body_record: None,
            last_token_usage_label: None,
            collapsed_turns,
            log: loaded.log,
            session_log_path,
            session_record_count: loaded.record_count,
            loaded_history_count,
            streaming_assistant_index: None,
            pending_notices: VecDeque::new(),
            indexed_sessions: Vec::new(),
            turn_finished_by_event: false,
            log_filter: CodexLogFilter::Conversation,
            search_query: String::new(),
            search_editing: false,
            exec_settings: CodexExecSettings::default(),
            one_shot_approval: None,
            diff_view: None,
            turn_picker: None,
            pending_decision: None,
            goal_mode,
            dod,
        };
        if pane.loaded_history_count > 0 {
            pane.push_log(
                CodexLogKind::System,
                format!(
                    "前回のCodex履歴を {} 件読み込みました。続きは Enter で送信できます。",
                    pane.loaded_history_count
                ),
            );
        }
        pane.push_log(
            CodexLogKind::System,
            "Codex入力欄で待機中。入力して Enter で依頼を送信します。",
        );
        Ok(pane)
    }

    #[cfg(test)]
    pub(crate) fn test_with_output(
        rows: u16,
        cols: u16,
        _scrollback_len: usize,
        output: &str,
    ) -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut pane = Self::spawn(
            Path::new("codex"),
            &cwd,
            "addness",
            "test-goal".to_string(),
            "Test goal".to_string(),
            String::new(),
            "TEST".to_string(),
        )
        .unwrap();
        pane.rows = rows.max(1);
        pane.cols = cols.max(1);
        pane.log.clear();
        pane.session_log_path = None;
        pane.session_record_count = 0;
        pane.loaded_history_count = 0;
        pane.turn_count = 0;
        pane.turn_body_recorded_count = 0;
        pane.pending_turn_body_records.clear();
        pane.last_completed_turn_body_record = None;
        pane.last_token_usage_label = None;
        pane.rendered_history_max_scrollback = None;
        pane.log_filter = CodexLogFilter::Conversation;
        pane.search_query.clear();
        pane.search_editing = false;
        pane.exec_settings = CodexExecSettings::default();
        pane.one_shot_approval = None;
        pane.diff_view = None;
        pane.addness_body_excerpt = None;
        pane.turn_picker = None;
        pane.child_process_label = None;
        pane.child_process_output.clear();
        pane.child_process_error_output.clear();
        pane.indexed_sessions.clear();
        pane.collapsed_turns.clear();
        pane.pending_decision = None;
        pane.current_turn_prompt = None;
        pane.current_turn_retry_prompt = None;
        pane.goal_mode = CodexGoalMode::default();
        for line in output.lines() {
            pane.push_log(CodexLogKind::Assistant, line.to_string());
        }
        pane.finished = true;
        pane
    }

    #[cfg(test)]
    pub(crate) fn test_add_completed_turn(&mut self, assistant_text: &str) {
        self.turn_running = false;
        self.turn_count = self.turn_count.saturating_add(1);
        self.push_log(CodexLogKind::Turn, format!("Turn {}", self.turn_count));
        self.push_log(CodexLogKind::Assistant, assistant_text.to_string());
        self.queue_completed_turn_body_record();
    }

    /// ログを delta 行スクロールする（正=過去へ、負=最新へ）。
    pub fn scroll_lines(&mut self, delta: isize) {
        let max = self.max_view_scrollback();
        let target = (self.scrollback as isize + delta).max(0) as usize;
        self.scrollback = target.min(max);
    }

    /// 1 ページ分の行数（スクロール量）。
    pub fn page(&self) -> usize {
        self.rows.saturating_sub(3).max(1) as usize
    }

    /// 最古（バッファ先頭）までスクロールする。
    pub fn scroll_to_top(&mut self) {
        self.scrollback = self.max_view_scrollback();
    }

    /// 最新（ライブ）位置へ戻す。
    pub fn scroll_to_live(&mut self) {
        self.scrollback = 0;
    }

    pub fn is_turn_running(&self) -> bool {
        self.turn_running || self.child.is_some()
    }

    pub fn run_state(&self) -> CodexRunState {
        if self.finished {
            CodexRunState::Completed
        } else if self.pending_decision.is_some() {
            CodexRunState::Confirming
        } else if self.current_command.is_some() {
            CodexRunState::CommandRunning
        } else if self.is_turn_running() {
            CodexRunState::Thinking
        } else {
            CodexRunState::InputWaiting
        }
    }

    pub fn current_command(&self) -> Option<&str> {
        self.current_command.as_deref()
    }

    pub fn current_command_elapsed_secs(&self) -> Option<u64> {
        self.current_command_started_at
            .map(|t| t.elapsed().as_secs())
    }

    pub fn turn_count(&self) -> usize {
        self.turn_count
    }

    pub fn take_completed_turn_body_record(&mut self) -> Option<CompletedTurnBodyRecord> {
        self.pending_turn_body_records.pop_front()
    }

    fn queue_completed_turn_body_record(&mut self) {
        if self.finished || self.turn_count == 0 {
            return;
        }
        let turn = self.turn_count;
        if turn <= self.turn_body_recorded_count {
            return;
        }
        self.turn_body_recorded_count = turn;
        let record = CompletedTurnBodyRecord {
            turn,
            prompt: self.completed_turn_prompt(turn),
            summary: self.work_summary_for_turn(turn),
        };
        self.last_completed_turn_body_record = Some(record.clone());
        self.pending_turn_body_records.push_back(record);
    }

    fn completed_turn_prompt(&self, turn: usize) -> Option<String> {
        self.current_turn_prompt
            .clone()
            .or_else(|| turn_title_prompt(&self.log, turn))
    }

    pub fn collapsed_turn_count(&self) -> usize {
        self.collapsed_turns.len()
    }

    pub fn turn_picker_open(&self) -> bool {
        self.turn_picker.is_some()
    }

    pub fn turn_picker_selected_turn(&self) -> Option<usize> {
        self.turn_picker.as_ref().map(|picker| picker.selected_turn)
    }

    pub fn turn_picker_items(&self) -> Vec<CodexTurnPickerItem> {
        (1..=self.turn_count)
            .filter(|turn| self.can_toggle_turn(*turn))
            .map(|turn| {
                let title =
                    summarize_turn_title(&self.log, turn).unwrap_or_else(|| format!("Turn {turn}"));
                CodexTurnPickerItem {
                    turn,
                    title,
                    collapsed: self.collapsed_turns.contains(&turn),
                    current: self.turn_running && turn == self.turn_count,
                }
            })
            .collect()
    }

    pub fn decision_banner(&self) -> Option<&CodexDecisionBanner> {
        self.pending_decision.as_ref()
    }

    pub fn last_assistant_text(&self) -> Option<&str> {
        self.log
            .iter()
            .rev()
            .find(|line| line.kind == CodexLogKind::Assistant)
            .map(|line| line.text.as_str())
    }

    pub fn goal_mode_label(&self) -> Option<String> {
        self.goal_mode.label()
    }

    pub fn set_addness_body_context(&mut self, body: Option<String>) -> bool {
        let next = body
            .as_deref()
            .map(|body| compact_multiline_excerpt(body, 3_000))
            .filter(|body| !body.trim().is_empty());
        if next == self.addness_body_excerpt {
            false
        } else {
            self.addness_body_excerpt = next;
            true
        }
    }

    pub fn input_line(&self) -> &str {
        &self.input_state.line
    }

    pub fn input_cursor(&self) -> usize {
        self.input_state.cursor
    }

    /// 現在の入力に対するスラッシュコマンド候補（コマンド名, 説明）。
    /// 入力が `/xxx` でコマンド名を入力中（空白・改行を含まない）のときのみ返す。
    /// 空の `/` は全コマンドを返す。
    pub fn slash_palette_suggestions(&self) -> Vec<(&'static str, &'static str)> {
        let line = self.input_state.line.as_str();
        let Some(rest) = line.strip_prefix('/') else {
            return Vec::new();
        };
        if rest.chars().any(char::is_whitespace) {
            return Vec::new();
        }
        let prefix = line.to_ascii_lowercase();
        SLASH_COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(prefix.as_str()))
            .copied()
            .collect()
    }

    /// スラッシュコマンドパレットを表示 / 操作すべき状態か。
    pub fn slash_palette_active(&self) -> bool {
        !self.finished
            && !self.is_search_editing()
            && self.pending_decision.is_none()
            && !self.slash_palette_suggestions().is_empty()
    }

    /// パレットで選択中の候補インデックス（候補数でクランプ済み）。
    pub fn slash_palette_selected(&self) -> usize {
        let n = self.slash_palette_suggestions().len();
        if n == 0 {
            0
        } else {
            self.slash_palette_selected.min(n - 1)
        }
    }

    /// パレットの選択を上下に移動する（端で反対側へラップ）。
    pub fn move_slash_palette_selection(&mut self, delta: isize) {
        let n = self.slash_palette_suggestions().len();
        if n == 0 {
            return;
        }
        let cur = self.slash_palette_selected.min(n - 1) as isize;
        self.slash_palette_selected = (cur + delta).rem_euclid(n as isize) as usize;
    }

    /// 選択中の候補で入力行を補完する（`/command ` に置き換え、引数入力へ移る）。
    pub fn accept_slash_palette_selection(&mut self) {
        let suggestions = self.slash_palette_suggestions();
        if suggestions.is_empty() {
            return;
        }
        let idx = self.slash_palette_selected.min(suggestions.len() - 1);
        let name = suggestions[idx].0;
        self.input_state.clear();
        self.input_state.insert_text(&format!("{name} "));
        self.slash_palette_selected = 0;
    }

    pub fn captures_input_key(&self, key: KeyEvent) -> bool {
        !self.finished
            && self.pending_decision.is_none()
            && !key.modifiers.contains(KeyModifiers::ALT)
            && self.input_state.captures_key(key)
    }

    pub fn queued_prompt_count(&self) -> usize {
        self.queued_prompts.len()
    }

    pub fn child_goal_is_active(&self, child: &ChildGoal) -> bool {
        self.active_work_package
            .as_ref()
            .is_some_and(|active| active.id == child.id)
    }

    pub fn active_work_package_label(&self) -> Option<String> {
        self.active_work_package.as_ref().map(|active| {
            format!(
                "#{} {}",
                active.ordinal,
                compact_one_line(&active.title, 120)
            )
        })
    }

    pub fn queued_work_package_label(&self) -> Option<String> {
        let works = self
            .queued_prompts
            .iter()
            .filter_map(|queued| queued.active_work.as_ref())
            .collect::<Vec<_>>();
        if works.is_empty() {
            return None;
        }
        let preview = works
            .iter()
            .take(2)
            .map(|work| format!("#{} {}", work.ordinal, compact_one_line(&work.title, 48)))
            .collect::<Vec<_>>()
            .join(" / ");
        let suffix = if works.len() > 2 {
            format!(" +{}件", works.len() - 2)
        } else {
            String::new()
        };
        Some(format!("待機{}件: {preview}{suffix}", works.len()))
    }

    pub fn filtered_log_lines(&self) -> Vec<&CodexLogLine> {
        let query = self.normalized_search_query();
        let collapse_turns = self.turns_are_collapsible_in_current_view();
        let mut current_collapsed_turn = None;
        let mut visible = Vec::new();
        for line in &self.log {
            if line.kind == CodexLogKind::Turn {
                current_collapsed_turn = turn_number_from_label(&line.text)
                    .filter(|n| collapse_turns && self.collapsed_turns.contains(n));
                if self.log_line_visible(line, &query) {
                    visible.push(line);
                }
                continue;
            }
            if current_collapsed_turn.is_some() {
                continue;
            }
            if self.log_line_visible(line, &query) {
                visible.push(line);
            }
        }
        visible
    }

    pub fn toggle_visible_turn_collapsed(&mut self) -> bool {
        if !self.turns_are_collapsible_in_current_view() {
            return false;
        }
        let target = if self.scrollback == 0 {
            self.latest_completed_turn()
        } else {
            self.turn_at_viewport_start()
                .filter(|turn| self.can_toggle_turn(*turn))
                .or_else(|| self.latest_completed_turn())
        };
        let Some(turn) = target else {
            return false;
        };
        self.toggle_turn_collapsed_by_number(turn)
    }

    pub fn toggle_turn_collapsed_by_number(&mut self, turn: usize) -> bool {
        if !self.turns_are_collapsible_in_current_view() {
            return false;
        }
        if !self.can_toggle_turn(turn) {
            return false;
        }
        self.toggle_turn_collapsed(turn);
        self.scroll_to_turn_header(turn);
        true
    }

    pub fn open_turn_by_number(&mut self, turn: usize) -> bool {
        if !self.turns_are_collapsible_in_current_view() {
            return false;
        }
        if !self.can_toggle_turn(turn) {
            return false;
        }
        self.collapsed_turns.remove(&turn);
        self.invalidate_rendered_history_metrics();
        self.scroll_to_turn_header(turn);
        true
    }

    pub fn close_turn_by_number(&mut self, turn: usize) -> bool {
        if !self.turns_are_collapsible_in_current_view() {
            return false;
        }
        if !self.can_toggle_turn(turn) {
            return false;
        }
        self.collapsed_turns.insert(turn);
        self.invalidate_rendered_history_metrics();
        self.scroll_to_turn_header(turn);
        true
    }

    pub fn open_all_turns(&mut self) {
        self.collapsed_turns.clear();
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn open_turn_picker(&mut self) -> bool {
        let items = self.turn_picker_items();
        let Some(first) = items.first() else {
            self.push_log(CodexLogKind::System, "開けるturnはまだありません");
            return false;
        };
        let selected_turn = items
            .iter()
            .find(|item| item.collapsed)
            .map(|item| item.turn)
            .unwrap_or(first.turn);
        self.turn_picker = Some(CodexTurnPicker { selected_turn });
        true
    }

    pub fn close_turn_picker(&mut self) {
        self.turn_picker = None;
    }

    pub fn move_turn_picker_selection(&mut self, delta: isize) -> bool {
        let items = self.turn_picker_items();
        if items.is_empty() {
            return false;
        }
        let Some(picker) = self.turn_picker.as_mut() else {
            return false;
        };
        let current_index = items
            .iter()
            .position(|item| item.turn == picker.selected_turn)
            .unwrap_or(0);
        let max = items.len().saturating_sub(1) as isize;
        let next = (current_index as isize + delta).clamp(0, max) as usize;
        picker.selected_turn = items[next].turn;
        true
    }

    pub fn open_selected_turn_from_picker(&mut self) -> bool {
        let Some(turn) = self.turn_picker_selected_turn() else {
            return false;
        };
        self.open_turn_by_number(turn)
    }

    pub fn close_selected_turn_from_picker(&mut self) -> bool {
        let Some(turn) = self.turn_picker_selected_turn() else {
            return false;
        };
        self.close_turn_by_number(turn)
    }

    pub fn toggle_selected_turn_from_picker(&mut self) -> bool {
        let Some(turn) = self.turn_picker_selected_turn() else {
            return false;
        };
        self.toggle_turn_collapsed_by_number(turn)
    }

    pub fn toggle_old_turns_collapsed(&mut self) {
        if self.collapsed_turns.is_empty() {
            self.collapse_completed_turns();
        } else {
            self.collapsed_turns.clear();
        }
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    fn toggle_turn_collapsed(&mut self, turn: usize) {
        if !self.collapsed_turns.remove(&turn) {
            self.collapsed_turns.insert(turn);
        }
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    fn collapse_completed_turns(&mut self) {
        self.collapsed_turns.clear();
        let keep_open = if self.turn_running {
            self.turn_count
        } else {
            self.turn_count.saturating_add(1)
        };
        for turn in 1..keep_open {
            self.collapsed_turns.insert(turn);
        }
    }

    fn turns_are_collapsible_in_current_view(&self) -> bool {
        self.search_query.trim().is_empty()
            && matches!(
                self.log_filter,
                CodexLogFilter::All | CodexLogFilter::Conversation
            )
    }

    fn turn_at_viewport_start(&self) -> Option<usize> {
        let lines = self.filtered_log_lines();
        let total_lines = lines
            .iter()
            .map(|line| rendered_log_line_height(&line.text, self.cols as usize))
            .sum::<usize>();
        let max_scrollback = self
            .rendered_history_max_scrollback
            .unwrap_or_else(|| total_lines.saturating_sub(self.log_viewport_height()));
        let viewport_top = max_scrollback.saturating_sub(self.scrollback);
        let mut active_turn = None;
        let mut line_top = 0usize;
        let mut viewport_reached = false;

        for line in lines {
            if line.kind == CodexLogKind::Turn {
                active_turn = turn_number_from_label(&line.text);
                if viewport_reached {
                    return active_turn;
                }
            }
            let line_bottom =
                line_top.saturating_add(rendered_log_line_height(&line.text, self.cols as usize));
            if viewport_top < line_bottom {
                if active_turn.is_some() {
                    return active_turn;
                }
                viewport_reached = true;
            }
            line_top = line_bottom;
        }

        active_turn
    }

    fn scroll_to_turn_header(&mut self, turn: usize) {
        let mut line_top = 0usize;
        for line in self.filtered_log_lines() {
            if line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn) {
                let max = self.max_view_scrollback();
                self.scrollback = max.saturating_sub(line_top);
                return;
            }
            line_top =
                line_top.saturating_add(rendered_log_line_height(&line.text, self.cols as usize));
        }
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    fn latest_completed_turn(&self) -> Option<usize> {
        if self.turn_count == 0 {
            return None;
        }
        if self.turn_running {
            return self.turn_count.checked_sub(1).filter(|turn| *turn > 0);
        }
        Some(self.turn_count)
    }

    fn can_toggle_turn(&self, turn: usize) -> bool {
        turn > 0 && turn <= self.turn_count && !(self.turn_running && turn == self.turn_count)
    }

    pub fn work_summary(&self) -> CodexWorkSummary {
        CodexWorkSummary {
            implemented: summarize_implemented_work(&self.log),
            checks: summarize_checks(&self.log),
            remaining: summarize_remaining_work(&self.log),
        }
    }

    pub fn final_body_record_context(&self) -> (Option<String>, CodexWorkSummary) {
        if let Some(record) = &self.last_completed_turn_body_record {
            return (record.prompt.clone(), record.summary.clone());
        }
        let prompt = self.current_turn_prompt.clone().or_else(|| {
            self.last_prompt()
                .filter(|p| !is_exit_prompt(p))
                .map(str::to_string)
        });
        (prompt, self.work_summary())
    }

    fn work_summary_for_turn(&self, turn: usize) -> CodexWorkSummary {
        let Some((start, end)) = turn_log_bounds(&self.log, turn) else {
            return self.work_summary();
        };
        let lines = &self.log[start + 1..end];
        CodexWorkSummary {
            implemented: summarize_implemented_work(lines),
            checks: summarize_checks(lines),
            remaining: summarize_remaining_work(lines),
        }
    }

    pub fn handle_decision_key(&mut self, key: KeyEvent) -> bool {
        let Some(decision) = self.pending_decision.clone() else {
            return false;
        };
        if !key.modifiers.is_empty() {
            return false;
        }
        let KeyCode::Char(ch) = key.code else {
            return false;
        };
        let ch = ch.to_ascii_lowercase();
        let response = decision.response_for_key(ch);
        let Some(response) = response else {
            return false;
        };
        self.resolve_pending_decision(decision, response, false);
        true
    }

    pub fn log_filter_label(&self) -> &'static str {
        self.log_filter.label()
    }

    pub fn search_query(&self) -> &str {
        &self.search_query
    }

    pub fn is_search_editing(&self) -> bool {
        self.search_editing
    }

    pub fn cycle_log_filter(&mut self) {
        self.log_filter = self.log_filter.next();
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn begin_search(&mut self) {
        self.search_editing = true;
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.search_editing = false;
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    pub fn handle_search_key(&mut self, key: KeyEvent) -> bool {
        if !self.search_editing {
            return false;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.search_editing = false;
                true
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                true
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search_query.clear();
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                true
            }
            KeyCode::Char(ch)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                self.search_query.push(ch);
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                true
            }
            _ => true,
        }
    }

    pub fn history_label(&self) -> String {
        if self.session_log_path.is_some() {
            format!("履歴 {}件 保存中", self.log.len())
        } else {
            format!("履歴 {}件 メモリのみ", self.log.len())
        }
    }

    pub fn history_path_label(&self) -> Option<String> {
        self.session_log_path.as_deref().map(compact_home_path)
    }

    pub fn loaded_history_count(&self) -> usize {
        self.loaded_history_count
    }

    pub fn settings_label(&self) -> String {
        self.exec_settings.label()
    }

    pub fn memory_mode_label(&self) -> String {
        self.exec_settings.memory_mode_label()
    }

    pub fn memory_mode_is_addness_safe(&self) -> bool {
        self.exec_settings.memory_mode_is_addness_safe()
    }

    pub fn cycle_model(&mut self) {
        self.exec_settings.model_override = None;
        let value = self.exec_settings.cycle_model();
        self.action = Some(format!("model: {value}"));
        self.push_activity(format!("Codex model を {value} に変更"));
        self.push_log(CodexLogKind::System, format!("次回ターンの model: {value}"));
    }

    fn set_model(&mut self, value: &str) {
        if let Some(model) = parse_builtin_model_choice(value) {
            self.exec_settings.model = model;
            self.exec_settings.model_override = None;
        } else {
            self.exec_settings.model = CodexModelChoice::Config;
            self.exec_settings.model_override = Some(value.to_string());
        }
        let value = self
            .exec_settings
            .model_override
            .as_deref()
            .unwrap_or_else(|| self.exec_settings.model.label())
            .to_string();
        self.action = Some(format!("model: {value}"));
        self.push_activity(format!("Codex model を {value} に変更"));
        self.push_log(CodexLogKind::System, format!("次回ターンの model: {value}"));
    }

    pub fn cycle_reasoning(&mut self) {
        let value = self.exec_settings.cycle_reasoning();
        self.action = Some(format!("reasoning: {value}"));
        self.push_activity(format!("Codex reasoning を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの reasoning effort: {value}"),
        );
    }

    fn set_reasoning(&mut self, value: CodexReasoningChoice) {
        self.exec_settings.reasoning = value;
        let value = self.exec_settings.reasoning.label();
        self.action = Some(format!("reasoning: {value}"));
        self.push_activity(format!("Codex reasoning を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの reasoning effort: {value}"),
        );
    }

    pub fn cycle_approval(&mut self) {
        let value = self.exec_settings.cycle_approval();
        self.action = Some(format!("approval: {value}"));
        self.push_activity(format!("Codex approval を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの approval: {value}"),
        );
    }

    fn set_approval(&mut self, value: CodexApprovalChoice) {
        self.exec_settings.approval = value;
        let value = self.exec_settings.approval.label();
        self.action = Some(format!("approval: {value}"));
        self.push_activity(format!("Codex approval を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの approval: {value}"),
        );
    }

    pub fn cycle_sandbox(&mut self) {
        let value = self.exec_settings.cycle_sandbox();
        self.action = Some(format!("sandbox: {value}"));
        self.push_activity(format!("Codex sandbox を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの sandbox: {value}"),
        );
    }

    fn set_sandbox(&mut self, value: CodexSandboxChoice) {
        self.exec_settings.sandbox = value;
        let value = self.exec_settings.sandbox.label();
        self.action = Some(format!("sandbox: {value}"));
        self.push_activity(format!("Codex sandbox を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの sandbox: {value}"),
        );
    }

    pub fn toggle_web_search(&mut self) {
        let enabled = self.exec_settings.toggle_web_search();
        let value = on_off(enabled);
        self.action = Some(format!("search: {value}"));
        self.push_activity(format!("Codex web search を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの search: {value}"),
        );
    }

    pub fn toggle_oss(&mut self) {
        let enabled = self.exec_settings.toggle_oss();
        let value = on_off(enabled);
        self.action = Some(format!("oss: {value}"));
        self.push_activity(format!("Codex OSS mode を {value} に変更"));
        self.push_log(CodexLogKind::System, format!("次回ターンの oss: {value}"));
    }

    fn set_remote_addr(&mut self, addr: Option<String>) {
        self.exec_settings.remote_addr = addr;
        let value = self.exec_settings.remote_addr.as_deref().unwrap_or("off");
        self.action = Some(format!("remote: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの remote: {value}"),
        );
    }

    fn set_remote_auth_token_env(&mut self, env: Option<String>) {
        self.exec_settings.remote_auth_token_env = env;
        let value = self
            .exec_settings
            .remote_auth_token_env
            .as_deref()
            .unwrap_or("off");
        self.action = Some(format!("remote-auth-token-env: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの remote-auth-token-env: {value}"),
        );
    }

    fn toggle_no_alt_screen(&mut self) {
        self.exec_settings.no_alt_screen = !self.exec_settings.no_alt_screen;
        let value = on_off(self.exec_settings.no_alt_screen);
        self.action = Some(format!("no-alt-screen: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの no-alt-screen: {value}"),
        );
    }

    pub fn cycle_local_provider(&mut self) {
        let value = self.exec_settings.cycle_local_provider();
        self.action = Some(format!("local provider: {value}"));
        self.push_activity(format!("Codex local provider を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの local provider: {value}"),
        );
    }

    fn set_profile(&mut self, profile: Option<String>) {
        self.exec_settings.profile = profile;
        let value = self.exec_settings.profile.as_deref().unwrap_or("config");
        self.action = Some(format!("profile: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの profile: {value}"),
        );
    }

    fn add_image_path(&mut self, path: String) {
        self.exec_settings.image_paths.push(path.clone());
        let count = self.exec_settings.image_paths.len();
        self.action = Some(format!("image: {count}件"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンに画像を添付: {path}"),
        );
    }

    fn remove_image_path(&mut self, index: usize) {
        if index >= self.exec_settings.image_paths.len() {
            self.push_log(CodexLogKind::Error, "image index が範囲外です");
            return;
        }
        let removed = self.exec_settings.image_paths.remove(index);
        self.action = Some(format!("image: {}件", self.exec_settings.image_paths.len()));
        self.push_log(CodexLogKind::System, format!("画像添付を削除: {removed}"));
    }

    fn clear_image_paths(&mut self) {
        self.exec_settings.image_paths.clear();
        self.action = Some("image: cleared".to_string());
        self.push_log(CodexLogKind::System, "画像添付をクリアしました");
    }

    fn add_writable_dir(&mut self, dir: String) {
        self.exec_settings.additional_dirs.push(dir.clone());
        let count = self.exec_settings.additional_dirs.len();
        self.action = Some(format!("add-dir: {count}件"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンに追加書込ディレクトリを渡します: {dir}"),
        );
    }

    fn clear_writable_dirs(&mut self) {
        self.exec_settings.additional_dirs.clear();
        self.action = Some("add-dir: cleared".to_string());
        self.push_log(CodexLogKind::System, "追加書込ディレクトリをクリアしました");
    }

    fn add_config_override(&mut self, value: String) {
        if let Some((key, raw_value)) = value.split_once('=')
            && memory_override_enables_global_memory(key, raw_value)
        {
            self.reject_global_memory_override();
            return;
        }
        self.exec_settings.config_overrides.push(value.clone());
        let count = self.exec_settings.config_overrides.len();
        self.action = Some(format!("config: {count}件"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンに -c {value} を渡します"),
        );
    }

    fn set_config_override_key(&mut self, key: &str, toml_value: String, label: &str) {
        if memory_override_enables_global_memory(key, &toml_value) {
            self.reject_global_memory_override();
            return;
        }
        self.exec_settings
            .config_overrides
            .retain(|entry| config_override_key(entry) != key);
        let value = format!("{key}={toml_value}");
        self.exec_settings.config_overrides.push(value.clone());
        self.action = Some(format!("{label}: set"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンに -c {value} を渡します"),
        );
    }

    fn clear_config_override_key(&mut self, key: &str, label: &str) {
        let before = self.exec_settings.config_overrides.len();
        self.exec_settings
            .config_overrides
            .retain(|entry| config_override_key(entry) != key);
        let removed = before.saturating_sub(self.exec_settings.config_overrides.len());
        self.action = Some(format!("{label}: cleared"));
        self.push_log(
            CodexLogKind::System,
            format!("{label} config override をクリアしました ({removed}件)"),
        );
    }

    fn restore_addness_memory_defaults(&mut self) {
        self.exec_settings.config_overrides.retain(|entry| {
            let key = config_override_key(entry);
            key != "memories.use_memories" && key != "memories.generate_memories"
        });
        self.exec_settings
            .config_overrides
            .extend(default_addness_memory_config_overrides());
        self.action = Some("memories: addness-default".to_string());
        self.push_log(
            CodexLogKind::System,
            "記憶先をAddness既定値に戻しました: 通常Codex memory off",
        );
    }

    fn reject_global_memory_override(&mut self) {
        self.restore_addness_memory_defaults();
        self.push_log(
            CodexLogKind::Error,
            "Addness TUIでは通常Codexのglobal memoryを有効化しません。プロジェクト固有メモは /remember <内容> でAddnessへ保存してください",
        );
    }

    fn clear_config_override_prefix(&mut self, prefix: &str, label: &str) {
        let before = self.exec_settings.config_overrides.len();
        self.exec_settings.config_overrides.retain(|entry| {
            let key = config_override_key(entry);
            key != prefix && !key.starts_with(&format!("{prefix}."))
        });
        let removed = before.saturating_sub(self.exec_settings.config_overrides.len());
        self.action = Some(format!("{label}: cleared"));
        self.push_log(
            CodexLogKind::System,
            format!("{label} config override をクリアしました ({removed}件)"),
        );
    }

    fn config_override_value_for(&self, key: &str) -> Option<&str> {
        self.exec_settings
            .config_overrides
            .iter()
            .find_map(|entry| config_override_value(entry, key))
    }

    fn clear_config_overrides(&mut self) {
        self.exec_settings.config_overrides.clear();
        self.exec_settings
            .config_overrides
            .extend(default_addness_memory_config_overrides());
        self.action = Some("config: addness-default".to_string());
        self.push_log(
            CodexLogKind::System,
            "追加 config override をクリアし、記憶先をAddness既定値に戻しました",
        );
    }

    fn add_enabled_feature(&mut self, feature: String) {
        self.exec_settings.enabled_features.push(feature.clone());
        self.action = Some(format!("enable: {feature}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンで feature を有効化: {feature}"),
        );
    }

    fn add_disabled_feature(&mut self, feature: String) {
        self.exec_settings.disabled_features.push(feature.clone());
        self.action = Some(format!("disable: {feature}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンで feature を無効化: {feature}"),
        );
    }

    fn toggle_strict_config(&mut self) {
        self.exec_settings.strict_config = !self.exec_settings.strict_config;
        let value = on_off(self.exec_settings.strict_config);
        self.action = Some(format!("strict-config: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの strict-config: {value}"),
        );
    }

    fn toggle_ignore_user_config(&mut self) {
        self.exec_settings.ignore_user_config = !self.exec_settings.ignore_user_config;
        let value = on_off(self.exec_settings.ignore_user_config);
        self.action = Some(format!("ignore-user-config: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの ignore-user-config: {value}"),
        );
    }

    fn toggle_ignore_rules(&mut self) {
        self.exec_settings.ignore_rules = !self.exec_settings.ignore_rules;
        let value = on_off(self.exec_settings.ignore_rules);
        self.action = Some(format!("ignore-rules: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの ignore-rules: {value}"),
        );
    }

    fn toggle_skip_git_repo_check(&mut self) {
        self.exec_settings.skip_git_repo_check = !self.exec_settings.skip_git_repo_check;
        let value = on_off(self.exec_settings.skip_git_repo_check);
        self.action = Some(format!("skip-git-check: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの skip-git-repo-check: {value}"),
        );
    }

    fn toggle_ephemeral(&mut self) {
        self.exec_settings.ephemeral = !self.exec_settings.ephemeral;
        let value = on_off(self.exec_settings.ephemeral);
        self.action = Some(format!("ephemeral: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの ephemeral: {value}"),
        );
    }

    fn toggle_bypass_approvals_and_sandbox(&mut self) {
        self.exec_settings.bypass_approvals_and_sandbox =
            !self.exec_settings.bypass_approvals_and_sandbox;
        let value = on_off(self.exec_settings.bypass_approvals_and_sandbox);
        self.action = Some(format!("bypass approvals+sandbox: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの dangerously-bypass-approvals-and-sandbox: {value}"),
        );
    }

    fn toggle_bypass_hook_trust(&mut self) {
        self.exec_settings.bypass_hook_trust = !self.exec_settings.bypass_hook_trust;
        let value = on_off(self.exec_settings.bypass_hook_trust);
        self.action = Some(format!("bypass-hook-trust: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの dangerously-bypass-hook-trust: {value}"),
        );
    }

    fn set_output_schema(&mut self, path: Option<String>) {
        self.exec_settings.output_schema = path;
        let value = self.exec_settings.output_schema.as_deref().unwrap_or("off");
        self.action = Some(format!("output-schema: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの output-schema: {value}"),
        );
    }

    fn set_output_last_message(&mut self, path: Option<String>) {
        self.exec_settings.output_last_message = path;
        let value = self
            .exec_settings
            .output_last_message
            .as_deref()
            .unwrap_or("off");
        self.action = Some(format!("output-last-message: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの output-last-message: {value}"),
        );
    }

    pub fn diff_view(&self) -> Option<&str> {
        self.diff_view.as_deref()
    }

    pub fn diff_label(&self) -> &'static str {
        if self.diff_view.is_some() {
            "diff:on"
        } else {
            "diff:off"
        }
    }

    pub fn toggle_diff_view(&mut self) {
        if self.diff_view.is_some() {
            self.diff_view = None;
            self.invalidate_rendered_history_metrics();
            self.scroll_to_live();
            self.action = Some("diff view off".to_string());
            return;
        }
        self.diff_view = Some(git_diff_preview(Path::new(&self.cwd)));
        self.invalidate_rendered_history_metrics();
        self.scroll_to_live();
        self.action = Some("diff view on".to_string());
    }

    pub fn sync_rendered_history_metrics(&mut self, total_lines: usize, viewport_height: usize) {
        let max = total_lines.saturating_sub(viewport_height);
        self.rendered_history_max_scrollback = Some(max);
        self.scrollback = self.scrollback.min(max);
    }

    fn invalidate_rendered_history_metrics(&mut self) {
        self.rendered_history_max_scrollback = None;
    }

    fn max_view_scrollback(&self) -> usize {
        if let Some(max) = self.rendered_history_max_scrollback {
            return max;
        }
        self.filtered_log_lines()
            .iter()
            .map(|line| rendered_log_line_height(&line.text, self.cols as usize))
            .sum::<usize>()
            .saturating_sub(self.log_viewport_height())
    }

    fn normalized_search_query(&self) -> String {
        self.search_query.trim().to_lowercase()
    }

    fn log_line_visible(&self, line: &CodexLogLine, query: &str) -> bool {
        if !matches_log_filter(line.kind, self.log_filter) {
            return false;
        }
        if query.is_empty()
            && self.log_filter == CodexLogFilter::Conversation
            && is_routine_codex_system_line(line)
        {
            return false;
        }
        if query.is_empty() {
            return true;
        }
        contains_case_insensitive(&line.text, query)
    }

    fn log_viewport_height(&self) -> usize {
        // 枠の上下と入力欄2行を除いた概算。描画側も同じ領域に収める。
        self.rows.saturating_sub(4).max(1) as usize
    }

    /// Addness 側の更新ログへ 1 行追加する（古いものから捨てて最大 50 件保持）。
    pub fn push_activity(&mut self, line: String) {
        self.activity.push(line);
        let len = self.activity.len();
        if len > 50 {
            self.activity.drain(0..len - 50);
        }
    }

    fn push_log(&mut self, kind: CodexLogKind, text: impl Into<String>) {
        self.invalidate_rendered_history_metrics();
        let line = CodexLogLine::new(kind, text);
        self.persist_session_record(CodexSessionRecord::Log {
            kind: line.kind,
            text: line.text.clone(),
        });
        self.log.push(line);
        self.trim_in_memory_log();
        if !matches!(kind, CodexLogKind::Assistant) {
            self.streaming_assistant_index = None;
        }
        if self.scrollback == 0 {
            self.scroll_to_live();
        } else {
            self.scrollback = self.scrollback.min(self.max_view_scrollback());
        }
    }

    fn append_assistant_delta(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.invalidate_rendered_history_metrics();
        if let Some(index) = self.streaming_assistant_index
            && let Some(line) = self.log.get_mut(index)
        {
            line.text.push_str(text);
            self.persist_session_record(CodexSessionRecord::AssistantDelta {
                text: text.to_string(),
            });
            return;
        }
        self.persist_session_record(CodexSessionRecord::Log {
            kind: CodexLogKind::Assistant,
            text: text.to_string(),
        });
        self.log
            .push(CodexLogLine::new(CodexLogKind::Assistant, text));
        self.trim_in_memory_log();
        self.streaming_assistant_index = Some(self.log.len().saturating_sub(1));
    }

    fn refresh_current_turn_title(&mut self) {
        let turn = self.turn_count;
        let Some(title) = summarize_turn_title(&self.log, turn) else {
            return;
        };
        let label = format!("Turn {turn} - {title}");
        if update_turn_log_line(&mut self.log, turn, label.clone()) {
            self.invalidate_rendered_history_metrics();
            self.persist_session_record(CodexSessionRecord::UpdateTurn { turn, text: label });
        }
    }

    fn trim_in_memory_log(&mut self) {
        let len = self.log.len();
        if len <= CODEX_SESSION_HISTORY_MAX_LOG_LINES {
            return;
        }
        let removed = len - CODEX_SESSION_HISTORY_MAX_LOG_LINES;
        self.log.drain(0..removed);
        self.streaming_assistant_index = self
            .streaming_assistant_index
            .and_then(|idx| idx.checked_sub(removed));
    }

    fn persist_raw_event(&mut self, stream: &str, line: &str) {
        self.persist_session_record(CodexSessionRecord::RawEvent {
            stream: stream.to_string(),
            line: line.to_string(),
        });
    }

    fn persist_session_record(&mut self, record: CodexSessionRecord) {
        let Some(path) = self.session_log_path.clone() else {
            return;
        };
        if append_codex_session_record(&path, &record).is_err() {
            return;
        }
        self.session_record_count = self.session_record_count.saturating_add(1);
        let should_trim = self.session_record_count > CODEX_SESSION_HISTORY_MAX_RECORDS
            || fs::metadata(&path)
                .map(|m| m.len() > CODEX_SESSION_HISTORY_MAX_BYTES)
                .unwrap_or(false);
        if should_trim && let Ok(count) = trim_codex_session_log(&path) {
            self.session_record_count = count;
        }
    }

    /// 子ゴールのライブリストを差し替える。新規 ID は一定時間ハイライトする。
    /// 初回（既存が空）の取得では全件を新着扱いしない。
    pub fn update_children(&mut self, incoming: Vec<ChildGoalUpdate>) {
        let had_any = !self.children.is_empty();
        let old_ids: std::collections::HashSet<String> =
            self.children.iter().map(|c| c.id.clone()).collect();
        let new_until = Instant::now() + std::time::Duration::from_secs(4);
        self.children = incoming
            .into_iter()
            .map(|child| {
                let is_new = had_any && !old_ids.contains(&child.id);
                ChildGoal {
                    new_until: is_new.then_some(new_until),
                    id: child.id,
                    title: child.title,
                    description: child.description,
                    icon: child.icon,
                    status_label: child.status_label,
                    is_completed: child.is_completed,
                }
            })
            .collect();
        self.refresh_active_work_package();
    }

    fn set_active_work_package(&mut self, idx: usize) {
        if let Some(child) = self.children.get(idx) {
            self.active_work_package = Some(ActiveWorkPackage {
                id: child.id.clone(),
                title: child.title.clone(),
                ordinal: idx + 1,
            });
        }
    }

    fn refresh_active_work_package(&mut self) {
        let Some(active) = self.active_work_package.clone() else {
            return;
        };
        if let Some((idx, child)) = self
            .children
            .iter()
            .enumerate()
            .find(|(_, child)| child.id == active.id)
        {
            self.active_work_package = Some(ActiveWorkPackage {
                id: child.id.clone(),
                title: child.title.clone(),
                ordinal: idx + 1,
            });
        } else {
            self.active_work_package = None;
        }
    }

    /// JSONL と子プロセス終了を取り込み、画面に影響する変化があれば `true` を返す。
    pub fn update(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            changed = true;
            match event {
                AgentProcessEvent::Stdout(line) => self.handle_stdout_line(&line),
                AgentProcessEvent::Stderr(line) => self.handle_stderr_line(&line),
            }
        }

        if let Some(child) = self.child.as_mut() {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let command_label = self.child_process_label.take();
                    self.child = None;
                    self.turn_running = false;
                    self.current_command = None;
                    self.current_command_started_at = None;
                    self.streaming_assistant_index = None;
                    self.pending_decision = None;
                    if let Some(label) = command_label {
                        self.flush_child_process_output(status.success(), &label);
                        if status.success() {
                            self.refresh_current_turn_title();
                            let message = format!("{label} が完了しました");
                            self.push_log(CodexLogKind::System, message.clone());
                            self.push_terminal_notice("Codex コマンド完了", message);
                        } else {
                            let message = format!("{label} が失敗しました: {status}");
                            self.push_log(CodexLogKind::Error, message.clone());
                            self.refresh_current_turn_title();
                            self.push_terminal_notice("Codex コマンド失敗", message);
                        }
                    } else if !self.turn_finished_by_event {
                        if status.success() {
                            self.refresh_current_turn_title();
                            self.queue_completed_turn_body_record();
                            self.push_log(CodexLogKind::System, "Codex ターンが完了しました");
                            self.push_terminal_notice("Codex 完了", "Codex の出力が完了しました");
                        } else {
                            let message = format!("Codex ターンが失敗しました: {status}");
                            self.push_log(CodexLogKind::Error, message.clone());
                            self.refresh_current_turn_title();
                            self.queue_completed_turn_body_record();
                            self.push_terminal_notice("Codex 失敗", message);
                        }
                    }
                    self.turn_finished_by_event = false;
                    self.current_turn_prompt = None;
                    self.current_turn_retry_prompt = None;
                    self.start_next_queued_turn_if_idle();
                    changed = true;
                }
                Ok(None) => {}
                Err(e) => {
                    self.child = None;
                    self.turn_running = false;
                    self.current_command = None;
                    self.current_command_started_at = None;
                    self.child_process_label = None;
                    self.child_process_output.clear();
                    self.child_process_error_output.clear();
                    self.pending_decision = None;
                    self.current_turn_prompt = None;
                    self.current_turn_retry_prompt = None;
                    let message = format!("Codex 状態確認に失敗: {e}");
                    self.push_log(CodexLogKind::Error, message.clone());
                    self.push_terminal_notice("Codex エラー", message);
                    self.start_next_queued_turn_if_idle();
                    changed = true;
                }
            }
        }

        changed
    }

    fn handle_stdout_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        self.persist_raw_event("stdout", trimmed);
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => self.handle_json_event(value),
            Err(_) if self.child_process_label.is_some() => {
                self.child_process_output.push(trimmed.to_string());
            }
            Err(_) => self.push_log(CodexLogKind::Event, format!("Codex 出力: {trimmed}")),
        }
    }

    fn handle_stderr_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return;
        }
        // Codex の PATH alias 警告など、会話上重要でない既知ノイズは表示しない。
        if trimmed.contains("could not create PATH aliases") {
            return;
        }
        self.persist_raw_event("stderr", trimmed);
        if let Some(decision) = decision_banner("stderr", Some(trimmed)) {
            self.set_pending_decision(decision);
        }
        if self.child_process_label.is_some() {
            self.child_process_error_output.push(trimmed.to_string());
            return;
        }
        self.push_log(CodexLogKind::Event, format!("Codex 通知: {trimmed}"));
    }

    fn flush_child_process_output(&mut self, success: bool, label: &str) {
        let mut output = Vec::new();
        if !self.child_process_output.is_empty() {
            output.append(&mut self.child_process_output);
        }
        if !self.child_process_error_output.is_empty() {
            if !output.is_empty() {
                output.push(String::new());
            }
            output.push("stderr:".to_string());
            output.append(&mut self.child_process_error_output);
        }
        if output.is_empty() {
            return;
        }
        let state = if success { "OK" } else { "FAIL" };
        let summary = child_process_output_summary(&output.join("\n"));
        self.push_log(CodexLogKind::Tool, format!("{state} {label} ({summary})"));
    }

    fn handle_json_event(&mut self, value: Value) {
        if let Some(summary) = token_usage_summary(&value) {
            self.last_token_usage_label = Some(summary);
        }

        // 型名文字列の解釈は backend に委ね、pane 側は正規化イベントで分岐する。
        match self.backend.parse_lifecycle_event(&value) {
            AgentEvent::SessionStarted(session_id) => {
                if let Some(session_id) = session_id {
                    self.thread_id = Some(session_id);
                }
                self.push_log(CodexLogKind::System, "Codex セッションを開始しました");
            }
            AgentEvent::TurnStarted => {
                self.turn_running = true;
                self.turn_finished_by_event = false;
                self.current_command = None;
                self.current_command_started_at = None;
                self.pending_decision = None;
                self.action = Some("依頼を確認中".to_string());
                if self.turn_count > 0 {
                    self.collapsed_turns.insert(self.turn_count);
                }
                self.turn_count = self.turn_count.saturating_add(1);
                let label = self
                    .current_turn_prompt
                    .as_deref()
                    .or_else(|| self.last_prompt())
                    .map(compact_turn_prompt)
                    .filter(|prompt| !prompt.is_empty())
                    .map(|prompt| format!("Turn {} - {prompt}", self.turn_count))
                    .unwrap_or_else(|| format!("Turn {}", self.turn_count));
                self.push_log(CodexLogKind::Turn, label);
            }
            AgentEvent::TurnCompleted => {
                self.turn_running = false;
                self.turn_finished_by_event = true;
                self.current_command = None;
                self.current_command_started_at = None;
                self.streaming_assistant_index = None;
                self.pending_decision = None;
                self.refresh_current_turn_title();
                self.queue_completed_turn_body_record();
                self.action = Some("応答完了".to_string());
                self.push_log(CodexLogKind::System, "Codex の応答が完了しました");
                self.push_terminal_notice("Codex 完了", "Codex の出力が完了しました");
            }
            AgentEvent::TurnFailed(message) => {
                self.turn_running = false;
                self.turn_finished_by_event = true;
                self.current_command = None;
                self.current_command_started_at = None;
                self.streaming_assistant_index = None;
                self.pending_decision = None;
                self.push_log(CodexLogKind::Error, message.clone());
                self.refresh_current_turn_title();
                self.queue_completed_turn_body_record();
                self.push_terminal_notice("Codex 失敗", message);
            }
            AgentEvent::Error(message) => {
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex エラー", message);
            }
            AgentEvent::Other => {
                let event_type = value.get("type").and_then(Value::as_str).unwrap_or("event");
                self.handle_generic_json_event(event_type, &value);
            }
        }
    }

    fn handle_generic_json_event(&mut self, event_type: &str, value: &Value) {
        if let Some(decision) = decision_banner(event_type, first_text_field(value).as_deref()) {
            self.set_pending_decision(decision);
        }

        if let Some(display) = tool_display(event_type, value) {
            if let Some(command) = display.command_text.as_deref() {
                if is_tool_completion_event(event_type) {
                    self.current_command = None;
                    self.current_command_started_at = None;
                } else {
                    self.current_command = Some(compact_tool_text(command));
                    self.current_command_started_at = Some(Instant::now());
                }
            }
            if let Some(action_text) = display.action_text.as_deref() {
                self.refresh_action_from_text(action_text);
            }
            self.record_addness_tool_activity(&display);
            self.push_log(CodexLogKind::Tool, display.label);
            return;
        }

        if event_type.contains("message") || event_type.contains("output_text") {
            let text = first_text_field(value).unwrap_or_default();
            if text.is_empty() {
                return;
            } else if event_type.contains("delta") {
                self.append_assistant_delta(&text);
            } else {
                self.streaming_assistant_index = None;
                self.push_log(CodexLogKind::Assistant, text);
            }
            return;
        }

        if event_type.contains("exec")
            || event_type.contains("tool")
            || event_type.contains("function")
            || event_type.contains("mcp")
        {
            let Some(text) = first_text_field(value).filter(|text| !text.is_empty()) else {
                return;
            };
            if is_tool_completion_event(event_type) {
                self.current_command = None;
                self.current_command_started_at = None;
            } else {
                self.current_command = Some(compact_tool_text(&text));
                self.current_command_started_at = Some(Instant::now());
            }
            self.refresh_action_from_text(&text);
            self.push_log(
                CodexLogKind::Tool,
                format!("{} {text}", generic_tool_state_label(event_type)),
            );
            return;
        }

        if let Some(text) = first_text_field(value)
            && let Some(text) = visible_event_text(event_type, &text)
        {
            self.push_log(CodexLogKind::Event, text);
            return;
        }

        if let Some(label) = generic_event_label(event_type) {
            self.push_log(CodexLogKind::Event, label.to_string());
        }
    }

    fn refresh_action_from_text(&mut self, text: &str) {
        if let Some(rest) = addness_command_rest(text) {
            let (label, kind) = action_label(rest);
            match kind {
                AddnessActionKind::Read => {
                    self.last_addness_read_at = Some(Instant::now());
                    self.last_addness_read_label = Some(label.clone());
                }
                AddnessActionKind::Write => {
                    self.last_addness_write_at = Some(Instant::now());
                    self.last_addness_write_label = Some(label.clone());
                }
            }
            self.action = Some(label);
        }
    }

    fn set_pending_decision(&mut self, decision: CodexDecisionBanner) {
        let message = decision.message.clone();
        self.pending_decision = Some(decision);
        self.push_terminal_notice("Codex 確認待ち", message);
    }

    fn resolve_pending_decision(
        &mut self,
        decision: CodexDecisionBanner,
        response: CodexDecisionResponse,
        auto: bool,
    ) {
        self.pending_decision = None;
        let response_label = decision.label_for_response(response);
        if auto {
            self.action = Some(format!("確認自動応答: {response_label}"));
            self.push_activity(format!("確認待ちに {response_label} で自動応答"));
            self.push_terminal_notice(
                "Codex 確認自動応答",
                format!("{response_label} を自動選択しました"),
            );
            return;
        }

        self.action = Some(format!("確認応答: {response_label}"));
        self.push_activity(format!("確認待ちに {response_label} で応答"));
        match response {
            CodexDecisionResponse::Accept => {
                if matches!(
                    decision.kind,
                    CodexDecisionKind::Approval | CodexDecisionKind::Permission
                ) && self.retry_current_turn_with_approval_override(
                    CodexApprovalChoice::Never,
                    "今回だけ承認",
                ) {
                    self.push_terminal_notice(
                        "Codex 確認応答",
                        "今回だけ承認として現在のturnを再実行します",
                    );
                } else {
                    self.push_terminal_notice(
                        "Codex 確認応答",
                        format!("{response_label} を選択しました"),
                    );
                }
            }
            CodexDecisionResponse::Always => {
                self.exec_settings.approval = CodexApprovalChoice::Never;
                self.exec_settings.bypass_approvals_and_sandbox = false;
                if self.retry_current_turn_after_permission_change("これからずっと許可") {
                    self.push_terminal_notice(
                        "Codex 確認応答",
                        "approval=neverに変更し、現在のturnを再実行します",
                    );
                } else {
                    self.push_terminal_notice("Codex 確認応答", "approval=neverに変更しました");
                }
            }
            CodexDecisionResponse::Deny => {
                self.push_terminal_notice("Codex 確認応答", "拒否したためturnを中断します");
                self.kill_current_turn();
            }
        }
    }

    fn retry_current_turn_with_approval_override(
        &mut self,
        approval: CodexApprovalChoice,
        reason: &str,
    ) -> bool {
        self.one_shot_approval = Some(approval);
        if self.retry_current_turn_after_permission_change(reason) {
            true
        } else {
            self.one_shot_approval = None;
            false
        }
    }

    fn retry_current_turn_after_permission_change(&mut self, reason: &str) -> bool {
        let Some(retry_prompt) = self.current_turn_retry_prompt.clone() else {
            return false;
        };
        let display_prompt = self
            .current_turn_prompt
            .clone()
            .unwrap_or_else(|| retry_prompt.clone());
        self.kill_current_turn();
        self.action = Some(format!("{reason}: turn再実行中"));
        self.push_log(
            CodexLogKind::System,
            format!("{reason}の設定で現在のturnを再実行します"),
        );
        self.start_turn_with_display_prompt(&retry_prompt, &display_prompt);
        true
    }

    fn record_addness_tool_activity(&mut self, display: &ToolDisplay) {
        let Some(command) = display.command_text.as_deref() else {
            return;
        };
        if addness_command_rest(command).is_none() && !looks_like_addness_command_text(command) {
            return;
        }
        let Some(activity) = addness_activity_summary(command, display.output_text.as_deref())
        else {
            return;
        };
        let now = chrono::Local::now().format("%H:%M");
        self.push_activity(format!("{now} {activity}"));
    }

    /// Codex の画面状態から、ホスト端末へ流すべき通知を 1 件だけ取り出す。
    pub fn take_terminal_notice(&mut self) -> Option<TerminalNotice> {
        self.pending_notices.pop_front()
    }

    fn push_terminal_notice(&mut self, title: impl Into<String>, message: impl Into<String>) {
        self.pending_notices.push_back(TerminalNotice {
            title: title.into(),
            message: message.into(),
        });
    }

    /// 描画領域に合わせて保持サイズを更新する（変化時のみ）。
    pub fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if rows == self.rows && cols == self.cols {
            return;
        }
        self.rows = rows;
        self.cols = cols;
        self.invalidate_rendered_history_metrics();
        self.scrollback = self.scrollback.min(self.max_view_scrollback());
    }

    /// キー入力を Addness 側の入力欄へ反映する。
    pub fn input(&mut self, key: KeyEvent) {
        if self.finished {
            return;
        }

        if self.is_turn_running()
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c' | 'C'))
        {
            self.kill_current_turn();
            self.start_next_queued_turn_if_idle();
            return;
        }

        if self.pending_decision.is_some() {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c' | 'C'))
            {
                self.kill_current_turn();
            }
            return;
        }

        let observed = self.input_state.observe_key(key);
        // 入力が変わったらパレットの選択を先頭へ戻す（↑↓/Tab は app 側で消費済み）。
        self.slash_palette_selected = 0;
        let Some(submitted) = observed else {
            return;
        };
        self.submit_user_line(&submitted);
    }

    pub fn paste_input(&mut self, text: &str) {
        if self.finished || self.pending_decision.is_some() {
            return;
        }
        self.input_state.insert_text(text);
    }

    /// TUI 側の操作からシステムプロンプト（F9 再開など）を codex に送信する。
    /// ユーザーの作業依頼ではないので last_prompt は更新しない。
    pub fn submit_system_line(&mut self, line: &str) {
        let submitted = normalize_submitted_line(line);
        if submitted.is_empty() || self.is_turn_running() || self.finished {
            return;
        }
        self.push_log(CodexLogKind::System, format!("Addness: {submitted}"));
        self.start_turn(&submitted);
    }

    fn submit_user_line(&mut self, submitted: &str) {
        let submitted = normalize_submitted_line(submitted);
        if submitted.is_empty() {
            return;
        }

        if self.handle_local_slash_command(&submitted) {
            return;
        }

        if self.is_turn_running() {
            self.record_and_run_user_line(submitted);
            return;
        }

        self.record_and_run_user_line(submitted);
    }

    fn record_and_run_user_line(&mut self, submitted: String) {
        self.input_state.record_submitted(&submitted);
        self.push_log(CodexLogKind::User, submitted.clone());

        if self.is_turn_running() {
            let count = self.queue_user_prompt(submitted);
            self.action = Some(format!("次ターン予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("Codex 実行中のため次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.run_submitted_line(submitted);
    }

    fn queue_user_prompt(&mut self, submitted: String) -> usize {
        self.queued_prompts.push_back(QueuedPrompt::user(submitted));
        self.queued_prompts.len()
    }

    fn queue_direct_prompt(&mut self, prompt: String, display_prompt: String) -> usize {
        self.queued_prompts
            .push_back(QueuedPrompt::direct(prompt, display_prompt));
        self.queued_prompts.len()
    }

    fn queue_direct_work_prompt(
        &mut self,
        prompt: String,
        display_prompt: String,
        active_work: ActiveWorkPackage,
    ) -> usize {
        self.queued_prompts.push_back(QueuedPrompt::direct_work(
            prompt,
            display_prompt,
            active_work,
        ));
        self.queued_prompts.len()
    }

    fn run_submitted_line(&mut self, submitted: String) {
        self.run_submitted_line_with_display(submitted.clone(), submitted);
    }

    fn run_submitted_line_with_display(&mut self, submitted: String, display_prompt: String) {
        if is_exit_prompt(&submitted) {
            self.finish_from_exit_command();
            return;
        }

        let exec_prompt = self.prompt_with_goal_mode(&submitted);
        self.start_turn_with_display_prompt(&exec_prompt, &display_prompt);
    }

    fn finish_from_exit_command(&mut self) {
        self.finished = true;
        self.queued_prompts.clear();
        self.pending_decision = None;
        self.current_command = None;
        self.current_command_started_at = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.push_log(CodexLogKind::System, "Codex セッションを終了します");
    }

    fn start_turn(&mut self, prompt: &str) {
        self.start_turn_with_display_prompt(prompt, prompt);
    }

    fn start_turn_with_display_prompt(&mut self, prompt: &str, display_prompt: &str) {
        if self.is_turn_running() {
            self.push_log(CodexLogKind::System, "前の Codex ターンがまだ実行中です");
            return;
        }

        let retry_prompt = prompt.to_string();
        let prompt = self.prompt_with_addness_context(prompt);
        let command_result = self.spawn_exec_process(&prompt);
        self.one_shot_approval = None;
        match command_result {
            Ok(child) => {
                self.child = Some(child);
                self.turn_running = true;
                self.turn_finished_by_event = false;
                self.pending_decision = None;
                self.diff_view = None;
                self.exec_settings.image_paths.clear();
                self.current_turn_prompt = Some(display_prompt.to_string());
                self.current_turn_retry_prompt = Some(retry_prompt);
                self.scroll_to_live();
            }
            Err(e) => {
                let message = format!("Codex の起動に失敗しました: {e}");
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex 起動失敗", message);
            }
        }
    }

    fn handle_local_slash_command(&mut self, submitted: &str) -> bool {
        let Some(command_line) = submitted.strip_prefix('/') else {
            return false;
        };
        if command_line.trim().is_empty() {
            return false;
        }

        let mut parts = command_line.trim().splitn(2, char::is_whitespace);
        let command = parts.next().unwrap_or_default().to_ascii_lowercase();
        let args = parts.next().unwrap_or_default().trim();
        match command.as_str() {
            "goal" => {
                self.handle_goal_slash_command(args);
                true
            }
            "settings" => {
                self.push_log(
                    CodexLogKind::System,
                    format!("Settings: {}", self.settings_label()),
                );
                true
            }
            "ide" => {
                self.push_ide_status();
                true
            }
            "cd" | "cwd" => {
                self.handle_cwd_slash_command(args);
                true
            }
            "codex" => {
                self.handle_codex_subcommand_slash_command(args);
                true
            }
            "exec" | "e" | "codex-exec" => {
                self.handle_exec_slash_command(args);
                true
            }
            "interactive" | "codex-interactive" => {
                self.handle_root_interactive_slash_command(args);
                true
            }
            "codex-help" => {
                self.handle_named_codex_subcommand("help", args);
                true
            }
            "codex-version" | "version" => {
                self.handle_named_codex_subcommand("version", args);
                true
            }
            "doctor" => {
                self.handle_named_codex_subcommand("doctor", args);
                true
            }
            "features" => {
                self.handle_named_codex_subcommand("features", args);
                true
            }
            "experimental" | "experiments" => {
                self.handle_named_codex_subcommand("features", args);
                true
            }
            "review" => {
                self.handle_review_slash_command(args);
                true
            }
            "exec-review" => {
                self.handle_exec_review_slash_command(args);
                true
            }
            "apply" | "a" => {
                self.handle_apply_slash_command(args);
                true
            }
            "import" => {
                self.handle_import_slash_command(args);
                true
            }
            "hooks" => {
                self.handle_hooks_slash_command(args);
                true
            }
            "mcp" => {
                self.handle_named_codex_subcommand("mcp", args);
                true
            }
            "apps" => {
                self.handle_apps_slash_command(args);
                true
            }
            "plugin" => {
                self.handle_named_codex_subcommand("plugin", args);
                true
            }
            "cloud" => {
                self.handle_named_codex_subcommand("cloud", args);
                true
            }
            "login" => {
                self.handle_named_codex_subcommand("login", args);
                true
            }
            "logout" => {
                self.handle_named_codex_subcommand("logout", args);
                true
            }
            "update" | "update-codex" | "codex-update" => {
                self.handle_named_codex_subcommand("update", args);
                true
            }
            "app" | "codex-app" => {
                self.handle_named_codex_subcommand("app", args);
                true
            }
            "app-server" => {
                self.handle_named_codex_subcommand("app-server", args);
                true
            }
            "remote-control" => {
                self.handle_named_codex_subcommand("remote-control", args);
                true
            }
            "sandbox-run" | "sandbox-command" | "codex-sandbox" => {
                self.handle_named_codex_subcommand("sandbox", args);
                true
            }
            "debug" => {
                self.handle_named_codex_subcommand("debug", args);
                true
            }
            "debug-config" | "debugconfig" => {
                self.push_debug_config();
                true
            }
            "feedback" => {
                self.push_feedback_draft(args);
                true
            }
            "test-approval" => {
                self.handle_test_approval_slash_command(args);
                true
            }
            "completion" => {
                self.handle_named_codex_subcommand("completion", args);
                true
            }
            "mcp-server" => {
                self.handle_named_codex_subcommand("mcp-server", args);
                true
            }
            "exec-server" => {
                self.handle_named_codex_subcommand("exec-server", args);
                true
            }
            "sessions" | "session-list" => {
                self.handle_sessions_slash_command(args);
                true
            }
            "resume-last" => {
                self.handle_resume_last_slash_command(args, false);
                true
            }
            "resume-last-all" | "resume-all-last" => {
                self.handle_resume_last_slash_command(args, true);
                true
            }
            "resume-session" | "resume-codex" => {
                self.handle_resume_session_slash_command(args, false);
                true
            }
            "resume-session-all" | "resume-codex-all" => {
                self.handle_resume_session_slash_command(args, true);
                true
            }
            "resume-interactive-last" | "interactive-resume-last" => {
                self.handle_root_resume_last_slash_command(args, false, false);
                true
            }
            "resume-interactive-last-all" | "interactive-resume-last-all" => {
                self.handle_root_resume_last_slash_command(args, true, false);
                true
            }
            "resume-interactive-last-noninteractive"
            | "resume-interactive-last-all-sessions"
            | "interactive-resume-last-noninteractive" => {
                self.handle_root_resume_last_slash_command(args, true, true);
                true
            }
            "resume-interactive-session" | "interactive-resume-session" => {
                self.handle_root_resume_session_slash_command(args, false);
                true
            }
            "resume-interactive-session-all" | "interactive-resume-session-all" => {
                self.handle_root_resume_session_slash_command(args, true);
                true
            }
            "codex-resume" | "resume-command" => {
                self.handle_root_session_command_slash_command("resume", args);
                true
            }
            "side" | "side-conversation" => {
                self.handle_side_slash_command(args);
                true
            }
            "fork" | "codex-fork" => {
                self.handle_root_session_command_slash_command("fork", args);
                true
            }
            "fork-last" => {
                self.handle_fork_last_slash_command(args, false);
                true
            }
            "fork-last-all" | "fork-all-last" => {
                self.handle_fork_last_slash_command(args, true);
                true
            }
            "fork-session" | "fork-codex" => {
                self.handle_fork_session_slash_command(args, false);
                true
            }
            "fork-session-all" | "fork-codex-all" => {
                self.handle_fork_session_slash_command(args, true);
                true
            }
            "archive" | "archive-session" | "archive-codex" => {
                self.handle_session_admin_slash_command("archive", args);
                true
            }
            "unarchive" | "unarchive-session" | "unarchive-codex" => {
                self.handle_session_admin_slash_command("unarchive", args);
                true
            }
            "delete" | "delete-session" | "delete-codex-session" => {
                self.handle_session_admin_slash_command("delete", args);
                true
            }
            "rename" | "rename-thread" | "thread-name" => {
                self.handle_rename_slash_command(args);
                true
            }
            "new" | "new-thread" | "new-chat" => {
                self.handle_new_thread_slash_command();
                true
            }
            "clear" | "clear-log" | "clear-terminal" => {
                self.handle_clear_log_slash_command();
                true
            }
            "stop" | "interrupt" => {
                self.handle_stop_slash_command(args);
                true
            }
            "init" => {
                self.handle_init_slash_command(args);
                true
            }
            "compact" => {
                self.handle_compact_slash_command(args);
                true
            }
            "plan" => {
                self.handle_plan_slash_command(args);
                true
            }
            "organize" | "team" | "delegate" | "breakdown" => {
                self.handle_organize_slash_command(args);
                true
            }
            "work" | "workon" | "child" | "child-goal" | "package" => {
                self.handle_child_work_slash_command(args);
                true
            }
            "model" => {
                if args.is_empty() {
                    self.cycle_model();
                } else {
                    self.set_model(args);
                }
                true
            }
            "reasoning" | "effort" => {
                if args.is_empty() {
                    self.cycle_reasoning();
                } else if let Some(choice) = parse_reasoning_choice(args) {
                    self.set_reasoning(choice);
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        "reasoning は config / low / medium / high / xhigh を指定してください",
                    );
                }
                true
            }
            "approval" | "approvals" => {
                if args.is_empty() {
                    self.cycle_approval();
                } else if let Some(choice) = parse_approval_choice(args) {
                    self.set_approval(choice);
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        "approval は config / untrusted / on-request / on-failure / never を指定してください",
                    );
                }
                true
            }
            "permissions" | "permission" => {
                self.handle_permissions_slash_command(args);
                true
            }
            "personality" | "persona" => {
                self.handle_personality_slash_command(args);
                true
            }
            "statusline" | "status-line" => {
                self.handle_statusline_slash_command(args);
                true
            }
            "theme" => {
                self.handle_theme_slash_command(args);
                true
            }
            "pets" | "pet" => {
                self.handle_pet_slash_command(args);
                true
            }
            "vim" => {
                self.handle_vim_slash_command(args);
                true
            }
            "raw" | "raw-output" => {
                self.handle_raw_output_slash_command(args);
                true
            }
            "keymap" => {
                self.handle_keymap_slash_command(args);
                true
            }
            "remember" | "memo" | "note" => {
                self.handle_remember_slash_command(args);
                true
            }
            "handoff" | "checkpoint" | "save-context" => {
                self.handle_handoff_slash_command(args);
                true
            }
            "memories" | "memory" => {
                self.handle_memories_slash_command(args);
                true
            }
            "sandbox" => {
                if args.is_empty() {
                    self.cycle_sandbox();
                } else if let Some(choice) = parse_sandbox_choice(args) {
                    self.set_sandbox(choice);
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        "sandbox は read-only / workspace-write / danger-full-access を指定してください",
                    );
                }
                true
            }
            "search" | "web-search" => {
                self.toggle_web_search();
                true
            }
            "oss" => {
                self.toggle_oss();
                true
            }
            "remote" => {
                self.handle_remote_slash_command(args);
                true
            }
            "remote-auth-token-env" | "remote-auth-env" => {
                self.handle_remote_auth_token_env_slash_command(args);
                true
            }
            "no-alt-screen" => {
                self.toggle_no_alt_screen();
                true
            }
            "color" | "colour" => {
                self.handle_color_slash_command(args);
                true
            }
            "local-provider" | "provider" => {
                self.handle_local_provider_slash_command(args);
                true
            }
            "profile" => {
                self.handle_profile_slash_command(args);
                true
            }
            "image" | "images" => {
                self.handle_image_slash_command(args);
                true
            }
            "attachments" | "attachment" => {
                self.handle_attachments_slash_command(args);
                true
            }
            "add-dir" | "adddir" => {
                self.handle_add_dir_slash_command(args);
                true
            }
            "sandbox-add-read-dir" | "sandbox-read-dir" => {
                self.handle_sandbox_add_read_dir_slash_command(args);
                true
            }
            "setup-default-sandbox" | "setup-sandbox" => {
                self.handle_setup_default_sandbox_slash_command();
                true
            }
            "config" => {
                self.handle_config_slash_command(args);
                true
            }
            "enable" => {
                self.handle_feature_slash_command(args, true);
                true
            }
            "disable" => {
                self.handle_feature_slash_command(args, false);
                true
            }
            "strict-config" => {
                self.toggle_strict_config();
                true
            }
            "ignore-user-config" => {
                self.toggle_ignore_user_config();
                true
            }
            "ignore-rules" => {
                self.toggle_ignore_rules();
                true
            }
            "skip-git-check" | "skip-git-repo-check" => {
                self.toggle_skip_git_repo_check();
                true
            }
            "ephemeral" => {
                self.toggle_ephemeral();
                true
            }
            "bypass" | "dangerously-bypass" => {
                self.toggle_bypass_approvals_and_sandbox();
                true
            }
            "bypass-hook-trust" | "dangerously-bypass-hook-trust" | "hook-trust-bypass" => {
                self.toggle_bypass_hook_trust();
                true
            }
            "output-schema" => {
                self.handle_output_schema_slash_command(args);
                true
            }
            "output-last-message" | "output-message" => {
                self.handle_output_last_message_slash_command(args);
                true
            }
            "diff" => {
                self.toggle_diff_view();
                self.push_log(
                    CodexLogKind::System,
                    format!("Diff view: {}", self.diff_label()),
                );
                true
            }
            "history" => {
                self.push_slash_history();
                true
            }
            "turn" | "turns" => {
                self.handle_turn_slash_command(args);
                true
            }
            "rollout" => {
                self.push_rollout_status();
                true
            }
            "ps" | "terminals" | "background" => {
                self.push_process_status();
                true
            }
            "btw" | "copy" | "copy-last" => {
                self.push_last_assistant_markdown();
                true
            }
            "skills" => {
                self.handle_skills_slash_command(args);
                true
            }
            "resume" => {
                self.submit_system_line(resume_prompt());
                true
            }
            "help" => {
                self.push_slash_help();
                true
            }
            "status" => {
                self.push_slash_status();
                true
            }
            "usage" | "tokens" | "token-usage" => {
                self.push_slash_usage();
                true
            }
            _ => false,
        }
    }

    fn handle_codex_subcommand_slash_command(&mut self, args: &str) {
        let args = match split_codex_command_args(args) {
            Ok(args) if args.is_empty() => {
                self.push_log(
                    CodexLogKind::Error,
                    "/codex にはサブコマンドを指定してください。例: /codex doctor",
                );
                return;
            }
            Ok(args) => args,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        let args = codex_named_subcommand_args_with_settings(args, &self.exec_settings);
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_exec_slash_command(&mut self, prompt: &str) {
        let prompt = normalize_submitted_line(prompt);
        if prompt.is_empty() {
            self.push_log(CodexLogKind::Error, "/exec には prompt を指定してください");
            return;
        }
        self.record_and_run_user_line(prompt);
    }

    fn handle_new_thread_slash_command(&mut self) {
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::Error,
                "Codex 実行中です。完了後に新しいセッションを開始してください",
            );
            return;
        }
        self.thread_id = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.pending_decision = None;
        self.action = Some("新しいCodexセッション".to_string());
        self.push_log(
            CodexLogKind::System,
            "新しい Codex セッションを開始します。次の入力は新規セッションへ送信します",
        );
    }

    fn handle_clear_log_slash_command(&mut self) {
        self.log.clear();
        self.collapsed_turns.clear();
        self.scrollback = 0;
        self.streaming_assistant_index = None;
        self.invalidate_rendered_history_metrics();
        self.push_log(CodexLogKind::System, "Codex 表示ログをクリアしました");
    }

    fn handle_stop_slash_command(&mut self, args: &str) {
        if self.is_turn_running() {
            self.kill_current_turn();
        } else {
            self.push_log(CodexLogKind::System, "停止中のCodexターンはありません");
        }
        if args == "all" || args == "queued" {
            let cleared = self.queued_prompts.len();
            self.queued_prompts.clear();
            self.push_log(
                CodexLogKind::System,
                format!("予約済みターンを {cleared} 件クリアしました"),
            );
        }
    }

    fn handle_init_slash_command(&mut self, args: &str) {
        let mut prompt = init_prompt().to_string();
        if !args.is_empty() {
            prompt.push_str("\n\nAdditional user instructions:\n");
            prompt.push_str(args);
        }
        self.record_and_run_user_line(prompt);
    }

    fn handle_compact_slash_command(&mut self, args: &str) {
        let mut prompt =
            "この会話を今後の継続に必要な情報へ圧縮してください。決定事項、変更済みファイル、未完了作業、検証結果を短く構造化して残してください。".to_string();
        if !args.trim().is_empty() {
            prompt.push_str("\n\n追加観点:\n");
            prompt.push_str(args.trim());
        }
        self.submit_system_line(&prompt);
    }

    fn handle_plan_slash_command(&mut self, args: &str) {
        let mut prompt =
            "実装に入る前に、現在の依頼を達成するための短い作業計画を作ってください。未確認事項、実装順、検証方法を具体化してください。".to_string();
        if !args.trim().is_empty() {
            prompt.push_str("\n\n対象依頼:\n");
            prompt.push_str(args.trim());
        }
        self.submit_system_line(&prompt);
    }

    fn handle_organize_slash_command(&mut self, args: &str) {
        let task = normalize_submitted_line(args);
        let prompt = addness_organize_prompt(task.as_str());
        let display = if task.is_empty() {
            "Addnessで作業分解して実装".to_string()
        } else {
            format!("Addnessで作業分解: {}", compact_one_line(&task, 160))
        };
        self.input_state.record_submitted(&display);
        self.push_log(CodexLogKind::User, display.clone());

        if self.is_turn_running() {
            let count = self.queue_direct_prompt(prompt, display);
            self.action = Some(format!("作業分解を予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("作業分解を次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.start_turn_with_display_prompt(&prompt, &display);
    }

    fn handle_child_work_slash_command(&mut self, args: &str) {
        let selector = normalize_submitted_line(args);
        if selector.is_empty() || selector.eq_ignore_ascii_case("list") {
            self.action = Some("子ゴール一覧を表示".to_string());
            self.push_log(CodexLogKind::System, child_goal_work_list(&self.children));
            return;
        }
        if selector.eq_ignore_ascii_case("all") || selector.eq_ignore_ascii_case("queue") {
            self.handle_child_work_all_slash_command();
            return;
        }

        let idx = match child_goal_index_for_selector(&self.children, &selector) {
            Ok(idx) => idx,
            Err(message) => {
                self.push_log(CodexLogKind::Error, message);
                return;
            }
        };
        let Some((prompt, display, action, active_work)) = self.child_goal_work_parts(idx) else {
            self.push_log(CodexLogKind::Error, "子ゴールが見つかりません");
            return;
        };
        self.set_active_work_package(idx);
        self.input_state.record_submitted(&display);
        self.push_log(CodexLogKind::User, display.clone());

        if self.is_turn_running() {
            let count = self.queue_direct_work_prompt(prompt, display, active_work);
            self.action = Some(format!("子ゴール着手を予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("子ゴール着手を次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.action = Some(action);
        self.start_turn_with_display_prompt(&prompt, &display);
    }

    fn handle_child_work_all_slash_command(&mut self) {
        let items = self
            .children
            .iter()
            .enumerate()
            .filter(|(_, child)| !child.is_completed)
            .filter_map(|(idx, _)| self.child_goal_work_parts(idx))
            .collect::<Vec<_>>();
        if items.is_empty() {
            self.push_log(
                CodexLogKind::Error,
                "未完了の子ゴールがありません。/organize で分解するか、Addness側で子ゴールを追加してください",
            );
            return;
        }

        let added = items.len();
        let display = format!("未完了子ゴールを一括着手 {added}件");
        self.input_state.record_submitted(&display);
        self.push_log(CodexLogKind::User, display);
        for (prompt, display_prompt, _, active_work) in items {
            self.queue_direct_work_prompt(prompt, display_prompt, active_work);
        }
        self.push_log(
            CodexLogKind::System,
            format!("未完了子ゴール {added}件をワークキューに追加しました"),
        );

        if self.is_turn_running() {
            self.action = Some(format!("子ゴール一括着手を予約 {added}件"));
            return;
        }

        self.start_next_queued_turn_if_idle();
    }

    fn child_goal_work_parts(
        &self,
        idx: usize,
    ) -> Option<(String, String, String, ActiveWorkPackage)> {
        let child = self.children.get(idx)?;
        let ordinal = idx + 1;
        let prompt = addness_child_goal_work_prompt(self, ordinal, child);
        let display = format!(
            "子ゴール着手 #{}: {}",
            ordinal,
            compact_one_line(&child.title, 160)
        );
        let action = format!(
            "子ゴール着手 #{}: {}",
            ordinal,
            compact_one_line(&child.title, 80)
        );
        let active_work = ActiveWorkPackage {
            id: child.id.clone(),
            title: child.title.clone(),
            ordinal,
        };
        Some((prompt, display, action, active_work))
    }

    fn handle_skills_slash_command(&mut self, args: &str) {
        let args = args.trim();
        if args.is_empty() || args == "list" {
            let roots = codex_skill_roots(&self.cwd);
            let skills = collect_skill_names_from_roots(&roots);
            if skills.is_empty() {
                self.push_log(CodexLogKind::System, "Skills: none found");
            } else {
                self.push_log(
                    CodexLogKind::System,
                    format!("Skills:\n{}", skills.join("\n")),
                );
            }
            return;
        }
        if args == "roots" {
            let roots = codex_skill_roots(&self.cwd)
                .iter()
                .map(|path| compact_home_path(path))
                .collect::<Vec<_>>();
            self.push_log(
                CodexLogKind::System,
                format!("Skill roots:\n{}", roots.join("\n")),
            );
            return;
        }
        self.submit_system_line(&format!(
            "Use the Codex skill named `{args}` for the next response if it is available and relevant."
        ));
    }

    fn handle_named_codex_subcommand(&mut self, name: &str, args: &str) {
        let args = match codex_named_subcommand_args(name, args) {
            Ok(args) => args,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        let args = codex_named_subcommand_args_with_settings(args, &self.exec_settings);
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_root_interactive_slash_command(&mut self, prompt: &str) {
        let args = codex_root_interactive_args(prompt.trim(), &self.cwd, &self.exec_settings);
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_review_slash_command(&mut self, args: &str) {
        let args = match codex_review_args(args, &self.cwd, &self.exec_settings) {
            Ok(args) => args,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_exec_review_slash_command(&mut self, args: &str) {
        let args = match codex_exec_review_args(args, &self.cwd, &self.exec_settings) {
            Ok(args) => args,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_apply_slash_command(&mut self, args: &str) {
        let args = match codex_apply_args(args, &self.exec_settings) {
            Ok(args) => args,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn push_ide_status(&mut self) {
        self.push_log(
            CodexLogKind::System,
            "IDE context: このAddness TUIではIDE選択範囲の直接取得は未接続です。画像は /image、追加ディレクトリは /add-dir、ファイル指定は通常入力で渡せます。",
        );
    }

    fn handle_test_approval_slash_command(&mut self, args: &str) {
        let message = if args.trim().is_empty() {
            "Test approval request from /test-approval".to_string()
        } else {
            args.trim().to_string()
        };
        self.set_pending_decision(CodexDecisionBanner::new(
            CodexDecisionKind::Permission,
            message,
        ));
    }

    fn handle_import_slash_command(&mut self, args: &str) {
        let cwd = Path::new(&self.cwd);
        let candidates = ["CLAUDE.md", ".claude/settings.json", ".mcp.json"];
        let found = candidates
            .iter()
            .filter_map(|path| {
                let full = cwd.join(path);
                full.is_file().then(|| (*path).to_string())
            })
            .collect::<Vec<_>>();
        match args.trim() {
            "" | "status" | "show" => {
                let label = if found.is_empty() {
                    "none".to_string()
                } else {
                    found.join(", ")
                };
                self.push_log(CodexLogKind::System, format!("Import candidates: {label}"));
            }
            "run" | "agents" | "claude" => {
                let mut prompt = "Claude Code など外部エージェント設定を確認し、このリポジトリでCodexが使うべき内容をAGENTS.mdへ安全に統合してください。既存AGENTS.mdがあれば破壊せず追記・更新してください。".to_string();
                if !found.is_empty() {
                    prompt.push_str("\n\n検出ファイル:\n");
                    prompt.push_str(&found.join("\n"));
                }
                self.submit_system_line(&prompt);
            }
            _ => self.push_log(
                CodexLogKind::Error,
                "import は status / run を指定してください",
            ),
        }
    }

    fn handle_hooks_slash_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "show" | "status" | "list" => {
                let values = self
                    .exec_settings
                    .config_overrides
                    .iter()
                    .filter(|entry| config_override_key(entry).starts_with("hooks"))
                    .cloned()
                    .collect::<Vec<_>>();
                self.push_numbered_settings_list("Hook overrides", &values);
            }
            "clear" | "config" | "default" => self.clear_config_override_prefix("hooks", "hooks"),
            value if value.contains('=') => {
                let (path, raw_value) = value.split_once('=').unwrap_or_default();
                let path = path.trim();
                let key = if path.starts_with("hooks") {
                    path.to_string()
                } else {
                    format!("hooks.{path}")
                };
                self.set_config_override_key(&key, raw_value.trim().to_string(), "hooks");
            }
            _ => self.push_log(
                CodexLogKind::Error,
                "hooks は key=value 形式で指定してください。例: /hooks my_hook.enabled=true",
            ),
        }
    }

    fn handle_apps_slash_command(&mut self, args: &str) {
        if args.trim().is_empty() || matches!(args, "show" | "status" | "list") {
            self.push_log(
                CodexLogKind::System,
                "Apps: /app でCodex Desktop、/app-server でapp-server、/remote-control でremote controlを実行できます",
            );
        } else {
            self.handle_named_codex_subcommand("app-server", args);
        }
    }

    fn start_codex_subcommand(&mut self, args: Vec<String>, label: String) {
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::Error,
                "Codex 実行中です。完了後にサブコマンドを実行してください",
            );
            return;
        }
        if self.finished {
            self.push_log(CodexLogKind::Error, "Codex ペイン終了後は実行できません");
            return;
        }
        match self.spawn_codex_subcommand_process(&args) {
            Ok(child) => {
                let category = codex_command_category(&args);
                self.child = Some(child);
                self.turn_running = true;
                self.turn_finished_by_event = false;
                self.pending_decision = None;
                self.diff_view = None;
                self.current_command = Some(label.clone());
                self.current_command_started_at = Some(Instant::now());
                self.child_process_label = Some(label.clone());
                self.child_process_output.clear();
                self.child_process_error_output.clear();
                self.action = Some(format!("{category}: {label}"));
                self.push_log(CodexLogKind::Tool, format!("RUNNING [{category}] {label}"));
                self.scroll_to_live();
            }
            Err(e) => {
                let message = format!("{label} の起動に失敗しました: {e}");
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex コマンド起動失敗", message);
            }
        }
    }

    fn handle_sessions_slash_command(&mut self, args: &str) {
        let limit = args
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(12)
            .min(50);
        match load_codex_session_candidates(limit) {
            Ok(sessions) => {
                self.indexed_sessions = sessions;
                self.push_codex_sessions_list();
            }
            Err(e) => self.push_log(
                CodexLogKind::Error,
                format!("Codex sessions の読み込みに失敗しました: {e}"),
            ),
        }
    }

    fn push_codex_sessions_list(&mut self) {
        if self.indexed_sessions.is_empty() {
            self.push_log(CodexLogKind::System, "Codex sessions: none");
            return;
        }
        let lines = self
            .indexed_sessions
            .iter()
            .enumerate()
            .map(|(idx, session)| session_list_line(idx + 1, session))
            .collect::<Vec<_>>()
            .join("\n");
        self.push_log(CodexLogKind::System, format!("Codex sessions:\n{lines}"));
    }

    fn handle_resume_last_slash_command(&mut self, args: &str, include_all: bool) {
        let prompt = if args.trim().is_empty() {
            resume_prompt().to_string()
        } else {
            args.trim().to_string()
        };
        let command = codex_exec_resume_args(None, true, include_all, &prompt, &self.exec_settings);
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_resume_session_slash_command(&mut self, args: &str, include_all: bool) {
        let Some((session_ref, prompt)) = split_first_arg(args) else {
            self.push_log(
                CodexLogKind::Error,
                "resume-session には番号・session id・session名のいずれかを指定してください",
            );
            return;
        };
        let Some(session) = self.resolve_session_ref(&session_ref) else {
            return;
        };
        let prompt = if prompt.trim().is_empty() {
            resume_prompt().to_string()
        } else {
            prompt.trim().to_string()
        };
        let command = codex_exec_resume_args(
            Some(&session),
            false,
            include_all,
            &prompt,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_root_resume_last_slash_command(
        &mut self,
        args: &str,
        include_all: bool,
        include_non_interactive: bool,
    ) {
        let command = codex_root_resume_args(
            None,
            true,
            include_all,
            include_non_interactive,
            args.trim(),
            &self.cwd,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_root_resume_session_slash_command(&mut self, args: &str, include_all: bool) {
        let Some((session_ref, prompt)) = split_first_arg(args) else {
            self.push_log(
                CodexLogKind::Error,
                "resume-interactive-session には番号・session id・session名のいずれかを指定してください",
            );
            return;
        };
        let Some(session) = self.resolve_session_ref(&session_ref) else {
            return;
        };
        let command = codex_root_resume_args(
            Some(&session),
            false,
            include_all,
            false,
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_root_session_command_slash_command(&mut self, command_name: &str, args: &str) {
        let command = match codex_root_session_command_args(
            command_name,
            args,
            &self.cwd,
            &self.exec_settings,
        ) {
            Ok(command) => command,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_side_slash_command(&mut self, prompt: &str) {
        let Some(thread_id) = self.thread_id.as_deref() else {
            self.push_log(
                CodexLogKind::Error,
                "/side はCodexセッション開始後に使えます。先に通常入力でCodexへ送信してください",
            );
            return;
        };
        let command = codex_fork_args(
            Some(thread_id),
            false,
            false,
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_fork_last_slash_command(&mut self, args: &str, include_all: bool) {
        let command = codex_fork_args(
            None,
            true,
            include_all,
            args.trim(),
            &self.cwd,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_fork_session_slash_command(&mut self, args: &str, include_all: bool) {
        let Some((session_ref, prompt)) = split_first_arg(args) else {
            self.push_log(
                CodexLogKind::Error,
                "fork-session には番号・session id・session名のいずれかを指定してください",
            );
            return;
        };
        let Some(session) = self.resolve_session_ref(&session_ref) else {
            return;
        };
        let command = codex_fork_args(
            Some(&session),
            false,
            include_all,
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_session_admin_slash_command(&mut self, command_name: &str, args: &str) {
        let Some((session_ref, rest)) = split_first_arg(args) else {
            self.push_log(
                CodexLogKind::Error,
                format!("{command_name}-session には番号・session id・session名のいずれかを指定してください"),
            );
            return;
        };
        let Some(session) = self.resolve_session_ref(&session_ref) else {
            return;
        };
        let mut extra_args = Vec::new();
        if command_name == "delete" && !looks_like_uuid(&session) {
            self.push_log(
                CodexLogKind::Error,
                "delete-session はUUIDに解決できるsessionだけ実行できます。先に /sessions で番号を確認してください",
            );
            return;
        }
        if !rest.trim().is_empty() {
            match split_codex_command_args(&rest) {
                Ok(extra) => extra_args = extra,
                Err(e) => {
                    self.push_log(CodexLogKind::Error, e.to_string());
                    return;
                }
            }
        }
        let command = codex_session_admin_args(
            command_name,
            &session,
            command_name == "delete",
            extra_args,
            &self.cwd,
            &self.exec_settings,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_rename_slash_command(&mut self, args: &str) {
        let title = normalize_submitted_line(args)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if title.is_empty() {
            self.push_log(
                CodexLogKind::Error,
                "/rename には新しいセッション名を指定してください",
            );
            return;
        }
        self.rename_current_thread_with_home(&title, None);
    }

    fn rename_current_thread_with_home(&mut self, title: &str, codex_home: Option<&Path>) {
        let Some(thread_id) = self.thread_id.clone() else {
            self.push_log(
                CodexLogKind::Error,
                "/rename はCodexセッション開始後に使えます。先に通常入力でCodexへ送信してください",
            );
            return;
        };
        let result = if let Some(home) = codex_home {
            append_codex_session_rename_to(home, &thread_id, title)
        } else {
            append_codex_session_rename(&thread_id, title)
        };
        match result {
            Ok(()) => {
                for session in &mut self.indexed_sessions {
                    if session.id == thread_id {
                        session.title = title.to_string();
                    }
                }
                self.action = Some(format!("renamed: {title}"));
                self.push_log(
                    CodexLogKind::System,
                    format!("Codex セッション名を更新しました: {title}"),
                );
            }
            Err(e) => self.push_log(
                CodexLogKind::Error,
                format!("Codex セッション名の更新に失敗しました: {e}"),
            ),
        }
    }

    fn resolve_session_ref(&mut self, session_ref: &str) -> Option<String> {
        if let Some(index) = parse_one_based_index(session_ref) {
            if self.indexed_sessions.is_empty() {
                self.indexed_sessions = match load_codex_session_candidates(12) {
                    Ok(sessions) => sessions,
                    Err(e) => {
                        self.push_log(
                            CodexLogKind::Error,
                            format!("Codex sessions の読み込みに失敗しました: {e}"),
                        );
                        return None;
                    }
                };
            }
            return match self.indexed_sessions.get(index) {
                Some(session) => Some(session.id.clone()),
                None => {
                    self.push_log(CodexLogKind::Error, "session番号が範囲外です");
                    None
                }
            };
        }
        Some(session_ref.to_string())
    }

    fn handle_local_provider_slash_command(&mut self, args: &str) {
        match args.to_ascii_lowercase().as_str() {
            "" => self.cycle_local_provider(),
            "config" | "clear" | "default" => {
                self.exec_settings.local_provider = CodexLocalProviderChoice::Config;
                self.action = Some("local provider: config".to_string());
                self.push_log(CodexLogKind::System, "次回ターンの local provider: config");
            }
            "lmstudio" | "lm-studio" => {
                self.exec_settings.local_provider = CodexLocalProviderChoice::LmStudio;
                self.action = Some("local provider: lmstudio".to_string());
                self.push_log(
                    CodexLogKind::System,
                    "次回ターンの local provider: lmstudio",
                );
            }
            "ollama" => {
                self.exec_settings.local_provider = CodexLocalProviderChoice::Ollama;
                self.action = Some("local provider: ollama".to_string());
                self.push_log(CodexLogKind::System, "次回ターンの local provider: ollama");
            }
            _ => self.push_log(
                CodexLogKind::Error,
                "local-provider は config / lmstudio / ollama を指定してください",
            ),
        }
    }

    fn handle_color_slash_command(&mut self, args: &str) {
        let value = if args.is_empty() {
            self.exec_settings.cycle_color()
        } else if let Some(choice) = parse_color_choice(args) {
            self.exec_settings.color = choice;
            choice.label()
        } else {
            self.push_log(
                CodexLogKind::Error,
                "color は never / auto / always を指定してください",
            );
            return;
        };
        self.action = Some(format!("color: {value}"));
        self.push_log(CodexLogKind::System, format!("次回ターンの color: {value}"));
    }

    fn handle_permissions_slash_command(&mut self, args: &str) {
        let args = args.trim();
        if args.is_empty() || matches!(args, "show" | "status") {
            self.push_permissions_status();
            return;
        }

        let Some((head, rest)) = split_first_arg(args) else {
            self.push_permissions_status();
            return;
        };
        match head.to_ascii_lowercase().as_str() {
            "approval" | "approvals" | "ask-for-approval" => {
                if let Some(choice) = parse_approval_choice(rest.trim()) {
                    self.set_approval(choice);
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        "permissions approval は config / untrusted / on-request / on-failure / never を指定してください",
                    );
                }
            }
            "sandbox" => {
                if let Some(choice) = parse_sandbox_choice(rest.trim()) {
                    self.set_sandbox(choice);
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        "permissions sandbox は read-only / workspace-write / danger-full-access を指定してください",
                    );
                }
            }
            "bypass" | "dangerously-bypass" => {
                self.toggle_bypass_approvals_and_sandbox();
            }
            value => {
                if let Some(choice) = parse_approval_choice(value) {
                    self.set_approval(choice);
                } else if let Some(choice) = parse_sandbox_choice(value) {
                    self.set_sandbox(choice);
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        "permissions は approval <policy> / sandbox <mode> / bypass を指定してください",
                    );
                }
            }
        }
    }

    fn push_permissions_status(&mut self) {
        self.push_log(
            CodexLogKind::System,
            format!(
                "Permissions: approval={} sandbox={} bypass={}",
                self.exec_settings.approval.label(),
                self.exec_settings.sandbox.label(),
                on_off(self.exec_settings.bypass_approvals_and_sandbox)
            ),
        );
    }

    fn handle_personality_slash_command(&mut self, args: &str) {
        match args.trim().to_ascii_lowercase().as_str() {
            "" | "show" | "status" => {
                let value = self
                    .config_override_value_for("personality")
                    .unwrap_or("config");
                self.push_log(CodexLogKind::System, format!("Personality: {value}"));
            }
            "clear" | "config" | "default" | "none" => {
                self.clear_config_override_key("personality", "personality");
            }
            "friendly" => {
                self.set_config_override_key("personality", toml_string("friendly"), "personality");
            }
            "pragmatic" => {
                self.set_config_override_key(
                    "personality",
                    toml_string("pragmatic"),
                    "personality",
                );
            }
            _ => {
                self.push_log(
                    CodexLogKind::Error,
                    "personality は friendly / pragmatic / clear を指定してください",
                );
            }
        }
    }

    fn handle_statusline_slash_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "show" | "status" => {
                let items = self
                    .config_override_value_for("tui.status_line")
                    .unwrap_or("config");
                let colors = self
                    .config_override_value_for("tui.status_line_use_colors")
                    .unwrap_or("config");
                self.push_log(
                    CodexLogKind::System,
                    format!("Statusline: items={items}, colors={colors}"),
                );
                return;
            }
            "items" | "list" => {
                self.push_log(
                    CodexLogKind::System,
                    "Statusline items: model_name, model_details, directory, thread_name, token_usage, account, agents_summary, collaboration_mode, remote_connection, forked_from",
                );
                return;
            }
            "clear" | "config" | "default" => {
                self.clear_config_override_key("tui.status_line", "statusline");
                self.clear_config_override_key("tui.status_line_use_colors", "statusline colors");
                return;
            }
            _ => {}
        }

        if let Some(rest) = args.strip_prefix("colors ") {
            if let Some(enabled) = parse_bool_choice(rest.trim()) {
                self.set_config_override_key(
                    "tui.status_line_use_colors",
                    enabled.to_string(),
                    "statusline colors",
                );
            } else {
                self.push_log(
                    CodexLogKind::Error,
                    "statusline colors は on / off を指定してください",
                );
            }
            return;
        }

        let items = parse_statusline_items(args);
        if items.is_empty() {
            self.push_log(
                CodexLogKind::Error,
                "statusline には item 名を指定してください。例: /statusline model_name directory thread_name token_usage",
            );
            return;
        }
        self.set_config_override_key("tui.status_line", toml_string_array(&items), "statusline");
    }

    fn handle_theme_slash_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "show" | "status" => {
                let value = self
                    .config_override_value_for("tui.theme")
                    .unwrap_or("config");
                self.push_log(CodexLogKind::System, format!("Theme: {value}"));
            }
            "clear" | "config" | "default" => {
                self.clear_config_override_key("tui.theme", "theme");
            }
            theme => {
                self.set_config_override_key("tui.theme", toml_string(theme), "theme");
            }
        }
    }

    fn handle_pet_slash_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "show" | "status" => {
                let pet = self
                    .config_override_value_for("tui.pet")
                    .unwrap_or("config");
                let anchor = self
                    .config_override_value_for("tui.pet_anchor")
                    .unwrap_or("config");
                self.push_log(
                    CodexLogKind::System,
                    format!("Pet: pet={pet}, anchor={anchor}"),
                );
            }
            "clear" | "config" | "default" => {
                self.clear_config_override_key("tui.pet", "pet");
                self.clear_config_override_key("tui.pet_anchor", "pet anchor");
            }
            "hide" | "off" | "none" => {
                self.set_config_override_key("tui.pet", toml_string("none"), "pet");
            }
            _ if args.starts_with("anchor ") => {
                let anchor = args.trim_start_matches("anchor ").trim();
                if anchor.is_empty() {
                    self.push_log(
                        CodexLogKind::Error,
                        "pets anchor には位置名を指定してください",
                    );
                } else {
                    self.set_config_override_key(
                        "tui.pet_anchor",
                        toml_string(anchor),
                        "pet anchor",
                    );
                }
            }
            pet => self.set_config_override_key("tui.pet", toml_string(pet), "pet"),
        }
    }

    fn handle_vim_slash_command(&mut self, args: &str) {
        let args = args.trim();
        let enabled = if args.is_empty() || args == "toggle" {
            self.config_override_value_for("tui.vim_mode_default") != Some("true")
        } else if matches!(args, "show" | "status") {
            let value = self
                .config_override_value_for("tui.vim_mode_default")
                .unwrap_or("config");
            self.push_log(CodexLogKind::System, format!("Vim mode default: {value}"));
            return;
        } else if matches!(args, "clear" | "config" | "default") {
            self.clear_config_override_key("tui.vim_mode_default", "vim");
            return;
        } else if let Some(enabled) = parse_bool_choice(args) {
            enabled
        } else {
            self.push_log(
                CodexLogKind::Error,
                "vim は on / off / clear を指定してください",
            );
            return;
        };
        self.set_config_override_key("tui.vim_mode_default", enabled.to_string(), "vim");
    }

    fn handle_raw_output_slash_command(&mut self, args: &str) {
        let args = args.trim();
        let enabled = if args.is_empty() || args == "toggle" {
            self.config_override_value_for("tui.raw_output_mode") != Some("true")
        } else if matches!(args, "show" | "status") {
            let value = self
                .config_override_value_for("tui.raw_output_mode")
                .unwrap_or("config");
            self.push_log(CodexLogKind::System, format!("Raw output mode: {value}"));
            return;
        } else if matches!(args, "clear" | "config" | "default") {
            self.clear_config_override_key("tui.raw_output_mode", "raw output");
            return;
        } else if let Some(enabled) = parse_bool_choice(args) {
            enabled
        } else {
            self.push_log(
                CodexLogKind::Error,
                "raw は on / off / clear を指定してください",
            );
            return;
        };
        self.set_config_override_key("tui.raw_output_mode", enabled.to_string(), "raw output");
    }

    fn handle_keymap_slash_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "show" | "status" => {
                let values = self
                    .exec_settings
                    .config_overrides
                    .iter()
                    .filter(|entry| config_override_key(entry).starts_with("tui.keymap"))
                    .cloned()
                    .collect::<Vec<_>>();
                self.push_numbered_settings_list("Keymap overrides", &values);
            }
            "clear" | "config" | "default" => {
                self.clear_config_override_prefix("tui.keymap", "keymap");
            }
            value if value.contains('=') => {
                let (path, raw_value) = value.split_once('=').unwrap_or_default();
                let path = path.trim();
                let key = if path.starts_with("tui.keymap") {
                    path.to_string()
                } else {
                    format!("tui.keymap.{path}")
                };
                self.set_config_override_key(&key, raw_value.trim().to_string(), "keymap");
            }
            _ => self.push_log(
                CodexLogKind::Error,
                "keymap は key=value 形式で指定してください。例: /keymap global.copy=[\"ctrl-y\"]",
            ),
        }
    }

    fn handle_memories_slash_command(&mut self, args: &str) {
        let args = args.trim();
        match args {
            "" | "show" | "status" => {
                let use_memories = self
                    .config_override_value_for("memories.use_memories")
                    .unwrap_or("config");
                let generate = self
                    .config_override_value_for("memories.generate_memories")
                    .unwrap_or("config");
                self.push_log(
                    CodexLogKind::System,
                    format!(
                        "記憶先: Addness DB。通常Codex memory: use={use_memories}, generate={generate}. プロジェクト固有の記憶は /remember で保存してください"
                    ),
                );
            }
            "clear" | "config" | "default" => {
                self.restore_addness_memory_defaults();
            }
            "on" | "enable" => {
                self.reject_global_memory_override();
            }
            "off" | "disable" => {
                self.set_config_override_key(
                    "memories.use_memories",
                    "false".to_string(),
                    "memories",
                );
                self.set_config_override_key(
                    "memories.generate_memories",
                    "false".to_string(),
                    "memories",
                );
            }
            value if value.contains('=') => {
                let (path, raw_value) = value.split_once('=').unwrap_or_default();
                let path = path.trim();
                let key = if path.starts_with("memories") {
                    path.to_string()
                } else {
                    format!("memories.{path}")
                };
                self.set_config_override_key(&key, raw_value.trim().to_string(), "memories");
            }
            _ => self.push_log(
                CodexLogKind::Error,
                "memories は Addness DB 固定です。status / off / clear を指定してください。プロジェクト固有メモは /remember <内容>",
            ),
        }
    }

    fn handle_remember_slash_command(&mut self, args: &str) {
        let note = normalize_submitted_line(args);
        if note.is_empty() {
            self.push_log(
                CodexLogKind::Error,
                "/remember にはAddnessへ残すプロジェクト固有メモを指定してください",
            );
            return;
        }

        let prompt = addness_remember_prompt(&note);
        let display = format!("Addnessに記憶: {}", compact_one_line(&note, 160));
        self.input_state.record_submitted(&display);
        self.push_log(CodexLogKind::User, display.clone());

        if self.is_turn_running() {
            let count = self.queue_direct_prompt(prompt, display);
            self.action = Some(format!("Addness記憶を予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("Addness記憶を次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.start_turn_with_display_prompt(&prompt, &display);
    }

    fn handle_handoff_slash_command(&mut self, args: &str) {
        let note = normalize_submitted_line(args);
        let summary = self.work_summary();
        let prompt = addness_handoff_prompt(note.as_str(), &summary);
        let display = if note.is_empty() {
            "Addnessに引き継ぎ保存".to_string()
        } else {
            format!("Addnessに引き継ぎ保存: {}", compact_one_line(&note, 160))
        };
        self.input_state.record_submitted(&display);
        self.push_log(CodexLogKind::User, display.clone());

        if self.is_turn_running() {
            let count = self.queue_direct_prompt(prompt, display);
            self.action = Some(format!("Addness引き継ぎ保存を予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("Addness引き継ぎ保存を次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.start_turn_with_display_prompt(&prompt, &display);
    }

    fn handle_cwd_slash_command(&mut self, args: &str) {
        if args.is_empty() || matches!(args, "show" | "status") {
            self.push_log(CodexLogKind::System, format!("Cwd: {}", self.cwd));
            return;
        }
        let path = match resolve_cwd_path(&self.cwd, args) {
            Ok(path) => path,
            Err(e) => {
                self.push_log(CodexLogKind::Error, e.to_string());
                return;
            }
        };
        self.cwd = path.display().to_string();
        self.action = Some(format!("cwd: {}", compact_home_path(&path)));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの cwd: {}", self.cwd),
        );
        if self.thread_id.is_some() {
            self.push_log(
                CodexLogKind::System,
                "既存Codexセッションの再開にはcwd変更は反映されません。新規セッションで有効です。",
            );
        }
    }

    fn handle_remote_slash_command(&mut self, args: &str) {
        match args {
            "" | "show" | "status" => {
                let value = self.exec_settings.remote_addr.as_deref().unwrap_or("off");
                self.push_log(CodexLogKind::System, format!("Remote: {value}"));
            }
            "clear" | "off" | "none" => self.set_remote_addr(None),
            addr => self.set_remote_addr(Some(addr.to_string())),
        }
    }

    fn handle_remote_auth_token_env_slash_command(&mut self, args: &str) {
        match args {
            "" | "show" | "status" => {
                let value = self
                    .exec_settings
                    .remote_auth_token_env
                    .as_deref()
                    .unwrap_or("off");
                self.push_log(
                    CodexLogKind::System,
                    format!("Remote auth token env: {value}"),
                );
            }
            "clear" | "off" | "none" => self.set_remote_auth_token_env(None),
            env => self.set_remote_auth_token_env(Some(env.to_string())),
        }
    }

    fn handle_profile_slash_command(&mut self, args: &str) {
        match args {
            "" => {
                let value = self.exec_settings.profile.as_deref().unwrap_or("config");
                self.push_log(CodexLogKind::System, format!("Profile: {value}"));
            }
            "clear" | "config" | "default" => self.set_profile(None),
            profile => self.set_profile(Some(profile.to_string())),
        }
    }

    fn handle_image_slash_command(&mut self, args: &str) {
        if args.is_empty() || args == "list" {
            let values = self.exec_settings.image_paths.clone();
            self.push_numbered_settings_list("Images", &values);
            return;
        }
        if args == "clear" {
            self.clear_image_paths();
            return;
        }
        if let Some(rest) = args.strip_prefix("remove ") {
            match parse_one_based_index(rest) {
                Some(index) => self.remove_image_path(index),
                None => self.push_log(
                    CodexLogKind::Error,
                    "image remove には番号を指定してください",
                ),
            }
            return;
        }
        self.add_image_path(args.to_string());
    }

    fn handle_add_dir_slash_command(&mut self, args: &str) {
        if args.is_empty() || args == "list" {
            let values = self.exec_settings.additional_dirs.clone();
            self.push_numbered_settings_list("Add dirs", &values);
            return;
        }
        if args == "clear" {
            self.clear_writable_dirs();
            return;
        }
        self.add_writable_dir(args.to_string());
    }

    fn handle_attachments_slash_command(&mut self, args: &str) {
        let args = args.trim();
        if args.is_empty() || args == "list" {
            let mut lines = Vec::new();
            lines.push("Images:".to_string());
            lines.extend(numbered_or_none(&self.exec_settings.image_paths));
            lines.push(String::new());
            lines.push("Add dirs:".to_string());
            lines.extend(numbered_or_none(&self.exec_settings.additional_dirs));
            self.push_log(CodexLogKind::System, lines.join("\n"));
            return;
        }
        if args == "clear" {
            self.clear_image_paths();
            self.clear_writable_dirs();
            return;
        }
        if let Some(rest) = args.strip_prefix("image ") {
            self.handle_image_slash_command(rest.trim());
            return;
        }
        if let Some(rest) = args.strip_prefix("add-dir ") {
            self.handle_add_dir_slash_command(rest.trim());
            return;
        }
        self.add_image_path(args.to_string());
    }

    fn handle_sandbox_add_read_dir_slash_command(&mut self, args: &str) {
        let dir = args.trim();
        if dir.is_empty() {
            self.push_log(
                CodexLogKind::Error,
                "sandbox-add-read-dir には absolute directory path を指定してください",
            );
            return;
        }
        self.add_writable_dir(dir.to_string());
        self.push_log(
            CodexLogKind::System,
            "Codex CLIの都合上、このTUIでは --add-dir として次ターンへ渡します",
        );
    }

    fn handle_setup_default_sandbox_slash_command(&mut self) {
        self.exec_settings.sandbox = CodexSandboxChoice::WorkspaceWrite;
        self.exec_settings.approval = CodexApprovalChoice::OnRequest;
        self.exec_settings.bypass_approvals_and_sandbox = false;
        self.action = Some("sandbox preset".to_string());
        self.push_log(
            CodexLogKind::System,
            "Default sandbox preset: sandbox=workspace-write approval=on-request",
        );
    }

    fn handle_config_slash_command(&mut self, args: &str) {
        if args.is_empty() || args == "list" {
            let values = self.exec_settings.config_overrides.clone();
            self.push_numbered_settings_list("Config overrides", &values);
            return;
        }
        if args == "clear" {
            self.clear_config_overrides();
            return;
        }
        if !args.contains('=') {
            self.push_log(
                CodexLogKind::Error,
                "config は key=value 形式で指定してください",
            );
            return;
        }
        self.add_config_override(args.to_string());
    }

    fn handle_feature_slash_command(&mut self, args: &str, enabled: bool) {
        if args.is_empty() {
            self.push_log(
                CodexLogKind::Error,
                if enabled {
                    "enable には feature 名を指定してください"
                } else {
                    "disable には feature 名を指定してください"
                },
            );
            return;
        }
        if enabled {
            self.add_enabled_feature(args.to_string());
        } else {
            self.add_disabled_feature(args.to_string());
        }
    }

    fn handle_output_schema_slash_command(&mut self, args: &str) {
        match args {
            "" | "show" | "status" => {
                let value = self.exec_settings.output_schema.as_deref().unwrap_or("off");
                self.push_log(CodexLogKind::System, format!("Output schema: {value}"));
            }
            "clear" | "off" | "none" => self.set_output_schema(None),
            path => self.set_output_schema(Some(path.to_string())),
        }
    }

    fn handle_output_last_message_slash_command(&mut self, args: &str) {
        match args {
            "" | "show" | "status" => {
                let value = self
                    .exec_settings
                    .output_last_message
                    .as_deref()
                    .unwrap_or("off");
                self.push_log(
                    CodexLogKind::System,
                    format!("Output last message: {value}"),
                );
            }
            "clear" | "off" | "none" => self.set_output_last_message(None),
            path => self.set_output_last_message(Some(path.to_string())),
        }
    }

    fn push_numbered_settings_list(&mut self, title: &str, values: &[String]) {
        if values.is_empty() {
            self.push_log(CodexLogKind::System, format!("{title}: none"));
            return;
        }
        let lines = values
            .iter()
            .enumerate()
            .map(|(idx, value)| format!("{}. {value}", idx + 1))
            .collect::<Vec<_>>()
            .join("\n");
        self.push_log(CodexLogKind::System, format!("{title}:\n{lines}"));
    }

    fn handle_goal_slash_command(&mut self, args: &str) {
        if args.is_empty() {
            self.push_goal_mode_status();
            return;
        }

        let mut parts = args.splitn(2, char::is_whitespace);
        let action = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();
        match action.to_ascii_lowercase().as_str() {
            "pause" => {
                if self.goal_mode.objective.is_none() {
                    self.push_log(CodexLogKind::System, "Goal mode: 目標は未設定です");
                    return;
                }
                self.goal_mode.paused = true;
                self.persist_goal_mode();
                self.action = Some("継続ゴール: 一時停止".to_string());
                self.push_activity("Goal mode を一時停止しました".to_string());
                self.push_log(CodexLogKind::System, "Goal mode を一時停止しました");
            }
            "resume" => {
                if self.goal_mode.objective.is_none() {
                    self.push_log(CodexLogKind::System, "Goal mode: 目標は未設定です");
                    return;
                }
                self.goal_mode.paused = false;
                self.persist_goal_mode();
                self.action = Some("継続ゴール: 有効".to_string());
                self.push_activity("Goal mode を再開しました".to_string());
                self.push_log(CodexLogKind::System, "Goal mode を再開しました");
            }
            "clear" => {
                self.goal_mode = CodexGoalMode::default();
                self.persist_goal_mode();
                self.action = Some("継続ゴール: 解除".to_string());
                self.push_activity("Goal mode を解除しました".to_string());
                self.push_log(CodexLogKind::System, "Goal mode を解除しました");
            }
            "set" if !rest.is_empty() => self.set_goal_mode_objective(rest),
            _ => self.set_goal_mode_objective(args),
        }
    }

    fn set_goal_mode_objective(&mut self, objective: &str) {
        let objective = normalize_submitted_line(objective);
        if objective.is_empty() {
            self.push_log(CodexLogKind::Error, "Goal mode の目標が空です");
            return;
        }
        if objective.chars().count() > 4_000 {
            self.push_log(
                CodexLogKind::Error,
                "Goal mode の目標は 4,000 文字以内にしてください",
            );
            return;
        }

        self.goal_mode = CodexGoalMode {
            objective: Some(objective.clone()),
            paused: false,
        };
        self.persist_goal_mode();
        self.action = Some("継続ゴール: 有効".to_string());
        self.push_activity("Goal mode を設定しました".to_string());
        self.push_log(CodexLogKind::System, format!("Goal mode: {objective}"));
    }

    fn push_goal_mode_status(&mut self) {
        let status = match self.goal_mode.label() {
            Some(label) => format!("Goal mode: {label}"),
            None => "Goal mode: off".to_string(),
        };
        self.push_log(CodexLogKind::System, status);
    }

    fn push_slash_help(&mut self) {
        self.push_log(CodexLogKind::System, self.backend.help_text().to_string());
    }

    fn push_slash_status(&mut self) {
        let goal = self.goal_mode.label().unwrap_or_else(|| "off".to_string());
        let thread = self.thread_id.as_deref().unwrap_or("new");
        self.push_log(
            CodexLogKind::System,
            format!(
                "状態: 継続ゴール={goal}, 設定={}, {}, turn={}, セッション={thread}",
                self.settings_label(),
                self.diff_label(),
                self.turn_count
            ),
        );
    }

    fn push_slash_usage(&mut self) {
        let usage = self
            .last_token_usage_label
            .as_deref()
            .unwrap_or("まだ取得できていません。Codex turn 実行後に表示されます。");
        self.push_log(CodexLogKind::System, format!("トークン: {usage}"));
    }

    fn push_debug_config(&mut self) {
        let codex_home = codex_home_dir()
            .as_deref()
            .map(compact_home_path)
            .unwrap_or_else(|| "unknown".to_string());
        let thread = self.thread_id.as_deref().unwrap_or("new");
        let history = self
            .history_path_label()
            .unwrap_or_else(|| "メモリのみ".to_string());
        let config = if self.exec_settings.config_overrides.is_empty() {
            "none".to_string()
        } else {
            self.exec_settings.config_overrides.join("\n  - ")
        };
        self.push_log(
            CodexLogKind::System,
            format!(
                "設定詳細:\ncodex_bin={}\n作業フォルダ={}\ncodex_home={codex_home}\nセッション={thread}\nturn={}\n設定={}\n履歴={history}\nconfig_overrides:\n  - {config}",
                compact_home_path(&self.codex_bin),
                self.cwd,
                self.turn_count,
                self.settings_label(),
            ),
        );
    }

    fn push_feedback_draft(&mut self, message: &str) {
        let thread = self.thread_id.as_deref().unwrap_or("new");
        let recent_errors = self
            .log
            .iter()
            .rev()
            .filter(|line| line.kind == CodexLogKind::Error)
            .take(3)
            .map(|line| format!("- {}", compact_tool_text(&line.text)))
            .collect::<Vec<_>>();
        let errors = if recent_errors.is_empty() {
            "none".to_string()
        } else {
            recent_errors.join("\n")
        };
        let user_message = if message.trim().is_empty() {
            "none".to_string()
        } else {
            compact_tool_text(message)
        };
        self.push_log(
            CodexLogKind::System,
            format!(
                "フィードバック下書き:\nユーザー入力={user_message}\n作業フォルダ={}\nセッション={thread}\nturn={}\n設定={}\n直近の失敗:\n{errors}",
                self.cwd,
                self.turn_count,
                self.settings_label()
            ),
        );
    }

    fn push_rollout_status(&mut self) {
        let thread = self.thread_id.as_deref().unwrap_or("new");
        let history = self
            .history_path_label()
            .unwrap_or_else(|| "メモリのみ".to_string());
        self.push_log(
            CodexLogKind::System,
            format!(
                "セッション保存: セッション={thread}, ローカル履歴={history}, 保存件数={}, 読込件数={}",
                self.session_record_count, self.loaded_history_count
            ),
        );
    }

    fn push_process_status(&mut self) {
        let current = self
            .current_command
            .as_deref()
            .or(self.child_process_label.as_deref())
            .unwrap_or("none");
        self.push_log(
            CodexLogKind::System,
            format!(
                "実行状況: 実行中={}, 現在={current}, 待機turn={}",
                on_off(self.is_turn_running()),
                self.queued_prompts.len()
            ),
        );
    }

    fn push_last_assistant_markdown(&mut self) {
        let Some(text) = self
            .log
            .iter()
            .rev()
            .find(|line| line.kind == CodexLogKind::Assistant && !line.text.trim().is_empty())
            .map(|line| compact_tool_text(&line.text))
        else {
            self.push_log(CodexLogKind::Error, "コピー対象のassistant応答がありません");
            return;
        };
        self.push_log(
            CodexLogKind::System,
            format!("直近の返答(markdown):\n{text}"),
        );
    }

    fn push_slash_history(&mut self) {
        let thread = self.thread_id.as_deref().unwrap_or("new");
        let path = self
            .history_path_label()
            .unwrap_or_else(|| "メモリのみ".to_string());
        self.push_log(
            CodexLogKind::System,
            format!(
                "履歴: {}, 読込={}件, セッション={thread}, 保存先={path}",
                self.history_label(),
                self.loaded_history_count
            ),
        );
    }

    fn handle_turn_slash_command(&mut self, args: &str) {
        let args = args.trim();
        if args.is_empty() || matches!(args, "status" | "show" | "list") {
            self.push_turn_status();
            return;
        }
        match args {
            "picker" | "panel" | "menu" | "list-open" => {
                self.open_turn_picker();
                return;
            }
            "all" | "open-all" | "expand-all" => {
                self.open_all_turns();
                self.push_log(CodexLogKind::System, "すべてのturnを展開しました");
                return;
            }
            "old" | "collapse-old" | "collapse-all" => {
                self.collapse_completed_turns();
                self.invalidate_rendered_history_metrics();
                self.scrollback = self.scrollback.min(self.max_view_scrollback());
                self.push_log(CodexLogKind::System, "完了済みturnを格納しました");
                return;
            }
            _ => {}
        }

        let Some((head, rest)) = split_first_arg(args) else {
            self.push_turn_status();
            return;
        };
        let (action, value) = match head.to_ascii_lowercase().as_str() {
            "open" | "expand" | "show" => ("open", rest.trim()),
            "close" | "collapse" | "hide" => ("close", rest.trim()),
            "toggle" | "t" => ("toggle", rest.trim()),
            _ => ("open", args),
        };
        let Some(turn) = parse_one_based_index(value).map(|idx| idx + 1) else {
            self.push_log(
                CodexLogKind::Error,
                "turn は番号、all、old、open N、close N、toggle N を指定してください",
            );
            return;
        };
        let changed = match action {
            "open" => self.open_turn_by_number(turn),
            "close" => self.close_turn_by_number(turn),
            "toggle" => self.toggle_turn_collapsed_by_number(turn),
            _ => false,
        };
        if changed {
            let label = match action {
                "open" => "展開",
                "close" => "格納",
                "toggle" => "開閉",
                _ => "操作",
            };
            self.push_log(
                CodexLogKind::System,
                format!("Turn {turn} を{label}しました"),
            );
        } else {
            self.push_log(CodexLogKind::Error, format!("Turn {turn} は開閉できません"));
        }
    }

    fn push_turn_status(&mut self) {
        let collapsed = if self.collapsed_turns.is_empty() {
            "none".to_string()
        } else {
            self.collapsed_turns
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(",")
        };
        self.push_log(
            CodexLogKind::System,
            format!(
                "turn一覧: 合計={}, 格納={collapsed}. /turn <N> で展開、/turn close <N> で格納、/turn all で全展開",
                self.turn_count
            ),
        );
    }

    fn persist_goal_mode(&mut self) {
        self.persist_session_record(CodexSessionRecord::GoalMode {
            objective: self.goal_mode.objective.clone(),
            paused: self.goal_mode.paused,
        });
    }

    fn prompt_with_goal_mode(&self, submitted: &str) -> String {
        let Some(objective) = self.goal_mode.objective.as_deref() else {
            return submitted.to_string();
        };
        if !self.goal_mode.is_active() {
            return submitted.to_string();
        }
        format!(
            r#"<user_request>
{submitted}
</user_request>

<persistent_goal_context>
Codex Goal mode is active.
Persistent goal:
{objective}

Keep working toward the persistent goal across turns. If the user_request conflicts with the goal, explain the conflict before proceeding.
</persistent_goal_context>"#
        )
    }

    fn prompt_with_addness_context(&self, prompt: &str) -> String {
        let branch = git_branch_label(Path::new(&self.cwd));
        if self.thread_id.is_some() {
            self.compact_addness_context_prompt(prompt, &branch)
        } else {
            self.full_addness_context_prompt(prompt, &branch)
        }
    }

    fn addness_context_parent(&self) -> String {
        if self.parent_goal_id == self.goal_id {
            "none".to_string()
        } else {
            format!(
                "{} ({})",
                compact_one_line(&self.parent_goal_title, 160),
                self.parent_goal_id
            )
        }
    }

    fn addness_context_dod(&self, max_chars: usize) -> String {
        if self.dod.trim().is_empty() {
            "未設定".to_string()
        } else {
            compact_one_line(&self.dod, max_chars)
        }
    }

    fn addness_context_goal_mode(&self) -> String {
        self.goal_mode
            .label()
            .map(|label| compact_one_line(&label, 240))
            .unwrap_or_else(|| "off".to_string())
    }

    fn full_addness_context_prompt(&self, prompt: &str, branch: &str) -> String {
        let parent = self.addness_context_parent();
        let dod = self.addness_context_dod(900);
        let goal_mode = self.addness_context_goal_mode();
        let user_request = user_request_prompt_block(prompt);
        let body_excerpt = self
            .addness_body_excerpt
            .as_deref()
            .unwrap_or("未取得または空");
        let child_goals = addness_child_goal_context(&self.children);
        let organization_hint = addness_organization_hint(prompt);

        format!(
            r#"{user_request}

<addness_tui_context role="supporting_project_memory">
Use this Addness snapshot as project-specific memory/source of truth. It prevents cross-project memory contamination, but it must not replace the user request above. Work like normal Codex: inspect the repo, implement or investigate, and verify.

Current Addness goal:
- id: {goal_id}
- title: {goal_title}
- status: {status}
- parent: {parent}
- cwd: {cwd}
- branch: {branch}
- DoD: {dod}
- goal mode: {goal_mode}

Known child goals from TUI snapshot:
{child_goals}

Body excerpt from TUI snapshot:
{body_excerpt}

Operating rule:
1. Make concrete progress on the user request; do not replace implementation with memory bookkeeping.
2. Treat this TUI snapshot as the first Addness recall. If it is enough for the current request, start from it immediately and inspect the repository like normal Codex. Read Addness via CLI only when more precise body/comments/deliverables/history could change the implementation decision.
3. For implementation or investigation requests, make a reasonable assumption from repo evidence and proceed unless the missing detail would make the result unsafe or likely wrong.
4. The TUI automatically records current branch/folder, turn completion, and session progress into `## Codex自動メモ(機械)`. Do not manually update Addness just to mirror routine progress.
5. Manually update Addness only when it improves future work: durable decisions, non-obvious constraints, DoD changes, useful child-goal decomposition, deliverables, or explicit handoff/memory requests.
6. Do not put this project's durable facts into Codex global memory; use Addness body/DoD/child goals/deliverables instead.
{organization_hint}
</addness_tui_context>

<execution_contract>
Act on the user_request first. Use Addness only as supporting memory and as the durable place for project-specific state.
</execution_contract>"#,
            goal_id = self.goal_id,
            goal_title = compact_one_line(&self.goal_title, 180),
            status = compact_one_line(&self.status_label, 80),
            cwd = self.cwd,
            branch = compact_one_line(branch, 160),
        )
    }

    fn compact_addness_context_prompt(&self, prompt: &str, branch: &str) -> String {
        let user_request = user_request_prompt_block(prompt);
        let organization_hint = addness_organization_hint(prompt);
        format!(
            r#"{user_request}

<addness_tui_context mode="compact" role="supporting_project_memory">
The full Addness snapshot was already provided earlier in this Codex thread. The user request above is primary; continue like normal Codex.

Current Addness goal:
- id: {goal_id}
- title: {goal_title}
- status: {status}
- cwd: {cwd}
- branch: {branch}
- DoD: {dod}
- goal mode: {goal_mode}

Rules:
1. Act on the user request first; do not spend the turn re-summarizing Addness.
2. Read Addness via CLI only if missing details could change the implementation decision.
3. For implementation or investigation requests, make a reasonable assumption from repo evidence and proceed unless the missing detail would make the result unsafe or likely wrong.
4. The TUI automatically records current branch/folder, turn completion, and session progress into `## Codex自動メモ(機械)`. Do not manually update Addness just to mirror routine progress.
5. Manually update Addness only for durable decisions, non-obvious constraints, DoD changes, useful child-goal decomposition, deliverables, or explicit handoff/memory requests.
6. Do not use Codex global memory for project-specific facts.
{organization_hint}
</addness_tui_context>

<execution_contract>
Act on the user_request first. Use Addness only as supporting memory and as the durable place for project-specific state.
</execution_contract>"#,
            goal_id = self.goal_id,
            goal_title = compact_one_line(&self.goal_title, 180),
            status = compact_one_line(&self.status_label, 80),
            cwd = self.cwd,
            branch = compact_one_line(branch, 160),
            dod = self.addness_context_dod(360),
            goal_mode = self.addness_context_goal_mode(),
        )
    }

    fn start_next_queued_turn_if_idle(&mut self) -> bool {
        if self.finished || self.is_turn_running() {
            return false;
        }
        let Some(queued) = self.queued_prompts.pop_front() else {
            return false;
        };
        let remaining = self.queued_prompts.len();
        if let Some(active) = queued.active_work.clone() {
            self.active_work_package = Some(active.clone());
            self.action = if remaining > 0 {
                Some(format!(
                    "子ゴール着手 #{}: {} 残り{}件",
                    active.ordinal,
                    compact_one_line(&active.title, 80),
                    remaining
                ))
            } else {
                Some(format!(
                    "子ゴール着手 #{}: {}",
                    active.ordinal,
                    compact_one_line(&active.title, 80)
                ))
            };
        } else if remaining > 0 {
            self.action = Some(format!("予約入力を実行中 残り{remaining}件"))
        } else {
            self.action = Some("予約入力を実行中".to_string())
        }
        self.push_log(CodexLogKind::System, "予約した入力を実行します");
        if queued.apply_goal_mode {
            self.run_submitted_line_with_display(queued.submitted, queued.display_prompt);
        } else {
            self.start_turn_with_display_prompt(&queued.submitted, &queued.display_prompt);
        }
        true
    }

    /// Addness ゴール文脈を子プロセスへ環境変数として注入する。
    /// ターン実行・サブコマンド実行の双方で共通の 11 変数を 1 箇所に集約する。
    /// `ADDNESS_TUI_CODEX` は現状 backend 固有の名前だが、挙動不変のため維持する。
    fn inject_addness_env(&self, cmd: &mut Command) {
        cmd.env("ADDNESS_TUI_CODEX", "1");
        cmd.env("ADDNESS_GOAL_ID", &self.goal_id);
        cmd.env("ADDNESS_GOAL_TITLE", &self.goal_title);
        cmd.env("ADDNESS_GOAL_STATUS", &self.status_label);
        cmd.env("ADDNESS_GOAL_DOD", &self.dod);
        cmd.env("ADDNESS_TASK_GOAL_ID", &self.goal_id);
        cmd.env("ADDNESS_TASK_GOAL_TITLE", &self.goal_title);
        cmd.env("ADDNESS_PARENT_GOAL_ID", &self.parent_goal_id);
        cmd.env("ADDNESS_PARENT_GOAL_TITLE", &self.parent_goal_title);
        cmd.env(
            "ADDNESS_WORKTREE_BRANCH",
            git_branch_label(Path::new(&self.cwd)),
        );
        cmd.env("ADDNESS_BIN", &self.addness_bin);
    }

    fn spawn_exec_process(&self, prompt: &str) -> Result<Child> {
        let mut cmd = Command::new(&self.codex_bin);
        let exec_settings = self.exec_settings_for_spawn();
        for arg in self
            .backend
            .turn_args(self.thread_id.as_deref(), &self.cwd, &exec_settings)
        {
            cmd.arg(arg);
        }

        cmd.current_dir(&self.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        self.inject_addness_env(&mut cmd);

        let mut child = cmd.spawn().context("Codex の起動に失敗しました")?;

        // Stdin 方式の backend はプロンプトを stdin へ書き込む（引数末尾の `-` と対）。
        if matches!(self.backend.prompt_delivery(), PromptDelivery::Stdin)
            && let Some(mut stdin) = child.stdin.take()
            && let Err(e) = stdin.write_all(prompt.as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e).context("Codex へのプロンプト送信に失敗しました");
        }

        let stdout = child
            .stdout
            .take()
            .context("Codex stdout の取得に失敗しました")?;
        let stderr = child
            .stderr
            .take()
            .context("Codex stderr の取得に失敗しました")?;

        spawn_line_reader(stdout, self.tx.clone(), false);
        spawn_line_reader(stderr, self.tx.clone(), true);

        Ok(child)
    }

    fn exec_settings_for_spawn(&self) -> CodexExecSettings {
        let mut settings = self.exec_settings.clone();
        if let Some(approval) = self.one_shot_approval {
            settings.approval = approval;
            settings.bypass_approvals_and_sandbox = false;
        }
        settings
    }

    fn spawn_codex_subcommand_process(&self, args: &[String]) -> Result<Child> {
        let mut cmd = Command::new(&self.codex_bin);
        for arg in args {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.cwd);
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        self.inject_addness_env(&mut cmd);

        let mut child = cmd
            .spawn()
            .context("codex サブコマンドの起動に失敗しました")?;
        let stdout = child
            .stdout
            .take()
            .context("codex サブコマンド stdout の取得に失敗しました")?;
        let stderr = child
            .stderr
            .take()
            .context("codex サブコマンド stderr の取得に失敗しました")?;

        spawn_line_reader(stdout, self.tx.clone(), false);
        spawn_line_reader(stderr, self.tx.clone(), true);

        Ok(child)
    }

    fn kill_current_turn(&mut self) {
        let label = self.child_process_label.take();
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.pending_decision = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        if let Some(label) = label {
            self.flush_child_process_output(false, &label);
            self.push_log(CodexLogKind::System, format!("{label} を中断しました"));
        } else {
            self.push_log(CodexLogKind::System, "Codex ターンを中断しました");
        }
    }

    /// ユーザーが codex に最後に送信した入力行。
    pub fn last_prompt(&self) -> Option<&str> {
        self.input_state.last_prompt.as_deref()
    }

    /// ユーザーが codex 内で `/exit` を送信し、その結果プロセスも終了しているか。
    pub fn should_close_after_exit_command(&self) -> bool {
        self.finished && self.input_state.exit_command_sent && !self.turn_running
    }

    /// 最初の実依頼を一度だけ body へ自動記録する。記録済み（or 失敗済み）なら None。
    /// 失敗しても再試行はせず、終了/中断時の記録に委ねる（best-effort）。
    pub fn prompt_needs_body_record(&self) -> Option<&str> {
        if self.body_record_done {
            return None;
        }
        self.current_turn_prompt
            .as_deref()
            .or_else(|| self.last_prompt())
    }

    /// 最初の実依頼の自動記録を済み（再試行しない）として扱う。
    pub fn mark_body_recorded_prompt(&mut self) {
        self.body_record_done = true;
    }

    /// codex プロセスを終了させる（ペインを閉じる時に呼ぶ）。
    /// kill 後に wait してゾンビプロセス化を防ぐ。
    pub fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.pending_decision = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.queued_prompts.clear();
    }

    /// DoD を更新する。内容が変わった場合のみ項目・判定を作り直し `true` を返す。
    pub fn set_dod(&mut self, dod: String) -> bool {
        if dod == self.dod {
            return false;
        }
        self.dod = dod;
        self.dod_items = split_dod_items(&self.dod);
        self.dod_checks = vec![None; self.dod_items.len()];
        true
    }

    /// DoD 自動判定の結果（項目インデックス → 達成可否）を反映する。
    pub fn apply_dod_results(&mut self, results: &[(usize, bool)]) {
        for &(i, met) in results {
            if let Some(slot) = self.dod_checks.get_mut(i) {
                *slot = Some(met);
            }
        }
    }
}

fn slash_help_text() -> &'static str {
    r#"Slash commands:
Codex CLI commands:
  /codex <args> - arbitrary codex subcommand
  /codex-help [command] - codex help
  /codex-version|/version - codex --version
  /new - start the next prompt in a new Codex session
  /clear - clear the visible Codex log
  /init [notes] - create or update AGENTS.md for future Codex sessions
  /ide - show IDE context availability in this TUI
  /exec|/e <prompt> - send directly to Codex without Goal mode wrapping
  /interactive [prompt] - root codex [PROMPT] with --no-alt-screen
  /review <args> - codex review
  /exec-review <args> - run Codex review in direct mode
  /apply|/a <task_id> - codex apply
  /import [status|run], /hooks [key=value|clear], /skills [list|name]
  /doctor, /features|/experimental, /mcp, /apps, /plugin, /cloud, /login, /logout
  /update|/update-codex, /app, /app-server, /remote-control, /debug, /completion
  /mcp-server, /exec-server, /sandbox-run <args>
Codex sessions:
  /sessions [N] - list local Codex sessions
  /codex-resume <args> - root codex resume; /resume is a TUI helper
  /resume-last [prompt], /resume-session <N|id> [prompt] - resume a Codex session directly
  /resume-last-all, /resume-session-all - include sessions outside the current cwd
  /resume-interactive-last, /resume-interactive-session <N|id> - root codex resume
  /side [prompt] - fork the current Codex session without replacing it
  /fork <args>, /fork-last [prompt], /fork-session <N|id> [prompt]
  /rename <title> - rename the current Codex session
  /archive <N|id>, /unarchive <N|id>, /delete <N|uuid>
Codex options for next turn:
  /settings, /cd <dir>, /model [name|config], /reasoning [effort]
  /permissions [approval <policy>|sandbox <mode>|bypass]
  /personality [friendly|pragmatic|clear], /statusline [items|colors on|off|clear]
  /theme [name|clear], /pets [name|hide|anchor <pos>|clear], /vim [on|off|clear], /raw [on|off|clear]
  /keymap [key=value|clear], /memories [status|off|clear] - Addness DB fixed memory
  /approval|/approvals [policy], /sandbox [mode], /sandbox-add-read-dir <path>, /setup-default-sandbox
  /color [never|auto|always], /search, /oss, /local-provider
  /remote <addr|clear>, /remote-auth-token-env <env|clear>, /no-alt-screen
  /profile <name|clear>, /image <path|list|clear|remove N>, /attachments [list|clear|image <path>|add-dir <path>]
  /add-dir <path|list|clear>
  /config <key=value|list|clear>, /enable <feature>, /disable <feature>
  /strict-config, /ignore-user-config, /ignore-rules, /skip-git-check, /ephemeral
  /bypass, /bypass-hook-trust, /output-schema <path|clear>, /output-last-message <path|clear>
TUI helpers:
  /goal <目標>, /goal pause, /goal resume, /goal clear
  /organize|/team [task] - Addness子ゴールへ分解し、最初の実装単位へ進む
  /work [next|all|N|id|title] - 子ゴールを実装ワークパッケージとして着手/キュー化
  /remember|/memo <内容> - Addness bodyのCodex作業メモへ保存
  /handoff [メモ] - 現在の会話をAddness bodyへ再開用に保存
  /diff, /history, /turn [picker|N|all|old|close N|toggle N], /rollout, /debug-config, /ps, /stop [all], /btw
  /feedback [message], /test-approval [message], /compact [notes], /plan [task]
  /resume, /status, /usage, /help, /exit"#
}

#[derive(Default)]
struct CodexInputState {
    line: String,
    cursor: usize,
    exit_command_sent: bool,
    last_prompt: Option<String>,
}

impl CodexInputState {
    fn record_submitted(&mut self, submitted: &str) {
        if !submitted.is_empty() && submitted != "/exit" {
            self.last_prompt = Some(submitted.to_string());
        }
        if submitted == "/exit" {
            self.exit_command_sent = true;
        }
    }

    fn captures_key(&self, key: KeyEvent) -> bool {
        if key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            return matches!(key.code, KeyCode::Char('c' | 'C' | 'u' | 'U'));
        }

        match key.code {
            KeyCode::Char(_)
            | KeyCode::Backspace
            | KeyCode::Delete
            | KeyCode::Enter
            | KeyCode::Esc
            | KeyCode::Left
            | KeyCode::Right
            | KeyCode::Home
            | KeyCode::End => true,
            KeyCode::Up | KeyCode::Down => self.line.contains('\n'),
            _ => false,
        }
    }

    fn observe_key(&mut self, key: KeyEvent) -> Option<String> {
        if key.modifiers.contains(KeyModifiers::ALT) {
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c' | 'C' | 'u' | 'U') = key.code {
                self.clear();
            }
            return None;
        }

        match key.code {
            KeyCode::Char(c) => self.insert_char(c),
            KeyCode::Backspace => self.delete_before_cursor(),
            KeyCode::Delete => self.delete_at_cursor(),
            KeyCode::Left => self.move_prev_char(),
            KeyCode::Right => self.move_next_char(),
            KeyCode::Up => self.move_vertical(true),
            KeyCode::Down => self.move_vertical(false),
            KeyCode::Home => self.move_line_start(),
            KeyCode::End => self.move_line_end(),
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.insert_char('\n');
                    return None;
                }

                let submitted = normalize_submitted_line(&self.line);
                self.clear();
                return Some(submitted);
            }
            KeyCode::Esc => {
                self.clear();
            }
            _ => {}
        }
        None
    }

    fn insert_text(&mut self, text: &str) {
        let normalized = normalize_input_text(text);
        self.line.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
    }

    fn insert_char(&mut self, ch: char) {
        self.line.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn delete_before_cursor(&mut self) {
        let Some(prev) = prev_char_boundary(&self.line, self.cursor) else {
            return;
        };
        self.line.drain(prev..self.cursor);
        self.cursor = prev;
    }

    fn delete_at_cursor(&mut self) {
        let Some(next) = next_char_boundary(&self.line, self.cursor) else {
            return;
        };
        self.line.drain(self.cursor..next);
    }

    fn move_prev_char(&mut self) {
        if let Some(prev) = prev_char_boundary(&self.line, self.cursor) {
            self.cursor = prev;
        }
    }

    fn move_next_char(&mut self) {
        if let Some(next) = next_char_boundary(&self.line, self.cursor) {
            self.cursor = next;
        }
    }

    fn move_line_start(&mut self) {
        let (start, _) = self.current_line_bounds();
        self.cursor = start;
    }

    fn move_line_end(&mut self) {
        let (_, end) = self.current_line_bounds();
        self.cursor = end;
    }

    fn move_vertical(&mut self, up: bool) {
        let (start, end) = self.current_line_bounds();
        let target_col = input_width(&self.line[start..self.cursor]);

        if up {
            if start == 0 {
                return;
            }
            let previous_end = start - 1;
            let previous_start = self.line[..previous_end]
                .rfind('\n')
                .map_or(0, |idx| idx + 1);
            self.cursor =
                byte_index_for_width(&self.line, previous_start, previous_end, target_col);
        } else {
            if end == self.line.len() {
                return;
            }
            let next_start = end + 1;
            let next_end = self.line[next_start..]
                .find('\n')
                .map_or(self.line.len(), |idx| next_start + idx);
            self.cursor = byte_index_for_width(&self.line, next_start, next_end, target_col);
        }
    }

    fn current_line_bounds(&self) -> (usize, usize) {
        let start = self.line[..self.cursor]
            .rfind('\n')
            .map_or(0, |idx| idx + 1);
        let end = self.line[self.cursor..]
            .find('\n')
            .map_or(self.line.len(), |idx| self.cursor + idx);
        (start, end)
    }

    fn clear(&mut self) {
        self.line.clear();
        self.cursor = 0;
    }
}

fn normalize_input_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn prev_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }
    text[..cursor].char_indices().last().map(|(idx, _)| idx)
}

fn next_char_boundary(text: &str, cursor: usize) -> Option<usize> {
    if cursor >= text.len() {
        return None;
    }
    text[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}

fn input_char_width(ch: char) -> usize {
    if ch == '\t' {
        4
    } else {
        UnicodeWidthChar::width(ch).unwrap_or(0)
    }
}

fn input_width(text: &str) -> usize {
    text.chars().map(input_char_width).sum()
}

fn byte_index_for_width(text: &str, start: usize, end: usize, target_width: usize) -> usize {
    let mut width = 0usize;
    let mut candidate = start;
    for (offset, ch) in text[start..end].char_indices() {
        let idx = start + offset;
        let ch_width = input_char_width(ch);
        if width + ch_width > target_width {
            return idx;
        }
        width += ch_width;
        candidate = idx + ch.len_utf8();
        if width >= target_width {
            return candidate;
        }
    }
    candidate
}

fn spawn_line_reader<R>(reader: R, tx: Sender<AgentProcessEvent>, stderr: bool)
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines() {
            let Ok(line) = line else {
                break;
            };
            let event = if stderr {
                AgentProcessEvent::Stderr(line)
            } else {
                AgentProcessEvent::Stdout(line)
            };
            if tx.send(event).is_err() {
                break;
            }
        }
    });
}

fn normalize_submitted_line(line: &str) -> String {
    normalize_input_text(line).trim().to_string()
}

enum AddnessActionKind {
    Read,
    Write,
}

fn addness_command_rest(text: &str) -> Option<&str> {
    let lower = text.to_ascii_lowercase();
    if let Some(idx) = lower.find("addness ") {
        return Some(text[idx + "addness ".len()..].trim());
    }

    for marker in [
        "\"$ADDNESS_BIN\" ",
        "'$ADDNESS_BIN' ",
        "$ADDNESS_BIN ",
        "${ADDNESS_BIN} ",
    ] {
        let marker_lower = marker.to_ascii_lowercase();
        if let Some(idx) = lower.find(&marker_lower) {
            return Some(text[idx + marker.len()..].trim());
        }
    }

    None
}

fn rest_has_flag(rest: &str, flag: &str) -> bool {
    let with_eq = format!("{flag}=");
    rest.split_whitespace()
        .any(|part| part == flag || part.starts_with(&with_eq))
}

fn addness_activity_summary(command: &str, output: Option<&str>) -> Option<String> {
    let rest = addness_command_rest(command).unwrap_or(command);
    let mut parts = rest.split_whitespace();
    let first = parts.next().unwrap_or("");
    let second = parts.next().unwrap_or("");
    let json_summary = output.and_then(addness_json_output_summary);

    let kind = match (first, second) {
        ("goal", "update") if rest_has_flag(rest, "--body") => "body変更",
        ("goal", "update") if rest_has_flag(rest, "--description") => "DoD変更",
        ("goal", "update") if rest_has_flag(rest, "--status") => "ステータス変更",
        ("goal", "update") if rest_has_flag(rest, "--title") => "タイトル変更",
        ("goal", "create") => "子ゴール追加",
        ("comment", "create") => "コメント追加",
        ("link", "progress") => "進捗リンク追加",
        ("link", "pr") => "PRリンク追加",
        ("deliverable", _) => "成果物変更",
        ("goal", "get" | "list" | "children" | "tree" | "search" | "siblings") => "ゴール読込",
        ("comment", _) => "コメント読込",
        ("status" | "summary", _) => "状態読込",
        _ => {
            if looks_like_addness_command_text(command) {
                "Addness操作"
            } else {
                return None;
            }
        }
    };

    let detail = json_summary
        .map(|summary| format!(": {summary}"))
        .unwrap_or_default();
    Some(format!("Addness {kind}{detail}"))
}

fn looks_like_addness_command_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("addness")
        || lower.contains("$addness_bin")
        || lower.contains(" goal ")
        || lower.contains(" comment ")
        || lower.contains(" deliverable ")
        || lower.contains(" link ")
}

fn addness_json_output_summary(output: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(output.trim()).ok()?;
    if let Some(title) = addness_json_title(&value) {
        return Some(compact_tool_text(title));
    }
    match value {
        Value::Array(items) => Some(format!("{}件", items.len())),
        Value::Object(map) => Some(format!("{}キー", map.len())),
        _ => Some("JSON".to_string()),
    }
}

fn addness_json_title(value: &Value) -> Option<&str> {
    value
        .get("title")
        .and_then(Value::as_str)
        .or_else(|| value.get("name").and_then(Value::as_str))
        .or_else(|| {
            value
                .get("goal")
                .and_then(|goal| goal.get("title"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            value
                .get("data")
                .and_then(|data| data.get("title"))
                .and_then(Value::as_str)
        })
}

fn rest_has_any_flag(rest: &str, flags: &[&str]) -> bool {
    flags.iter().any(|flag| rest_has_flag(rest, flag))
}

fn rest_flag_value(rest: &str, flag: &str) -> Option<String> {
    let with_eq = format!("{flag}=");
    let mut parts = rest.split_whitespace();
    while let Some(part) = parts.next() {
        if part == flag {
            return parts.next().map(trim_flag_value);
        }
        if let Some(value) = part.strip_prefix(&with_eq) {
            return Some(trim_flag_value(value));
        }
    }
    None
}

fn trim_flag_value(value: &str) -> String {
    value.trim_matches('"').trim_matches('\'').to_string()
}

/// `addness <サブコマンド…>` 文字列を「いま何をしているか」の表示ラベルへ変換する。
fn action_label(rest: &str) -> (String, AddnessActionKind) {
    let mut it = rest.split_whitespace();
    let a = it.next().unwrap_or("");
    let b = it.next().unwrap_or("");
    match (a, b) {
        ("goal", "create") => ("子ゴールを書込中".to_string(), AddnessActionKind::Write),
        ("goal", "update") if rest_has_any_flag(rest, &["--body", "--body-file"]) => {
            ("現状(body)を書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_any_flag(rest, &["--description", "--description-file"]) => {
            ("方針(DoD)を書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_flag(rest, "--status") => {
            ("ステータスを書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_flag(rest, "--title") => {
            ("タイトルを書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") if rest_has_any_flag(rest, &["--due-date", "--clear-due-date"]) => {
            ("期限を書込中".to_string(), AddnessActionKind::Write)
        }
        ("goal", "update") => ("ゴールを書込中".to_string(), AddnessActionKind::Write),
        ("goal", "get" | "list" | "children" | "tree" | "search" | "siblings") => {
            ("ゴール文脈を読込中".to_string(), AddnessActionKind::Read)
        }
        ("comment", "create" | "update" | "delete" | "resolve" | "unresolve" | "react") => {
            ("コメントを書込中".to_string(), AddnessActionKind::Write)
        }
        ("comment", _) => ("コメントを参照中".to_string(), AddnessActionKind::Read),
        ("notification", "send") => {
            let label = match rest_flag_value(rest, "--kind").as_deref() {
                Some("done") => "作業完了通知を送信中",
                Some("review") => "確認依頼通知を送信中",
                Some("blocked") => "ブロック通知を送信中",
                _ => "通知コメントを送信中",
            };
            (label.to_string(), AddnessActionKind::Write)
        }
        ("notification", _) => ("通知を処理中".to_string(), AddnessActionKind::Read),
        ("link", "pr") => ("PRリンクを書込中".to_string(), AddnessActionKind::Write),
        ("link", "progress") => ("進捗コメントを書込中".to_string(), AddnessActionKind::Write),
        ("link" | "deliverable", _) => ("成果物を書込中".to_string(), AddnessActionKind::Write),
        ("today", _) => ("今日のtodoを更新中".to_string(), AddnessActionKind::Write),
        ("status" | "summary", _) => ("状況を確認中".to_string(), AddnessActionKind::Read),
        (cmd, _) if !cmd.is_empty() => (format!("addness {cmd} 実行中"), AddnessActionKind::Read),
        _ => ("addness を実行中".to_string(), AddnessActionKind::Read),
    }
}

/// DoD テキストを行単位の項目リストへ分割する（空行は除外）。
fn split_dod_items(dod: &str) -> Vec<String> {
    dod.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn codex_session_log_path(goal_id: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join(".addness")
            .join(CODEX_SESSION_HISTORY_DIR)
            .join(format!("{}.jsonl", safe_path_component(goal_id)))
    })
}

fn safe_path_component(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            out.push(ch);
        } else {
            out.push('_');
        }
        if out.len() >= 96 {
            break;
        }
    }
    if out.is_empty() {
        "codex-session".to_string()
    } else {
        out
    }
}

fn compact_home_path(path: &Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}

fn expand_tilde_path(input: &str) -> PathBuf {
    if input == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    if let Some(rest) = input.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(input)
}

fn resolve_cwd_path(current: &str, input: &str) -> Result<PathBuf> {
    let input = input.trim();
    if input.is_empty() {
        anyhow::bail!("cwd にはディレクトリを指定してください");
    }
    let path = expand_tilde_path(input);
    let path = if path.is_absolute() {
        path
    } else {
        Path::new(current).join(path)
    };
    if !path.is_dir() {
        anyhow::bail!("cwd がディレクトリではありません: {}", path.display());
    }
    Ok(path.canonicalize().unwrap_or(path))
}

fn load_codex_session(path: &Path) -> Result<LoadedCodexSession> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LoadedCodexSession {
                log: Vec::new(),
                record_count: 0,
                goal_mode: CodexGoalMode::default(),
            });
        }
        Err(e) => {
            return Err(e).with_context(|| format!("履歴ファイルを開けません: {}", path.display()));
        }
    };

    let mut log = Vec::new();
    let mut goal_mode = CodexGoalMode::default();
    let mut record_count = 0usize;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        record_count = record_count.saturating_add(1);
        let Ok(record) = serde_json::from_str::<CodexSessionRecord>(&line) else {
            continue;
        };
        match record {
            CodexSessionRecord::Log { kind, text } => log.push(CodexLogLine::new(kind, text)),
            CodexSessionRecord::UpdateTurn { turn, text } => {
                update_turn_log_line(&mut log, turn, text);
            }
            CodexSessionRecord::AssistantDelta { text } => {
                if let Some(last) = log.last_mut()
                    && last.kind == CodexLogKind::Assistant
                {
                    last.text.push_str(&text);
                    continue;
                }
                log.push(CodexLogLine::new(CodexLogKind::Assistant, text));
            }
            CodexSessionRecord::GoalMode { objective, paused } => {
                goal_mode = CodexGoalMode { objective, paused };
            }
            CodexSessionRecord::RawEvent { .. } => {}
        }
        if log.len() > CODEX_SESSION_HISTORY_MAX_LOG_LINES {
            let removed = log.len() - CODEX_SESSION_HISTORY_MAX_LOG_LINES;
            log.drain(0..removed);
        }
    }

    Ok(LoadedCodexSession {
        log,
        record_count,
        goal_mode,
    })
}

fn append_codex_session_record(path: &Path, record: &CodexSessionRecord) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("履歴ディレクトリを作成できません: {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("履歴ファイルを開けません: {}", path.display()))?;
    serde_json::to_writer(&mut file, record).context("履歴レコードのJSON化に失敗しました")?;
    writeln!(file).context("履歴レコードの書き込みに失敗しました")?;
    Ok(())
}

fn trim_codex_session_log(path: &Path) -> Result<usize> {
    let file = File::open(path)
        .with_context(|| format!("履歴ファイルを開けません: {}", path.display()))?;
    let lines = BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();

    let mut kept = Vec::new();
    let mut bytes = 0u64;
    for line in lines.iter().rev() {
        let line_bytes = line.len() as u64 + 1;
        if kept.len() >= CODEX_SESSION_HISTORY_MAX_RECORDS {
            break;
        }
        if !kept.is_empty() && bytes.saturating_add(line_bytes) > CODEX_SESSION_HISTORY_MAX_BYTES {
            break;
        }
        kept.push(line.clone());
        bytes = bytes.saturating_add(line_bytes);
    }
    kept.reverse();
    let mut file = File::create(path)
        .with_context(|| format!("履歴ファイルを切り詰められません: {}", path.display()))?;
    for line in &kept {
        writeln!(file, "{line}").context("履歴ファイルの再書き込みに失敗しました")?;
    }
    Ok(kept.len())
}

fn rendered_log_line_height(text: &str, width: usize) -> usize {
    let content_width = width.saturating_sub(CODEX_LOG_PREFIX_WIDTH).max(1);
    let normalized = text.replace('\r', "");
    let mut total = 0usize;
    for segment in normalized.split('\n') {
        let width = UnicodeWidthStr::width(segment);
        total += width.saturating_add(content_width - 1) / content_width;
        if segment.is_empty() {
            total += 1;
        }
    }
    total.max(1)
}

fn contains_case_insensitive(text: &str, lowercase_query: &str) -> bool {
    if lowercase_query.is_empty() {
        return true;
    }
    if lowercase_query.is_ascii() {
        let needle = lowercase_query.as_bytes();
        return text
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle));
    }
    text.to_lowercase().contains(lowercase_query)
}

fn is_routine_codex_system_line(line: &CodexLogLine) -> bool {
    if line.kind != CodexLogKind::System {
        return false;
    }
    matches!(
        line.text.as_str(),
        "Codex セッションを開始しました"
            | "Codex の応答が完了しました"
            | "Codex ターンが完了しました"
            | "Codex入力欄で待機中。入力して Enter で依頼を送信します。"
    )
}

fn compact_turn_prompt(prompt: &str) -> String {
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 64;
    if normalized.chars().count() <= MAX_CHARS {
        return normalized;
    }
    let mut out = normalized.chars().take(MAX_CHARS).collect::<String>();
    out.push_str("...");
    out
}

fn is_exit_prompt(prompt: &str) -> bool {
    matches!(prompt.trim(), "/exit" | "/quit")
}

fn turn_number_from_label(label: &str) -> Option<usize> {
    let rest = label.strip_prefix("Turn ")?;
    let digits = rest
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    digits.parse().ok()
}

fn turn_title_prompt(log: &[CodexLogLine], turn: usize) -> Option<String> {
    log.iter()
        .find(|line| {
            line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn)
        })
        .and_then(|line| {
            line.text
                .split_once(" - ")
                .map(|(_, prompt)| prompt.to_string())
        })
        .filter(|prompt| !prompt.trim().is_empty())
}

fn update_turn_log_line(log: &mut [CodexLogLine], turn: usize, text: String) -> bool {
    let Some(line) = log.iter_mut().find(|line| {
        line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn)
    }) else {
        return false;
    };
    if line.text == text {
        return false;
    }
    line.text = text;
    true
}

fn summarize_turn_title(log: &[CodexLogLine], turn: usize) -> Option<String> {
    let (start, end) = turn_log_bounds(log, turn)?;
    let lines = &log[start + 1..end];
    summarize_turn_assistant_title(lines).or_else(|| summarize_turn_tool_title(lines))
}

fn turn_log_bounds(log: &[CodexLogLine], turn: usize) -> Option<(usize, usize)> {
    let start = log.iter().position(|line| {
        line.kind == CodexLogKind::Turn && turn_number_from_label(&line.text) == Some(turn)
    })?;
    let end = log[start + 1..]
        .iter()
        .position(|line| line.kind == CodexLogKind::Turn)
        .map(|offset| start + 1 + offset)
        .unwrap_or(log.len());
    Some((start, end))
}

fn summarize_turn_assistant_title(lines: &[CodexLogLine]) -> Option<String> {
    lines
        .iter()
        .rev()
        .filter(|line| line.kind == CodexLogKind::Assistant)
        .flat_map(|line| line.text.lines())
        .find_map(clean_assistant_title_line)
}

fn clean_assistant_title_line(line: &str) -> Option<String> {
    let title = line
        .trim()
        .trim_start_matches(['#', '-', '*', '•', ' '])
        .trim();
    if title.is_empty() || title.starts_with("```") {
        return None;
    }
    if matches!(
        title,
        "対応しました"
            | "対応しました。"
            | "実装しました"
            | "実装しました。"
            | "完了しました"
            | "完了しました。"
    ) {
        return None;
    }
    Some(compact_turn_prompt(title))
}

fn summarize_turn_tool_title(lines: &[CodexLogLine]) -> Option<String> {
    lines
        .iter()
        .rev()
        .filter(|line| line.kind == CodexLogKind::Tool)
        .find_map(|line| summarize_turn_tool_line(&line.text))
}

fn summarize_turn_tool_line(text: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("*** update file:")
        || lower.contains("*** add file:")
        || lower.contains("*** delete file:")
        || lower.contains("apply_patch")
        || lower.starts_with("edit ")
    {
        return Some(
            extract_code_edit_path(text)
                .map(|path| format!("コード編集: {path}"))
                .unwrap_or_else(|| "コード編集".to_string()),
        );
    }
    for (needle, label) in [
        ("cargo fmt", "フォーマット確認"),
        ("cargo test", "テスト実行"),
        ("cargo clippy", "clippy確認"),
        ("cargo build", "ビルド確認"),
        ("git diff", "差分確認"),
        ("git status", "差分確認"),
        (" goal get ", "Addnessゴール確認"),
        (" goal update ", "Addnessゴール更新"),
        (" notification send ", "Addness通知送信"),
        (" deliverable ", "成果物更新"),
    ] {
        if lower.contains(needle) {
            return Some(label.to_string());
        }
    }
    let first = text.lines().next()?.trim();
    let first = strip_turn_tool_state(first);
    (!first.is_empty()).then(|| compact_turn_prompt(&compact_tool_text(first)))
}

fn strip_turn_tool_state(text: &str) -> &str {
    for state in ["RUNNING", "EDIT", "DIFF", "FAIL", "OK"] {
        if let Some(rest) = text.strip_prefix(state)
            && rest.chars().next().is_some_and(char::is_whitespace)
        {
            return rest.trim_start();
        }
    }
    text
}

fn extract_code_edit_path(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        [
            "*** Update File: ",
            "*** Add File: ",
            "*** Delete File: ",
            "*** Move to: ",
        ]
        .into_iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix).map(str::to_string))
    })
}

fn summarize_implemented_work(log: &[CodexLogLine]) -> Vec<String> {
    log.iter()
        .rev()
        .filter(|line| line.kind == CodexLogKind::Assistant)
        .flat_map(|line| line.text.lines())
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with("```"))
        .take(3)
        .map(compact_tool_text)
        .collect()
}

fn summarize_checks(log: &[CodexLogLine]) -> Vec<String> {
    let mut out = Vec::new();
    for line in log.iter().filter(|line| line.kind == CodexLogKind::Tool) {
        let lower = line.text.to_ascii_lowercase();
        if !(lower.contains("cargo fmt")
            || lower.contains("cargo clippy")
            || lower.contains("cargo test")
            || lower.contains("cargo build")
            || lower.contains("git diff"))
        {
            continue;
        }
        let summary = line
            .text
            .lines()
            .find(|text| {
                text.contains("test result:")
                    || text.contains("Finished ")
                    || text.contains("files changed")
            })
            .or_else(|| line.text.lines().find(|text| text.contains("exit ")))
            .unwrap_or_else(|| line.text.lines().next().unwrap_or(""));
        let summary = compact_tool_text(summary);
        if !summary.is_empty() && !out.iter().any(|seen| seen == &summary) {
            out.push(summary);
        }
    }
    out
}

fn summarize_remaining_work(log: &[CodexLogLine]) -> Vec<String> {
    let mut out = Vec::new();
    for line in log.iter().rev() {
        if matches!(line.kind, CodexLogKind::Error) {
            out.push(compact_tool_text(&line.text));
        }
        if out.len() >= 3 {
            break;
        }
    }
    if out.is_empty() {
        out.push("未記録".to_string());
    }
    out
}

fn string_at_any(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

fn token_usage_summary(value: &Value) -> Option<String> {
    let payload = value.get("payload").unwrap_or(value);
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| value.get("type").and_then(Value::as_str))
        .unwrap_or_default();
    let method = payload
        .get("method")
        .and_then(Value::as_str)
        .or_else(|| value.get("method").and_then(Value::as_str))
        .unwrap_or_default();
    let looks_like_token_usage = event_type == "token_count"
        || method.eq_ignore_ascii_case("thread/tokenUsage/updated")
        || method.eq_ignore_ascii_case("token_count");
    if !looks_like_token_usage {
        return None;
    }

    let info = payload
        .get("info")
        .or_else(|| value.get("info"))
        .unwrap_or(payload);
    let mut parts = Vec::new();
    if let Some(last) = token_usage_group(info, "last_token_usage") {
        parts.push(format!("last {last}"));
    }
    if let Some(total) = token_usage_group(info, "total_token_usage") {
        parts.push(format!("session {total}"));
    }
    if let Some(context_window) = u64_at_path(info, &["model_context_window"])
        .or_else(|| u64_at_path(payload, &["model_context_window"]))
    {
        parts.push(format!("context={}", format_count(context_window)));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

fn token_usage_group(value: &Value, key: &str) -> Option<String> {
    let usage = value.get(key)?;
    let mut parts = Vec::new();
    for (key, label) in [
        ("total_tokens", "total"),
        ("input_tokens", "input"),
        ("cached_input_tokens", "cached"),
        ("output_tokens", "output"),
        ("reasoning_output_tokens", "reasoning"),
    ] {
        if let Some(count) = u64_at_path(usage, &[key]) {
            parts.push(format!("{label}={}", format_count(count)));
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn u64_at_path(value: &Value, path: &[&str]) -> Option<u64> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_u64()
}

fn format_count(value: u64) -> String {
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (idx, ch) in digits.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn nested_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| {
            error
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| error.as_str())
        })
        .map(str::to_string)
}

fn decision_banner(event_type: &str, text: Option<&str>) -> Option<CodexDecisionBanner> {
    let event_lower = event_type.to_ascii_lowercase();
    let event_requests_decision = [
        "approval",
        "confirm",
        "permission",
        "decision",
        "requires_action",
        "input_requested",
        "user_input",
        "consent",
    ]
    .iter()
    .any(|needle| event_lower.contains(needle));

    let message = text
        .map(compact_tool_text)
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| event_type.to_string());
    let text_lower = message.to_lowercase();
    let kind = if [
        "danger",
        "dangerous",
        "destructive",
        "rm ",
        "delete",
        "reset --hard",
        "force",
        "破壊",
        "危険",
    ]
    .iter()
    .any(|needle| text_lower.contains(needle))
    {
        CodexDecisionKind::Dangerous
    } else if ["permission", "forbidden", "403", "権限", "許可"]
        .iter()
        .any(|needle| text_lower.contains(needle) || event_lower.contains(needle))
    {
        CodexDecisionKind::Permission
    } else if ["approval", "approve", "承認"]
        .iter()
        .any(|needle| text_lower.contains(needle) || event_lower.contains(needle))
    {
        CodexDecisionKind::Approval
    } else {
        CodexDecisionKind::YesNo
    };
    let text_requests_decision = [
        "yes/no",
        "y/n",
        "approve",
        "approval",
        "allow",
        "deny",
        "confirm",
        "permission",
        "proceed",
        "承認",
        "許可",
        "確認",
        "続行",
    ]
    .iter()
    .any(|needle| text_lower.contains(needle));

    (event_requests_decision || text_requests_decision)
        .then_some(CodexDecisionBanner::new(kind, message))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ToolDisplay {
    label: String,
    action_text: Option<String>,
    command_text: Option<String>,
    output_text: Option<String>,
}

fn tool_display(event_type: &str, value: &Value) -> Option<ToolDisplay> {
    if !is_tool_like_event(event_type, value) {
        return None;
    }

    let name = tool_name(value).unwrap_or_else(|| event_type.to_string());
    let command = command_text(value).map(|text| compact_tool_text(&text));
    let output = tool_output_text(value).map(|text| compact_tool_text(&text));
    let exit_code = scalar_field_text(value, &["exit_code", "exitCode"]);

    let primary = command.as_deref().or(output.as_deref())?;

    let is_code_edit = is_code_edit_tool(event_type, &name, command.as_deref(), output.as_deref());
    let state = tool_state_label(
        event_type,
        exit_code.as_deref(),
        command.as_deref(),
        is_code_edit,
    );
    let label = format!("{state} {primary}");

    Some(ToolDisplay {
        label,
        action_text: command.clone(),
        command_text: command,
        output_text: output,
    })
}

fn tool_state_label(
    event_type: &str,
    exit_code: Option<&str>,
    command: Option<&str>,
    is_code_edit: bool,
) -> &'static str {
    let lower_event = event_type.to_ascii_lowercase();
    let lower_command = command.unwrap_or("").to_ascii_lowercase();
    if lower_event.contains("begin") || lower_event.contains("start") {
        return "RUNNING";
    }
    if lower_command.contains("git diff") || lower_command.contains("git status") {
        return "DIFF";
    }
    if let Some(code) = exit_code {
        if code == "0" {
            return if is_code_edit { "EDIT" } else { "OK" };
        }
        return "FAIL";
    }
    if lower_event.contains("failed") || lower_event.contains("error") {
        return "FAIL";
    }
    if is_code_edit {
        return "EDIT";
    }
    if is_tool_completion_event(event_type) {
        return "OK";
    }
    "RUNNING"
}

fn is_code_edit_tool(
    event_type: &str,
    name: &str,
    command: Option<&str>,
    output: Option<&str>,
) -> bool {
    let event_type = event_type.to_ascii_lowercase();
    let name = name.to_ascii_lowercase();
    if name == "apply_patch"
        || name.contains("edit")
        || name.contains("write_file")
        || name.contains("file_write")
        || name.contains("update_file")
    {
        return true;
    }
    [Some(event_type.as_str()), command, output]
        .into_iter()
        .flatten()
        .any(looks_like_code_edit_text)
}

fn looks_like_code_edit_text(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("apply_patch")
        || text.contains("*** Begin Patch")
        || text.contains("*** Update File:")
        || text.contains("*** Add File:")
        || text.contains("*** Delete File:")
}

fn is_tool_completion_event(event_type: &str) -> bool {
    let event_type = event_type.to_ascii_lowercase();
    event_type.contains("end")
        || event_type.contains("completed")
        || event_type.contains("finished")
        || event_type.contains("failed")
        || event_type.contains("result")
}

fn generic_tool_state_label(event_type: &str) -> &'static str {
    let lower = event_type.to_ascii_lowercase();
    if lower.contains("failed") || lower.contains("error") {
        "FAIL"
    } else if is_tool_completion_event(event_type) {
        "OK"
    } else {
        "RUNNING"
    }
}

fn generic_event_label(event_type: &str) -> Option<&'static str> {
    let lower = event_type.to_ascii_lowercase();
    if lower.contains("approval") || lower.contains("confirm") {
        Some("確認待ち")
    } else if lower.contains("failed") || lower.contains("error") {
        Some("Codex エラー")
    } else {
        None
    }
}

fn visible_event_text(event_type: &str, text: &str) -> Option<String> {
    let text = compact_tool_text(text);
    if text.is_empty() || is_internal_event_text(event_type, &text) {
        None
    } else {
        Some(text)
    }
}

fn is_internal_event_text(event_type: &str, text: &str) -> bool {
    let event = event_type.trim().to_ascii_lowercase();
    let text = text.trim().to_ascii_lowercase();
    if text == event {
        return true;
    }
    if text.starts_with("item.") || text.starts_with("response.") || text.starts_with("response_") {
        return true;
    }
    text.contains("item.completed")
        || text.contains("response_item")
        || text.contains("function_call_output")
        || text.contains("event_msg")
}

fn child_process_output_summary(output: &str) -> String {
    let count = output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    if count == 0 {
        "出力なし".to_string()
    } else if count == 1 {
        compact_one_line(output.trim(), 80)
    } else {
        format!("出力{count}行")
    }
}

fn addness_child_goal_context(children: &[ChildGoal]) -> String {
    if children.is_empty() {
        return "- 未取得またはなし".to_string();
    }
    children
        .iter()
        .take(12)
        .map(|child| {
            let dod = child
                .description
                .as_deref()
                .map(str::trim)
                .filter(|dod| !dod.is_empty())
                .map(|dod| format!(" / DoD: {}", compact_one_line(dod, 160)))
                .unwrap_or_default();
            format!(
                "- {} {} [{}]{}",
                child.icon,
                compact_one_line(&child.title, 120),
                compact_one_line(&child.status_label, 40),
                dod
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn child_goal_work_list(children: &[ChildGoal]) -> String {
    if children.is_empty() {
        return "子ゴールはまだ取得されていません。先に /organize で作業分解するか、Addness側で子ゴールを作成してください。".to_string();
    }
    let rows = children
        .iter()
        .enumerate()
        .map(|(idx, child)| {
            let dod = child
                .description
                .as_deref()
                .map(str::trim)
                .filter(|dod| !dod.is_empty())
                .map(|dod| format!("\n     DoD: {}", compact_one_line(dod, 140)))
                .unwrap_or_default();
            format!(
                "  {}. {} {} [{}]{}",
                idx + 1,
                child.icon,
                compact_one_line(&child.title, 120),
                compact_one_line(&child.status_label, 40),
                dod
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "子ゴールを選んで着手できます:\n{rows}\n\n/work next、/work all、または /work <番号|id|タイトル>"
    )
}

fn child_goal_index_for_selector(children: &[ChildGoal], selector: &str) -> Result<usize, String> {
    if children.is_empty() {
        return Err(
            "子ゴールがありません。/organize で分解してから /work next を使ってください"
                .to_string(),
        );
    }
    let selector = selector.trim();
    if selector.eq_ignore_ascii_case("next") {
        return Ok(children
            .iter()
            .position(|child| !child.is_completed)
            .unwrap_or(0));
    }
    if let Ok(n) = selector.parse::<usize>() {
        if (1..=children.len()).contains(&n) {
            return Ok(n - 1);
        }
        return Err(format!(
            "子ゴール番号 {n} は範囲外です。1..{} を指定してください",
            children.len()
        ));
    }
    if let Some((idx, _)) = children
        .iter()
        .enumerate()
        .find(|(_, child)| child.id == selector)
    {
        return Ok(idx);
    }

    let selector_lower = selector.to_ascii_lowercase();
    let id_matches = children
        .iter()
        .enumerate()
        .filter(|(_, child)| child.id.to_ascii_lowercase().starts_with(&selector_lower))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    match id_matches.as_slice() {
        [idx] => return Ok(*idx),
        [] => {}
        _ => {
            return Err(format!(
                "子ゴールID `{selector}` は複数候補に一致します。番号で指定してください"
            ));
        }
    }

    let title_matches = children
        .iter()
        .enumerate()
        .filter(|(_, child)| child.title.to_ascii_lowercase().contains(&selector_lower))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    match title_matches.as_slice() {
        [idx] => Ok(*idx),
        [] => Err(format!(
            "子ゴール `{selector}` が見つかりません。/work で一覧を確認してください"
        )),
        _ => Err(format!(
            "子ゴール `{selector}` は複数候補に一致します。番号で指定してください"
        )),
    }
}

fn addness_child_goal_work_prompt(pane: &CodexPane, ordinal: usize, child: &ChildGoal) -> String {
    let dod = child
        .description
        .as_deref()
        .map(str::trim)
        .filter(|dod| !dod.is_empty())
        .unwrap_or("未設定");
    format!(
        r#"Addness子ゴールを1つの実装ワークパッケージとして扱い、DoDを満たすところまで進めてください。

対象子ゴール:
- 番号: {ordinal}
- id: {child_id}
- title: {child_title}
- status: {child_status}
- DoD: {dod}

親ゴール:
- id: {parent_id}
- title: {parent_title}
- status: {parent_status}

進め方:
1. まずリポジトリを読み、対象ファイル・既存パターン・検証方法を確認する。
2. TUI snapshotで足りなければ、必要な範囲だけ `"$ADDNESS_BIN" goal get {child_id} --json --with-deliverable --with-comment` で追加確認する。
3. DoDが空または実装判断に足りない場合は、完了状態として具体化し、必要なら `"$ADDNESS_BIN" goal update {child_id} --description-file <file> --json` で書き込む。
4. 実装・テスト・lintを進める。独立した小作業がさらに見つかった場合だけAddness子ゴールへ分ける。
5. 完了時は検証結果と残りを短く報告し、長期判断・成果物・引き継ぎがあれば Addness CLI で body/link/子ゴールを更新する。

Addness整理だけで終わらず、このターンで実装または検証に着手してください。"#,
        child_id = child.id,
        child_title = compact_one_line(&child.title, 180),
        child_status = compact_one_line(&child.status_label, 80),
        parent_id = pane.goal_id,
        parent_title = compact_one_line(&pane.goal_title, 180),
        parent_status = compact_one_line(&pane.status_label, 80),
    )
}

fn addness_organization_hint(prompt: &str) -> &'static str {
    let prompt = prompt.trim();
    if prompt.is_empty() || prompt.contains("Addnessを作業DBとして使い、この依頼を組織的に分解")
    {
        return "";
    }
    let lower = prompt.to_ascii_lowercase();
    let strong = [
        "子ゴール",
        "サブエージェント",
        "並列",
        "委任",
        "分解",
        "組織",
        "ワークパッケージ",
        "subagent",
        "sub-agent",
        "delegate",
        "breakdown",
        "organize",
        "team",
    ]
    .iter()
    .any(|signal| prompt.contains(signal) || lower.contains(signal));
    let broad_signal_count = [
        "改善",
        "改良",
        "実装",
        "検証",
        "設計",
        "追加",
        "整理",
        "全部",
        "まとめて",
        "複数",
        "仕組み",
        "内部",
        "ui",
        "tui",
        "coding score",
    ]
    .iter()
    .filter(|signal| prompt.contains(**signal) || lower.contains(**signal))
    .count();
    if !(strong || prompt.chars().count() > 240 || broad_signal_count >= 3) {
        return "";
    }
    r#"

Organization hint:
This request looks broad or multi-part. If repo evidence confirms independent work packages, create/update Addness child goals with clear DoD/body, optionally delegate independent slices to subagents/parallel tools, then implement the highest-priority slice in this turn."#
}

fn compact_multiline_excerpt(text: &str, max_chars: usize) -> String {
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (idx, ch) in normalized.chars().enumerate() {
        if idx == max_chars {
            out.push_str("\n... omitted");
            return out;
        }
        out.push(ch);
    }
    out
}

fn compact_one_line(text: &str, max_chars: usize) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out = String::new();
    for (idx, ch) in one_line.chars().enumerate() {
        if idx == max_chars {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn is_tool_like_event(event_type: &str, value: &Value) -> bool {
    let event_type = event_type.to_ascii_lowercase();
    if event_type.contains("exec")
        || event_type.contains("tool")
        || event_type.contains("function")
        || event_type.contains("mcp")
    {
        return true;
    }
    value_has_tool_hint(value)
}

fn value_has_tool_hint(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(is_tool_like_name)
            {
                return true;
            }
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(is_tool_like_kind)
            {
                return true;
            }
            if map
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(is_tool_like_kind)
            {
                return true;
            }
            if map.contains_key("codex_command")
                || map.contains_key("codex_parsed_cmd")
                || map.contains_key("parsed_cmd")
            {
                return true;
            }
            if map.contains_key("cmd")
                && (map.contains_key("workdir")
                    || map.contains_key("cwd")
                    || map.contains_key("yield_time_ms")
                    || map.contains_key("sandbox_permissions"))
            {
                return true;
            }

            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
                "function",
            ] {
                if let Some(child) = map.get(key)
                    && value_has_tool_hint(child)
                {
                    return true;
                }
            }
            false
        }
        Value::Array(values) => values.iter().any(value_has_tool_hint),
        _ => false,
    }
}

fn is_tool_like_kind(kind: &str) -> bool {
    let kind = kind.to_ascii_lowercase();
    kind.contains("tool")
        || kind.contains("exec")
        || kind.contains("mcp")
        || kind == "function_call"
        || kind == "local_shell_call"
        || kind == "shell_call"
}

fn is_tool_like_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.contains("exec")
        || name.contains("tool")
        || name.contains("mcp")
        || name == "apply_patch"
        || name == "shell_command"
        || name == "run_command"
        || name == "terminal"
        || name == "bash"
}

fn tool_name(value: &Value) -> Option<String> {
    recursive_scalar_field_text(
        value,
        &[
            "name",
            "tool_name",
            "toolName",
            "function_name",
            "functionName",
        ],
    )
}

fn first_text_field(value: &Value) -> Option<String> {
    const TEXT_KEYS: &[&str] = &[
        "message", "text", "content", "delta", "summary", "output", "stdout", "stderr",
    ];

    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(values) => values.iter().find_map(first_text_field),
        Value::Object(map) => {
            for key in TEXT_KEYS {
                if let Some(found) = map.get(*key).and_then(first_text_field)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
            ] {
                if let Some(found) = map.get(key).and_then(first_text_field)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn command_text(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in [
                "command",
                "cmd",
                "codex_command",
                "parsed_cmd",
                "codex_parsed_cmd",
                "interaction_input",
                "argv",
                "args",
                "arguments",
                "input",
            ] {
                if let Some(text) = map.get(key).and_then(command_text)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            for key in [
                "payload",
                "item",
                "call",
                "tool_call",
                "toolCall",
                "data",
                "event",
                "result",
            ] {
                if let Some(text) = map.get(key).and_then(command_text)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            None
        }
        Value::Array(values) => {
            let parts = values
                .iter()
                .filter_map(command_text)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>();
            (!parts.is_empty()).then(|| parts.join(" "))
        }
        Value::String(s) => json_string_value(s)
            .and_then(|value| command_text(&value))
            .or_else(|| Some(s.clone())),
        _ => None,
    }
}

fn tool_output_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Array(values) => values.iter().find_map(tool_output_text),
        Value::Object(map) => {
            for key in [
                "formatted_output",
                "aggregated_output",
                "stdout",
                "stderr",
                "output",
                "delta",
            ] {
                if let Some(found) = map.get(key).and_then(tool_output_text)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
            ] {
                if let Some(found) = map.get(key).and_then(tool_output_text)
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn scalar_field_text(value: &Value, keys: &[&str]) -> Option<String> {
    recursive_scalar_field_text(value, keys).map(|text| compact_tool_text(&text))
}

fn recursive_scalar_field_text(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(text) = map.get(*key).and_then(scalar_value_text)
                    && !text.is_empty()
                {
                    return Some(text);
                }
            }
            for key in [
                "payload",
                "item",
                "event",
                "data",
                "result",
                "call",
                "tool_call",
                "toolCall",
                "function",
                "arguments",
                "params",
                "input",
            ] {
                if let Some(found) = map
                    .get(key)
                    .and_then(|child| recursive_scalar_field_text(child, keys))
                    && !found.is_empty()
                {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(values) => values
            .iter()
            .find_map(|child| recursive_scalar_field_text(child, keys)),
        Value::String(s) => {
            json_string_value(s).and_then(|child| recursive_scalar_field_text(&child, keys))
        }
        _ => None,
    }
}

fn scalar_value_text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn json_string_value(s: &str) -> Option<Value> {
    let trimmed = s.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

fn compact_tool_text(text: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let trimmed = text.trim();
    let mut out = String::new();
    for (idx, ch) in trimmed.chars().enumerate() {
        if idx == MAX_CHARS {
            out.push_str("...");
            return out;
        }
        out.push(ch);
    }
    out
}

fn user_request_prompt_block(prompt: &str) -> String {
    if prompt.trim_start().starts_with("<user_request>") {
        prompt.to_string()
    } else {
        format!("<user_request>\n{prompt}\n</user_request>")
    }
}

pub fn resume_prompt() -> &'static str {
    "Addnessの対象ゴールを読み、前回の続きから再開してください。bodyの `## Codex作業メモ` / `## Codex決定ログ` / `## PR/Release Traceability`、DoD、子ゴール、コメント、成果物を確認し、前回の続き・未完了・次の一手を3行以内で整理したら、そのまま必要なリポジトリ確認・実装・検証へ進んでください。Addnessを読んだだけでターンを終えないでください。通常進捗保存はTUIの `## Codex自動メモ(機械)` に任せ、必要な決定/成果物/子ゴールだけ手動更新してください。"
}

fn addness_organize_prompt(task: &str) -> String {
    let target = if task.trim().is_empty() {
        "現在のユーザー依頼、継続ゴール、Addness body/DoD/子ゴールから次に進めるべき作業"
            .to_string()
    } else {
        task.trim().to_string()
    };
    format!(
        r#"Addnessを作業DBとして使い、この依頼を組織的に分解してから実装へ進んでください。
Addness TUI は誰でも `addness` と打てば起動できる通常の入口です。Codex は Addness CLI で goal body/description/子ゴールを書き込めます。

対象:
{target}

進め方:
1. 既存のTUI snapshot（body/DoD/子ゴール/ブランチ）とリポジトリを確認し、実装判断に必要な不足だけAddness CLIで追加確認する。
2. 2つ以上の独立した作業単位、未完了の引き継ぎ、またはサブエージェントに渡せる単位がある場合だけ、Addness子ゴールを作成または更新する。
3. 子ゴールを作る場合は title=作業名、description=完了状態、body=入力情報・対象ファイル・実装方針・検証方法・次の手 に分ける。作成後に `goal update <CHILD_GOAL_ID> --body-file <file> --json` でbodyを入れる。
4. サブエージェント/並列作業ツールが利用でき、独立性が高い作業だけ委任する。委任できない場合は、子ゴールを作った上でメインエージェントが最優先の子ゴールから実装する。
5. 分解だけで終わらず、最初の実装単位に着手し、可能な範囲で検証する。
6. ユーザーへの最終報告は「作った/使った子ゴール」「実装したこと」「検証」「残り」を短く出す。"#
    )
}

fn addness_remember_prompt(note: &str) -> String {
    format!(
        r#"Addnessの対象ゴールを、このプロジェクト専用の長期作業メモリDBとして更新してください。

記録したい内容:
{note}

手順:
1. `"$ADDNESS_BIN" goal get "$ADDNESS_GOAL_ID" --json --with-deliverable --with-comment` で現在のbody/DoD/子ゴール/成果物を確認する。
2. body の `## Codex作業メモ` に、重複を避けてこの内容を短く統合する。決定事項なら `## Codex決定ログ` にも追記する。
3. 独立した作業単位・未完了の引き継ぎ・サブエージェント委任に向く内容なら、子ゴールを作成または更新して分ける。
4. `## Codex自動メモ(機械)` はTUIの領域なので編集しない。既存bodyを壊さず、長くなる場合は `goal update --body-file` を使う。
5. Codex global memory には保存しない。コード変更は不要。
6. Addnessのどこを更新したかを1-2行で報告する。"#
    )
}

fn addness_handoff_prompt(note: &str, summary: &CodexWorkSummary) -> String {
    let note = if note.trim().is_empty() {
        "- 追加メモなし".to_string()
    } else {
        format!("- {}", note.trim())
    };
    let implemented = handoff_bullets(&summary.implemented);
    let checks = handoff_bullets(&summary.checks);
    let remaining = handoff_bullets(&summary.remaining);
    format!(
        r#"Addnessの対象ゴールへ、このCodexセッションの引き継ぎ点を保存してください。

これはコード変更依頼ではありません。通常Codexの会話圧縮ではなく、Addnessをプロジェクト別DBとして使うための保存作業です。

追加メモ:
{note}

TUIが抽出した直近サマリー:
実装・対応:
{implemented}

検証:
{checks}

未完了・注意:
{remaining}

保存手順:
1. `"$ADDNESS_BIN" goal get "$ADDNESS_GOAL_ID" --json --with-deliverable --with-comment` で現body/DoD/成果物/コメントを読む。
2. body の `## Codex作業メモ` を、作業フォルダ、ブランチ、現在地、実施内容、検証結果、未完了点、次の手が分かる再開用メモへ短く更新する。
3. 重要な決定がある場合だけ `## Codex決定ログ` に `YYYY-MM-DD HH:MM - 決定: ... / 理由: ... / 影響: ...` 形式で追記する。
4. PR/tag/release/CIに関係する情報がある場合だけ `## PR/Release Traceability` を更新する。
5. 独立した未完了作業やサブエージェントに渡す単位がある場合だけ、子ゴールを作成または更新する。機械的に子ゴールを増やさない。
6. `## Codex自動メモ(機械)` はTUIの領域なので編集しない。既存bodyを壊さず、長くなる場合は `goal update --body-file` を使う。
7. Codex global memory には保存しない。逐語ログを溜めず、次回再開に必要な事実だけを残す。

保存後、body/決定ログ/Traceability/子ゴール/成果物のどれを更新したかだけ短く報告してください。
"#
    )
}

fn handoff_bullets(items: &[String]) -> String {
    if items.is_empty() {
        return "- 未抽出".to_string();
    }
    items
        .iter()
        .take(5)
        .map(|item| format!("- {}", compact_one_line(item, 180)))
        .collect::<Vec<_>>()
        .join("\n")
}

fn init_prompt() -> &'static str {
    r#"Initialize this repository for future Codex work.

Inspect the repository structure, existing docs, package manifests, build scripts, test commands, linters, formatters, and any project-specific conventions. Create or update AGENTS.md at the repository root with concise, actionable instructions for future Codex sessions.

Include:
- Build, test, lint, and format commands that are actually available.
- Coding style and architecture conventions visible in the repository.
- Git, PR, or release practices if they are documented.
- Any project-specific safety notes or generated-file rules.

Preserve existing AGENTS.md content that is still accurate. Do not invent commands that are not supported by the repository. After editing, run the relevant formatting or validation command if one is available."#
}

fn addness_tui_developer_instructions() -> &'static str {
    r#"Addness TUIから起動されました。

目的:
通常Codexと同じ速度で調査・実装・検証しながら、プロジェクト固有の長期状態だけをAddnessへ残してください。
Addnessはmemory.mdの代替となるプロジェクト別DBです。通常memoryは複数プロジェクトの状態が混ざりやすいため、
このプロジェクト固有の現在地・判断・決定・次の手はAddnessを真実源として扱います。
Addness TUIは誰でも `addness` と打てば起動できる通常の入口です。CodexはAddness CLIでgoal body/DoD/子ゴールを書き込めます。

起動直後:
- Addness CLI を実行せず、ユーザーの最初の入力を待ってください。
- 軽い挨拶や単純な表示確認には、TUIから渡された軽量コンテキストだけで即応してください。

TUIから渡される軽量コンテキスト:
- ADDNESS_GOAL_ID/TITLE/STATUS/DOD: 作業対象ゴール
- ADDNESS_PARENT_GOAL_ID/TITLE: 起動元の親ゴール（ある場合）
- ADDNESS_WORKTREE_BRANCH: 起動した作業ツリーのgitブランチ

実行ループ:
1. TUIから渡されたbody/DoD/子ゴール/ブランチのsnapshotを最初の想起として扱ってください。
2. snapshotで足りるなら、追加のAddness読込に1ターンを費やさず、通常Codexと同じように進めます。
3. デフォルトの進み方は「TUI snapshotを見る → リポジトリを読む → 実装/調査する → 検証する」です。
4. Addnessを読んだだけで作業完了にしない。読んだ内容を使って、コード変更・検証・具体提案へ進みます。
5. 実装や調査の依頼では、repoから合理的に判断できるなら確認質問やAddness整理を挟まず手を動かします。
6. ユーザーへの返答では、Addness運用の説明より実装・判断・検証結果を先に出します。

追加読込が必要な時:
- body全文、コメント、成果物、過去決定などの不足が実装判断を変え得る場合だけ、Addness CLIで追加想起します。
- body、DoD(description/definitionOfDone)、コメント、成果物、子ゴール、作業フォルダ/ブランチのうち、実装判断に必要な不足分だけを確認する。
- 追加読込したら、読んだ内容を長く説明せず、その情報を使って実装・調査・提案へ戻ります。

Addness DBの置き場所:
- DoD/完了基準: goal description
- 現在地、作業フォルダ、ブランチ、次の手、長期判断: body の `## Codex作業メモ`
- 決定事項: body の `## Codex決定ログ`
- PR/tag/release/CI: deliverable/link と body の `## PR/Release Traceability`
- 独立した作業単位、サブエージェント委任、未完了の引き継ぎ: 子ゴール（title=作業名、description=DoD、body=入力情報/ブランチ/次の手）
- 構造化フィールドに置けない短い質問や補足: コメント

CLI最小操作:
- 読む: `"$ADDNESS_BIN" goal get "$ADDNESS_GOAL_ID" --json --with-deliverable --with-comment`
- body更新: `"$ADDNESS_BIN" goal update "$ADDNESS_GOAL_ID" --body-file <file> --json`
- DoD更新: `"$ADDNESS_BIN" goal update "$ADDNESS_GOAL_ID" --description-file <file> --json`
- 子ゴール作成: `"$ADDNESS_BIN" goal create --title "..." --parent "$ADDNESS_GOAL_ID" --description "..." --json`
- 子ゴールbody: 作成結果のidへ `"$ADDNESS_BIN" goal update <CHILD_GOAL_ID> --body-file <file> --json`
- PR紐づけ: `"$ADDNESS_BIN" link pr --goal "$ADDNESS_GOAL_ID" --url "<PR_URL>" --name "<name>" --json`

書き込みルール:
- 起動しただけでは Addness に書き込まない。
- TUIが作業フォルダ・ブランチ・turn完了・セッション終了サマリを `## Codex自動メモ(機械)` に自動記録します。
  この自動記録で足りる通常の進捗は、Codexが手動でbody更新しなくて構いません。
- メインエージェントは実装・調査・検証を止めない。TUIの自動記録に任せられる進捗保存のために、手を止めてAddness更新ターンへ寄せない。
- 手動でAddnessに書き込むのは、自動メモでは足りない長期的判断・決定・重要制約、DoD変更、成果物/PR、独立した作業単位として残すべき子ゴール、明示的な `/remember`・`/handoff` 相当の内容がある時だけ。
- 手動更新する時は現bodyを読み、手書きメモと `## Codex自動メモ(機械)` を壊さず、自分の専用ブロックだけを更新する。長文は `goal update --body-file` を使う。
- 子ゴールは毎ターン機械的に作らない。作業分解・並列化・サブエージェント化・引き継ぎに役立つ時だけCodex自身が作成/更新する。
- tag/releaseを作成したら deliverable/link に紐づけ、`## PR/Release Traceability` にPR・tag・release URL・CI結果を残す。
- DoDが不十分なら、足りない観点を短く整理してユーザーに確認し、合意後に更新する。

再開時:
body の `## Codex作業メモ`、`## Codex決定ログ`、`## PR/Release Traceability`、DoD、子ゴール、コメント、成果物を必要な範囲で読み、
前回の続き・未完了・次の一手を3行以内で整理してから、そのままリポジトリ確認・実装・検証へ進んでください。

共有:
Addnessを読んだ/更新した場合は、対象(body/DoD/子ゴール/コメント/成果物)だけ短く共有してください。"#
}

enum CodexConfigKey {
    DeveloperInstructions,
    ModelReasoningEffort,
}

impl CodexConfigKey {
    fn as_str(&self) -> &'static str {
        match self {
            CodexConfigKey::DeveloperInstructions => "developer_instructions",
            CodexConfigKey::ModelReasoningEffort => "model_reasoning_effort",
        }
    }
}

fn codex_config_arg(key: CodexConfigKey, value: &str) -> String {
    format!("{}={}", key.as_str(), toml_basic_string(value))
}

fn push_global_exec_settings(
    args: &mut Vec<String>,
    settings: &CodexExecSettings,
    include_sandbox: bool,
) {
    if let Some(remote) = &settings.remote_addr {
        args.push("--remote".to_string());
        args.push(remote.clone());
    }
    if let Some(env) = &settings.remote_auth_token_env {
        args.push("--remote-auth-token-env".to_string());
        args.push(env.clone());
    }
    if settings.no_alt_screen {
        args.push("--no-alt-screen".to_string());
    }
    if settings.bypass_approvals_and_sandbox {
        args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
    } else if let Some(approval) = settings.approval.cli_arg() {
        // `-a` is a global Codex option. `codex exec -a ...` is rejected, so keep it before `exec`.
        args.push("-a".to_string());
        args.push(approval.to_string());
    }
    if settings.bypass_hook_trust {
        args.push("--dangerously-bypass-hook-trust".to_string());
    }
    if settings.strict_config {
        args.push("--strict-config".to_string());
    }

    if include_sandbox
        && !settings.bypass_approvals_and_sandbox
        && settings.sandbox != CodexSandboxChoice::WorkspaceWrite
    {
        args.push("-s".to_string());
        args.push(settings.sandbox.cli_arg().to_string());
    }
    if settings.web_search {
        args.push("--search".to_string());
    }
    if settings.oss {
        args.push("--oss".to_string());
    }
    if let Some(provider) = settings.local_provider.cli_arg() {
        args.push("--local-provider".to_string());
        args.push(provider.to_string());
    }
    if let Some(profile) = &settings.profile {
        args.push("-p".to_string());
        args.push(profile.clone());
    }
    for dir in &settings.additional_dirs {
        args.push("--add-dir".to_string());
        args.push(dir.clone());
    }
    for config in &settings.config_overrides {
        args.push("-c".to_string());
        args.push(config.clone());
    }
    for feature in &settings.enabled_features {
        args.push("--enable".to_string());
        args.push(feature.clone());
    }
    for feature in &settings.disabled_features {
        args.push("--disable".to_string());
        args.push(feature.clone());
    }
}

fn push_optional_exec_settings(args: &mut Vec<String>, settings: &CodexExecSettings) {
    if settings.ignore_user_config {
        args.push("--ignore-user-config".to_string());
    }
    if settings.ignore_rules {
        args.push("--ignore-rules".to_string());
    }
    if settings.skip_git_repo_check {
        args.push("--skip-git-repo-check".to_string());
    }
    if settings.ephemeral {
        args.push("--ephemeral".to_string());
    }
    if let Some(model) = settings.model_cli_arg() {
        args.push("-m".to_string());
        args.push(model.to_string());
    }
    if let Some(reasoning) = settings.reasoning.config_value() {
        args.push("-c".to_string());
        args.push(codex_config_arg(
            CodexConfigKey::ModelReasoningEffort,
            reasoning,
        ));
    }
    for image in &settings.image_paths {
        args.push("-i".to_string());
        args.push(image.clone());
    }
    if let Some(schema) = &settings.output_schema {
        args.push("--output-schema".to_string());
        args.push(schema.clone());
    }
    if let Some(path) = &settings.output_last_message {
        args.push("-o".to_string());
        args.push(path.clone());
    }
}

fn codex_exec_resume_args(
    session_id: Option<&str>,
    use_last: bool,
    include_all: bool,
    prompt: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    push_global_exec_settings(&mut args, settings, true);
    args.extend(["exec", "resume", "--json"].into_iter().map(str::to_string));
    push_optional_exec_settings(&mut args, settings);
    args.push("-c".to_string());
    args.push(developer_instructions);
    if include_all {
        args.push("--all".to_string());
    }
    if use_last {
        args.push("--last".to_string());
    } else if let Some(session_id) = session_id {
        args.push(session_id.to_string());
    }
    args.push(prompt.to_string());
    args
}

fn push_root_interactive_settings(
    args: &mut Vec<String>,
    cwd: &str,
    settings: &CodexExecSettings,
    force_no_alt_screen: bool,
) {
    push_global_exec_settings(args, settings, true);
    if force_no_alt_screen && !args.iter().any(|arg| arg == "--no-alt-screen") {
        args.push("--no-alt-screen".to_string());
    }
    args.push("-C".to_string());
    args.push(cwd.to_string());
    if let Some(model) = settings.model_cli_arg() {
        args.push("-m".to_string());
        args.push(model.to_string());
    }
    if let Some(reasoning) = settings.reasoning.config_value() {
        args.push("-c".to_string());
        args.push(codex_config_arg(
            CodexConfigKey::ModelReasoningEffort,
            reasoning,
        ));
    }
    for image in &settings.image_paths {
        args.push("-i".to_string());
        args.push(image.clone());
    }
}

fn push_addness_developer_instructions(args: &mut Vec<String>) {
    args.push("-c".to_string());
    args.push(codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    ));
}

fn codex_root_interactive_args(
    prompt: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    if !prompt.is_empty() {
        args.push(prompt.to_string());
    }
    args
}

fn codex_root_resume_args(
    session_id: Option<&str>,
    use_last: bool,
    include_all: bool,
    include_non_interactive: bool,
    prompt: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    args.push("resume".to_string());
    if include_all {
        args.push("--all".to_string());
    }
    if include_non_interactive {
        args.push("--include-non-interactive".to_string());
    }
    if use_last {
        args.push("--last".to_string());
    } else if let Some(session_id) = session_id {
        args.push(session_id.to_string());
    }
    if !prompt.is_empty() {
        args.push(prompt.to_string());
    }
    args
}

fn codex_root_session_command_args(
    command_name: &str,
    raw_args: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    args.push(command_name.to_string());
    args.append(&mut parsed);
    Ok(args)
}

fn codex_fork_args(
    session_id: Option<&str>,
    use_last: bool,
    include_all: bool,
    prompt: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, true);
    push_addness_developer_instructions(&mut args);
    args.push("fork".to_string());
    if include_all {
        args.push("--all".to_string());
    }
    if use_last {
        args.push("--last".to_string());
    } else if let Some(session_id) = session_id {
        args.push(session_id.to_string());
    }
    if !prompt.is_empty() {
        args.push(prompt.to_string());
    }
    args
}

fn codex_session_admin_args(
    command_name: &str,
    session: &str,
    force: bool,
    mut extra_args: Vec<String>,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, false);
    args.push(command_name.to_string());
    if force {
        args.push("--force".to_string());
    }
    args.push(session.to_string());
    args.append(&mut extra_args);
    args
}

fn codex_review_args(
    raw_args: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    push_root_interactive_settings(&mut args, cwd, settings, false);
    args.push("review".to_string());
    args.push("-c".to_string());
    args.push(developer_instructions);
    args.append(&mut parsed);
    Ok(args)
}

fn codex_apply_args(raw_args: &str, settings: &CodexExecSettings) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    if parsed.is_empty() {
        anyhow::bail!("apply には task id を指定してください");
    }
    let mut args = Vec::new();
    for config in &settings.config_overrides {
        args.push("-c".to_string());
        args.push(config.clone());
    }
    for feature in &settings.enabled_features {
        args.push("--enable".to_string());
        args.push(feature.clone());
    }
    for feature in &settings.disabled_features {
        args.push("--disable".to_string());
        args.push(feature.clone());
    }
    args.push("apply".to_string());
    args.append(&mut parsed);
    Ok(args)
}

fn push_optional_exec_review_settings(args: &mut Vec<String>, settings: &CodexExecSettings) {
    if settings.ignore_user_config {
        args.push("--ignore-user-config".to_string());
    }
    if settings.ignore_rules {
        args.push("--ignore-rules".to_string());
    }
    if settings.skip_git_repo_check {
        args.push("--skip-git-repo-check".to_string());
    }
    if settings.ephemeral {
        args.push("--ephemeral".to_string());
    }
    if let Some(model) = settings.model_cli_arg() {
        args.push("-m".to_string());
        args.push(model.to_string());
    }
    if let Some(reasoning) = settings.reasoning.config_value() {
        args.push("-c".to_string());
        args.push(codex_config_arg(
            CodexConfigKey::ModelReasoningEffort,
            reasoning,
        ));
    }
    if let Some(schema) = &settings.output_schema {
        args.push("--output-schema".to_string());
        args.push(schema.clone());
    }
    if let Some(path) = &settings.output_last_message {
        args.push("-o".to_string());
        args.push(path.clone());
    }
}

fn codex_exec_review_args(
    raw_args: &str,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Result<Vec<String>> {
    let mut parsed = split_codex_command_args(raw_args)?;
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    push_global_exec_settings(&mut args, settings, true);
    args.push("-C".to_string());
    args.push(cwd.to_string());
    args.extend(["exec", "review", "--json"].into_iter().map(str::to_string));
    push_optional_exec_review_settings(&mut args, settings);
    args.push("-c".to_string());
    args.push(developer_instructions);
    args.append(&mut parsed);
    Ok(args)
}

fn codex_exec_args(
    thread_id: Option<&str>,
    cwd: &str,
    settings: &CodexExecSettings,
) -> Vec<String> {
    let developer_instructions = codex_config_arg(
        CodexConfigKey::DeveloperInstructions,
        addness_tui_developer_instructions(),
    );
    let mut args = Vec::new();
    if let Some(thread_id) = thread_id {
        push_global_exec_settings(&mut args, settings, true);
        args.extend(["exec", "resume", "--json"].into_iter().map(str::to_string));
        push_optional_exec_settings(&mut args, settings);
        args.push("-c".to_string());
        args.push(developer_instructions);
        args.push(thread_id.to_string());
        args.push("-".to_string());
        return args;
    }

    push_global_exec_settings(&mut args, settings, false);
    args.extend(
        ["exec", "--json", "--color"]
            .into_iter()
            .map(str::to_string),
    );
    args.push(settings.color.label().to_string());
    if !settings.bypass_approvals_and_sandbox {
        args.push("-s".to_string());
        args.push(settings.sandbox.cli_arg().to_string());
    }
    args.push("-C".to_string());
    args.push(cwd.to_string());
    push_optional_exec_settings(&mut args, settings);
    args.push("-c".to_string());
    args.push(developer_instructions);
    args.push("-".to_string());
    args
}

fn toml_basic_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// DoD 自動判定で codex に強制する出力 JSON Schema。
/// `{ "results": [ { "index": <int>, "met": <bool> } ] }` の形を要求する。
pub fn dod_assessment_schema() -> String {
    r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["results"],
  "properties": {
    "results": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["index", "met"],
        "properties": {
          "index": { "type": "integer", "minimum": 0 },
          "met": { "type": "boolean" }
        }
      }
    }
  }
}"#
    .to_string()
}

/// DoD 自動判定用のプロンプトを組み立てる。各項目に番号を振って提示する。
pub fn build_dod_assessment_prompt(items: &[String]) -> String {
    let mut listed = String::new();
    for (i, item) in items.iter().enumerate() {
        listed.push_str(&format!("{i}: {item}\n"));
    }
    format!(
        r#"あなたはコードレビュー担当です。コードの変更は一切行わないでください（read-only）。

現在のリポジトリの作業ツリーの状態（`git diff HEAD` や関連ファイルの内容）を調べ、
以下の各「完了基準(DoD)項目」が**現時点で満たされているか**を判定してください。

DoD項目（番号: 内容）:
{listed}
判定結果は、指定された JSON Schema に厳密に従って出力してください。
各項目について index（番号）と met（満たされていれば true、そうでなければ false）を返します。
確証が持てない項目は met=false としてください。"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    /// 入力受付状態（未終了）のテスト用ペインを作る。
    fn live_pane() -> CodexPane {
        let mut pane = CodexPane::test_with_output(10, 80, 0, "");
        pane.finished = false;
        pane
    }

    fn type_input(pane: &mut CodexPane, text: &str) {
        for ch in text.chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
    }

    #[test]
    fn slash_palette_filters_by_prefix() {
        let mut pane = live_pane();
        type_input(&mut pane, "/re");
        let names: Vec<&str> = pane
            .slash_palette_suggestions()
            .iter()
            .map(|(name, _)| *name)
            .collect();
        assert!(pane.slash_palette_active());
        assert!(names.contains(&"/remember"));
        assert!(names.contains(&"/reasoning"));
        assert!(names.iter().all(|name| name.starts_with("/re")));
        assert!(!names.contains(&"/goal"));
    }

    #[test]
    fn slash_palette_hidden_once_args_typed() {
        let mut pane = live_pane();
        type_input(&mut pane, "/exec ");
        assert!(pane.slash_palette_suggestions().is_empty());
        assert!(!pane.slash_palette_active());
    }

    #[test]
    fn slash_palette_hidden_for_plain_text() {
        let mut pane = live_pane();
        type_input(&mut pane, "hello");
        assert!(pane.slash_palette_suggestions().is_empty());
    }

    #[test]
    fn slash_palette_tab_completes_selected_command() {
        let mut pane = live_pane();
        type_input(&mut pane, "/rem");
        assert_eq!(pane.slash_palette_suggestions().len(), 1);
        pane.accept_slash_palette_selection();
        assert_eq!(pane.input_line(), "/remember ");
        // 補完で空白が入るためパレットは閉じる。
        assert!(pane.slash_palette_suggestions().is_empty());
    }

    #[test]
    fn slash_palette_selection_moves_wraps_and_resets_on_input() {
        let mut pane = live_pane();
        type_input(&mut pane, "/re");
        let n = pane.slash_palette_suggestions().len();
        assert!(n >= 2);
        assert_eq!(pane.slash_palette_selected(), 0);
        pane.move_slash_palette_selection(1);
        assert_eq!(pane.slash_palette_selected(), 1);
        // 先頭で -1 すると末尾へラップする。
        pane.move_slash_palette_selection(-1);
        pane.move_slash_palette_selection(-1);
        assert_eq!(pane.slash_palette_selected(), n - 1);
        // 文字入力で選択は先頭へ戻る。
        type_input(&mut pane, "s");
        assert_eq!(pane.slash_palette_selected(), 0);
    }

    fn has_addness_developer_instructions(args: &[String]) -> bool {
        args.iter()
            .any(|arg| arg.starts_with("developer_instructions="))
    }

    fn has_addness_memory_defaults(args: &[String]) -> bool {
        args.windows(2)
            .any(|pair| pair == ["-c", "memories.use_memories=false"])
            && args
                .windows(2)
                .any(|pair| pair == ["-c", "memories.generate_memories=false"])
    }

    fn submit_line(pane: &mut CodexPane, text: &str) {
        for ch in text.chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        pane.input(key(KeyCode::Enter));
    }

    #[test]
    fn input_state_records_last_prompt_on_enter() {
        let mut state = CodexInputState::default();
        for ch in "  cargo test を実行して  ".chars() {
            assert!(state.observe_key(key(KeyCode::Char(ch))).is_none());
        }
        let submitted = state.observe_key(key(KeyCode::Enter));
        assert_eq!(submitted.as_deref(), Some("cargo test を実行して"));
        state.record_submitted(submitted.as_deref().unwrap());

        assert_eq!(state.last_prompt.as_deref(), Some("cargo test を実行して"));
    }

    #[test]
    fn input_state_detects_exit_command_on_enter() {
        let mut state = CodexInputState::default();
        for ch in "/exit".chars() {
            assert!(state.observe_key(key(KeyCode::Char(ch))).is_none());
        }
        let submitted = state.observe_key(key(KeyCode::Enter)).unwrap();
        state.record_submitted(&submitted);

        assert!(state.exit_command_sent);
    }

    #[test]
    fn input_state_paste_preserves_newlines_as_single_prompt() {
        let mut state = CodexInputState::default();
        state.insert_text("first line\r\nsecond line\nthird line");

        let submitted = state.observe_key(key(KeyCode::Enter));

        assert_eq!(
            submitted.as_deref(),
            Some("first line\nsecond line\nthird line")
        );
        assert_eq!(state.line, "");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn input_state_edits_at_cursor_with_arrows_and_delete() {
        let mut state = CodexInputState::default();
        state.insert_text("abcd");
        assert!(state.observe_key(key(KeyCode::Left)).is_none());
        assert!(state.observe_key(key(KeyCode::Left)).is_none());
        assert!(state.observe_key(key(KeyCode::Char('X'))).is_none());
        assert!(state.observe_key(key(KeyCode::Delete)).is_none());

        assert_eq!(state.line, "abXd");
        assert_eq!(state.cursor, "abX".len());
    }

    #[test]
    fn input_state_shift_enter_inserts_newline() {
        let mut state = CodexInputState::default();
        state.insert_text("before");
        assert!(
            state
                .observe_key(modified_key(KeyCode::Enter, KeyModifiers::SHIFT))
                .is_none()
        );
        state.insert_text("after");

        let submitted = state.observe_key(key(KeyCode::Enter));

        assert_eq!(submitted.as_deref(), Some("before\nafter"));
    }

    #[test]
    fn input_state_vertical_arrows_move_between_lines() {
        let mut state = CodexInputState::default();
        state.insert_text("abc\ndef");
        assert!(state.observe_key(key(KeyCode::Up)).is_none());
        assert!(state.observe_key(key(KeyCode::Char('X'))).is_none());

        assert_eq!(state.line, "abcX\ndef");
    }

    #[test]
    fn input_during_running_queues_next_turn() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        for ch in "j/kも入力".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        pane.input(key(KeyCode::Enter));

        assert_eq!(pane.queued_prompt_count(), 1);
        assert_eq!(pane.input_line(), "");
        assert_eq!(pane.last_prompt(), Some("j/kも入力"));
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::User && line.text == "j/kも入力")
        );
        assert!(pane.log.iter().any(
            |line| line.kind == CodexLogKind::System && line.text.contains("次のターンに予約")
        ));
    }

    #[test]
    fn queued_exit_finishes_when_turn_becomes_idle() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        for ch in "/exit".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        pane.input(key(KeyCode::Enter));

        assert!(!pane.finished);
        assert_eq!(pane.queued_prompt_count(), 1);

        pane.turn_running = false;
        assert!(pane.start_next_queued_turn_if_idle());

        assert!(pane.finished);
        assert_eq!(pane.queued_prompt_count(), 0);
    }

    #[test]
    fn goal_slash_command_sets_goal_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/goal Finish the migration");

        assert_eq!(
            pane.goal_mode.objective.as_deref(),
            Some("Finish the migration")
        );
        assert!(pane.goal_mode.is_active());
        assert_eq!(pane.turn_count(), 0);
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System && line.text.contains("Finish the migration")
        }));
    }

    #[test]
    fn goal_slash_command_pauses_resumes_and_clears_goal() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/goal Finish the migration");
        submit_line(&mut pane, "/goal pause");
        assert_eq!(
            pane.goal_mode.objective.as_deref(),
            Some("Finish the migration")
        );
        assert!(pane.goal_mode.paused);

        submit_line(&mut pane, "/goal resume");
        assert!(pane.goal_mode.is_active());

        submit_line(&mut pane, "/goal clear");
        assert_eq!(pane.goal_mode.objective, None);
        assert!(!pane.goal_mode.is_active());
    }

    #[test]
    fn goal_mode_wraps_next_prompt_when_active() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.goal_mode = CodexGoalMode {
            objective: Some("Finish the migration".to_string()),
            paused: false,
        };

        let prompt = pane.prompt_with_goal_mode("run tests");

        assert!(prompt.starts_with("<user_request>\nrun tests\n</user_request>"));
        assert!(prompt.contains("Codex Goal mode is active"));
        assert!(prompt.contains("Finish the migration"));
        assert!(prompt.contains("run tests"));

        pane.goal_mode.paused = true;
        assert_eq!(pane.prompt_with_goal_mode("run tests"), "run tests");
    }

    #[test]
    fn addness_context_preserves_goal_mode_user_request_first() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.goal_mode = CodexGoalMode {
            objective: Some("Finish the migration".to_string()),
            paused: false,
        };

        let goal_prompt = pane.prompt_with_goal_mode("run tests");
        let prompt = pane.prompt_with_addness_context(&goal_prompt);

        assert!(prompt.starts_with("<user_request>\nrun tests\n</user_request>"));
        assert_eq!(prompt.matches("<user_request>").count(), 1);
        assert!(prompt.contains("<persistent_goal_context>"));
        assert!(prompt.contains("Finish the migration"));
        assert!(prompt.contains(r#"<addness_tui_context role="supporting_project_memory">"#));
        let request_idx = prompt.find("run tests").unwrap();
        assert!(request_idx < prompt.find("Finish the migration").unwrap());
        assert!(request_idx < prompt.find("<addness_tui_context").unwrap());
    }

    #[test]
    fn addness_context_wraps_prompt_with_memory_contract() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.goal_id = "goal-1".to_string();
        pane.goal_title = "Addness in Codex".to_string();
        pane.status_label = "進行中".to_string();
        pane.parent_goal_id = "parent-1".to_string();
        pane.parent_goal_title = "Root goal".to_string();
        pane.cwd = "/repo".to_string();
        pane.dod = "AddnessをDBとして使う".to_string();
        pane.set_addness_body_context(Some(
            "## Codex作業メモ\n- 現在地: 設計済み\n- 次の手: 実装".to_string(),
        ));
        pane.update_children(vec![ChildGoalUpdate {
            id: "child-1".to_string(),
            title: "プロンプト改善".to_string(),
            description: Some("ユーザー依頼を先に実装できる状態".to_string()),
            icon: "[ ]",
            status_label: "未着手".to_string(),
            is_completed: false,
        }]);

        let prompt = pane.prompt_with_addness_context("実装して");

        assert!(prompt.starts_with("<user_request>\n実装して\n</user_request>"));
        assert!(prompt.contains(r#"<addness_tui_context role="supporting_project_memory">"#));
        assert!(prompt.contains("must not replace the user request above"));
        assert!(prompt.contains("id: goal-1"));
        assert!(prompt.contains("title: Addness in Codex"));
        assert!(prompt.contains("parent: Root goal (parent-1)"));
        assert!(prompt.contains("- branch:"));
        assert!(prompt.contains("Known child goals from TUI snapshot"));
        assert!(prompt.contains("[ ] プロンプト改善 [未着手]"));
        assert!(prompt.contains("DoD: ユーザー依頼を先に実装できる状態"));
        assert!(prompt.contains("Body excerpt from TUI snapshot"));
        assert!(prompt.contains("現在地: 設計済み"));
        assert!(prompt.contains("Make concrete progress"));
        assert!(prompt.contains("Treat this TUI snapshot as the first Addness recall"));
        assert!(prompt.contains("inspect the repository like normal Codex"));
        assert!(prompt.contains("make a reasonable assumption from repo evidence"));
        assert!(prompt.contains("The TUI automatically records current branch/folder"));
        assert!(prompt.contains("Do not manually update Addness just to mirror routine progress"));
        assert!(prompt.contains("Manually update Addness only when it improves future work"));
        assert!(
            prompt.contains("Do not put this project's durable facts into Codex global memory")
        );
        assert!(prompt.contains("<user_request>\n実装して\n</user_request>"));
        assert!(prompt.contains("Act on the user_request first"));
    }

    #[test]
    fn addness_context_adds_organization_hint_only_for_broad_requests() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");

        let simple = pane.prompt_with_addness_context("run tests");
        assert!(!simple.contains("Organization hint"));

        let broad = pane.prompt_with_addness_context(
            "TUIをCodexより見やすく改善して、内部の仕組みも整理し、複数の実装を進めて",
        );
        assert!(broad.starts_with("<user_request>\nTUIをCodexより見やすく改善"));
        assert!(broad.contains("Organization hint"));
        assert!(broad.contains("Addness child goals with clear DoD/body"));
        assert!(broad.contains("highest-priority slice"));

        pane.thread_id = Some("thread-1".to_string());
        let compact = pane.prompt_with_addness_context("子ゴールごとにサブエージェントへ委任して");
        assert!(compact.contains(r#"<addness_tui_context mode="compact""#));
        assert!(compact.contains("Organization hint"));
    }

    #[test]
    fn addness_body_context_refresh_updates_next_prompt() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");

        assert!(
            pane.set_addness_body_context(
                Some("## Codex作業メモ\n- 現在地: 古い状態".to_string(),)
            )
        );
        assert!(
            !pane.set_addness_body_context(Some(
                "## Codex作業メモ\n- 現在地: 古い状態".to_string(),
            ))
        );
        assert!(pane.set_addness_body_context(Some(
            "## Codex作業メモ\n- 現在地: 新しい状態\n- 次の手: 実装を続ける".to_string(),
        )));

        let prompt = pane.prompt_with_addness_context("続けて");

        assert!(prompt.contains("現在地: 新しい状態"));
        assert!(prompt.contains("次の手: 実装を続ける"));
        assert!(!prompt.contains("現在地: 古い状態"));
    }

    #[test]
    fn addness_context_is_compact_after_thread_started() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.thread_id = Some("thread-1".to_string());
        pane.goal_id = "goal-1".to_string();
        pane.goal_title = "Addness in Codex".to_string();
        pane.status_label = "進行中".to_string();
        pane.cwd = "/repo".to_string();
        pane.dod = "AddnessをDBとして使う".to_string();
        pane.set_addness_body_context(Some(
            "## Codex作業メモ\n- 現在地: この詳細は2ターン目に繰り返さない".to_string(),
        ));
        pane.update_children(vec![ChildGoalUpdate {
            id: "child-1".to_string(),
            title: "2ターン目に繰り返さない子ゴール詳細".to_string(),
            description: Some("この詳細も2ターン目に繰り返さない".to_string()),
            icon: "[ ]",
            status_label: "未着手".to_string(),
            is_completed: false,
        }]);

        let prompt = pane.prompt_with_addness_context("次はテストを書いて");

        assert!(prompt.starts_with("<user_request>\n次はテストを書いて\n</user_request>"));
        assert!(
            prompt.contains(
                r#"<addness_tui_context mode="compact" role="supporting_project_memory">"#
            )
        );
        assert!(prompt.contains("The full Addness snapshot was already provided"));
        assert!(prompt.contains("The user request above is primary"));
        assert!(prompt.contains("Act on the user request first"));
        assert!(prompt.contains("make a reasonable assumption from repo evidence"));
        assert!(prompt.contains("The TUI automatically records current branch/folder"));
        assert!(prompt.contains("Do not manually update Addness just to mirror routine progress"));
        assert!(prompt.contains("Manually update Addness only for durable decisions"));
        assert!(prompt.contains("- branch:"));
        assert!(!prompt.contains("Body excerpt from TUI snapshot"));
        assert!(!prompt.contains("Known child goals from TUI snapshot"));
        assert!(!prompt.contains("この詳細は2ターン目に繰り返さない"));
        assert!(!prompt.contains("2ターン目に繰り返さない子ゴール詳細"));
        assert!(!prompt.contains("この詳細も2ターン目に繰り返さない"));
        assert!(prompt.contains("<user_request>\n次はテストを書いて\n</user_request>"));
        assert!(prompt.contains("Act on the user_request first"));
    }

    #[test]
    fn resume_prompt_moves_from_addness_recall_to_work() {
        let prompt = resume_prompt();

        assert!(prompt.contains("3行以内で整理"));
        assert!(prompt.contains("リポジトリ確認・実装・検証へ進んでください"));
        assert!(prompt.contains("Addnessを読んだだけでターンを終えない"));
        assert!(prompt.contains("通常進捗保存はTUI"));
        assert!(!prompt.contains("短く整理してから進めてください"));
    }

    #[test]
    fn scrollback_clamps_to_log_history() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "1\n2\n3\n4\n5\n6\n7\n8\n9\n10");
        pane.resize(6, 20);

        pane.scroll_lines(99);

        assert_eq!(pane.scrollback, pane.max_view_scrollback());
    }

    #[test]
    fn rendered_history_metrics_define_exact_scrollback_limit() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");

        pane.sync_rendered_history_metrics(100, 10);
        pane.scroll_to_top();
        assert_eq!(pane.scrollback, 90);

        pane.sync_rendered_history_metrics(12, 10);
        assert_eq!(pane.scrollback, 2);
    }

    #[test]
    fn thread_started_event_records_thread_id() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"thread.started","thread_id":"abc"}"#);

        assert_eq!(pane.thread_id.as_deref(), Some("abc"));
        assert!(
            pane.log
                .iter()
                .any(|line| line.text == "Codex セッションを開始しました")
        );
        assert!(!pane.log.iter().any(|line| line.text.contains("abc")));
    }

    #[test]
    fn turn_started_event_records_turn_separator() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.input_state.last_prompt = Some("  実装を進めて\nください  ".to_string());

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Turn);
        assert_eq!(line.text, "Turn 1 - 実装を進めて ください");
        assert!(pane.is_turn_running());
    }

    #[test]
    fn run_state_tracks_command_and_confirmation() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        assert_eq!(pane.run_state(), CodexRunState::Completed);
        pane.finished = false;
        assert_eq!(pane.run_state(), CodexRunState::InputWaiting);

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        assert_eq!(pane.run_state(), CodexRunState::Thinking);

        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo test","codex_cwd":"/repo"}"#,
        );
        assert_eq!(pane.run_state(), CodexRunState::CommandRunning);
        assert_eq!(pane.current_command(), Some("cargo test"));

        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? y/n"}"#);
        let banner = pane.decision_banner().unwrap();
        assert_eq!(pane.run_state(), CodexRunState::Confirming);
        assert_eq!(banner.kind, CodexDecisionKind::Approval);

        assert!(pane.handle_decision_key(key(KeyCode::Char('a'))));
        assert!(pane.decision_banner().is_none());
        assert_eq!(pane.run_state(), CodexRunState::CommandRunning);
    }

    #[test]
    fn decision_key_accepts_always_allow_choice() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.handle_stdout_line(
            r#"{"type":"permission_requested","message":"Need filesystem permission"}"#,
        );

        let banner = pane.decision_banner().unwrap();
        assert_eq!(banner.always_choice(), Some(('l', "これからずっと許可")));

        assert!(pane.handle_decision_key(key(KeyCode::Char('l'))));

        assert!(pane.decision_banner().is_none());
        assert_eq!(pane.action.as_deref(), Some("確認応答: これからずっと許可"));
        assert_eq!(pane.exec_settings.approval, CodexApprovalChoice::Never);
        assert!(!pane.exec_settings.bypass_approvals_and_sandbox);
        assert!(
            pane.activity
                .iter()
                .any(|line| line.contains("確認待ちに これからずっと許可 で応答"))
        );
    }

    #[test]
    fn yes_no_decision_stays_manual_until_user_chooses() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.handle_stdout_line(r#"{"type":"input_requested","message":"Continue? y/n"}"#);

        let banner = pane.decision_banner().unwrap();
        assert_eq!(banner.kind, CodexDecisionKind::YesNo);
        assert_eq!(pane.run_state(), CodexRunState::Confirming);
        assert_eq!(pane.action.as_deref(), Some("依頼を確認中"));

        assert!(pane.handle_decision_key(key(KeyCode::Char('y'))));

        assert!(pane.decision_banner().is_none());
        assert_eq!(pane.action.as_deref(), Some("確認応答: Yes"));
        assert!(
            pane.activity
                .iter()
                .any(|line| line.contains("確認待ちに Yes で応答"))
        );
    }

    #[test]
    fn permission_decision_stays_manual() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.handle_stdout_line(
            r#"{"type":"permission_requested","message":"Need filesystem permission"}"#,
        );

        let banner = pane.decision_banner().unwrap();
        assert_eq!(banner.kind, CodexDecisionKind::Permission);
        assert_eq!(pane.action.as_deref(), Some("依頼を確認中"));
    }

    #[test]
    fn old_turns_are_collapsed_until_toggled_open() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.push_log(CodexLogKind::Assistant, "old response");
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.push_log(CodexLogKind::Assistant, "new response");

        let collapsed = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(collapsed.iter().any(|text| text.starts_with("Turn 1")));
        assert!(!collapsed.contains(&"old response"));
        assert!(collapsed.contains(&"new response"));

        pane.toggle_old_turns_collapsed();
        let expanded = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(expanded.contains(&"old response"));
    }

    #[test]
    fn filtered_log_lines_supports_filter_and_search() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.push_log(CodexLogKind::Turn, "Turn 1");
        pane.push_log(CodexLogKind::User, "cargo test して");
        pane.push_log(CodexLogKind::Assistant, "確認します");
        pane.push_log(CodexLogKind::Tool, "exec_command_begin: cargo test");
        pane.push_log(CodexLogKind::Error, "limit");

        let conversation = pane.filtered_log_lines();
        assert_eq!(conversation.len(), 4);
        assert!(conversation.iter().all(|line| matches!(
            line.kind,
            CodexLogKind::Turn | CodexLogKind::User | CodexLogKind::Assistant | CodexLogKind::Error
        )));

        pane.cycle_log_filter();
        pane.search_query = "cargo".to_string();
        let tools = pane.filtered_log_lines();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].kind, CodexLogKind::Tool);
    }

    #[test]
    fn conversation_filter_hides_routine_system_noise() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.push_log(CodexLogKind::System, "Codex セッションを開始しました");
        pane.push_log(CodexLogKind::System, "Settings: model=config");
        pane.push_log(CodexLogKind::Assistant, "確認しました");

        let visible = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();

        assert!(!visible.contains(&"Codex セッションを開始しました"));
        assert!(visible.contains(&"Settings: model=config"));
        assert!(visible.contains(&"確認しました"));
    }

    #[test]
    fn error_event_sets_terminal_notice() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"error","message":"limit"}"#);

        let notice = pane.take_terminal_notice().unwrap();
        assert_eq!(notice.title, "Codex エラー");
        assert_eq!(notice.message, "limit");
    }

    #[test]
    fn turn_completed_sets_terminal_notice() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        let notice = pane.take_terminal_notice().unwrap();
        assert_eq!(notice.title, "Codex 完了");
        assert_eq!(notice.message, "Codex の出力が完了しました");
    }

    #[test]
    fn approval_event_sets_terminal_notice_without_overwriting_next_notice() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? yes/no"}"#);
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        let first = pane.take_terminal_notice().unwrap();
        let second = pane.take_terminal_notice().unwrap();
        assert_eq!(first.title, "Codex 確認待ち");
        assert!(first.message.contains("yes/no"));
        assert_eq!(second.title, "Codex 完了");
    }

    #[test]
    fn assistant_delta_appends_to_same_line() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"hel"}"#);
        pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"lo"}"#);

        let assistant = pane
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Assistant)
            .collect::<Vec<_>>();
        assert_eq!(assistant.len(), 1);
        assert_eq!(assistant[0].text, "hello");
    }

    #[test]
    fn session_history_persists_raw_events_and_restores_display_log() {
        let path = std::env::temp_dir().join(format!(
            "addness-codex-history-{}-{}.jsonl",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&path);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        {
            let history_path = path.clone();
            let mut pane = CodexPane::spawn_inner(CodexPaneSpawnOptions {
                codex_bin: Path::new("codex"),
                cwd: &cwd,
                addness_bin: "addness",
                goal_id: "goal/history".to_string(),
                goal_title: "History goal".to_string(),
                dod: String::new(),
                status_label: "TEST".to_string(),
                session_log_path: Some(history_path),
            })
            .unwrap();
            pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"hel"}"#);
            pane.handle_stdout_line(r#"{"type":"agent_message.delta","delta":"lo"}"#);
        }

        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains(r#""record":"raw_event""#));
        assert!(saved.contains(r#""record":"assistant_delta""#));

        let history_path = path.clone();
        let pane = CodexPane::spawn_inner(CodexPaneSpawnOptions {
            codex_bin: Path::new("codex"),
            cwd: &cwd,
            addness_bin: "addness",
            goal_id: "goal/history".to_string(),
            goal_title: "History goal".to_string(),
            dod: String::new(),
            status_label: "TEST".to_string(),
            session_log_path: Some(history_path),
        })
        .unwrap();

        assert!(pane.loaded_history_count() > 0);
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::Assistant && line.text == "hello")
        );
        assert!(pane.history_label().contains("保存中"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn response_item_exec_command_arguments_json_renders_tool_line() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"cargo test\",\"workdir\":\"/repo\"}"}}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(!line.text.contains("exec_command"));
        assert!(line.text.contains("cargo test"));
        assert!(!line.text.contains("/repo"));
    }

    #[test]
    fn addness_bin_exec_command_updates_action_indicator() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"\\\"$ADDNESS_BIN\\\" goal get goal-1 --json\",\"workdir\":\"/repo\"}"}}"#,
        );

        assert_eq!(pane.action.as_deref(), Some("ゴール文脈を読込中"));
        assert!(pane.last_addness_read_at.is_some());
    }

    #[test]
    fn exec_command_begin_renders_command_from_native_fields() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo check","codex_cwd":"/repo"}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(!line.text.contains("exec_command_begin"));
        assert!(line.text.contains("cargo check"));
        assert!(!line.text.contains("/repo"));
        assert_eq!(pane.current_command(), Some("cargo check"));
    }

    #[test]
    fn apply_patch_renders_as_code_edit_tool() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"apply_patch","arguments":"*** Begin Patch\n*** Update File: src/tui/ui.rs\n@@\n*** End Patch\n"}}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(line.text.starts_with("EDIT "));
        assert!(line.text.contains("*** Update File: src/tui/ui.rs"));
    }

    #[test]
    fn exec_command_end_renders_exit_code_and_output() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_end","cmd":"cargo check","exit_code":0,"formatted_output":"Finished dev profile"}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert!(!line.text.contains("exec_command_end"));
        assert!(!line.text.contains("exit 0"));
        assert!(line.text.contains("cargo check"));
        assert!(!line.text.contains("Finished dev profile"));
        assert_eq!(pane.current_command(), None);
    }

    #[test]
    fn internal_item_completed_event_is_not_logged() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"item.completed"}"#);

        assert!(pane.log.is_empty());
    }

    #[test]
    fn generic_text_event_hides_internal_event_type() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"item.completed","message":"処理を完了しました"}"#);

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Event);
        assert_eq!(line.text, "処理を完了しました");
    }

    #[test]
    fn update_children_preserves_completed_status() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");

        pane.update_children(vec![ChildGoalUpdate {
            id: "goal-1".to_string(),
            title: "完了済みの子ゴール".to_string(),
            description: Some("完了済みも一覧に残す".to_string()),
            icon: "[x]",
            status_label: "完了".to_string(),
            is_completed: true,
        }]);

        assert_eq!(pane.children.len(), 1);
        assert_eq!(pane.children[0].icon, "[x]");
        assert_eq!(pane.children[0].status_label, "完了");
        assert!(pane.children[0].is_completed);
    }

    #[test]
    fn turn_completed_clears_current_command() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo check","codex_cwd":"/repo"}"#,
        );
        assert_eq!(pane.current_command(), Some("cargo check"));

        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        assert_eq!(pane.current_command(), None);
    }

    #[test]
    fn completed_turn_body_record_is_taken_once() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        assert_eq!(pane.take_completed_turn_body_record(), None);

        pane.test_add_completed_turn("done");
        let record = pane.take_completed_turn_body_record().unwrap();
        assert_eq!(record.turn, 1);
        assert_eq!(record.summary.implemented, vec!["done"]);
        assert_eq!(pane.take_completed_turn_body_record(), None);

        pane.test_add_completed_turn("next");
        pane.turn_running = true;
        let record = pane.take_completed_turn_body_record().unwrap();
        assert_eq!(record.turn, 2);
        assert_eq!(record.summary.implemented, vec!["next"]);
        assert_eq!(pane.take_completed_turn_body_record(), None);

        pane.test_add_completed_turn("final");
        pane.finished = true;
        let record = pane.take_completed_turn_body_record().unwrap();
        assert_eq!(record.turn, 3);
        assert_eq!(record.summary.implemented, vec!["final"]);
        assert_eq!(pane.take_completed_turn_body_record(), None);
    }

    #[test]
    fn completed_turn_body_record_keeps_completed_prompt_when_next_is_queued() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.current_turn_prompt = Some("first request".to_string());

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.push_log(CodexLogKind::Assistant, "first implementation");
        pane.input_state.record_submitted("second request");
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);

        let record = pane.take_completed_turn_body_record().unwrap();
        assert_eq!(record.turn, 1);
        assert_eq!(record.prompt.as_deref(), Some("first request"));
        assert_eq!(record.summary.implemented, vec!["first implementation"]);
        assert!(
            !record
                .summary
                .implemented
                .iter()
                .any(|line| line.contains("second request"))
        );
    }

    #[test]
    fn prompt_body_record_prefers_current_turn_prompt_over_queued_prompt() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.current_turn_prompt = Some("current request".to_string());
        pane.input_state.record_submitted("queued request");

        assert_eq!(pane.prompt_needs_body_record(), Some("current request"));
    }

    #[test]
    fn final_body_record_context_uses_last_completed_turn_not_exit_prompt() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.current_turn_prompt = Some("implement feature".to_string());

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        pane.push_log(CodexLogKind::Assistant, "implemented feature");
        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);
        pane.input_state.record_submitted("/exit");
        pane.finished = true;

        let (prompt, summary) = pane.final_body_record_context();
        assert_eq!(prompt.as_deref(), Some("implement feature"));
        assert_eq!(summary.implemented, vec!["implemented feature"]);
    }

    #[test]
    fn final_body_record_context_uses_current_turn_when_closing_mid_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.current_turn_prompt = Some("mid turn work".to_string());
        pane.push_log(CodexLogKind::Assistant, "partial progress");

        let (prompt, summary) = pane.final_body_record_context();
        assert_eq!(prompt.as_deref(), Some("mid turn work"));
        assert_eq!(summary.implemented, vec!["partial progress"]);
    }

    #[test]
    fn addness_tool_activity_summarizes_semantic_change() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"$ADDNESS_BIN goal update goal-1 --body memo --json\",\"workdir\":\"/repo\"}"},"formatted_output":"{\"title\":\"重要ゴール\"}"}"#,
        );

        assert!(pane.last_addness_write_at.is_some());
        assert!(
            pane.activity
                .iter()
                .any(|line| line.contains("Addness body変更: 重要ゴール"))
        );
    }

    #[test]
    fn addness_tool_activity_accepts_capitalized_entry_command() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"response_item","payload":{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"Addness goal create --title UI改善 --parent goal-1 --description DoD --json\",\"workdir\":\"/repo\"}"},"formatted_output":"{\"title\":\"UI改善\"}"}"#,
        );

        assert!(pane.last_addness_write_at.is_some());
        assert!(
            pane.activity
                .iter()
                .any(|line| line.contains("Addness 子ゴール追加: UI改善"))
        );
    }

    #[test]
    fn subcommand_output_is_flushed_as_single_tool_log() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.child_process_output = vec!["server-a".to_string(), "server-b".to_string()];
        pane.child_process_error_output = vec!["warn".to_string()];

        pane.flush_child_process_output(true, "codex mcp list");

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Tool);
        assert_eq!(line.text, "OK codex mcp list (出力4行)");
        assert!(pane.child_process_output.is_empty());
        assert!(pane.child_process_error_output.is_empty());
    }

    #[test]
    fn work_summary_extracts_assistant_checks_and_errors() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.push_log(
            CodexLogKind::Assistant,
            "UIを改善しました\nテストを追加しました",
        );
        pane.push_log(
            CodexLogKind::Tool,
            "OK cargo test (exit 0)\ntest result: ok. 99 passed; 0 failed;",
        );
        pane.push_log(CodexLogKind::Error, "残課題: なし");

        let summary = pane.work_summary();
        assert!(
            summary
                .implemented
                .iter()
                .any(|line| line.contains("UIを改善"))
        );
        assert!(
            summary
                .checks
                .iter()
                .any(|line| line.contains("test result: ok"))
        );
        assert!(summary.remaining.iter().any(|line| line.contains("残課題")));
    }

    #[test]
    fn agent_message_text_is_not_classified_as_tool() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"agent_message","message":"I would run cargo test next."}"#,
        );

        let line = pane.log.last().unwrap();
        assert_eq!(line.kind, CodexLogKind::Assistant);
        assert_eq!(line.text, "I would run cargo test next.");
    }

    #[test]
    fn codex_backend_normalizes_lifecycle_events() {
        let backend = CodexBackend;

        let started = serde_json::json!({"type": "thread.started", "thread_id": "abc123"});
        assert!(matches!(
            backend.parse_lifecycle_event(&started),
            AgentEvent::SessionStarted(Some(id)) if id == "abc123"
        ));
        assert_eq!(
            backend.session_id_from_event(&started).as_deref(),
            Some("abc123")
        );

        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.started"})),
            AgentEvent::TurnStarted
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.completed"})),
            AgentEvent::TurnCompleted
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.finished"})),
            AgentEvent::TurnCompleted
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "turn.failed"})),
            AgentEvent::TurnFailed(_)
        ));
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "error"})),
            AgentEvent::Error(_)
        ));
        // ツール/本文など表層イベントは Other に集約される。
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "item.completed"})),
            AgentEvent::Other
        ));
    }

    #[test]
    fn codex_exec_args_start_new_json_turn() {
        let settings = CodexExecSettings::default();
        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["exec", "--json"]));
        assert!(args.contains(&"--json".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-s", "workspace-write"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(args.iter().any(|arg| {
            arg.starts_with("developer_instructions=")
                && arg.contains("ADDNESS_WORKTREE_BRANCH")
                && arg.contains("Addness TUIは誰でも `addness` と打てば起動")
                && arg.contains("snapshotを最初の想起として扱ってください")
                && arg.contains("実装判断を変え得る場合だけ")
                && arg.contains("TUI snapshotを見る → リポジトリを読む → 実装/調査する → 検証する")
                && arg.contains("追加読込が必要な時")
                && arg.contains("実装判断に必要な不足分だけ")
                && arg.contains("turn完了・セッション終了サマリ")
                && arg.contains("手動でbody更新しなくて構いません")
                && arg.contains("手を止めてAddness更新ターンへ寄せない")
                && arg.contains("手動でAddnessに書き込むのは、自動メモでは足りない")
                && !arg.contains("Addnessに書き込むのは、作業を始めた時")
                && !arg.contains("読み取り時:")
        }));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "memories.use_memories=false"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "memories.generate_memories=false"])
        );
    }

    #[test]
    fn addness_developer_instructions_keep_codex_execution_primary() {
        let instructions = addness_tui_developer_instructions();

        assert!(instructions.chars().count() < 3_000);
        assert!(instructions.contains("通常Codexと同じ速度で調査・実装・検証"));
        assert!(instructions.contains("Addnessはmemory.mdの代替となるプロジェクト別DB"));
        assert!(instructions.contains("Addness TUIは誰でも `addness` と打てば起動"));
        assert!(instructions.contains("Addness CLIでgoal body/DoD/子ゴールを書き込めます"));
        assert!(
            instructions
                .contains("TUI snapshotを見る → リポジトリを読む → 実装/調査する → 検証する")
        );
        assert!(instructions.contains("Addnessを読んだだけで作業完了にしない"));
        assert!(
            instructions.contains("repoから合理的に判断できるなら確認質問やAddness整理を挟まず")
        );
        assert!(instructions.contains("Addness DBの置き場所"));
        assert!(instructions.contains("CLI最小操作"));
        assert!(instructions.contains("goal update \"$ADDNESS_GOAL_ID\" --body-file"));
        assert!(instructions.contains("goal update \"$ADDNESS_GOAL_ID\" --description-file"));
        assert!(instructions.contains("goal create --title \"...\" --parent \"$ADDNESS_GOAL_ID\""));
        assert!(instructions.contains("body=入力情報/ブランチ/次の手"));
        assert!(instructions.contains("goal update <CHILD_GOAL_ID> --body-file"));
        assert!(instructions.contains("子ゴールは毎ターン機械的に作らない"));
    }

    #[test]
    fn codex_exec_args_resume_existing_json_thread() {
        let settings = CodexExecSettings::default();
        let args = codex_exec_args(Some("thread-1"), "/repo", &settings);

        assert!(
            args.windows(3)
                .any(|triple| triple == ["exec", "resume", "--json"])
        );
        assert!(args.contains(&"--json".to_string()));
        assert!(args.contains(&"thread-1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        assert!(!args.contains(&"-C".to_string()));
        assert!(!args.contains(&"-s".to_string()));
    }

    #[test]
    fn codex_exec_resume_args_include_selected_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::Medium;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.image_paths.push("/tmp/shot.png".to_string());
        settings.output_schema = Some("/tmp/schema.json".to_string());
        settings.output_last_message = Some("/tmp/last.txt".to_string());

        let args = codex_exec_resume_args(None, true, false, "continue", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert_eq!(args.first().map(String::as_str), Some("-a"));
        assert_eq!(args.get(1).map(String::as_str), Some("on-request"));
        assert_eq!(args.get(2).map(String::as_str), Some("-s"));
        assert!(args.contains(&"--last".to_string()));
        assert!(args.contains(&"--json".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/shot.png"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-schema", "/tmp/schema.json"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-o", "/tmp/last.txt"]));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert_eq!(args.last().map(String::as_str), Some("continue"));

        let session_args =
            codex_exec_resume_args(Some("session-1"), false, true, "next", &settings);
        assert!(session_args.contains(&"session-1".to_string()));
        assert!(!session_args.contains(&"--last".to_string()));
        assert!(session_args.contains(&"--all".to_string()));
        assert_eq!(session_args.last().map(String::as_str), Some("next"));
    }

    #[test]
    fn codex_root_interactive_args_include_prompt_and_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model_override = Some("gpt-custom".to_string());
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;

        let args = codex_root_interactive_args("hello codex", "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(has_addness_memory_defaults(&args));
        assert_eq!(args.last().map(String::as_str), Some("hello codex"));

        let no_prompt = codex_root_interactive_args("", "/repo", &settings);
        assert!(has_addness_memory_defaults(&no_prompt));
        assert_ne!(no_prompt.last().map(String::as_str), Some(""));
    }

    #[test]
    fn codex_root_resume_args_include_interactive_resume_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;

        let args = codex_root_resume_args(None, true, true, true, "continue", "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(has_addness_developer_instructions(&args));
        assert!(has_addness_memory_defaults(&args));
        assert!(args.contains(&"resume".to_string()));
        assert!(args.contains(&"--all".to_string()));
        assert!(args.contains(&"--include-non-interactive".to_string()));
        assert!(args.contains(&"--last".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("continue"));
        assert_eq!(codex_command_category(&args), "session");

        let session_args = codex_root_resume_args(
            Some("session-1"),
            false,
            false,
            false,
            "next",
            "/repo",
            &settings,
        );
        assert!(session_args.contains(&"session-1".to_string()));
        assert!(!session_args.contains(&"--last".to_string()));
        assert!(!session_args.contains(&"--include-non-interactive".to_string()));
        assert!(has_addness_developer_instructions(&session_args));
        assert!(has_addness_memory_defaults(&session_args));
        assert_eq!(session_args.last().map(String::as_str), Some("next"));
    }

    #[test]
    fn codex_root_session_command_args_pass_through_args_and_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model_override = Some("gpt-custom".to_string());
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;

        let args = codex_root_session_command_args(
            "fork",
            "--all 019f3042-1234-7000-8000-123456789abc \"try this\"",
            "/repo",
            &settings,
        )
        .unwrap();

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.contains(&"fork".to_string()));
        assert!(has_addness_developer_instructions(&args));
        assert!(has_addness_memory_defaults(&args));
        assert!(args.contains(&"--all".to_string()));
        assert!(args.contains(&"019f3042-1234-7000-8000-123456789abc".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("try this"));
    }

    #[test]
    fn codex_fork_args_include_root_interactive_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::High;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.web_search = true;
        settings.image_paths.push("/tmp/shot.png".to_string());

        let args = codex_fork_args(Some("session-1"), false, true, "branch", "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/shot.png"]));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"high\"")
        );
        assert!(has_addness_developer_instructions(&args));
        assert!(has_addness_memory_defaults(&args));
        assert!(args.contains(&"fork".to_string()));
        assert!(args.contains(&"--all".to_string()));
        assert!(args.contains(&"session-1".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("branch"));

        let last_args = codex_fork_args(None, true, false, "", "/repo", &settings);
        assert!(last_args.contains(&"--last".to_string()));
        assert!(!last_args.contains(&"--all".to_string()));
        assert!(has_addness_developer_instructions(&last_args));
        assert!(has_addness_memory_defaults(&last_args));
    }

    #[test]
    fn codex_session_admin_args_include_root_interactive_settings() {
        let mut settings = CodexExecSettings::default();
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.model = CodexModelChoice::Gpt5;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings
            .config_overrides
            .push("features.foo=true".to_string());

        let args = codex_session_admin_args(
            "delete",
            "019f3042-1234-7000-8000-123456789abc",
            true,
            vec!["--dry-run".to_string()],
            "/repo",
            &settings,
        );

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(args.contains(&"delete".to_string()));
        assert!(args.contains(&"--force".to_string()));
        assert!(args.contains(&"019f3042-1234-7000-8000-123456789abc".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("--dry-run"));
        assert_eq!(codex_command_category(&args), "session");
    }

    #[test]
    fn codex_exec_review_args_include_exec_review_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::High;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.strict_config = true;
        settings.ignore_rules = true;
        settings.output_schema = Some("/tmp/schema.json".to_string());

        let args = codex_exec_review_args("--uncommitted --title WIP", "/repo", &settings).unwrap();

        assert!(args.windows(2).any(|pair| pair == ["-a", "on-request"]));
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(
            args.windows(3)
                .any(|triple| triple == ["exec", "review", "--json"])
        );
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"high\"")
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-schema", "/tmp/schema.json"])
        );
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(args.contains(&"--uncommitted".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--title", "WIP"]));
        assert_eq!(codex_command_category(&args), "agent");
    }

    #[test]
    fn codex_review_args_include_root_review_settings() {
        let mut settings = CodexExecSettings::default();
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.model = CodexModelChoice::Gpt5;
        settings.strict_config = true;
        settings
            .config_overrides
            .push("features.foo=true".to_string());

        let args = codex_review_args("--base main --title Check", "/repo", &settings).unwrap();

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-C", "/repo"]));
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(args.contains(&"review".to_string()));
        assert!(
            args.iter()
                .any(|arg| arg.starts_with("developer_instructions="))
        );
        assert!(args.windows(2).any(|pair| pair == ["--base", "main"]));
        assert!(args.windows(2).any(|pair| pair == ["--title", "Check"]));
        assert_eq!(codex_command_category(&args), "workspace");
    }

    #[test]
    fn codex_apply_args_include_apply_settings() {
        let mut settings = CodexExecSettings::default();
        settings
            .config_overrides
            .push("features.foo=true".to_string());
        settings.enabled_features.push("responses_api".to_string());
        settings.disabled_features.push("legacy_mode".to_string());

        let args = codex_apply_args("task-1", &settings).unwrap();

        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--enable", "responses_api"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable", "legacy_mode"])
        );
        assert!(args.contains(&"apply".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("task-1"));
        assert_eq!(codex_command_category(&args), "workspace");

        let err = codex_apply_args("", &settings).unwrap_err();
        assert!(err.to_string().contains("task id"));
    }

    #[test]
    fn codex_exec_args_include_selected_exec_settings() {
        let mut settings = CodexExecSettings::default();
        settings.model = CodexModelChoice::Gpt5;
        settings.reasoning = CodexReasoningChoice::High;
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        let args = codex_exec_args(None, "/repo", &settings);

        assert!(
            args.windows(2)
                .next()
                .is_some_and(|pair| pair == ["-a", "on-request"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-5"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
        assert!(
            args.iter()
                .any(|arg| arg == "model_reasoning_effort=\"high\"")
        );
    }

    #[test]
    fn codex_exec_args_include_custom_model_override() {
        let mut settings = CodexExecSettings::default();
        settings.model_override = Some("gpt-custom".to_string());

        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
    }

    #[test]
    fn codex_exec_args_include_advanced_codex_cli_options() {
        let mut settings = CodexExecSettings::default();
        settings.web_search = true;
        settings.oss = true;
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.remote_auth_token_env = Some("CODEX_REMOTE_TOKEN".to_string());
        settings.no_alt_screen = true;
        settings.local_provider = CodexLocalProviderChoice::Ollama;
        settings.profile = Some("work".to_string());
        settings.additional_dirs.push("/tmp/extra".to_string());
        settings.image_paths.push("/tmp/shot.png".to_string());
        settings
            .config_overrides
            .push("features.foo=true".to_string());
        settings.enabled_features.push("responses_api".to_string());
        settings.disabled_features.push("legacy_mode".to_string());
        settings.strict_config = true;
        settings.ignore_user_config = true;
        settings.ignore_rules = true;
        settings.skip_git_repo_check = true;
        settings.ephemeral = true;
        settings.bypass_hook_trust = true;
        settings.color = CodexColorChoice::Always;
        settings.output_schema = Some("/tmp/schema.json".to_string());
        settings.output_last_message = Some("/tmp/last.txt".to_string());
        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.contains(&"--search".to_string()));
        assert!(args.contains(&"--oss".to_string()));
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(args.contains(&"--dangerously-bypass-hook-trust".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--color", "always"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote-auth-token-env", "CODEX_REMOTE_TOKEN"])
        );
        assert!(args.contains(&"--no-alt-screen".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--local-provider", "ollama"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-p", "work"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--add-dir", "/tmp/extra"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-i", "/tmp/shot.png"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--enable", "responses_api"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable", "legacy_mode"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-schema", "/tmp/schema.json"])
        );
        assert!(args.windows(2).any(|pair| pair == ["-o", "/tmp/last.txt"]));
        assert!(args.contains(&"--ignore-user-config".to_string()));
        assert!(args.contains(&"--ignore-rules".to_string()));
        assert!(args.contains(&"--skip-git-repo-check".to_string()));
        assert!(args.contains(&"--ephemeral".to_string()));
    }

    #[test]
    fn codex_exec_args_bypass_omits_approval_and_sandbox_flags() {
        let mut settings = CodexExecSettings::default();
        settings.approval = CodexApprovalChoice::OnRequest;
        settings.sandbox = CodexSandboxChoice::ReadOnly;
        settings.bypass_approvals_and_sandbox = true;

        let args = codex_exec_args(None, "/repo", &settings);

        assert!(args.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(!args.contains(&"-a".to_string()));
        assert!(!args.contains(&"-s".to_string()));
    }

    #[test]
    fn split_codex_command_args_handles_quotes_and_escapes() {
        let args = split_codex_command_args(r#"review --title "hello world" a\ b"#).unwrap();

        assert_eq!(args, vec!["review", "--title", "hello world", "a b"]);
    }

    #[test]
    fn split_codex_command_args_rejects_unclosed_quotes() {
        let err = split_codex_command_args(r#"doctor "oops"#).unwrap_err();

        assert!(err.to_string().contains("引用符"));
    }

    #[test]
    fn codex_named_subcommand_args_cover_common_codex_commands() {
        assert_eq!(
            codex_named_subcommand_args("doctor", "--help").unwrap(),
            vec!["doctor", "--help"]
        );
        assert_eq!(
            codex_named_subcommand_args("features", "").unwrap(),
            vec!["features", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("mcp", "").unwrap(),
            vec!["mcp", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("plugin", "").unwrap(),
            vec!["plugin", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("cloud", "").unwrap(),
            vec!["cloud", "list"]
        );
        assert_eq!(
            codex_named_subcommand_args("login", "").unwrap(),
            vec!["login", "status"]
        );
        assert_eq!(
            codex_named_subcommand_args("app-server", "").unwrap(),
            vec!["app-server", "daemon", "version"]
        );
        assert_eq!(
            codex_named_subcommand_args("mcp-server", "").unwrap(),
            vec!["mcp-server", "--help"]
        );
        assert_eq!(
            codex_named_subcommand_args("exec-server", "").unwrap(),
            vec!["exec-server", "--help"]
        );
        assert!(
            codex_named_subcommand_args("codex-help", "mcp")
                .err()
                .is_some()
        );
        assert_eq!(
            codex_named_subcommand_args("help", "mcp").unwrap(),
            vec!["help", "mcp"]
        );
        assert_eq!(
            codex_named_subcommand_args("version", "").unwrap(),
            vec!["--version"]
        );
        assert_eq!(
            codex_named_subcommand_args("review", "--uncommitted").unwrap(),
            vec!["review", "--uncommitted"]
        );
        assert_eq!(
            codex_named_subcommand_args("exec-review", "--uncommitted").unwrap(),
            vec!["exec", "review", "--json", "--uncommitted"]
        );
        assert_eq!(
            codex_named_subcommand_args("apply", "task-1").unwrap(),
            vec!["apply", "task-1"]
        );
    }

    #[test]
    fn codex_command_category_labels_management_commands() {
        assert_eq!(codex_command_category(&["login".to_string()]), "auth");
        assert_eq!(codex_command_category(&["cloud".to_string()]), "cloud");
        assert_eq!(
            codex_command_category(&["app-server".to_string()]),
            "server"
        );
        assert_eq!(codex_command_category(&["mcp".to_string()]), "config");
        assert_eq!(codex_command_category(&["delete".to_string()]), "session");
        assert_eq!(
            codex_command_category(&[
                "-a".to_string(),
                "on-request".to_string(),
                "--no-alt-screen".to_string(),
                "fork".to_string(),
            ]),
            "session"
        );
    }

    #[test]
    fn command_preview_collapses_addness_developer_instructions() {
        let preview = command_preview(&[
            "-c".to_string(),
            codex_config_arg(
                CodexConfigKey::DeveloperInstructions,
                addness_tui_developer_instructions(),
            ),
            "fork".to_string(),
            "--last".to_string(),
        ]);

        assert!(preview.contains("developer_instructions=<Addness DB>"));
        assert!(preview.contains("fork --last"));
        assert!(!preview.contains("通常Codexと同じ速度"));
    }

    #[test]
    fn codex_named_subcommand_args_requires_apply_task_id() {
        let err = codex_named_subcommand_args("apply", "").unwrap_err();

        assert!(err.to_string().contains("task id"));
    }

    #[test]
    fn codex_named_subcommand_args_with_settings_prefixes_global_config() {
        let mut settings = CodexExecSettings::default();
        settings.remote_addr = Some("ws://127.0.0.1:7777".to_string());
        settings.strict_config = true;
        settings
            .config_overrides
            .push("features.foo=true".to_string());
        settings.enabled_features.push("responses_api".to_string());
        settings.disabled_features.push("legacy_mode".to_string());

        let args =
            codex_named_subcommand_args_with_settings(vec!["mcp".into(), "list".into()], &settings);

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--remote", "ws://127.0.0.1:7777"])
        );
        assert!(args.contains(&"--strict-config".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-c", "features.foo=true"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--enable", "responses_api"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--disable", "legacy_mode"])
        );
        assert_eq!(codex_command_category(&args), "config");

        let version_args =
            codex_named_subcommand_args_with_settings(vec!["--version".into()], &settings);
        assert_eq!(version_args, vec!["--version".to_string()]);
    }

    #[test]
    fn codex_arbitrary_agent_subcommands_keep_addness_db_contract() {
        let settings = CodexExecSettings::default();

        for raw in [
            vec!["exec".to_string(), "--json".to_string()],
            vec!["review".to_string(), "--uncommitted".to_string()],
            vec!["resume".to_string(), "--last".to_string()],
            vec!["fork".to_string(), "--last".to_string()],
        ] {
            let args = codex_named_subcommand_args_with_settings(raw, &settings);
            assert!(has_addness_memory_defaults(&args), "{args:?}");
            assert!(has_addness_developer_instructions(&args), "{args:?}");
        }

        let config_args =
            codex_named_subcommand_args_with_settings(vec!["mcp".into(), "list".into()], &settings);
        assert!(has_addness_memory_defaults(&config_args));
        assert!(!has_addness_developer_instructions(&config_args));

        let version_args =
            codex_named_subcommand_args_with_settings(vec!["--version".into()], &settings);
        assert_eq!(version_args, vec!["--version".to_string()]);
    }

    #[test]
    fn parse_codex_session_meta_line_extracts_picker_fields() {
        let session = parse_codex_session_meta_line(
            r#"{"timestamp":"2026-07-05T01:00:00Z","type":"session_meta","payload":{"id":"019f3042-1234-7000-8000-123456789abc","timestamp":"2026-07-05T00:59:00Z","cwd":"/repo","thread_name":"作業メモ"}}"#,
        )
        .unwrap();

        assert_eq!(session.id, "019f3042-1234-7000-8000-123456789abc");
        assert_eq!(session.title, "作業メモ");
        assert_eq!(session.updated_at, "2026-07-05T00:59:00Z");
        assert_eq!(session.cwd.as_deref(), Some("/repo"));
    }

    #[test]
    fn load_codex_session_candidates_merges_index_and_session_meta() {
        let root = std::env::temp_dir().join(format!(
            "addness-codex-index-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let sessions_dir = root.join("sessions").join("2026").join("07").join("05");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        std::fs::write(
            root.join("session_index.jsonl"),
            r#"{"id":"019f3042-1234-7000-8000-123456789abc","thread_name":"index title","updated_at":"2026-07-05T01:02:00Z"}"#,
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join("rollout.jsonl"),
            r#"{"timestamp":"2026-07-05T01:00:00Z","type":"session_meta","payload":{"id":"019f3042-1234-7000-8000-123456789abc","timestamp":"2026-07-05T01:00:00Z","cwd":"/repo","thread_name":"meta title"}}"#,
        )
        .unwrap();

        let sessions = load_codex_session_candidates_from(&root, 10).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].title, "index title");
        assert_eq!(sessions[0].updated_at, "2026-07-05T01:02:00Z");
        assert_eq!(sessions[0].cwd.as_deref(), Some("/repo"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn session_reference_helpers_handle_numbers_and_uuids() {
        assert_eq!(parse_one_based_index("1"), Some(0));
        assert!(looks_like_uuid("019f3042-1234-7000-8000-123456789abc"));
        assert!(!looks_like_uuid("session-name"));
        assert_eq!(
            split_first_arg("1 continue from here"),
            Some(("1".to_string(), "continue from here".to_string()))
        );
    }

    #[test]
    fn settings_slash_commands_update_local_state_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/model");
        submit_line(&mut pane, "/reasoning");
        submit_line(&mut pane, "/approval");
        submit_line(&mut pane, "/sandbox");
        submit_line(&mut pane, "/settings");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(
            pane.settings_label(),
            "model:gpt-5.5 effort:low approval:untrusted sandbox:danger-full-access config:2"
        );
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::System && line.text.contains("Settings:"))
        );
    }

    #[test]
    fn exec_slash_requires_prompt_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/exec");
        submit_line(&mut pane, "/e");

        assert_eq!(pane.turn_count(), 0);
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::Error && line.text.contains("/exec には prompt")
        }));
    }

    #[test]
    fn slash_help_text_groups_codex_commands_and_tui_helpers() {
        let text = slash_help_text();

        assert!(text.contains("Codex CLI commands:"));
        assert!(text.contains("/codex-help [command]"));
        assert!(text.contains("/new - start the next prompt"));
        assert!(text.contains("/clear - clear the visible Codex log"));
        assert!(text.contains("/init [notes]"));
        assert!(text.contains("/ide - show IDE context"));
        assert!(text.contains("/compact [notes]"));
        assert!(text.contains("/import [status|run]"));
        assert!(text.contains("/hooks [key=value|clear]"));
        assert!(text.contains("/skills [list|name]"));
        assert!(text.contains("/exec|/e <prompt>"));
        assert!(text.contains("/sandbox-run <args>"));
        assert!(text.contains("/features|/experimental"));
        assert!(text.contains("/color [never|auto|always]"));
        assert!(text.contains("/approval|/approvals [policy]"));
        assert!(text.contains("Codex sessions:"));
        assert!(text.contains("/codex-resume <args> - root codex resume"));
        assert!(text.contains("/resume is a TUI helper"));
        assert!(text.contains("/side [prompt]"));
        assert!(text.contains("/rename <title>"));
        assert!(text.contains("Codex options for next turn:"));
        assert!(text.contains("/permissions [approval <policy>|sandbox <mode>|bypass]"));
        assert!(text.contains("/personality [friendly|pragmatic|clear]"));
        assert!(text.contains("/statusline [items|colors on|off|clear]"));
        assert!(text.contains("/theme [name|clear]"));
        assert!(text.contains("/keymap [key=value|clear]"));
        assert!(text.contains("/memories [status|off|clear]"));
        assert!(text.contains("Addness DB fixed memory"));
        assert!(text.contains("/sandbox-add-read-dir <path>"));
        assert!(text.contains("/attachments [list|clear|image <path>|add-dir <path>]"));
        assert!(text.contains("TUI helpers:"));
        assert!(text.contains("/organize|/team [task]"));
        assert!(text.contains("/work [next|all|N|id|title]"));
        assert!(text.contains("/remember|/memo <内容>"));
        assert!(text.contains("/handoff [メモ]"));
        assert!(text.contains("/rollout"));
        assert!(text.contains("/debug-config"));
        assert!(text.contains("/ps"));
        assert!(text.contains("/btw"));
        assert!(text.contains("/turn [picker|N|all|old|close N|toggle N]"));
        assert!(text.contains("/feedback [message]"));
        assert!(text.contains("/test-approval [message]"));
        assert!(text.contains("/usage"));
    }

    #[test]
    fn token_usage_summary_extracts_event_msg_token_count() {
        let value = serde_json::json!({
            "type": "event_msg",
            "payload": {
                "type": "token_count",
                "info": {
                    "total_token_usage": {
                        "input_tokens": 325860030_u64,
                        "cached_input_tokens": 314921088_u64,
                        "output_tokens": 774156_u64,
                        "reasoning_output_tokens": 267102_u64,
                        "total_tokens": 326634186_u64
                    },
                    "last_token_usage": {
                        "input_tokens": 126457_u64,
                        "cached_input_tokens": 123264_u64,
                        "output_tokens": 239_u64,
                        "reasoning_output_tokens": 16_u64,
                        "total_tokens": 126696_u64
                    },
                    "model_context_window": 258400_u64
                }
            }
        });

        let summary = token_usage_summary(&value).unwrap();

        assert!(summary.contains("last total=126,696"));
        assert!(summary.contains("session total=326,634,186"));
        assert!(summary.contains("context=258,400"));
    }

    #[test]
    fn usage_slash_shows_last_token_usage_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        pane.handle_stdout_line(
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":325860030,"cached_input_tokens":314921088,"output_tokens":774156,"reasoning_output_tokens":267102,"total_tokens":326634186},"last_token_usage":{"input_tokens":126457,"cached_input_tokens":123264,"output_tokens":239,"reasoning_output_tokens":16,"total_tokens":126696},"model_context_window":258400}}}"#,
        );
        submit_line(&mut pane, "/usage");

        assert_eq!(pane.turn_count(), 0);
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System
                && line.text.contains("トークン:")
                && line.text.contains("last total=126,696")
                && line.text.contains("context=258,400")
        }));
    }

    #[test]
    fn normal_tui_config_slashes_update_config_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/theme ansi");
        submit_line(&mut pane, "/pets hide");
        submit_line(&mut pane, "/pets anchor screen-bottom");
        submit_line(&mut pane, "/vim on");
        submit_line(&mut pane, "/raw on");
        submit_line(&mut pane, r#"/keymap global.copy=["ctrl-y"]"#);
        submit_line(&mut pane, "/memories off");
        submit_line(&mut pane, "/sandbox-add-read-dir /tmp/readable");
        submit_line(&mut pane, "/setup-default-sandbox");
        submit_line(&mut pane, "/attachments image /tmp/shot.png");
        submit_line(&mut pane, "/attachments add-dir /tmp/extra");

        assert_eq!(pane.turn_count(), 0);
        for expected in [
            r#"tui.theme="ansi""#,
            r#"tui.pet="none""#,
            r#"tui.pet_anchor="screen-bottom""#,
            "tui.vim_mode_default=true",
            "tui.raw_output_mode=true",
            r#"tui.keymap.global.copy=["ctrl-y"]"#,
            "memories.use_memories=false",
            "memories.generate_memories=false",
        ] {
            assert!(
                pane.exec_settings
                    .config_overrides
                    .contains(&expected.to_string())
            );
        }
        assert_eq!(pane.exec_settings.approval, CodexApprovalChoice::OnRequest);
        assert_eq!(
            pane.exec_settings.sandbox,
            CodexSandboxChoice::WorkspaceWrite
        );
        assert!(
            pane.exec_settings
                .image_paths
                .contains(&"/tmp/shot.png".to_string())
        );
        assert!(
            pane.exec_settings
                .additional_dirs
                .contains(&"/tmp/readable".to_string())
        );
        assert!(
            pane.exec_settings
                .additional_dirs
                .contains(&"/tmp/extra".to_string())
        );
    }

    #[test]
    fn remember_slash_queues_addness_body_memory_prompt_when_running() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        submit_line(
            &mut pane,
            "/remember このプロジェクトではAddnessをDBとして使う",
        );

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.queued_prompts.len(), 1);
        let queued = pane.queued_prompts.front().unwrap();
        assert!(!queued.apply_goal_mode);
        assert!(queued.display_prompt.contains("Addnessに記憶"));
        assert!(queued.submitted.contains("## Codex作業メモ"));
        assert!(queued.submitted.contains("子ゴールを作成または更新"));
        assert!(
            queued
                .submitted
                .contains("Codex global memory には保存しない")
        );
        assert!(queued.submitted.contains("AddnessをDBとして使う"));
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::User && line.text.contains("Addnessに記憶")
        }));
    }

    #[test]
    fn handoff_slash_queues_addness_handoff_prompt_when_running() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;
        pane.push_log(CodexLogKind::Assistant, "承認UIとAddness記憶導線を改善した");
        pane.push_log(
            CodexLogKind::Tool,
            "OK cargo test\ntest result: ok. 224 passed".to_string(),
        );

        submit_line(&mut pane, "/handoff 次回は実TUIで目視確認する");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.queued_prompts.len(), 1);
        let queued = pane.queued_prompts.front().unwrap();
        assert!(!queued.apply_goal_mode);
        assert!(queued.display_prompt.contains("Addnessに引き継ぎ保存"));
        assert!(
            queued
                .submitted
                .contains("このCodexセッションの引き継ぎ点を保存")
        );
        assert!(queued.submitted.contains("## Codex作業メモ"));
        assert!(queued.submitted.contains("## Codex決定ログ"));
        assert!(queued.submitted.contains("子ゴールを作成または更新"));
        assert!(
            queued
                .submitted
                .contains("Codex global memory には保存しない")
        );
        assert!(queued.submitted.contains("次回は実TUIで目視確認する"));
        assert!(
            queued
                .submitted
                .contains("承認UIとAddness記憶導線を改善した")
        );
        assert!(queued.submitted.contains("test result: ok. 224 passed"));
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::User && line.text.contains("Addnessに引き継ぎ保存")
        }));
    }

    #[test]
    fn organize_slash_queues_addness_work_breakdown_prompt_when_running() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        submit_line(&mut pane, "/organize dashboard UX and subgoal delegation");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.queued_prompts.len(), 1);
        assert_eq!(pane.action.as_deref(), Some("作業分解を予約 1件"));
        let queued = pane.queued_prompts.front().unwrap();
        assert!(!queued.apply_goal_mode);
        assert!(
            queued
                .display_prompt
                .contains("Addnessで作業分解: dashboard UX and subgoal delegation")
        );
        assert!(queued.submitted.contains("Addnessを作業DBとして使い"));
        assert!(
            queued
                .submitted
                .contains("Addness TUI は誰でも `addness` と打てば起動")
        );
        assert!(
            queued
                .submitted
                .contains("Addness CLI で goal body/description/子ゴール")
        );
        assert!(
            queued
                .submitted
                .contains("title=作業名、description=完了状態、body=入力情報")
        );
        assert!(queued.submitted.contains("サブエージェント/並列作業ツール"));
        assert!(queued.submitted.contains("分解だけで終わらず"));
    }

    #[test]
    fn work_slash_lists_child_goal_work_packages_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.update_children(vec![ChildGoalUpdate {
            id: "child-1".to_string(),
            title: "承認UIを整理".to_string(),
            description: Some("何を承認するか一目で分かる状態".to_string()),
            icon: "[~]",
            status_label: "進行中".to_string(),
            is_completed: false,
        }]);

        submit_line(&mut pane, "/work");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.action.as_deref(), Some("子ゴール一覧を表示"));
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System
                && line.text.contains("1. [~] 承認UIを整理")
                && line.text.contains("/work next")
                && line.text.contains("/work all")
                && line.text.contains("DoD: 何を承認するか")
        }));
    }

    #[test]
    fn work_slash_queues_next_child_goal_as_implementation_package() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;
        pane.update_children(vec![
            ChildGoalUpdate {
                id: "child-done".to_string(),
                title: "完了済み".to_string(),
                description: Some("完了している".to_string()),
                icon: "[x]",
                status_label: "完了".to_string(),
                is_completed: true,
            },
            ChildGoalUpdate {
                id: "child-work".to_string(),
                title: "子ゴール着手導線".to_string(),
                description: Some("/work next で実装に入れる状態".to_string()),
                icon: "[ ]",
                status_label: "未着手".to_string(),
                is_completed: false,
            },
        ]);

        submit_line(&mut pane, "/work next");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.queued_prompts.len(), 1);
        assert_eq!(pane.action.as_deref(), Some("子ゴール着手を予約 1件"));
        assert_eq!(
            pane.active_work_package_label().as_deref(),
            Some("#2 子ゴール着手導線")
        );
        assert!(pane.child_goal_is_active(&pane.children[1]));
        let queued = pane.queued_prompts.front().unwrap();
        assert!(!queued.apply_goal_mode);
        assert!(
            queued
                .display_prompt
                .contains("子ゴール着手 #2: 子ゴール着手導線")
        );
        assert!(queued.submitted.contains("実装ワークパッケージ"));
        assert!(queued.submitted.contains("id: child-work"));
        assert!(queued.submitted.contains("/work next で実装に入れる状態"));
        assert!(
            queued
                .submitted
                .contains("\"$ADDNESS_BIN\" goal get child-work")
        );
        assert!(
            queued
                .submitted
                .contains("goal update child-work --description-file")
        );
        assert!(queued.submitted.contains("実装または検証に着手"));
    }

    #[test]
    fn work_all_queues_each_incomplete_child_goal_as_work_package() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;
        pane.update_children(vec![
            ChildGoalUpdate {
                id: "child-done".to_string(),
                title: "完了済み".to_string(),
                description: Some("完了している".to_string()),
                icon: "[x]",
                status_label: "完了".to_string(),
                is_completed: true,
            },
            ChildGoalUpdate {
                id: "child-a".to_string(),
                title: "承認UI".to_string(),
                description: Some("確認内容が明確".to_string()),
                icon: "[ ]",
                status_label: "未着手".to_string(),
                is_completed: false,
            },
            ChildGoalUpdate {
                id: "child-b".to_string(),
                title: "ログ要約".to_string(),
                description: Some("内部イベントが見えない".to_string()),
                icon: "[~]",
                status_label: "進行中".to_string(),
                is_completed: false,
            },
        ]);

        submit_line(&mut pane, "/work all");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.queued_prompts.len(), 2);
        assert_eq!(pane.action.as_deref(), Some("子ゴール一括着手を予約 2件"));
        assert_eq!(
            pane.queued_work_package_label().as_deref(),
            Some("待機2件: #2 承認UI / #3 ログ要約")
        );
        assert!(pane.active_work_package_label().is_none());
        assert_eq!(
            pane.queued_prompts[0]
                .active_work
                .as_ref()
                .map(|work| (work.ordinal, work.id.as_str())),
            Some((2, "child-a"))
        );
        assert_eq!(
            pane.queued_prompts[1]
                .active_work
                .as_ref()
                .map(|work| (work.ordinal, work.id.as_str())),
            Some((3, "child-b"))
        );
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System
                && line.text.contains("未完了子ゴール 2件をワークキューに追加")
        }));
    }

    #[test]
    fn active_work_package_follows_child_goal_refresh() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;
        pane.update_children(vec![ChildGoalUpdate {
            id: "child-work".to_string(),
            title: "古いタイトル".to_string(),
            description: Some("着手対象".to_string()),
            icon: "[ ]",
            status_label: "未着手".to_string(),
            is_completed: false,
        }]);
        submit_line(&mut pane, "/work 1");
        assert_eq!(
            pane.active_work_package_label().as_deref(),
            Some("#1 古いタイトル")
        );

        pane.update_children(vec![
            ChildGoalUpdate {
                id: "child-other".to_string(),
                title: "先に入った子ゴール".to_string(),
                description: None,
                icon: "[ ]",
                status_label: "未着手".to_string(),
                is_completed: false,
            },
            ChildGoalUpdate {
                id: "child-work".to_string(),
                title: "新しいタイトル".to_string(),
                description: Some("着手対象".to_string()),
                icon: "[~]",
                status_label: "進行中".to_string(),
                is_completed: false,
            },
        ]);

        assert_eq!(
            pane.active_work_package_label().as_deref(),
            Some("#2 新しいタイトル")
        );
        assert!(pane.child_goal_is_active(&pane.children[1]));
    }

    #[test]
    fn memories_clear_restores_addness_safe_default() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        pane.exec_settings.config_overrides = vec![
            "memories.use_memories=true".to_string(),
            "memories.generate_memories=true".to_string(),
        ];

        submit_line(&mut pane, "/memories clear");

        assert_eq!(
            pane.config_override_value_for("memories.use_memories"),
            Some("false")
        );
        assert_eq!(
            pane.config_override_value_for("memories.generate_memories"),
            Some("false")
        );
    }

    #[test]
    fn memories_on_is_rejected_and_keeps_addness_db_state_explicit() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        assert_eq!(pane.memory_mode_label(), "Addness DB / Codex memory off");
        assert!(pane.memory_mode_is_addness_safe());

        submit_line(&mut pane, "/memories on");

        assert_eq!(pane.memory_mode_label(), "Addness DB / Codex memory off");
        assert!(pane.memory_mode_is_addness_safe());
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::Error
                && line
                    .text
                    .contains("通常Codexのglobal memoryを有効化しません")
        }));
    }

    #[test]
    fn config_slash_cannot_enable_global_memory() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/config memories.use_memories=true");
        submit_line(&mut pane, "/config memories={use_memories=true}");

        assert_eq!(
            pane.config_override_value_for("memories.use_memories"),
            Some("false")
        );
        assert_eq!(
            pane.config_override_value_for("memories.generate_memories"),
            Some("false")
        );
        assert!(pane.memory_mode_is_addness_safe());
        assert!(
            !pane
                .exec_settings
                .config_overrides
                .iter()
                .any(|entry| entry.contains("memories.use_memories=true"))
        );
    }

    #[test]
    fn one_shot_approval_override_does_not_mutate_persistent_settings() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.exec_settings.approval = CodexApprovalChoice::OnRequest;
        pane.one_shot_approval = Some(CodexApprovalChoice::Never);

        let settings = pane.exec_settings_for_spawn();

        assert_eq!(settings.approval, CodexApprovalChoice::Never);
        assert_eq!(pane.exec_settings.approval, CodexApprovalChoice::OnRequest);
    }

    #[test]
    fn local_status_slashes_render_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.push_log(CodexLogKind::Assistant, "## Result\nDone");

        submit_line(&mut pane, "/btw");
        submit_line(&mut pane, "/ps");
        submit_line(&mut pane, "/rollout");
        submit_line(&mut pane, "/debug-config");
        submit_line(&mut pane, "/skills");
        submit_line(&mut pane, "/import status");
        submit_line(&mut pane, "/hooks");
        submit_line(&mut pane, "/ide");
        submit_line(&mut pane, "/feedback broken thing");

        assert_eq!(pane.turn_count(), 0);
        for expected in [
            "直近の返答",
            "実行状況:",
            "セッション保存:",
            "設定詳細:",
            "Skills:",
            "Import candidates:",
            "Hook overrides",
            "IDE context:",
            "フィードバック下書き:",
        ] {
            assert!(
                pane.log.iter().any(|line| {
                    line.kind == CodexLogKind::System && line.text.contains(expected)
                })
            );
        }
    }

    #[test]
    fn turn_slash_opens_collapsed_turn_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.test_add_completed_turn("first response");
        pane.test_add_completed_turn("second response");
        pane.toggle_old_turns_collapsed();

        assert_eq!(pane.collapsed_turn_count(), 2);

        submit_line(&mut pane, "/turn 1");

        assert_eq!(pane.turn_count(), 2);
        assert_eq!(pane.collapsed_turn_count(), 1);
        assert!(!pane.collapsed_turns.contains(&1));
        assert!(pane.collapsed_turns.contains(&2));
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System && line.text.contains("Turn 1 を展開")
        }));
    }

    #[test]
    fn turn_slash_opens_collapsed_turn_immediately_while_running() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.test_add_completed_turn("first response");
        pane.test_add_completed_turn("second response");
        pane.toggle_old_turns_collapsed();
        pane.turn_running = true;

        submit_line(&mut pane, "/turn 1");

        assert!(pane.is_turn_running());
        assert_eq!(pane.queued_prompt_count(), 0);
        assert_eq!(pane.collapsed_turn_count(), 1);
        assert!(!pane.collapsed_turns.contains(&1));
        assert!(pane.collapsed_turns.contains(&2));
    }

    #[test]
    fn turn_slash_accepts_multi_digit_turn_numbers() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        for idx in 1..=10 {
            pane.test_add_completed_turn(&format!("response {idx}"));
        }
        pane.toggle_old_turns_collapsed();

        submit_line(&mut pane, "/turn 10");

        assert_eq!(pane.turn_count(), 10);
        assert_eq!(pane.collapsed_turn_count(), 9);
        assert!(!pane.collapsed_turns.contains(&10));
        assert!(pane.collapsed_turns.contains(&1));
    }

    #[test]
    fn turn_picker_slash_opens_explicit_turn_panel() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.test_add_completed_turn("first response");
        pane.test_add_completed_turn("second response");
        pane.toggle_old_turns_collapsed();

        submit_line(&mut pane, "/turn picker");

        assert!(pane.turn_picker_open());
        assert_eq!(pane.turn_picker_selected_turn(), Some(1));
        assert_eq!(pane.turn_picker_items().len(), 2);
    }

    #[test]
    fn test_approval_slash_sets_pending_decision_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/test-approval check this");

        assert_eq!(pane.turn_count(), 0);
        assert!(pane.pending_decision.as_ref().is_some_and(|decision| {
            decision.kind == CodexDecisionKind::Permission
                && decision.message.contains("check this")
                && decision.always_choice().is_some()
        }));
    }

    #[test]
    fn rename_current_thread_appends_codex_session_index() {
        let root = std::env::temp_dir().join(format!(
            "addness-codex-rename-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.thread_id = Some("019f3042-1234-7000-8000-123456789abc".to_string());

        pane.rename_current_thread_with_home("new thread title", Some(&root));

        let sessions = load_codex_session_candidates_from(&root, 10).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "019f3042-1234-7000-8000-123456789abc");
        assert_eq!(sessions[0].title, "new thread title");
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System && line.text.contains("new thread title")
        }));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn personality_and_statusline_slashes_update_config_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/personality pragmatic");
        submit_line(&mut pane, "/statusline model_name directory token_usage");
        submit_line(&mut pane, "/statusline colors on");

        assert_eq!(pane.turn_count(), 0);
        assert!(
            pane.exec_settings
                .config_overrides
                .contains(&r#"personality="pragmatic""#.to_string())
        );
        assert!(pane.exec_settings.config_overrides.contains(
            &r#"tui.status_line=["model_name", "directory", "token_usage"]"#.to_string()
        ));
        assert!(
            pane.exec_settings
                .config_overrides
                .contains(&"tui.status_line_use_colors=true".to_string())
        );
    }

    #[test]
    fn new_slash_starts_next_turn_without_existing_thread() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.thread_id = Some("thread-1".to_string());

        submit_line(&mut pane, "/new");

        assert_eq!(pane.turn_count(), 0);
        assert!(pane.thread_id.is_none());
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System && line.text.contains("新しい Codex セッション")
        }));

        let args = codex_exec_args(pane.thread_id.as_deref(), &pane.cwd, &pane.exec_settings);
        assert!(args.windows(2).any(|pair| pair == ["exec", "--json"]));
        assert!(!args.contains(&"resume".to_string()));
    }

    #[test]
    fn clear_slash_clears_visible_log_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.push_log(CodexLogKind::User, "old prompt");
        pane.push_log(CodexLogKind::Assistant, "old answer");

        submit_line(&mut pane, "/clear");

        assert_eq!(pane.turn_count(), 0);
        assert!(!pane.log.iter().any(|line| line.text.contains("old prompt")));
        assert!(!pane.log.iter().any(|line| line.text.contains("old answer")));
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System && line.text.contains("表示ログをクリア")
        }));
    }

    #[test]
    fn init_slash_queues_agents_initialization_prompt_when_running() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        submit_line(&mut pane, "/init prefer cargo commands");

        assert_eq!(pane.turn_count(), 0);
        assert_eq!(pane.queued_prompts.len(), 1);
        let queued = pane.queued_prompts.front().unwrap();
        assert!(queued.submitted.contains("Initialize this repository"));
        assert!(queued.submitted.contains("AGENTS.md"));
        assert!(queued.submitted.contains("prefer cargo commands"));
    }

    #[test]
    fn side_slash_requires_existing_thread_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/side investigate alternative");

        assert_eq!(pane.turn_count(), 0);
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::Error && line.text.contains("/side はCodexセッション開始後")
        }));
    }

    #[test]
    fn permissions_slash_updates_approval_and_sandbox_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/permissions approval never");
        submit_line(&mut pane, "/permissions sandbox read-only");
        submit_line(&mut pane, "/permissions status");

        assert_eq!(pane.turn_count(), 0);
        let label = pane.settings_label();
        assert!(label.contains("approval:never"));
        assert!(label.contains("sandbox:read-only"));
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System
                && line.text.contains("Permissions:")
                && line.text.contains("approval=never")
                && line.text.contains("sandbox=read-only")
        }));
    }

    #[test]
    fn apply_short_alias_requires_task_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/a");

        assert_eq!(pane.turn_count(), 0);
        assert!(
            pane.log
                .iter()
                .any(|line| { line.kind == CodexLogKind::Error && line.text.contains("task id") })
        );
    }

    #[test]
    fn settings_slash_commands_accept_direct_values() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/model gpt-custom");
        submit_line(&mut pane, "/reasoning high");
        submit_line(&mut pane, "/approvals never");
        submit_line(&mut pane, "/sandbox read-only");

        assert_eq!(pane.turn_count(), 0);
        let label = pane.settings_label();
        assert!(label.contains("model:gpt-custom"));
        assert!(label.contains("effort:high"));
        assert!(label.contains("approval:never"));
        assert!(label.contains("sandbox:read-only"));

        let args = codex_exec_args(None, "/repo", &pane.exec_settings);
        assert!(args.windows(2).any(|pair| pair == ["-m", "gpt-custom"]));
        assert!(args.windows(2).any(|pair| pair == ["-a", "never"]));
        assert!(args.windows(2).any(|pair| pair == ["-s", "read-only"]));
    }

    #[test]
    fn advanced_slash_commands_update_exec_settings_without_starting_turn() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/search");
        submit_line(&mut pane, "/oss");
        submit_line(&mut pane, "/remote ws://127.0.0.1:7777");
        submit_line(&mut pane, "/remote-auth-token-env CODEX_REMOTE_TOKEN");
        submit_line(&mut pane, "/no-alt-screen");
        submit_line(&mut pane, "/local-provider ollama");
        submit_line(&mut pane, "/profile work");
        submit_line(&mut pane, "/image /tmp/shot.png");
        submit_line(&mut pane, "/add-dir /tmp/extra");
        submit_line(&mut pane, "/config features.foo=true");
        submit_line(&mut pane, "/enable responses_api");
        submit_line(&mut pane, "/disable legacy_mode");
        submit_line(&mut pane, "/strict-config");
        submit_line(&mut pane, "/ignore-user-config");
        submit_line(&mut pane, "/ignore-rules");
        submit_line(&mut pane, "/skip-git-check");
        submit_line(&mut pane, "/ephemeral");
        submit_line(&mut pane, "/bypass-hook-trust");
        submit_line(&mut pane, "/color auto");
        submit_line(&mut pane, "/output-schema /tmp/schema.json");
        submit_line(&mut pane, "/output-last-message /tmp/last.txt");

        assert_eq!(pane.turn_count(), 0);
        let label = pane.settings_label();
        assert!(label.contains("search:on"));
        assert!(label.contains("oss:on"));
        assert!(label.contains("remote:ws://127.0.0.1:7777"));
        assert!(label.contains("remote-auth-env:CODEX_REMOTE_TOKEN"));
        assert!(label.contains("no-alt-screen"));
        assert!(label.contains("provider:ollama"));
        assert!(label.contains("profile:work"));
        assert!(label.contains("image:1"));
        assert!(label.contains("add-dir:1"));
        assert!(label.contains("config:3"));
        assert!(label.contains("enable:1"));
        assert!(label.contains("disable:1"));
        assert!(label.contains("strict-config"));
        assert!(label.contains("ignore-user-config"));
        assert!(label.contains("ignore-rules"));
        assert!(label.contains("skip-git-check"));
        assert!(label.contains("ephemeral"));
        assert!(label.contains("bypass-hook-trust"));
        assert!(label.contains("color:auto"));
        assert!(label.contains("output-schema"));
        assert!(label.contains("output-last-message"));
    }

    #[test]
    fn config_clear_keeps_addness_memory_defaults() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;

        submit_line(&mut pane, "/config features.foo=true");
        submit_line(&mut pane, "/config clear");

        assert!(
            !pane
                .exec_settings
                .config_overrides
                .contains(&"features.foo=true".to_string())
        );
        assert_eq!(
            pane.config_override_value_for("memories.use_memories"),
            Some("false")
        );
        assert_eq!(
            pane.config_override_value_for("memories.generate_memories"),
            Some("false")
        );
    }

    #[test]
    fn cwd_slash_command_updates_next_exec_root() {
        let root = std::env::temp_dir().join(format!(
            "addness-codex-cwd-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();

        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.cwd = root.display().to_string();

        submit_line(&mut pane, "/cd nested");

        let expected = nested.canonicalize().unwrap();
        assert_eq!(Path::new(&pane.cwd), expected.as_path());
        let args = codex_exec_args(None, &pane.cwd, &pane.exec_settings);
        assert!(
            args.windows(2)
                .any(|pair| pair == ["-C", expected.to_string_lossy().as_ref()])
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn dod_prompt_lists_items() {
        let prompt = build_dod_assessment_prompt(&["A".to_string(), "B".to_string()]);

        assert!(prompt.contains("0: A"));
        assert!(prompt.contains("1: B"));
    }
}
