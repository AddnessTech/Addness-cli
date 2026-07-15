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
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::config::{AgentLanguage, Settings};

mod claude;
mod claude_resident;
mod codex;
mod codex_appserver;

use self::codex::{
    CodexApprovalChoice, CodexExecSettings, CodexLocalProviderChoice, CodexModelChoice,
    CodexReasoningChoice, CodexSandboxChoice, append_codex_session_rename,
    append_codex_session_rename_to, codex_apply_args, codex_command_category, codex_exec_args,
    codex_exec_resume_args, codex_exec_review_args, codex_fork_args, codex_home_dir,
    codex_named_subcommand_args, codex_named_subcommand_args_with_settings, codex_review_args,
    codex_root_interactive_args, codex_root_resume_args, codex_root_session_command_args,
    codex_session_admin_args, codex_skill_roots, default_addness_memory_config_overrides,
    load_codex_session_candidates, parse_approval_choice, parse_builtin_model_choice,
    parse_color_choice, parse_reasoning_choice, parse_sandbox_choice,
};

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
    ("/undo", "直近ターンのチェックポイントへ戻す"),
    ("/stop", "実行中のターンを中断"),
    ("/sessions", "セッション候補を一覧から選択"),
    ("/resume", "セッションを選んで再開"),
    ("/resume-memo", "Addnessの作業メモ・決定ログから続きを再開"),
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
    ("/lang", "エージェントの応答言語を設定"),
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
    ("/exit", "エージェントペインを終了する"),
    ("/help", "スラッシュコマンド一覧を表示"),
];

/// `SLASH_COMMANDS` のうち ClaudeCode バックエンドでは候補に出さないコマンド名。
///
/// 判定基準は2つ（どちらかに該当する codex CLI 1:1 コマンド）:
/// - `start_codex_subcommand` 経由で codex サブコマンドを起動するもの（ClaudeCode では
///   ガードに引っかかり「codex専用コマンドです」とだけ表示される）
/// - `CodexExecSettings`（codex専用の `-c key=value` オーバーライドや `codex_home` 上の
///   セッション管理ファイル）にのみ作用し、ClaudeCode のターン起動引数
///   （`claude::exec_args` / `ClaudeExecSettings`）には一切反映されないもの
///
/// なお `/image`・`/attachments`・`/fork` は両バックエンドで共通の操作として扱えるよう
/// ここから外してある（画像は `append_claude_image_paths` でプロンプトへ、`/fork` は
/// dispatch で `/fork-last` / `/fork-session` へ委譲する）。
const CODEX_ONLY_SLASH_COMMANDS: &[&str] = &[
    "/review",
    "/apply",
    "/cloud",
    "/apps",
    "/import",
    "/hooks",
    "/codex",
    "/login",
    "/logout",
    "/profile",
    "/mcp",
    "/theme",
    "/vim",
    "/personality",
    "/search",
    "/config",
    "/archive",
    "/rename",
];

/// パレット候補・`/help` の一覧に `name` を出してよいかどうか。
fn slash_command_visible_for_kind(name: &str, kind: AgentKind) -> bool {
    kind != AgentKind::ClaudeCode || !CODEX_ONLY_SLASH_COMMANDS.contains(&name)
}

const CODEX_SESSION_HISTORY_DIR: &str = "codex-sessions";
const CLAUDE_SESSION_HISTORY_DIR: &str = "claude-sessions";
const CODEX_SESSION_HISTORY_MAX_LOG_LINES: usize = 5_000;
const CODEX_SESSION_HISTORY_MAX_RECORDS: usize = 20_000;
const CODEX_SESSION_HISTORY_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// 入力履歴（↑↓で呼び戻す過去プロンプト）の保持上限。
const INPUT_HISTORY_MAX: usize = 200;
/// 常駐 Claude Code の interrupt がグレースフルに完了するのを待つ上限。
/// 超えたら従来どおり kill し、次ターンで `--resume` 再起動する。
const CLAUDE_INTERRUPT_GRACE: Duration = Duration::from_secs(2);
/// 常駐 Claude Code の set_model / set_permission_mode 応答を待つ上限。
/// 超えたら stdin close → 新フラグ + `--resume` で再起動へフォールバックする。
const CLAUDE_SETTING_CHANGE_GRACE: Duration = Duration::from_secs(2);
/// 常駐 Claude Code をアイドル回収するまでの無操作時間（メモリ約300MB対策）。
const CLAUDE_RESIDENT_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// 常駐 codex app-server の interrupt がグレースフルに完了するのを待つ上限。
/// 超えたら kill し、次ターンで thread/resume 再起動する（Claude 側の定数を流用）。
const CODEX_APPSERVER_INTERRUPT_GRACE: Duration = CLAUDE_INTERRUPT_GRACE;
/// 常駐 codex app-server の thread/settings/update 応答を待つ上限。
/// 超えたら再起動 + thread/resume へフォールバックする。
const CODEX_APPSERVER_SETTING_CHANGE_GRACE: Duration = CLAUDE_SETTING_CHANGE_GRACE;
/// 常駐 codex app-server をアイドル回収するまでの無操作時間（RSS 約38MB だが Claude 側と揃える）。
const CODEX_APPSERVER_IDLE_TIMEOUT: Duration = CLAUDE_RESIDENT_IDLE_TIMEOUT;
/// 常駐 codex app-server のハンドシェイク（initialize / thread/start・resume）および
/// turn/start 応答を待つ上限。codex 0.142.5 が想定外の応答形を返して固まった場合の保険で、
/// 超えたらワンショットへフォールバックして「考え中」フリーズを回避する。
const CODEX_APPSERVER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
/// 実行中コマンドのライブ出力バッファに保持する末尾行数。
const CODEX_APPSERVER_OUTPUT_TAIL_LINES: usize = 3;
/// @メンションのファイル候補を一度に表示する最大件数。
const MENTION_PALETTE_MAX: usize = 10;

/// TUI が起動するエージェントバックエンドの種別。
/// バックエンド分岐（起動引数・イベントパース・セッション探索・表示名）で match する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Codex,
    ClaudeCode,
}

impl AgentKind {
    /// 小文字ラベル（ステータス行・アクティビティログの文中で使う）。
    pub fn label(self) -> &'static str {
        match self {
            AgentKind::Codex => "codex",
            AgentKind::ClaudeCode => "claude code",
        }
    }

    /// 表示名（見出し・パネルタイトル用）。
    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::Codex => "Codex",
            AgentKind::ClaudeCode => "Claude Code",
        }
    }

    /// 子プロセスへ渡す `ADDNESS_TUI_BACKEND` の値。
    fn backend_env_value(self) -> &'static str {
        match self {
            AgentKind::Codex => "codex",
            AgentKind::ClaudeCode => "claude",
        }
    }

    /// セッションログの保存先ディレクトリ名（`~/.addness/<dir>/`）。
    fn session_history_dir(self) -> &'static str {
        match self {
            AgentKind::Codex => CODEX_SESSION_HISTORY_DIR,
            AgentKind::ClaudeCode => CLAUDE_SESSION_HISTORY_DIR,
        }
    }
}

/// claude 実行ファイルのパスを解決する。
/// 環境変数 `ADDNESS_CLAUDE_BIN` を最優先で見て、無ければ PATH 上を探す。
/// 見つからなければ `None`。
pub fn claude_path() -> Option<PathBuf> {
    // 明示指定（別パスにインストールした場合や検証用の上書き）を最優先。
    if let Some(bin) = std::env::var_os("ADDNESS_CLAUDE_BIN") {
        let cand = PathBuf::from(bin);
        if cand.is_file() {
            return Some(cand);
        }
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let cand = dir.join("claude");
        if cand.is_file() {
            return Some(cand);
        }
    }
    None
}

/// 常駐（多ターン 1 プロセス）モードを既定で使うか。`ADDNESS_CLAUDE_RESIDENT=0` で
/// ワンショット（1 ターン 1 プロセス）へ退避する。
fn claude_resident_default_enabled() -> bool {
    match std::env::var("ADDNESS_CLAUDE_RESIDENT") {
        Ok(value) => value != "0",
        Err(_) => true,
    }
}

/// 常駐モードで承認バナー表示中の can_use_tool 要求（応答に必要な情報）。
#[derive(Debug, Clone)]
struct ClaudePendingTool {
    /// 応答時にエコーバックする request_id。
    request_id: String,
    /// 許可時に `updatedInput` として返す入力（そのまま返す）。
    input: Value,
    /// 「これからずっと許可」で sticky 許可リストへ入れるルール。
    rules: Vec<String>,
}

/// 常駐モードで送信済みの set_model / set_permission_mode 応答待ち（フォールバック判定用）。
#[derive(Debug, Clone)]
struct ClaudePendingSettingChange {
    request_id: String,
    deadline: Instant,
    label: String,
}

/// codex app-server を常駐モードで既定利用するか。`ADDNESS_CODEX_RESIDENT=0` で
/// 従来の `codex exec --json`（1 ターン 1 プロセス）へ退避する。
fn codex_appserver_default_enabled() -> bool {
    match std::env::var("ADDNESS_CODEX_RESIDENT") {
        Ok(value) => value != "0",
        Err(_) => true,
    }
}

/// 常駐 codex app-server のハンドシェイク/ターン進行フェーズ。
#[derive(Debug, Clone, PartialEq, Eq)]
enum CodexAppServerPhase {
    /// 未起動、または死亡・回収済み。
    Idle,
    /// initialize 送信済み、応答待ち。id を保持。
    Initializing { request_id: u64 },
    /// thread/start または thread/resume 送信済み、応答待ち。resume 失敗時のフォールバック判定に使う。
    StartingThread { request_id: u64, resuming: bool },
    /// thread 確立済み。turn/start 可能。
    Ready,
}

/// codex app-server の thread/settings/update 応答待ち（フォールバック判定用）。
#[derive(Debug, Clone)]
struct CodexAppServerSettingChange {
    request_id: u64,
    deadline: Instant,
    label: String,
}

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

/// 実行中ターン中、会話フィルタでも直近のTool/Eventログを混ぜて見せるための判定。
/// `live_tool_start` は現在実行中ターンの開始インデックス（`None` なら混在させない）。
fn is_live_turn_tool_line(
    kind: CodexLogKind,
    index: usize,
    live_tool_start: Option<usize>,
) -> bool {
    matches!(kind, CodexLogKind::Tool | CodexLogKind::Event)
        && live_tool_start.is_some_and(|start| index >= start)
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

fn short_session_id(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

enum CodexProcessEvent {
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
    /// 会話スレッド ID（Codex thread_id / Claude session_id を共用）の永続化。
    /// `None` は `/new` によるリセット（tombstone）を表す。
    ThreadId {
        id: Option<String>,
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
    /// ログから復元した会話スレッド ID。最後の `ThreadId` レコードの値。
    thread_id: Option<String>,
}

struct CodexPaneSpawnOptions<'a> {
    kind: AgentKind,
    codex_bin: &'a Path,
    cwd: &'a Path,
    addness_bin: &'a str,
    goal_id: String,
    goal_title: String,
    dod: String,
    status_label: String,
    session_log_path: Option<PathBuf>,
    input_history_path: Option<PathBuf>,
    /// クリップボード画像の保存先（`~/.addness/attachments/`）。テストでは一時ディレクトリを注入する。
    attachments_dir: Option<PathBuf>,
}

/// 長文ペーストを入力欄で畳み込んだときの保持データ。
/// 入力欄にはプレースホルダだけ挿入し、送信時に全文へ展開する。
#[derive(Debug, Clone)]
struct StoredPaste {
    /// 入力欄へ挿入したプレースホルダ文字列（例: `[貼り付け#1: 120行]`）。
    placeholder: String,
    /// 展開時に差し戻す全文（改行正規化済み）。
    full: String,
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
    /// 通知の文脈（ゴール名など）。OS 通知の本文に添える。空なら省略。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))] // macOSのOS通知経路でのみ読む
    pub context: Option<String>,
    /// ターン完了通知のターン所要秒。`Some(n)` かつ閾値未満なら通知を抑制する。
    /// 承認・その他の通知は `None`（常に通知する）。
    pub turn_elapsed_secs: Option<u64>,
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
            // 見た目の目立ちやすさのため、他の状態と区別できるアイコンを付ける。
            Self::Confirming => "⏸ 承認待ち",
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

/// 確認バナー（YesNo）に紐づく、Accept 時に実行するペイン固有アクション。
/// codex/claude の承認リトライ経路とは別に、Addness 独自の操作を確認付きで走らせるのに使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneDecisionAction {
    /// 直近チェックポイントへ作業ツリーを戻す（/undo）。
    Undo,
}

/// ペインが保持するチェックポイントのスタック上限（超過分は古い ref から掃除する）。
const CHECKPOINT_STACK_MAX: usize = 10;

/// ターン開始時に取ったチェックポイント（git ref）1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Checkpoint {
    /// 更新した ref 名（例: `refs/addness/checkpoint-<slug>-<seq>`）。
    ref_name: String,
    /// 対応するターン番号（表示・確認メッセージ用）。
    turn: usize,
}

/// App（ホスト）へ依頼するチェックポイント作成要求。git 実行は非同期ジョブで行う。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointRequest {
    pub cwd: String,
    pub ref_name: String,
    pub turn: usize,
    pub message: String,
}

/// App（ホスト）へ依頼する undo（チェックポイント復元）要求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoRequest {
    pub cwd: String,
    pub ref_name: String,
    pub turn: usize,
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

/// 汎用リストピッカー（モデル・reasoning・approval・sandbox・セッション選択）の 1 項目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexListPickerItem {
    /// 一覧に出す主ラベル（モデル名、セッション ID 先頭 8 桁など）。
    pub label: String,
    /// 補足（説明、更新時刻+タイトルなど）。空可。
    pub detail: String,
    /// 確定時にアクションへ渡す値（`set_model` への引数、セッション ID など）。
    pub value: String,
    /// 現在値マーカー。
    pub current: bool,
}

/// リストピッカー確定（Enter）時に実行するアクション。既存の setter / ハンドラへ委譲する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexListPickerAction {
    SetModel,
    /// Claude では effort。
    SetReasoning,
    /// Claude では permission-mode。
    SetApproval,
    /// Codex のみ。
    SetSandbox,
    /// エージェントの応答言語（両バックエンド共通）。
    SetLanguage,
    /// Enter=resume / f=fork。
    ResumeSession,
}

/// 候補一覧から ↑↓ + Enter で選ぶ汎用ピッカー（turn ピッカーと並置するボトムモーダル）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexListPicker {
    pub title: String,
    pub action: CodexListPickerAction,
    pub items: Vec<CodexListPickerItem>,
    pub selected: usize,
}

/// ピッカーの初期選択位置（`current` の項目。無ければ先頭）。
fn list_picker_initial_selection(items: &[CodexListPickerItem]) -> usize {
    items.iter().position(|item| item.current).unwrap_or(0)
}

/// 設定ピッカーの `config` 項目に付ける補足説明（それ以外の項目は空）。
fn config_choice_detail(is_config: bool) -> String {
    if is_config {
        "CLI設定に従う".to_string()
    } else {
        String::new()
    }
}

/// セッション候補をピッカー項目へ変換する。`current` は現在の thread_id と一致する候補。
fn session_picker_items(
    candidates: &[CodexSessionCandidate],
    thread_id: Option<&str>,
) -> Vec<CodexListPickerItem> {
    candidates
        .iter()
        .map(|session| CodexListPickerItem {
            label: short_session_id(&session.id).to_string(),
            detail: format!("{}  {}", session.updated_at, session.title),
            value: session.id.clone(),
            current: thread_id == Some(session.id.as_str()),
        })
        .collect()
}

/// 状態パネルに表示する直近アクション履歴の保持件数（パンくず表示用）。
const RECENT_ACTIONS_CAP: usize = 5;

/// 状態パネルに保持するサブエージェント（Claude Code の `Task`/`Agent` ツール起動）履歴の
/// 保持件数上限。ターン跨ぎでも保持するため `RECENT_ACTIONS_CAP` より大きめに取る。
const SUBAGENTS_CAP: usize = 20;

/// サブエージェント 1 件の状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SubagentState {
    /// 起動済みで、対応する tool_result 未受信（実行中）。
    Running,
    /// tool_result を正常受信（完了）。
    Completed,
    /// tool_result がエラーだった（失敗）。
    Failed,
}

impl SubagentState {
    fn icon(self) -> &'static str {
        match self {
            SubagentState::Running => "●",
            SubagentState::Completed => "✔",
            SubagentState::Failed => "✖",
        }
    }
}

/// 状態パネルに表示する 1 件のサブエージェント起動情報。
#[derive(Debug, Clone)]
struct SubagentEntry {
    /// 起動元 tool_use の `id`。対応する tool_result とのマッチングに使う。
    tool_use_id: Option<String>,
    /// 表示ラベル（description 優先）。
    label: String,
    /// `subagent_type`（Explore, general-purpose 等）。未取得なら None。
    #[allow(dead_code)] // 現状は表示に使わないが、将来のフィルタ/集計向けに保持する。
    agent_type: Option<String>,
    state: SubagentState,
    started_at: Instant,
    /// 完了/失敗した時刻（経過時間表示の固定に使う）。実行中は None。
    finished_at: Option<Instant>,
}

/// アクション種別（状態パネルのパンくずで先頭に付けるアイコンを決める）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecentActionKind {
    /// シェルコマンド実行（CommandExecution / codex サブコマンド等）。
    Command,
    /// MCP ツール呼び出し。
    Mcp,
    /// ファイル変更。
    FileChange,
    /// 上記に当てはまらないツール利用（Claude の ToolUse 汎用イベント等）。
    Tool,
}

impl RecentActionKind {
    fn icon(self) -> &'static str {
        match self {
            RecentActionKind::Command => "▶",
            RecentActionKind::Mcp => "◆",
            RecentActionKind::FileChange => "✎",
            RecentActionKind::Tool => "•",
        }
    }
}

/// 状態パネルのパンくずに表示する 1 件の直近アクション。
#[derive(Debug, Clone)]
struct RecentAction {
    kind: RecentActionKind,
    label: String,
    /// 現在のターン内での通し番号（1 始まり）。
    turn_seq: u32,
}

/// 埋め込み codex セッションの状態。
pub struct CodexPane {
    /// このペインが起動するエージェントバックエンドの種別。
    kind: AgentKind,
    codex_bin: PathBuf,
    addness_bin: String,
    child: Option<Child>,
    tx: Sender<CodexProcessEvent>,
    rx: Receiver<CodexProcessEvent>,
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
    /// 設定変更・モード切替など、恒常的に表示し続けたい状態メモ（例: "model: gpt-5"）。
    /// ユーザー操作で明示的に変更されるまで保持し、ターンの開始/終了では変化しない。
    status_note: Option<String>,
    /// ターン内の作業インジケータ（例: "依頼を確認中", "ツール実行: X", "応答完了"）。
    /// ターン開始時（`begin_turn_work`）と終了時（`end_turn_work`）に明示的にリセットする。
    work_action: Option<String>,
    /// codex が現在実行中として報告したコマンド。
    current_command: Option<String>,
    current_command_started_at: Option<Instant>,
    /// 直近アクション履歴（固定長リングバッファ）。「今」表示は最新 1 件しか見せないため、
    /// コマンドが高速連続実行されると途中のアクションが一瞬で上書きされ見逃される。
    /// ここに直近 N 件を種別アイコン付きラベルとターン内通し番号として積み、状態パネルの
    /// パンくずに表示する。ターン開始でクリアする。
    recent_actions: VecDeque<RecentAction>,
    /// `recent_actions` に積んだ直近アクションの総数（ターン内通し番号の採番に使う）。
    recent_action_seq: u32,
    /// Claude Code が `Task`/`Agent` ツールで起動したサブエージェントの履歴。
    /// ターンが変わっても実行中エントリはクリアせず保持する（`clear_recent_actions` の対象外）。
    subagents: VecDeque<SubagentEntry>,
    /// 実行中ターンの開始時刻。経過時間表示と完了時の所要時間算出に使う。
    turn_started_at: Option<Instant>,
    /// 直近に再描画へ反映した経過秒。秒表示が変わったフレームだけ再描画するために保持する。
    last_elapsed_tick_secs: Option<u64>,
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
    /// 入力欄で畳み込み中の長文ペースト（送信時に全文へ展開してクリア）。
    pending_pastes: Vec<StoredPaste>,
    /// 畳み込みペーストの通し番号（プレースホルダの `#N`）。送信でリセットする。
    paste_seq: usize,
    /// クリップボード画像の保存先ディレクトリ（`~/.addness/attachments/`）。テストでは注入する。
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))] // Ctrl+V取り込みはmacOSのみ
    attachments_dir: Option<PathBuf>,
    /// スラッシュコマンドパレットで選択中の候補インデックス。入力が変わると 0 に戻す。
    slash_palette_selected: usize,
    /// `@` メンションのファイル候補パレットで選択中のインデックス。入力が変わると 0 に戻す。
    mention_palette_selected: usize,
    /// Esc でメンションパレットを閉じた状態。次の入力で解除する。
    mention_palette_dismissed: bool,
    /// 入力履歴の保存先（`~/.addness/input-history.jsonl`）。テストでは None。
    input_history_path: Option<PathBuf>,
    /// Esc 中断の 1 回目押下（アーム済み）状態。次の Esc で中断する。
    esc_interrupt_armed: bool,
    queued_prompts: VecDeque<QueuedPrompt>,
    /// `codex exec --json` が返した Codex thread id。2ターン目以降の resume に使う。
    thread_id: Option<String>,
    /// 表示ログから復元した thread_id を保持中で、まだライブイベントで裏取りできていない状態。
    /// resume が失敗したら 1 回だけ新規セッションへフォールバックするために使う。
    thread_id_restored: bool,
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
    /// エージェントへ注入する応答言語設定（両バックエンド共通）。
    /// `~/.addness/config.json` に永続化し、起動時に読み込む。
    agent_language: AgentLanguage,
    /// 次回 `codex exec` 起動時に使う設定。
    exec_settings: CodexExecSettings,
    /// 現在turnを承認後に再実行する場合だけ使う一時的な approval override。
    one_shot_approval: Option<CodexApprovalChoice>,
    /// 作業ツリーの diff 表示。Some の間はログ領域を diff ビューとして使う。
    diff_view: Option<String>,
    /// 格納済み turn を明示的に選んで展開・格納するためのパネル。
    turn_picker: Option<CodexTurnPicker>,
    /// 候補一覧から ↑↓ + Enter で確定する汎用ピッカー（モデル・セッション等）。
    list_picker: Option<CodexListPicker>,
    /// Addness 独自UI上で `codex exec` に注入する永続目標。
    goal_mode: CodexGoalMode,
    pending_decision: Option<CodexDecisionBanner>,
    /// ClaudeCode バックエンドの次ターン設定（codex の `exec_settings` と並置）。
    /// codex 経路では未使用。
    claude_settings: claude::ClaudeExecSettings,
    /// 承認 Accept で「今回だけ」許可するツールルール（`--allowedTools`、claude 専用）。
    /// 次ターン起動時に消費して空へ戻す。codex の `one_shot_approval` に相当。
    claude_one_shot_allowed_tools: Vec<String>,
    /// 次ターンを `--fork-session` で開始するか（/fork-* で立てる。1 ターンで消費）。
    claude_fork_next: bool,
    /// 承認バナー表示中に、Accept/Always で適用する `--allowedTools` ルール（claude 専用）。
    claude_pending_allowed_tools: Vec<String>,
    /// 承認バナー表示中の拒否内容そのもの（ループガード用、claude 専用）。
    claude_pending_denials: Vec<claude::ClaudeDenial>,
    /// 直前の承認リトライで許可した拒否内容（claude 専用）。
    /// リトライ結果で同一の拒否が再発したらループとみなしてエラー表示する。
    claude_approved_denials: Vec<claude::ClaudeDenial>,
    /// 常駐（多ターン 1 プロセス）モードを使うか（ClaudeCode 専用）。
    /// 既定は環境変数で決まり、常駐 spawn 失敗時はこのセッションだけ false へ落とす。
    claude_resident_enabled: bool,
    /// 常駐 Claude Code プロセスのクライアント。None=未起動 / 死亡 / アイドル回収済み。
    claude_resident: Option<claude_resident::ResidentClient>,
    /// 常駐プロセスの最終アクティビティ時刻（アイドル回収判定に使う）。
    claude_resident_last_activity: Option<Instant>,
    /// interrupt を送ってから result（中断完了）を待つ期限。超えたら kill フォールバック。
    claude_interrupt_deadline: Option<Instant>,
    /// interrupt 要求中フラグ。result 受信時に中断完了として扱い、キューを自動開始しない。
    claude_interrupting: bool,
    /// 承認バナー表示中の can_use_tool 要求（応答用）。
    claude_pending_tool: Option<ClaudePendingTool>,
    /// 送信済みの set_model / set_permission_mode 応答待ち。
    claude_pending_setting_change: Option<ClaudePendingSettingChange>,
    /// 設定変更が control_request では反映できず、アイドル時に再起動が必要な状態。
    claude_resident_restart_pending: bool,
    /// result の `total_cost_usd`（セッション累積、保存のみ）。
    claude_total_cost_usd: Option<f64>,
    /// result の usage 要約（保存のみ）。
    claude_last_usage: Option<String>,
    /// result の usage から算出したコンテキスト占有トークン数（ヘッダ/`/usage` 表示用）。
    claude_context_tokens: Option<u64>,
    /// system/init が報告する現在の model（保存のみ、表示同期用）。
    claude_active_model: Option<String>,
    /// system/init が報告する現在の permissionMode（保存のみ、表示同期用）。
    claude_active_permission_mode: Option<String>,
    /// codex app-server 常駐モードを使うか（Codex 専用）。既定は環境変数で決まり、
    /// spawn / initialize 失敗時はこのセッションだけ false へ落として `codex exec --json` に退避する。
    codex_appserver_enabled: bool,
    /// 常駐 codex app-server プロセスのクライアント。None=未起動 / 死亡 / アイドル回収済み。
    codex_appserver: Option<codex_appserver::AppServerClient>,
    /// 常駐 app-server のハンドシェイク/ターン進行フェーズ。
    codex_appserver_phase: CodexAppServerPhase,
    /// thread 確立を待って送るターン本文（ハンドシェイク完了時に turn/start する）。
    codex_appserver_pending_turn: Option<String>,
    /// 保留ターンに添付する画像パス（turn/start の localImage 入力に載せる）。
    codex_appserver_pending_images: Vec<String>,
    /// 進行中ターンの turn.id（turn/interrupt に使う）。
    codex_appserver_turn_id: Option<String>,
    /// 送信済み turn/start の JSON-RPC id（応答で turn.id を確定するのに使う）。
    codex_appserver_turn_req_id: Option<u64>,
    /// 承認バナー表示中のサーバ発リクエスト（応答用）。
    codex_appserver_pending_approval: Option<codex_appserver::ApprovalRequest>,
    /// 送信済みの thread/settings/update 応答待ち。
    codex_appserver_pending_setting: Option<CodexAppServerSettingChange>,
    /// 設定変更が settings/update では反映できず、アイドル時に再起動が必要な状態。
    codex_appserver_restart_pending: bool,
    /// 常駐プロセスの最終アクティビティ時刻（アイドル回収判定に使う）。
    codex_appserver_last_activity: Option<Instant>,
    /// interrupt を送ってから turn/completed(interrupted) を待つ期限。超えたら kill フォールバック。
    codex_appserver_interrupt_deadline: Option<Instant>,
    /// ハンドシェイク（initialize / thread/start・resume）と turn/start 応答を待つ期限。
    /// 進捗（各送信成功）のたびに延長し、turn/start 応答受信で消す。超えたらワンショットへフォールバック。
    codex_appserver_handshake_deadline: Option<Instant>,
    /// interrupt 要求中フラグ。turn/completed 受信時に中断完了として扱い、キューを自動開始しない。
    codex_appserver_interrupting: bool,
    /// 実行中コマンドのライブ出力バッファ（itemId → 末尾 N 行）。item/completed で解放する。
    codex_appserver_output: HashMap<String, VecDeque<String>>,
    /// 現在ライブ出力を受けている commandExecution の itemId（末尾行の描画対象）。
    codex_appserver_running_item: Option<String>,
    /// 直近の thread/tokenUsage/updated（保存のみ）。
    codex_appserver_token_usage: Option<codex_appserver::TokenUsageInfo>,
    /// ターン単位チェックポイント（git ref）のスタック。末尾が最新。/undo で末尾から遡る。
    checkpoints: Vec<Checkpoint>,
    /// チェックポイント ref 名に使う単調増加シーケンス。
    checkpoint_seq: usize,
    /// App が非同期で処理するチェックポイント作成要求（ターン開始時にセット）。
    /// ジョブは 1 件ずつ処理されるため、消化前に複数溜まっても取りこぼさないようキューで持つ。
    pending_checkpoint_requests: VecDeque<CheckpointRequest>,
    /// App が非同期で処理する undo（復元）要求（/undo 確認 Accept 時にセット）。
    pending_undo_request: Option<UndoRequest>,
    /// 確認バナー（YesNo）に紐づく Accept 時アクション。/undo 確認で使う。
    pending_decision_action: Option<PaneDecisionAction>,
    /// 実行中スピナーのtick。ターン実行中のみ一定間隔で進める（純関数でtick→文字を選ぶため、
    /// ここでは単調増加するカウンタとして保持するだけ）。
    activity_spin_tick: u64,
    /// 直近にスピナーtickを進めた時刻。次に進めるべきタイミングの判定に使う。
    last_spin_advance_at: Option<Instant>,
    /// 承認待ち（`pending_decision`）が始まった時刻。長時間放置の検知に使う。
    pending_decision_started_at: Option<Instant>,
    /// 承認待ちの再通知（リマインド）を最後に送った時刻。
    last_confirming_reminder_at: Option<Instant>,
}

impl CodexPane {
    /// codex exec JSON セッションを開始する。
    ///
    /// 起動時点では Codex プロセスを走らせず、Addness 側の入力欄で最初の依頼を待つ。
    /// 最初の Enter で `codex exec --json` を起動し、以降は `codex exec resume` を使う。
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        codex_bin: &Path,
        cwd: &Path,
        addness_bin: &str,
        goal_id: String,
        goal_title: String,
        dod: String,
        status_label: String,
        kind: AgentKind,
    ) -> Result<Self> {
        Self::spawn_inner(CodexPaneSpawnOptions {
            kind,
            codex_bin,
            cwd,
            addness_bin,
            session_log_path: agent_session_log_path(kind, &goal_id),
            input_history_path: input_history_file_path(),
            attachments_dir: attachments_dir_path(),
            goal_id,
            goal_title,
            dod,
            status_label,
        })
    }

    fn spawn_inner(options: CodexPaneSpawnOptions<'_>) -> Result<Self> {
        let CodexPaneSpawnOptions {
            kind,
            codex_bin,
            cwd,
            addness_bin,
            goal_id,
            goal_title,
            dod,
            status_label,
            session_log_path,
            input_history_path,
            attachments_dir,
        } = options;
        let dod_items = split_dod_items(&dod);
        let dod_checks = vec![None; dod_items.len()];
        let (tx, rx) = mpsc::channel::<CodexProcessEvent>();
        let loaded = session_log_path
            .as_deref()
            .map(load_codex_session)
            .transpose()
            .unwrap_or(None)
            .unwrap_or_else(|| LoadedCodexSession {
                log: Vec::new(),
                record_count: 0,
                goal_mode: CodexGoalMode::default(),
                thread_id: None,
            });
        let goal_mode = loaded.goal_mode.clone();

        // 保存されていた thread_id の復元を検討する。Claude はセッション実体が cwd 依存の
        // ファイルなので、現在の cwd 配下に実体が無ければ復元を破棄して新規で開始する。
        // Codex は事前検証せず、resume 失敗時のフォールバックで吸収する。
        enum ThreadRestore {
            None,
            Resumed(String),
            ClaudeMissing,
        }
        let thread_restore = match loaded.thread_id.clone() {
            Some(id) => {
                if kind == AgentKind::ClaudeCode && !claude::session_file_exists(cwd, &id) {
                    ThreadRestore::ClaudeMissing
                } else {
                    ThreadRestore::Resumed(id)
                }
            }
            None => ThreadRestore::None,
        };
        let restored_thread_id = match &thread_restore {
            ThreadRestore::Resumed(id) => Some(id.clone()),
            _ => None,
        };
        let loaded_history_count = loaded.log.len();
        let turn_count = loaded
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Turn)
            .count();
        let collapsed_turns = (1..turn_count).collect::<BTreeSet<_>>();

        let mut input_state = CodexInputState::default();
        if let Some(path) = input_history_path.as_deref() {
            input_state.history = load_input_history(path);
        }

        // グローバル設定から応答言語を読み込む（未設定・読込失敗時は auto）。
        let agent_language = Settings::load()
            .map(|settings| settings.agent_language())
            .unwrap_or_default();

        let mut pane = Self {
            kind,
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
            status_note: None,
            work_action: None,
            current_command: None,
            current_command_started_at: None,
            recent_actions: VecDeque::new(),
            recent_action_seq: 0,
            subagents: VecDeque::new(),
            turn_started_at: None,
            last_elapsed_tick_secs: None,
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
            input_state,
            pending_pastes: Vec::new(),
            paste_seq: 0,
            attachments_dir,
            slash_palette_selected: 0,
            mention_palette_selected: 0,
            mention_palette_dismissed: false,
            input_history_path,
            esc_interrupt_armed: false,
            queued_prompts: VecDeque::new(),
            thread_id: restored_thread_id.clone(),
            thread_id_restored: restored_thread_id.is_some(),
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
            agent_language,
            exec_settings: CodexExecSettings::default(),
            one_shot_approval: None,
            diff_view: None,
            turn_picker: None,
            list_picker: None,
            pending_decision: None,
            goal_mode,
            dod,
            claude_settings: claude::ClaudeExecSettings::default(),
            claude_one_shot_allowed_tools: Vec::new(),
            claude_fork_next: false,
            claude_pending_allowed_tools: Vec::new(),
            claude_pending_denials: Vec::new(),
            claude_approved_denials: Vec::new(),
            claude_resident_enabled: kind == AgentKind::ClaudeCode
                && claude_resident_default_enabled(),
            claude_resident: None,
            claude_resident_last_activity: None,
            claude_interrupt_deadline: None,
            claude_interrupting: false,
            claude_pending_tool: None,
            claude_pending_setting_change: None,
            claude_resident_restart_pending: false,
            claude_total_cost_usd: None,
            claude_last_usage: None,
            claude_context_tokens: None,
            claude_active_model: None,
            claude_active_permission_mode: None,
            codex_appserver_enabled: kind == AgentKind::Codex && codex_appserver_default_enabled(),
            codex_appserver: None,
            codex_appserver_phase: CodexAppServerPhase::Idle,
            codex_appserver_pending_turn: None,
            codex_appserver_pending_images: Vec::new(),
            codex_appserver_turn_id: None,
            codex_appserver_turn_req_id: None,
            codex_appserver_pending_approval: None,
            codex_appserver_pending_setting: None,
            codex_appserver_restart_pending: false,
            codex_appserver_last_activity: None,
            codex_appserver_interrupt_deadline: None,
            codex_appserver_handshake_deadline: None,
            codex_appserver_interrupting: false,
            codex_appserver_output: HashMap::new(),
            codex_appserver_running_item: None,
            codex_appserver_token_usage: None,
            checkpoints: Vec::new(),
            checkpoint_seq: 0,
            pending_checkpoint_requests: VecDeque::new(),
            pending_undo_request: None,
            pending_decision_action: None,
            activity_spin_tick: 0,
            last_spin_advance_at: None,
            pending_decision_started_at: None,
            last_confirming_reminder_at: None,
        };
        let name = kind.display_name();
        if pane.loaded_history_count > 0 {
            pane.push_log(
                CodexLogKind::System,
                format!(
                    "前回の{name}履歴を {} 件読み込みました。続きは Enter で送信できます。",
                    pane.loaded_history_count
                ),
            );
        }
        pane.push_log(
            CodexLogKind::System,
            format!("{name}入力欄で待機中。入力して Enter で依頼を送信します。"),
        );
        match thread_restore {
            ThreadRestore::Resumed(id) => {
                pane.push_log(
                    CodexLogKind::System,
                    format!(
                        "前回のセッション（{}…）を自動再開します。新規で始める場合は /new",
                        short_session_id(&id)
                    ),
                );
            }
            ThreadRestore::ClaudeMissing => {
                pane.push_log(
                    CodexLogKind::System,
                    "前回セッションが見つからないため新規セッションで開始します",
                );
            }
            ThreadRestore::None => {}
        }
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
            AgentKind::Codex,
        )
        .unwrap();
        pane.rows = rows.max(1);
        pane.cols = cols.max(1);
        pane.log.clear();
        pane.session_log_path = None;
        pane.input_history_path = None;
        pane.attachments_dir = None;
        pane.pending_pastes.clear();
        pane.paste_seq = 0;
        pane.input_state.history.clear();
        pane.mention_palette_selected = 0;
        pane.mention_palette_dismissed = false;
        pane.esc_interrupt_armed = false;
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
        pane.agent_language = AgentLanguage::Auto;
        pane.exec_settings = CodexExecSettings::default();
        pane.one_shot_approval = None;
        pane.claude_settings = claude::ClaudeExecSettings::default();
        pane.claude_one_shot_allowed_tools = Vec::new();
        pane.claude_fork_next = false;
        pane.claude_pending_allowed_tools = Vec::new();
        pane.claude_pending_denials = Vec::new();
        pane.claude_approved_denials = Vec::new();
        pane.claude_resident_enabled = false;
        pane.claude_resident = None;
        pane.claude_resident_last_activity = None;
        pane.claude_interrupt_deadline = None;
        pane.claude_interrupting = false;
        pane.claude_pending_tool = None;
        pane.claude_pending_setting_change = None;
        pane.claude_resident_restart_pending = false;
        pane.claude_total_cost_usd = None;
        pane.claude_last_usage = None;
        pane.claude_context_tokens = None;
        pane.claude_active_model = None;
        pane.claude_active_permission_mode = None;
        pane.codex_appserver_enabled = false;
        pane.codex_appserver = None;
        pane.codex_appserver_phase = CodexAppServerPhase::Idle;
        pane.codex_appserver_pending_turn = None;
        pane.codex_appserver_turn_id = None;
        pane.codex_appserver_turn_req_id = None;
        pane.codex_appserver_pending_approval = None;
        pane.codex_appserver_pending_setting = None;
        pane.codex_appserver_restart_pending = false;
        pane.codex_appserver_last_activity = None;
        pane.codex_appserver_interrupt_deadline = None;
        pane.codex_appserver_handshake_deadline = None;
        pane.codex_appserver_interrupting = false;
        pane.codex_appserver_output.clear();
        pane.codex_appserver_running_item = None;
        pane.codex_appserver_token_usage = None;
        pane.checkpoints.clear();
        pane.checkpoint_seq = 0;
        pane.pending_checkpoint_requests.clear();
        pane.pending_undo_request = None;
        pane.pending_decision_action = None;
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
        pane.activity_spin_tick = 0;
        pane.last_spin_advance_at = None;
        pane.pending_decision_started_at = None;
        pane.last_confirming_reminder_at = None;
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

    /// テスト用: 承認待ちバナーを直接立てる（ui 側のレンダリングテストから使う）。
    #[cfg(test)]
    pub(crate) fn test_set_pending_decision(&mut self, kind: CodexDecisionKind, message: &str) {
        self.set_pending_decision(CodexDecisionBanner::new(kind, message.to_string()));
    }

    /// テスト用: ターン実行中フラグを直接切り替える（ui 側のレンダリングテストから使う）。
    #[cfg(test)]
    pub(crate) fn test_set_turn_running(&mut self, running: bool) {
        self.turn_running = running;
    }

    /// テスト用: 実行中コマンドを直接設定する（ui 側の「今」表示テストから使う）。
    #[cfg(test)]
    pub(crate) fn test_set_current_command(&mut self, command: &str) {
        self.record_current_command(RecentActionKind::Command, command.to_string());
    }

    /// テスト用: 実行中サブエージェントを直接積む（ui 側のヘッダ/状態パネル表示テストから使う）。
    #[cfg(test)]
    pub(crate) fn test_add_running_subagent(&mut self, label: &str) {
        self.record_subagent_launch(None, label.to_string(), None);
    }

    /// このペインのエージェントバックエンド種別。
    pub fn kind(&self) -> AgentKind {
        self.kind
    }

    /// ログを delta 行スクロールする（正=過去へ、負=最新へ）。
    pub fn scroll_lines(&mut self, delta: isize) {
        let max = self.max_view_scrollback();
        let target = (self.scrollback as isize + delta).max(0) as usize;
        self.scrollback = target.min(max);
    }

    /// テスト用: 会話ログに Assistant 行を積み、ライブ位置へ戻す（スクロール余地を作る）。
    #[cfg(test)]
    pub(crate) fn seed_assistant_log_for_test(&mut self, lines: usize) {
        for i in 0..lines {
            self.push_log(CodexLogKind::Assistant, format!("行 {i}"));
        }
        self.scroll_to_live();
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

    /// 設定変更等の恒常メッセージ（非実行中の「今」表示に使う）。
    pub fn status_note(&self) -> Option<&str> {
        self.status_note.as_deref()
    }

    pub(crate) fn set_status_note(&mut self, note: impl Into<String>) {
        self.status_note = Some(note.into());
    }

    /// ターン内の作業インジケータ（実行中の「今」表示のフォールバックに使う）。
    pub fn work_action(&self) -> Option<&str> {
        self.work_action.as_deref()
    }

    pub(crate) fn set_work_action(&mut self, action: impl Into<String>) {
        self.work_action = Some(action.into());
    }

    /// ターン開始時に呼ぶ。前ターンの作業インジケータが残っていても必ずクリアしてから
    /// 開始ラベル（通常は「依頼を確認中」）を設定する。
    fn begin_turn_work(&mut self, label: impl Into<String>) {
        self.work_action = None;
        self.set_work_action(label);
    }

    /// ターン終了時（応答完了/中断完了）に呼ぶ。ターン中に積み上がった作業インジケータを
    /// 明示的にクリアしてから終端ラベルを設定する。
    fn end_turn_work(&mut self, label: impl Into<String>) {
        self.work_action = None;
        self.set_work_action(label);
    }

    pub fn current_command_elapsed_secs(&self) -> Option<u64> {
        self.current_command_started_at
            .map(|t| t.elapsed().as_secs())
    }

    /// 「今」欄（current_command）を更新すると同時に、状態パネルのパンくず表示用リングバッファへ
    /// 直近アクションとして積む一元ヘルパー。CommandExecution / McpToolCall / FileChange の開始や
    /// Claude の ToolUse 等、"いま何をしているか" を更新する箇所はすべてここを経由する。
    fn record_current_command(&mut self, kind: RecentActionKind, label: impl Into<String>) {
        let label = label.into();
        self.current_command = Some(label.clone());
        self.current_command_started_at = Some(Instant::now());
        self.recent_action_seq = self.recent_action_seq.saturating_add(1);
        self.recent_actions.push_back(RecentAction {
            kind,
            label,
            turn_seq: self.recent_action_seq,
        });
        while self.recent_actions.len() > RECENT_ACTIONS_CAP {
            self.recent_actions.pop_front();
        }
    }

    /// ターン開始時に直近アクション履歴をクリアする（パンくずは実行中ターンの分だけ見せる）。
    fn clear_recent_actions(&mut self) {
        self.recent_actions.clear();
        self.recent_action_seq = 0;
    }

    /// 状態パネルのパンくず表示用に、直近アクションを古い順に整形して返す。
    /// 各要素は「アイコン ラベル #ターン内通し番号」の形式で、幅に応じたトリムは呼び出し側で行う。
    pub fn recent_action_breadcrumbs(&self) -> Vec<String> {
        self.recent_actions
            .iter()
            .map(|action| {
                format!(
                    "{} {} #{}",
                    action.kind.icon(),
                    action.label,
                    action.turn_seq
                )
            })
            .collect()
    }

    #[cfg(test)]
    fn recent_actions_len(&self) -> usize {
        self.recent_actions.len()
    }

    /// サブエージェント起動（`Task`/`Agent` ツール）を「実行中」として記録する。
    /// ターンをまたいでも保持するため `clear_recent_actions` の対象にはしない。
    fn record_subagent_launch(
        &mut self,
        tool_use_id: Option<String>,
        label: String,
        agent_type: Option<String>,
    ) {
        self.subagents.push_back(SubagentEntry {
            tool_use_id,
            label,
            agent_type,
            state: SubagentState::Running,
            started_at: Instant::now(),
            finished_at: None,
        });
        while self.subagents.len() > SUBAGENTS_CAP {
            self.subagents.pop_front();
        }
    }

    /// tool_result を受けて、対応するサブエージェントの状態を完了/失敗へ遷移させる。
    /// `tool_use_id` が一致する実行中エントリのみ更新する（Claude Code の tool_result は
    /// 常に `tool_use_id` を持つため、id 不一致=サブエージェント以外の tool_result として無視する）。
    fn resolve_subagent_result(&mut self, tool_use_id: Option<&str>, is_error: bool) {
        let idx = tool_use_id.and_then(|id| {
            self.subagents
                .iter()
                .position(|entry| entry.tool_use_id.as_deref() == Some(id))
        });
        let Some(idx) = idx else {
            return;
        };
        if let Some(entry) = self.subagents.get_mut(idx) {
            if entry.state != SubagentState::Running {
                return;
            }
            entry.state = if is_error {
                SubagentState::Failed
            } else {
                SubagentState::Completed
            };
            entry.finished_at = Some(Instant::now());
        }
    }

    /// 実行中のサブエージェント数。ヘッダ行・状態パネルの集計表示に使う。
    pub fn subagent_running_count(&self) -> usize {
        self.subagents
            .iter()
            .filter(|entry| entry.state == SubagentState::Running)
            .count()
    }

    /// 状態パネルに表示するサブエージェント一覧を、実行中を優先しつつ新しい順に整形して返す。
    /// `limit` は呼び出し側（パネル高さに応じた表示件数トリム）で決める。
    pub fn subagent_status_lines(&self, limit: usize) -> Vec<String> {
        if limit == 0 || self.subagents.is_empty() {
            return Vec::new();
        }
        let mut running: Vec<&SubagentEntry> = Vec::new();
        let mut finished: Vec<&SubagentEntry> = Vec::new();
        for entry in &self.subagents {
            if entry.state == SubagentState::Running {
                running.push(entry);
            } else {
                finished.push(entry);
            }
        }
        // 完了/失敗は新しい順（直近ほど関心が高い）、実行中はそのまま起動順で先頭にまとめる。
        finished.reverse();
        running
            .into_iter()
            .chain(finished)
            .take(limit)
            .map(|entry| {
                let elapsed = match entry.finished_at {
                    Some(finished_at) => finished_at.duration_since(entry.started_at).as_secs(),
                    None => entry.started_at.elapsed().as_secs(),
                };
                format!("{} {} ({}秒)", entry.state.icon(), entry.label, elapsed)
            })
            .collect()
    }

    #[cfg(test)]
    fn subagents_len(&self) -> usize {
        self.subagents.len()
    }

    /// 実行中ターンの経過秒（開始からの秒）。ターン非実行時は None。
    pub fn turn_elapsed_secs(&self) -> Option<u64> {
        if self.is_turn_running() {
            self.turn_started_at.map(|t| t.elapsed().as_secs())
        } else {
            None
        }
    }

    /// いま表示すべき経過秒。コマンド実行中はコマンド経過、それ以外はターン経過。
    fn current_display_elapsed_secs(&self) -> Option<u64> {
        if !self.is_turn_running() {
            return None;
        }
        if self.current_command.is_some() {
            self.current_command_elapsed_secs()
        } else {
            self.turn_elapsed_secs()
        }
    }

    /// 状態ラベルに経過時間を添えた表示（例: 「考え中 1:23」）。
    pub fn run_state_elapsed_label(&self) -> String {
        let label = self.run_state().label();
        match self.current_display_elapsed_secs() {
            Some(secs) => format!("{label} {}", format_elapsed(secs)),
            None => label.to_string(),
        }
    }

    /// ターン開始/終了に応じて経過タイマーを管理し、表示秒が変わったら true を返す。
    /// タイマー表示のためだけの毎フレーム再描画を避け、秒が繰り上がった時だけ再描画させる。
    fn manage_turn_timer(&mut self) -> bool {
        if self.is_turn_running() {
            if self.turn_started_at.is_none() {
                self.turn_started_at = Some(Instant::now());
            }
        } else {
            self.turn_started_at = None;
        }
        let current = self.current_display_elapsed_secs();
        if current != self.last_elapsed_tick_secs {
            self.last_elapsed_tick_secs = current;
            true
        } else {
            false
        }
    }

    /// 実行中スピナーのtick（文字選択は ui 側の純関数に任せる）。
    pub fn activity_spin_tick(&self) -> u64 {
        self.activity_spin_tick
    }

    /// ターン実行中のみ、一定間隔でスピナーtickを進める。動いていることが見た目で分かるように、
    /// 進んだフレームだけ再描画対象として true を返す（アイドル時は無駄な再描画をしない）。
    fn advance_activity_spin(&mut self) -> bool {
        const SPIN_INTERVAL: Duration = Duration::from_millis(120);
        if !self.is_turn_running() {
            self.last_spin_advance_at = None;
            return false;
        }
        let should_advance = match self.last_spin_advance_at {
            Some(t) => t.elapsed() >= SPIN_INTERVAL,
            None => true,
        };
        if should_advance {
            self.activity_spin_tick = self.activity_spin_tick.wrapping_add(1);
            self.last_spin_advance_at = Some(Instant::now());
        }
        should_advance
    }

    /// 承認待ち（`pending_decision`）が一定時間続いたら、見逃し防止のため通知を再送する。
    /// 一度きりの通知（`set_pending_decision` 時）だけでは長時間の放置に気づけないため、
    /// 待機が続く間は間隔を空けて繰り返し知らせる。
    fn remind_confirming_if_stale(&mut self) {
        const REMIND_INTERVAL: Duration = Duration::from_secs(5 * 60);
        if self.pending_decision.is_none() {
            self.pending_decision_started_at = None;
            self.last_confirming_reminder_at = None;
            return;
        }
        let started = *self
            .pending_decision_started_at
            .get_or_insert_with(Instant::now);
        let last_reminder = *self.last_confirming_reminder_at.get_or_insert(started);
        if last_reminder.elapsed() < REMIND_INTERVAL {
            return;
        }
        self.last_confirming_reminder_at = Some(Instant::now());
        let Some(decision) = self.pending_decision.clone() else {
            return;
        };
        let name = self.kind.display_name();
        let waited = format_elapsed(started.elapsed().as_secs());
        self.push_terminal_notice(
            format!("{name} 承認待ち継続中 ({waited})"),
            decision.message,
        );
    }

    /// 完了したターンの所要時間を控えめな System 行として残す。
    fn record_turn_duration(&mut self) {
        if let Some(started) = self.turn_started_at {
            let secs = started.elapsed().as_secs();
            self.push_log(
                CodexLogKind::System,
                format!("所要 {}", format_elapsed(secs)),
            );
        }
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
        let kind = self.kind;
        SLASH_COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(prefix.as_str()))
            .filter(|(name, _)| slash_command_visible_for_kind(name, kind))
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

    /// 現在の入力に対する `@` メンションのファイル候補（ペインの cwd 基準の相対パス）。
    pub fn mention_palette_suggestions(&self) -> Vec<MentionCandidate> {
        let Some((_, query)) = active_mention(&self.input_state.line, self.input_state.cursor)
        else {
            return Vec::new();
        };
        mention_candidates(Path::new(&self.cwd), &query)
    }

    /// `@` メンションのファイル候補パレットを表示 / 操作すべき状態か。
    pub fn mention_palette_active(&self) -> bool {
        !self.finished
            && !self.mention_palette_dismissed
            && !self.is_search_editing()
            && self.pending_decision.is_none()
            && !self.slash_palette_active()
            && !self.mention_palette_suggestions().is_empty()
    }

    /// メンションパレットで選択中の候補インデックス（候補数でクランプ済み）。
    pub fn mention_palette_selected(&self) -> usize {
        let n = self.mention_palette_suggestions().len();
        if n == 0 {
            0
        } else {
            self.mention_palette_selected.min(n - 1)
        }
    }

    /// メンションパレットの選択を上下に移動する（端で反対側へラップ）。
    pub fn move_mention_palette_selection(&mut self, delta: isize) {
        let n = self.mention_palette_suggestions().len();
        if n == 0 {
            return;
        }
        let cur = self.mention_palette_selected.min(n - 1) as isize;
        self.mention_palette_selected = (cur + delta).rem_euclid(n as isize) as usize;
    }

    /// 選択中の候補で `@` 以降を相対パスに置換して確定する。
    /// ディレクトリならそのまま潜って絞り込みを続け、ファイルなら末尾に空白を付けて確定する。
    pub fn accept_mention_palette_selection(&mut self) {
        let candidates = self.mention_palette_suggestions();
        if candidates.is_empty() {
            return;
        }
        let idx = self.mention_palette_selected.min(candidates.len() - 1);
        let candidate = candidates[idx].clone();
        let Some((at, _)) = active_mention(&self.input_state.line, self.input_state.cursor) else {
            return;
        };
        let replacement = if candidate.is_dir {
            candidate.insert
        } else {
            format!("{} ", candidate.insert)
        };
        self.input_state
            .replace_to_cursor(at + '@'.len_utf8(), &replacement);
        self.mention_palette_selected = 0;
    }

    /// Esc でメンションパレットだけを閉じる（入力は消さない）。
    pub fn dismiss_mention_palette(&mut self) {
        self.mention_palette_dismissed = true;
    }

    /// Esc 中断の 1 回目押下（アーム）。次の Esc で中断する旨を表示する。
    pub fn arm_esc_interrupt(&mut self) {
        self.esc_interrupt_armed = true;
        self.push_log(
            CodexLogKind::System,
            "もう一度 Esc でターンを中断します".to_string(),
        );
    }

    /// Esc 中断がアーム済みなら true を返してアームを解除する。
    pub fn take_esc_interrupt_armed(&mut self) -> bool {
        std::mem::take(&mut self.esc_interrupt_armed)
    }

    /// Esc でターンを中断する。実行中でなければ何もしない。
    pub fn interrupt_turn_by_esc(&mut self) -> bool {
        if !self.is_turn_running() {
            return false;
        }
        // 常駐モードはまず graceful interrupt を試み、2 秒で完了しなければ update() 側で kill する。
        if self.claude_resident.is_some() && self.request_claude_interrupt() {
            self.push_log(CodexLogKind::System, "Esc でターンを中断します".to_string());
            let queued = self.queued_prompts.len();
            if queued > 0 {
                self.push_log(
                    CodexLogKind::System,
                    format!("予約{queued}件は保留中（/stop queued で破棄可）"),
                );
            }
            return true;
        }
        if self.codex_appserver.is_some() && self.request_codex_appserver_interrupt() {
            self.push_log(CodexLogKind::System, "Esc でターンを中断します".to_string());
            let queued = self.queued_prompts.len();
            if queued > 0 {
                self.push_log(
                    CodexLogKind::System,
                    format!("予約{queued}件は保留中（/stop queued で破棄可）"),
                );
            }
            return true;
        }
        self.kill_current_turn();
        self.push_log(
            CodexLogKind::System,
            "Esc でターンを中断しました".to_string(),
        );
        // /stop と同様、中断時は予約ターンを自動開始しない。予約が残っていれば保留中である
        // ことを知らせる。
        let queued = self.queued_prompts.len();
        if queued > 0 {
            self.push_log(
                CodexLogKind::System,
                format!("予約{queued}件は保留中（/stop queued で破棄可）"),
            );
        }
        true
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
        let live_tool_start = self.live_turn_tool_start_index();
        let mut current_collapsed_turn = None;
        let mut visible = Vec::new();
        for (index, line) in self.log.iter().enumerate() {
            if line.kind == CodexLogKind::Turn {
                current_collapsed_turn = turn_number_from_label(&line.text)
                    .filter(|n| collapse_turns && self.collapsed_turns.contains(n));
                if self.log_line_visible(line, &query, index, live_tool_start) {
                    visible.push(line);
                }
                continue;
            }
            if current_collapsed_turn.is_some() {
                continue;
            }
            if self.log_line_visible(line, &query, index, live_tool_start) {
                visible.push(line);
            }
        }
        visible
    }

    /// 実行中ターン中は会話フィルタでも直近のTool/Eventログを混ぜて表示するため、
    /// 現在実行中ターンの開始位置（直近のTurn行のインデックス）を返す。
    /// 実行中でない、または会話フィルタ以外のときは None（従来通りログを汚さない）。
    fn live_turn_tool_start_index(&self) -> Option<usize> {
        if self.log_filter != CodexLogFilter::Conversation || !self.is_turn_running() {
            return None;
        }
        self.log
            .iter()
            .rposition(|line| line.kind == CodexLogKind::Turn)
    }

    /// ストリーミング中（未完成）の assistant ログ行への参照を返す。
    /// 描画側で「Markdown 整形せずプレーン表示する行」を識別するために使う。
    pub fn streaming_assistant_line(&self) -> Option<&CodexLogLine> {
        self.streaming_assistant_index
            .and_then(|index| self.log.get(index))
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

    pub fn list_picker_open(&self) -> bool {
        self.list_picker.is_some()
    }

    pub fn list_picker(&self) -> Option<&CodexListPicker> {
        self.list_picker.as_ref()
    }

    /// 汎用リストピッカーを開く。turn ピッカーと同時には開かない（後勝ちで閉じる）。
    fn open_list_picker(&mut self, picker: CodexListPicker) {
        self.turn_picker = None;
        self.list_picker = Some(picker);
    }

    pub fn close_list_picker(&mut self) {
        self.list_picker = None;
    }

    /// リストピッカーの選択移動（turn ピッカーと同じく clamp・ラップなし）。
    pub fn move_list_picker_selection(&mut self, delta: isize) {
        let Some(picker) = self.list_picker.as_mut() else {
            return;
        };
        if picker.items.is_empty() {
            return;
        }
        let max = picker.items.len().saturating_sub(1) as isize;
        picker.selected = (picker.selected as isize + delta).clamp(0, max) as usize;
    }

    /// リストピッカーの確定。既存の setter / ハンドラへ委譲する。
    /// `fork` は ResumeSession のときだけ有効（それ以外のアクションでは無視して開いたまま）。
    pub fn accept_list_picker(&mut self, fork: bool) {
        let Some(picker) = self.list_picker.as_ref() else {
            return;
        };
        if fork && picker.action != CodexListPickerAction::ResumeSession {
            return;
        }
        let action = picker.action;
        let Some(item) = picker.items.get(picker.selected) else {
            self.list_picker = None;
            return;
        };
        let value = item.value.clone();
        self.list_picker = None;
        match action {
            CodexListPickerAction::SetModel => self.set_model(&value),
            CodexListPickerAction::SetReasoning => self.handle_reasoning_slash_command(&value),
            CodexListPickerAction::SetApproval => self.handle_approval_slash_command(&value),
            CodexListPickerAction::SetSandbox => self.handle_sandbox_slash_command(&value),
            CodexListPickerAction::SetLanguage => {
                if let Some(choice) = AgentLanguage::parse(&value) {
                    self.set_agent_language(choice);
                }
            }
            CodexListPickerAction::ResumeSession => {
                if self.kind == AgentKind::ClaudeCode {
                    self.start_claude_resume(Some(&value), fork, "");
                } else if fork {
                    self.handle_fork_session_slash_command(&value, false);
                } else {
                    self.handle_resume_session_slash_command(&value, false);
                }
            }
        }
    }

    /// `/model`（引数なし）: kind 別のモデル候補ピッカーを開く。
    /// model_override 設定中はどれも current にせず、タイトルに現在値を出す。
    fn open_model_picker(&mut self) {
        let (items, override_name) = if self.kind == AgentKind::ClaudeCode {
            let override_name = self.claude_settings.model_override().map(str::to_string);
            let current = if override_name.is_some() {
                None
            } else {
                Some(self.claude_settings.model_choice())
            };
            let items = [
                claude::ClaudeModelChoice::Config,
                claude::ClaudeModelChoice::Fable,
                claude::ClaudeModelChoice::Opus,
                claude::ClaudeModelChoice::Sonnet,
                claude::ClaudeModelChoice::Haiku,
            ]
            .into_iter()
            .map(|choice| CodexListPickerItem {
                label: choice.label().to_string(),
                detail: config_choice_detail(choice == claude::ClaudeModelChoice::Config),
                value: choice.label().to_string(),
                current: current == Some(choice),
            })
            .collect::<Vec<_>>();
            (items, override_name)
        } else {
            let override_name = self.exec_settings.model_override.clone();
            let current = if override_name.is_some() {
                None
            } else {
                Some(self.exec_settings.model)
            };
            let items = [
                CodexModelChoice::Config,
                CodexModelChoice::Gpt55,
                CodexModelChoice::Gpt5,
                CodexModelChoice::O3,
            ]
            .into_iter()
            .map(|choice| CodexListPickerItem {
                label: choice.label().to_string(),
                detail: config_choice_detail(choice == CodexModelChoice::Config),
                value: choice.label().to_string(),
                current: current == Some(choice),
            })
            .collect::<Vec<_>>();
            (items, override_name)
        };
        let title = match override_name {
            Some(name) => format!(" Model選択 — 現在: {name}（任意名は /model <name>） "),
            None => " Model選択 — 任意名は /model <name> ".to_string(),
        };
        let selected = list_picker_initial_selection(&items);
        self.open_list_picker(CodexListPicker {
            title,
            action: CodexListPickerAction::SetModel,
            items,
            selected,
        });
    }

    /// `/reasoning` `/effort`（引数なし）: kind 別の推論強度候補ピッカーを開く。
    fn open_reasoning_picker(&mut self) {
        let (title, items) = if self.kind == AgentKind::ClaudeCode {
            let current = self.claude_settings.effort_choice();
            let items = [
                claude::ClaudeEffortChoice::Config,
                claude::ClaudeEffortChoice::Low,
                claude::ClaudeEffortChoice::Medium,
                claude::ClaudeEffortChoice::High,
                claude::ClaudeEffortChoice::XHigh,
                claude::ClaudeEffortChoice::Max,
            ]
            .into_iter()
            .map(|choice| CodexListPickerItem {
                label: choice.label().to_string(),
                detail: config_choice_detail(choice == claude::ClaudeEffortChoice::Config),
                value: choice.label().to_string(),
                current: choice == current,
            })
            .collect::<Vec<_>>();
            (" Effort選択 ".to_string(), items)
        } else {
            let current = self.exec_settings.reasoning;
            let items = [
                CodexReasoningChoice::Config,
                CodexReasoningChoice::Low,
                CodexReasoningChoice::Medium,
                CodexReasoningChoice::High,
                CodexReasoningChoice::XHigh,
            ]
            .into_iter()
            .map(|choice| CodexListPickerItem {
                label: choice.label().to_string(),
                detail: config_choice_detail(choice == CodexReasoningChoice::Config),
                value: choice.label().to_string(),
                current: choice == current,
            })
            .collect::<Vec<_>>();
            (" Reasoning選択 ".to_string(), items)
        };
        let selected = list_picker_initial_selection(&items);
        self.open_list_picker(CodexListPicker {
            title,
            action: CodexListPickerAction::SetReasoning,
            items,
            selected,
        });
    }

    /// `/approval` `/permissions`（引数なし）: kind 別の承認モード候補ピッカーを開く。
    fn open_approval_picker(&mut self) {
        let (title, items) = if self.kind == AgentKind::ClaudeCode {
            let current = self.claude_settings.permission_mode_choice();
            let items = [
                claude::ClaudePermissionMode::Config,
                claude::ClaudePermissionMode::Plan,
                claude::ClaudePermissionMode::AcceptEdits,
                claude::ClaudePermissionMode::DontAsk,
                claude::ClaudePermissionMode::BypassPermissions,
                claude::ClaudePermissionMode::DangerouslySkipPermissions,
            ]
            .into_iter()
            .map(|choice| CodexListPickerItem {
                label: choice.label().to_string(),
                detail: if choice.is_dangerously_skip() {
                    "全権限チェックをバイパス（危険）".to_string()
                } else {
                    config_choice_detail(choice == claude::ClaudePermissionMode::Config)
                },
                value: choice.label().to_string(),
                current: choice == current,
            })
            .collect::<Vec<_>>();
            (" Permission-mode選択 ".to_string(), items)
        } else {
            let current = self.exec_settings.approval;
            let items = [
                CodexApprovalChoice::Config,
                CodexApprovalChoice::Untrusted,
                CodexApprovalChoice::OnRequest,
                CodexApprovalChoice::OnFailure,
                CodexApprovalChoice::Never,
            ]
            .into_iter()
            .map(|choice| CodexListPickerItem {
                label: choice.label().to_string(),
                detail: config_choice_detail(choice == CodexApprovalChoice::Config),
                value: choice.label().to_string(),
                current: choice == current,
            })
            .collect::<Vec<_>>();
            (" Approval選択 ".to_string(), items)
        };
        let selected = list_picker_initial_selection(&items);
        self.open_list_picker(CodexListPicker {
            title,
            action: CodexListPickerAction::SetApproval,
            items,
            selected,
        });
    }

    /// `/sandbox`（引数なし・Codex のみ）: sandbox 候補ピッカーを開く。
    fn open_sandbox_picker(&mut self) {
        let current = self.exec_settings.sandbox;
        let items = [
            CodexSandboxChoice::ReadOnly,
            CodexSandboxChoice::WorkspaceWrite,
            CodexSandboxChoice::DangerFullAccess,
        ]
        .into_iter()
        .map(|choice| CodexListPickerItem {
            label: choice.label().to_string(),
            detail: String::new(),
            value: choice.label().to_string(),
            current: choice == current,
        })
        .collect::<Vec<_>>();
        let selected = list_picker_initial_selection(&items);
        self.open_list_picker(CodexListPicker {
            title: " Sandbox選択 ".to_string(),
            action: CodexListPickerAction::SetSandbox,
            items,
            selected,
        });
    }

    /// `/sessions` `/resume`（引数なし）: セッション候補ピッカーを開く。
    /// 候補は `indexed_sessions` にも格納し、従来の `/resume-session <番号>` の
    /// 番号解決と整合させる。候補 0 件なら開かない。
    fn open_session_picker(&mut self, limit: usize) {
        let candidates = if self.kind == AgentKind::ClaudeCode {
            self.load_claude_session_candidates(limit)
        } else {
            match load_codex_session_candidates(limit) {
                Ok(sessions) => sessions,
                Err(e) => {
                    self.push_log(
                        CodexLogKind::Error,
                        format!("Codex sessions の読み込みに失敗しました: {e}"),
                    );
                    return;
                }
            }
        };
        if candidates.is_empty() {
            self.push_log(CodexLogKind::System, "セッションがありません");
            return;
        }
        let items = session_picker_items(&candidates, self.thread_id.as_deref());
        self.indexed_sessions = candidates;
        let selected = list_picker_initial_selection(&items);
        self.open_list_picker(CodexListPicker {
            title: format!(" {} セッション選択 ", self.kind.display_name()),
            action: CodexListPickerAction::ResumeSession,
            items,
            selected,
        });
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

    /// ヘッダー表示用のフィルタラベル。実行中ターン中は会話フィルタでも
    /// 直近のTool/Eventログを混ぜて表示しているため、その旨を示す接尾辞を付ける。
    pub fn log_filter_display_label(&self) -> String {
        if self.live_turn_tool_start_index().is_some() {
            format!("{}+実行中", self.log_filter_label())
        } else {
            self.log_filter_label().to_string()
        }
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
        match self.kind {
            AgentKind::Codex => self.exec_settings.label(),
            AgentKind::ClaudeCode => self.claude_settings.label(),
        }
    }

    pub fn memory_mode_label(&self) -> String {
        match self.kind {
            AgentKind::Codex => self.exec_settings.memory_mode_label(),
            // Claude Code はプロジェクト固有メモリを Addness に集約する（グローバルメモリ非使用）。
            AgentKind::ClaudeCode => "Addness DB memory".to_string(),
        }
    }

    pub fn memory_mode_is_addness_safe(&self) -> bool {
        match self.kind {
            AgentKind::Codex => self.exec_settings.memory_mode_is_addness_safe(),
            AgentKind::ClaudeCode => true,
        }
    }

    pub fn cycle_model(&mut self) {
        if self.kind == AgentKind::ClaudeCode {
            let value = self.claude_settings.cycle_model();
            self.set_status_note(format!("model: {value}"));
            self.push_activity(format!("Claude Code model を {value} に変更"));
            self.push_log(CodexLogKind::System, format!("次回ターンの model: {value}"));
            self.push_claude_resident_model();
            return;
        }
        self.exec_settings.model_override = None;
        let value = self.exec_settings.cycle_model();
        self.set_status_note(format!("model: {value}"));
        self.push_activity(format!("Codex model を {value} に変更"));
        self.push_log(CodexLogKind::System, format!("次回ターンの model: {value}"));
        self.push_codex_appserver_model();
    }

    fn set_model(&mut self, value: &str) {
        if self.kind == AgentKind::ClaudeCode {
            let value = self.claude_settings.set_model(value);
            self.set_status_note(format!("model: {value}"));
            self.push_activity(format!("Claude Code model を {value} に変更"));
            self.push_log(CodexLogKind::System, format!("次回ターンの model: {value}"));
            self.push_claude_resident_model();
            return;
        }
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
        self.set_status_note(format!("model: {value}"));
        self.push_activity(format!("Codex model を {value} に変更"));
        self.push_log(CodexLogKind::System, format!("次回ターンの model: {value}"));
        self.push_codex_appserver_model();
    }

    /// 現在の応答言語設定と環境変数から合成した最終的な developer instructions。
    /// 全バックエンドの起動引数はこの文字列を使う（言語指示の注入漏れ防止）。
    fn composed_developer_instructions(&self) -> String {
        compose_developer_instructions(self.agent_language, current_lang_env().as_deref())
    }

    /// `/lang`（`/language`）: 引数なしはピッカー、引数ありは直接設定。
    fn handle_language_slash_command(&mut self, args: &str) {
        if args.is_empty() {
            self.open_language_picker();
            return;
        }
        match AgentLanguage::parse(args) {
            Some(choice) => self.set_agent_language(choice),
            None => self.push_log(
                CodexLogKind::Error,
                "応答言語は auto / ja / en / off を指定してください",
            ),
        }
    }

    /// `/lang`（引数なし）: auto / 日本語 / English / off の選択ピッカーを開く。
    fn open_language_picker(&mut self) {
        let current = self.agent_language;
        let items = [
            AgentLanguage::Auto,
            AgentLanguage::Ja,
            AgentLanguage::En,
            AgentLanguage::Off,
        ]
        .into_iter()
        .map(|choice| CodexListPickerItem {
            label: choice.label().to_string(),
            detail: String::new(),
            value: choice.config_value().to_string(),
            current: choice == current,
        })
        .collect::<Vec<_>>();
        let selected = list_picker_initial_selection(&items);
        self.open_list_picker(CodexListPicker {
            title: " 応答言語を選択 ".to_string(),
            action: CodexListPickerAction::SetLanguage,
            items,
            selected,
        });
    }

    /// 応答言語を設定し、グローバル設定へ永続化する。
    /// 常駐プロセス（Claude / Codex app-server）は起動時に developer instructions を
    /// 渡すため、変更は次の thread/セッションから有効。生存中の常駐には再起動保留を立てる。
    fn set_agent_language(&mut self, language: AgentLanguage) {
        self.agent_language = language;
        if let Err(err) =
            Settings::load().and_then(|mut settings| settings.set_agent_language(language))
        {
            self.push_log(
                CodexLogKind::Error,
                format!("応答言語設定の保存に失敗しました: {err}"),
            );
        }
        let label = language.label();
        self.set_status_note(format!("lang: {}", language.config_value()));
        self.push_activity(format!("応答言語を {label} に変更"));

        let mut resident_pending = false;
        if self.claude_resident.is_some() {
            self.claude_resident_restart_pending = true;
            resident_pending = true;
        }
        if self.codex_appserver.is_some() {
            self.codex_appserver_restart_pending = true;
            resident_pending = true;
        }
        if resident_pending {
            self.push_log(
                CodexLogKind::System,
                format!("応答言語を {label} に変更しました（常駐プロセス再起動後に反映）"),
            );
        } else {
            self.push_log(
                CodexLogKind::System,
                format!("応答言語を {label} に変更しました（次のセッションから反映）"),
            );
        }
    }

    pub fn cycle_reasoning(&mut self) {
        if self.kind == AgentKind::ClaudeCode {
            let value = self.claude_settings.cycle_effort();
            self.set_status_note(format!("effort: {value}"));
            self.push_activity(format!("Claude Code effort を {value} に変更"));
            self.note_claude_effort_change(value);
            return;
        }
        let value = self.exec_settings.cycle_reasoning();
        self.set_status_note(format!("reasoning: {value}"));
        self.push_activity(format!("Codex reasoning を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの reasoning effort: {value}"),
        );
        self.push_codex_appserver_effort();
    }

    /// ClaudeCode の `/effort` フリーテキスト設定。
    fn set_claude_effort(&mut self, value: claude::ClaudeEffortChoice) {
        let value = self.claude_settings.set_effort(value);
        self.set_status_note(format!("effort: {value}"));
        self.push_activity(format!("Claude Code effort を {value} に変更"));
        self.note_claude_effort_change(value);
    }

    /// effort 変更をログに出し、常駐が生きていれば再起動保留を立てる。
    /// effort には set_effort 相当の control_request が無いため、常駐は再起動 + resume で反映する。
    fn note_claude_effort_change(&mut self, value: &str) {
        if self.claude_resident.is_some() {
            self.claude_resident_restart_pending = true;
            self.push_log(
                CodexLogKind::System,
                format!("effort を {value} に変更しました（常駐プロセス再接続後に反映）"),
            );
        } else {
            self.push_log(
                CodexLogKind::System,
                format!("次回ターンの effort: {value}"),
            );
        }
    }

    fn set_reasoning(&mut self, value: CodexReasoningChoice) {
        self.exec_settings.reasoning = value;
        let value = self.exec_settings.reasoning.label();
        self.set_status_note(format!("reasoning: {value}"));
        self.push_activity(format!("Codex reasoning を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの reasoning effort: {value}"),
        );
        self.push_codex_appserver_effort();
    }

    pub fn cycle_approval(&mut self) {
        if self.kind == AgentKind::ClaudeCode {
            let previous = self.claude_settings.permission_mode_choice();
            let value = self.claude_settings.cycle_permission_mode();
            self.set_status_note(format!("permission: {value}"));
            self.push_activity(format!("Claude Code permission-mode を {value} に変更"));
            self.push_log(
                CodexLogKind::System,
                format!("次回ターンの permission-mode: {value}"),
            );
            self.push_claude_resident_permission_mode(previous);
            return;
        }
        let value = self.exec_settings.cycle_approval();
        self.set_status_note(format!("approval: {value}"));
        self.push_activity(format!("Codex approval を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの approval: {value}"),
        );
        self.push_codex_appserver_approval();
    }

    /// ClaudeCode の permission-mode 設定（`/permissions <mode>`）。
    fn set_claude_permission_mode(&mut self, value: claude::ClaudePermissionMode) {
        let previous = self.claude_settings.permission_mode_choice();
        let value = self.claude_settings.set_permission_mode(value);
        self.set_status_note(format!("permission: {value}"));
        self.push_activity(format!("Claude Code permission-mode を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの permission-mode: {value}"),
        );
        self.push_claude_resident_permission_mode(previous);
    }

    fn set_approval(&mut self, value: CodexApprovalChoice) {
        self.exec_settings.approval = value;
        let value = self.exec_settings.approval.label();
        self.set_status_note(format!("approval: {value}"));
        self.push_activity(format!("Codex approval を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの approval: {value}"),
        );
        self.push_codex_appserver_approval();
    }

    pub fn cycle_sandbox(&mut self) {
        if self.kind == AgentKind::ClaudeCode {
            self.push_log(
                CodexLogKind::System,
                "Claude Code では権限は F4（permission-mode: config/plan/acceptEdits/dontAsk/bypassPermissions/skip-permissions）で切り替えます。F5 のサンドボックス設定は使いません",
            );
            return;
        }
        let value = self.exec_settings.cycle_sandbox();
        self.set_status_note(format!("sandbox: {value}"));
        self.push_activity(format!("Codex sandbox を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの sandbox: {value}"),
        );
        self.push_codex_appserver_sandbox();
    }

    fn set_sandbox(&mut self, value: CodexSandboxChoice) {
        self.exec_settings.sandbox = value;
        let value = self.exec_settings.sandbox.label();
        self.set_status_note(format!("sandbox: {value}"));
        self.push_activity(format!("Codex sandbox を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの sandbox: {value}"),
        );
        self.push_codex_appserver_sandbox();
    }

    /// `/reasoning` `/effort`。引数なしはピッカー、引数ありは従来どおり直接指定。
    fn handle_reasoning_slash_command(&mut self, args: &str) {
        if args.is_empty() {
            self.open_reasoning_picker();
            return;
        }
        if self.kind == AgentKind::ClaudeCode {
            if let Some(choice) = claude::parse_effort_choice(args) {
                self.set_claude_effort(choice);
            } else {
                self.push_log(
                    CodexLogKind::Error,
                    "effort は config / low / medium / high / xhigh / max を指定してください",
                );
            }
        } else if let Some(choice) = parse_reasoning_choice(args) {
            self.set_reasoning(choice);
        } else {
            self.push_log(
                CodexLogKind::Error,
                "reasoning は config / low / medium / high / xhigh を指定してください",
            );
        }
    }

    /// `/approval` `/approvals`。引数なしはピッカー、引数ありは従来どおり直接指定。
    fn handle_approval_slash_command(&mut self, args: &str) {
        if args.is_empty() {
            self.open_approval_picker();
            return;
        }
        if self.kind == AgentKind::ClaudeCode {
            if let Some(mode) = claude::parse_permission_mode(args) {
                self.set_claude_permission_mode(mode);
            } else {
                self.push_log(
                    CodexLogKind::Error,
                    "permission-mode は config / plan / acceptEdits / dontAsk / bypassPermissions / skip-permissions を指定してください",
                );
            }
        } else if let Some(choice) = parse_approval_choice(args) {
            self.set_approval(choice);
        } else {
            self.push_log(
                CodexLogKind::Error,
                "approval は config / untrusted / on-request / on-failure / never を指定してください",
            );
        }
    }

    /// `/sandbox`。Codex は引数なしでピッカー、引数ありで直接指定。
    /// Claude Code では従来どおり非対応の案内だけ出す（`cycle_sandbox` が案内して no-op）。
    fn handle_sandbox_slash_command(&mut self, args: &str) {
        if self.kind == AgentKind::ClaudeCode {
            self.cycle_sandbox();
            return;
        }
        if args.is_empty() {
            self.open_sandbox_picker();
            return;
        }
        if let Some(choice) = parse_sandbox_choice(args) {
            self.set_sandbox(choice);
        } else {
            self.push_log(
                CodexLogKind::Error,
                "sandbox は read-only / workspace-write / danger-full-access を指定してください",
            );
        }
    }

    pub fn toggle_web_search(&mut self) {
        let enabled = self.exec_settings.toggle_web_search();
        let value = on_off(enabled);
        self.set_status_note(format!("search: {value}"));
        self.push_activity(format!("Codex web search を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの search: {value}"),
        );
    }

    pub fn toggle_oss(&mut self) {
        let enabled = self.exec_settings.toggle_oss();
        let value = on_off(enabled);
        self.set_status_note(format!("oss: {value}"));
        self.push_activity(format!("Codex OSS mode を {value} に変更"));
        self.push_log(CodexLogKind::System, format!("次回ターンの oss: {value}"));
    }

    fn set_remote_addr(&mut self, addr: Option<String>) {
        self.exec_settings.remote_addr = addr;
        let value = self
            .exec_settings
            .remote_addr
            .as_deref()
            .unwrap_or("off")
            .to_string();
        self.set_status_note(format!("remote: {value}"));
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
            .unwrap_or("off")
            .to_string();
        self.set_status_note(format!("remote-auth-token-env: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの remote-auth-token-env: {value}"),
        );
    }

    fn toggle_no_alt_screen(&mut self) {
        self.exec_settings.no_alt_screen = !self.exec_settings.no_alt_screen;
        let value = on_off(self.exec_settings.no_alt_screen);
        self.set_status_note(format!("no-alt-screen: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの no-alt-screen: {value}"),
        );
    }

    pub fn cycle_local_provider(&mut self) {
        let value = self.exec_settings.cycle_local_provider();
        self.set_status_note(format!("local provider: {value}"));
        self.push_activity(format!("Codex local provider を {value} に変更"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの local provider: {value}"),
        );
    }

    fn set_profile(&mut self, profile: Option<String>) {
        self.exec_settings.profile = profile;
        let value = self
            .exec_settings
            .profile
            .as_deref()
            .unwrap_or("config")
            .to_string();
        self.set_status_note(format!("profile: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの profile: {value}"),
        );
    }

    fn add_image_path(&mut self, path: String) {
        self.exec_settings.image_paths.push(path.clone());
        let count = self.exec_settings.image_paths.len();
        self.set_status_note(format!("image: {count}件"));
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
        self.set_status_note(format!("image: {}件", self.exec_settings.image_paths.len()));
        self.push_log(CodexLogKind::System, format!("画像添付を削除: {removed}"));
    }

    fn clear_image_paths(&mut self) {
        self.exec_settings.image_paths.clear();
        self.set_status_note("image: cleared".to_string());
        self.push_log(CodexLogKind::System, "画像添付をクリアしました");
    }

    /// ClaudeCode では画像を CLI 引数で渡せないため、添付画像パスを
    /// プロンプト末尾に追記して Read ツールで読ませる。消費後はリストをクリアする。
    fn append_claude_image_paths(&mut self, prompt: String) -> String {
        if self.kind != AgentKind::ClaudeCode || self.exec_settings.image_paths.is_empty() {
            return prompt;
        }
        let mut out = prompt;
        out.push_str("\n\n添付画像（Readツールで内容を確認してください）:\n");
        for path in &self.exec_settings.image_paths {
            out.push_str(&format!("- {path}\n"));
        }
        self.exec_settings.image_paths.clear();
        out
    }

    fn add_writable_dir(&mut self, dir: String) {
        self.exec_settings.additional_dirs.push(dir.clone());
        let count = self.exec_settings.additional_dirs.len();
        self.set_status_note(format!("add-dir: {count}件"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンに追加書込ディレクトリを渡します: {dir}"),
        );
    }

    fn clear_writable_dirs(&mut self) {
        self.exec_settings.additional_dirs.clear();
        self.set_status_note("add-dir: cleared".to_string());
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
        self.set_status_note(format!("config: {count}件"));
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
        self.set_status_note(format!("{label}: set"));
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
        self.set_status_note(format!("{label}: cleared"));
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
        self.set_status_note("memories: addness-default".to_string());
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
        self.set_status_note(format!("{label}: cleared"));
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
        self.set_status_note("config: addness-default".to_string());
        self.push_log(
            CodexLogKind::System,
            "追加 config override をクリアし、記憶先をAddness既定値に戻しました",
        );
    }

    fn add_enabled_feature(&mut self, feature: String) {
        self.exec_settings.enabled_features.push(feature.clone());
        self.set_status_note(format!("enable: {feature}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンで feature を有効化: {feature}"),
        );
    }

    fn add_disabled_feature(&mut self, feature: String) {
        self.exec_settings.disabled_features.push(feature.clone());
        self.set_status_note(format!("disable: {feature}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンで feature を無効化: {feature}"),
        );
    }

    fn toggle_strict_config(&mut self) {
        self.exec_settings.strict_config = !self.exec_settings.strict_config;
        let value = on_off(self.exec_settings.strict_config);
        self.set_status_note(format!("strict-config: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの strict-config: {value}"),
        );
    }

    fn toggle_ignore_user_config(&mut self) {
        self.exec_settings.ignore_user_config = !self.exec_settings.ignore_user_config;
        let value = on_off(self.exec_settings.ignore_user_config);
        self.set_status_note(format!("ignore-user-config: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの ignore-user-config: {value}"),
        );
    }

    fn toggle_ignore_rules(&mut self) {
        self.exec_settings.ignore_rules = !self.exec_settings.ignore_rules;
        let value = on_off(self.exec_settings.ignore_rules);
        self.set_status_note(format!("ignore-rules: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの ignore-rules: {value}"),
        );
    }

    fn toggle_skip_git_repo_check(&mut self) {
        self.exec_settings.skip_git_repo_check = !self.exec_settings.skip_git_repo_check;
        let value = on_off(self.exec_settings.skip_git_repo_check);
        self.set_status_note(format!("skip-git-check: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの skip-git-repo-check: {value}"),
        );
    }

    fn toggle_ephemeral(&mut self) {
        self.exec_settings.ephemeral = !self.exec_settings.ephemeral;
        let value = on_off(self.exec_settings.ephemeral);
        self.set_status_note(format!("ephemeral: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの ephemeral: {value}"),
        );
    }

    fn toggle_bypass_approvals_and_sandbox(&mut self) {
        self.exec_settings.bypass_approvals_and_sandbox =
            !self.exec_settings.bypass_approvals_and_sandbox;
        let value = on_off(self.exec_settings.bypass_approvals_and_sandbox);
        self.set_status_note(format!("bypass approvals+sandbox: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの dangerously-bypass-approvals-and-sandbox: {value}"),
        );
    }

    fn toggle_bypass_hook_trust(&mut self) {
        self.exec_settings.bypass_hook_trust = !self.exec_settings.bypass_hook_trust;
        let value = on_off(self.exec_settings.bypass_hook_trust);
        self.set_status_note(format!("bypass-hook-trust: {value}"));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの dangerously-bypass-hook-trust: {value}"),
        );
    }

    fn set_output_schema(&mut self, path: Option<String>) {
        self.exec_settings.output_schema = path;
        let value = self
            .exec_settings
            .output_schema
            .as_deref()
            .unwrap_or("off")
            .to_string();
        self.set_status_note(format!("output-schema: {value}"));
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
            .unwrap_or("off")
            .to_string();
        self.set_status_note(format!("output-last-message: {value}"));
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
            self.set_status_note("diff view off".to_string());
            return;
        }
        self.diff_view = Some(git_diff_preview(Path::new(&self.cwd)));
        self.invalidate_rendered_history_metrics();
        self.scroll_to_live();
        self.set_status_note("diff view on".to_string());
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

    fn log_line_visible(
        &self,
        line: &CodexLogLine,
        query: &str,
        index: usize,
        live_tool_start: Option<usize>,
    ) -> bool {
        let matches_filter = matches_log_filter(line.kind, self.log_filter)
            || is_live_turn_tool_line(line.kind, index, live_tool_start);
        if !matches_filter {
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
            // トリムで末尾以外へ押し出された最後の ThreadId が失われうるため、
            // 生きている thread_id を末尾へ書き直して復元可能に保つ。
            if let Some(id) = self.thread_id.clone() {
                let record = CodexSessionRecord::ThreadId { id: Some(id) };
                if append_codex_session_record(&path, &record).is_ok() {
                    self.session_record_count = self.session_record_count.saturating_add(1);
                }
            }
        }
    }

    /// thread_id を設定し、変化した場合のみ `ThreadId` レコードを永続化する。
    /// Claude の init は毎ターン同じ id を再送するため、同値スキップで重複記録を防ぐ。
    /// ライブイベント/ユーザー操作由来の設定なので、復元フラグは常に解除する。
    fn set_thread_id(&mut self, id: Option<String>) {
        self.thread_id_restored = false;
        if self.thread_id == id {
            return;
        }
        self.thread_id = id.clone();
        self.persist_session_record(CodexSessionRecord::ThreadId { id });
    }

    /// 復元した thread_id での resume が失敗したとき、次ターンを新規セッションで開始する。
    /// 復元フラグが立っているときだけ作用し、tombstone を書いてフラグを下ろす。
    fn drop_restored_thread_on_failure(&mut self) {
        if !self.thread_id_restored {
            return;
        }
        self.set_thread_id(None);
        self.push_log(
            CodexLogKind::System,
            "保存されていた前回セッションの再開に失敗したため、次のターンは新規セッションで開始します",
        );
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
                CodexProcessEvent::Stdout(line) => self.handle_stdout_line(&line),
                CodexProcessEvent::Stderr(line) => self.handle_stderr_line(&line),
            }
        }

        // 常駐 Claude Code プロセスのポーリング（死亡検知・中断/設定タイムアウト・アイドル回収）。
        // 常駐時は self.child は None なので、下の従来ターン終了検知とは競合しない。
        if self.kind == AgentKind::ClaudeCode {
            changed |= self.poll_claude_resident();
        }
        // 常駐 codex app-server プロセスのポーリング（死亡検知・中断/設定タイムアウト・アイドル回収）。
        if self.kind == AgentKind::Codex {
            changed |= self.poll_codex_appserver();
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
                    // Claude は result イベント（turn_finished_by_event=true）で承認バナーを
                    // 立ててからプロセス終了するため、ここでバナーを消さない。
                    if self.kind == AgentKind::Codex {
                        self.pending_decision = None;
                    }
                    let name = self.kind.display_name();
                    if let Some(label) = command_label {
                        self.flush_child_process_output(status.success(), &label);
                        if status.success() {
                            self.refresh_current_turn_title();
                            let message = format!("{label} が完了しました");
                            self.push_log(CodexLogKind::System, message.clone());
                            self.push_terminal_notice(format!("{name} コマンド完了"), message);
                        } else {
                            let message = format!("{label} が失敗しました: {status}");
                            self.push_log(CodexLogKind::Error, message.clone());
                            self.refresh_current_turn_title();
                            self.push_terminal_notice(format!("{name} コマンド失敗"), message);
                        }
                    } else if !self.turn_finished_by_event {
                        // イベントで終了検知できなかったフォールバック経路。他 3 経路と同型に
                        // record_turn_duration() + push_turn_complete_notice() で締める。
                        if status.success() {
                            self.refresh_current_turn_title();
                            self.queue_completed_turn_body_record();
                            self.push_log(
                                CodexLogKind::System,
                                format!("{name} ターンが完了しました"),
                            );
                            self.record_turn_duration();
                            self.push_turn_complete_notice(
                                format!("{name} 完了"),
                                format!("{name} の出力が完了しました"),
                            );
                        } else {
                            let message = format!("{name} ターンが失敗しました: {status}");
                            self.push_log(CodexLogKind::Error, message.clone());
                            self.refresh_current_turn_title();
                            self.queue_completed_turn_body_record();
                            self.record_turn_duration();
                            self.push_turn_complete_notice(format!("{name} 失敗"), message);
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
                    let name = self.kind.display_name();
                    let message = format!("{name} 状態確認に失敗: {e}");
                    self.push_log(CodexLogKind::Error, message.clone());
                    self.push_terminal_notice(format!("{name} エラー"), message);
                    self.start_next_queued_turn_if_idle();
                    changed = true;
                }
            }
        }

        // 経過時間タイマー: 表示秒が繰り上がったフレームだけ再描画させる。
        changed |= self.manage_turn_timer();
        // 実行中スピナー: ターン実行中だけ一定間隔で回す。
        changed |= self.advance_activity_spin();
        // 承認待ちが長時間続いていたら見逃し防止の再通知を送る。
        self.remind_confirming_if_stale();

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
            Err(_) => {
                let name = self.kind.display_name();
                self.push_log(CodexLogKind::Event, format!("{name} 出力: {trimmed}"));
            }
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
        // 古い claude CLI が `--include-partial-messages` を拒否した場合、ストリーミングを
        // 無効化して現在のターンを一度だけ自動再試行する（sticky フラグで無限ループを防ぐ）。
        if self.kind == AgentKind::ClaudeCode
            && !self.claude_settings.no_partial_messages()
            && claude::stderr_indicates_no_partial_messages(trimmed)
        {
            self.disable_claude_partial_messages_and_retry();
            return;
        }
        // 旧 stderr ヒューリスティック承認はワンショット専用（常駐 app-server / 常駐 Claude では
        // 承認はサーバ発リクエスト / can_use_tool の正規経路で届くため使わない）。
        if self.codex_appserver.is_none()
            && self.claude_resident.is_none()
            && let Some(decision) = decision_banner("stderr", Some(trimmed))
        {
            self.set_pending_decision(decision);
        }
        if self.child_process_label.is_some() {
            self.child_process_error_output.push(trimmed.to_string());
            return;
        }
        let name = self.kind.display_name();
        self.push_log(CodexLogKind::Event, format!("{name} 通知: {trimmed}"));
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
        if self.kind == AgentKind::ClaudeCode {
            self.handle_claude_json_event(&value);
            return;
        }

        // 常駐 app-server の JSON-RPC メッセージは別経路で処理する。codex 0.142.5 は
        // 応答・通知に "jsonrpc":"2.0" を付けないため、キーの有無ではなく形で判定する。
        // ワンショット exec の snake_case イベント・codex サブコマンド JSON は looks_like_jsonrpc が false。
        if self.codex_appserver.is_some() && codex_appserver::looks_like_jsonrpc(&value) {
            self.handle_codex_appserver_message(&value);
            return;
        }

        if let Some(summary) = token_usage_summary(&value) {
            self.last_token_usage_label = Some(summary);
        }

        let event_type = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("event")
            .to_string();

        match event_type.as_str() {
            "thread.started" => {
                if let Some(thread_id) = string_at_any(&value, &["thread_id", "threadId", "id"]) {
                    self.set_thread_id(Some(thread_id));
                    self.push_log(CodexLogKind::System, "Codex セッションを開始しました");
                } else {
                    self.push_log(CodexLogKind::System, "Codex セッションを開始しました");
                }
            }
            "turn.started" => {
                self.turn_running = true;
                self.turn_finished_by_event = false;
                self.current_command = None;
                self.current_command_started_at = None;
                self.clear_recent_actions();
                self.pending_decision = None;
                self.begin_turn_work("依頼を確認中");
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
            "turn.completed" | "turn.finished" => {
                self.turn_running = false;
                self.turn_finished_by_event = true;
                self.current_command = None;
                self.current_command_started_at = None;
                self.streaming_assistant_index = None;
                self.pending_decision = None;
                self.refresh_current_turn_title();
                self.queue_completed_turn_body_record();
                self.end_turn_work("応答完了");
                self.push_log(CodexLogKind::System, "Codex の応答が完了しました");
                self.record_turn_duration();
                self.push_turn_complete_notice("Codex 完了", "Codex の出力が完了しました");
            }
            "turn.failed" => {
                self.turn_running = false;
                self.turn_finished_by_event = true;
                self.current_command = None;
                self.current_command_started_at = None;
                self.streaming_assistant_index = None;
                self.pending_decision = None;
                let message = nested_error_message(&value)
                    .or_else(|| first_text_field(&value))
                    .unwrap_or_else(|| "Codex ターンが失敗しました".to_string());
                self.push_log(CodexLogKind::Error, message.clone());
                self.refresh_current_turn_title();
                self.queue_completed_turn_body_record();
                self.push_turn_complete_notice("Codex 失敗", message);
                self.drop_restored_thread_on_failure();
            }
            "error" => {
                let message = nested_error_message(&value)
                    .or_else(|| first_text_field(&value))
                    .unwrap_or_else(|| "Codex エラー".to_string());
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex エラー", message);
                self.drop_restored_thread_on_failure();
            }
            _ => self.handle_generic_json_event(&event_type, &value),
        }
    }

    /// Claude Code の stream-json イベントを処理する（codex 経路のヒューリスティックは使わない）。
    fn handle_claude_json_event(&mut self, value: &Value) {
        match claude::event_type(value) {
            "system" => {
                if claude::event_subtype(value) == Some("init") {
                    if let Some(session_id) = claude::session_id(value) {
                        // session_id は codex の thread_id フィールドを共用する。
                        self.set_thread_id(Some(session_id));
                    }
                    // 毎ターン再送される init から現在の model/permissionMode を表示同期用に保存。
                    if let Some(model) = value.get("model").and_then(Value::as_str) {
                        self.claude_active_model = Some(model.to_string());
                    }
                    if let Some(mode) = value.get("permissionMode").and_then(Value::as_str) {
                        self.claude_active_permission_mode = Some(mode.to_string());
                    }
                    self.begin_claude_turn();
                }
                // thinking_tokens など他の system サブタイプは無視する。
            }
            "assistant" => self.handle_claude_assistant(value),
            "user" => self.handle_claude_tool_results(value),
            "control_request" => self.handle_claude_control_request(value),
            "control_cancel_request" => self.handle_claude_control_cancel(value),
            "control_response" => self.handle_claude_control_response(value),
            "result" => {
                // total_cost_usd / usage / session_id を保存する（表示は後続タスク）。
                if let Some(cost) = value.get("total_cost_usd").and_then(Value::as_f64) {
                    self.claude_total_cost_usd = Some(cost);
                }
                if let Some(ctx) = claude::context_tokens(value) {
                    self.claude_context_tokens = Some(ctx);
                }
                if let Some(session_id) = claude::session_id(value) {
                    self.set_thread_id(Some(session_id));
                }
                let result = claude::parse_result(value);
                self.handle_claude_result(result);
            }
            "stream_event" => {
                // `--include-partial-messages` の text_delta をトークン単位で逐次表示する。
                // thinking_delta / input_json_delta 等はここでは表示しない（完成形で出す）。
                if let Some(delta) = claude::stream_text_delta(value) {
                    self.append_assistant_delta(delta);
                }
            }
            "error" => {
                let message =
                    first_text_field(value).unwrap_or_else(|| "Claude Code エラー".to_string());
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Claude Code エラー", message);
                self.drop_restored_thread_on_failure();
            }
            // rate_limit_event など未知・非表示イベントは無視する。
            _ => {}
        }
    }

    /// Claude のターン開始表示（codex の thread.started + turn.started 相当）。
    fn begin_claude_turn(&mut self) {
        if self.turn_count > 0 {
            self.collapsed_turns.insert(self.turn_count);
        }
        self.turn_running = true;
        self.turn_finished_by_event = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.clear_recent_actions();
        self.begin_turn_work("依頼を確認中");
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

    fn handle_claude_assistant(&mut self, value: &Value) {
        // 逐次表示していたストリーミング行のインデックス（あれば）。
        // 最初の Text ブロックはこの行を完成形テキストで上書き（置換）し、2 番目以降の
        // Text ブロックは新しい Assistant 行として push する。単一 Text ブロック（現行 CLI の
        // 実挙動）では従来どおり逐次表示行がそのまま活きる見た目になり、複数 Text ブロックが
        // 1 イベントに同梱されても欠落・連結が起きない。
        let mut streaming_index = self.streaming_assistant_index.take();
        for block in claude::assistant_blocks(value) {
            match block {
                claude::ClaudeBlock::Text(text) => {
                    if let Some(index) = streaming_index.take() {
                        self.overwrite_assistant_line(index, text);
                    } else {
                        self.push_log(CodexLogKind::Assistant, text);
                    }
                }
                claude::ClaudeBlock::Thinking(text) => {
                    // reasoning 相当。Event 種別（薄色）で控えめに出す。
                    self.push_log(
                        CodexLogKind::Event,
                        format!("(思考) {}", compact_one_line(&text, 2_000)),
                    );
                }
                claude::ClaudeBlock::ToolUse {
                    name,
                    summary,
                    edit_patch,
                    id,
                    subagent,
                } => {
                    if let Some(summary) = &summary {
                        self.record_current_command(
                            RecentActionKind::Tool,
                            compact_tool_text(summary),
                        );
                    }
                    self.set_work_action(format!("ツール実行: {name}"));
                    if let Some(subagent) = subagent {
                        self.record_subagent_launch(id, subagent.label, subagent.agent_type);
                    }
                    // Edit/Write は変更内容を色付き差分プレビュー行（見出し + diff）として残す。
                    // ラベル行と重複しないよう、パッチがあればそれをツール実行行にする。
                    if let Some(patch) = edit_patch {
                        self.push_log(CodexLogKind::Tool, patch);
                    } else {
                        let label = match &summary {
                            Some(summary) => format!("{name} {}", compact_tool_text(summary)),
                            None => name.clone(),
                        };
                        self.push_log(CodexLogKind::Tool, label);
                    }
                }
            }
        }
    }

    /// ストリーミング済みの Assistant 行を完成形テキストで置換する。
    /// 行が既に trim 等で消えていれば新しい行として push する。
    fn overwrite_assistant_line(&mut self, index: usize, text: String) {
        let current = match self.log.get(index) {
            Some(line) => line.text.clone(),
            None => {
                self.push_log(CodexLogKind::Assistant, text);
                return;
            }
        };
        if current == text {
            return;
        }
        // ストリーミング済みテキストの続き（差分サフィックス）なら delta として永続化して
        // 再読込時の整合を保つ。前方一致しない場合はメモリ上のみ置換する。
        let suffix = text.strip_prefix(current.as_str()).map(str::to_string);
        if let Some(line) = self.log.get_mut(index) {
            line.text = text;
        }
        self.invalidate_rendered_history_metrics();
        if let Some(suffix) = suffix {
            self.persist_session_record(CodexSessionRecord::AssistantDelta { text: suffix });
        }
    }

    fn handle_claude_tool_results(&mut self, value: &Value) {
        for result in claude::tool_results(value) {
            self.current_command = None;
            self.current_command_started_at = None;
            self.resolve_subagent_result(result.tool_use_id.as_deref(), result.is_error);
            if result.is_error {
                let text = if result.text.is_empty() {
                    "(エラー詳細なし)".to_string()
                } else {
                    compact_one_line(&result.text, 500)
                };
                self.push_log(CodexLogKind::Error, format!("ツール失敗: {text}"));
            } else {
                let summary = child_process_output_summary(&result.text);
                self.push_log(CodexLogKind::Tool, format!("OUT {summary}"));
            }
        }
    }

    fn handle_claude_result(&mut self, result: claude::ClaudeResult) {
        if let Some(usage) = result.usage_label {
            self.claude_last_usage = Some(usage.clone());
            self.last_token_usage_label = Some(usage);
        }
        self.turn_running = false;
        self.turn_finished_by_event = true;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.refresh_current_turn_title();
        self.queue_completed_turn_body_record();
        self.record_turn_duration();
        self.claude_resident_last_activity = Some(Instant::now());

        // 常駐モードで interrupt 要求中に来た result（aborted_streaming）は中断完了として扱う。
        // 既存仕様どおり、中断時はキューを自動開始しない。
        if self.claude_interrupting {
            self.claude_interrupting = false;
            self.claude_interrupt_deadline = None;
            self.pending_decision = None;
            self.claude_pending_tool = None;
            self.current_turn_prompt = None;
            self.current_turn_retry_prompt = None;
            self.end_turn_work("中断完了");
            self.push_log(CodexLogKind::System, "ターンを中断しました");
            let queued = self.queued_prompts.len();
            if queued > 0 {
                self.push_log(
                    CodexLogKind::System,
                    format!("予約{queued}件は保留中（/stop queued で破棄可）"),
                );
            }
            return;
        }

        if !result.denials.is_empty() {
            // 常駐モードでは承認は can_use_tool で解決するため denials は基本来ない。
            // 来た場合（ワンショット経路）は従来どおりリトライバナーを出す。
            self.set_claude_permission_decision(&result.denials);
            return;
        }

        // 拒否なく完了 → ループガードの承認控えをクリアする。
        self.claude_approved_denials.clear();

        if result.is_error {
            let message = result
                .text
                .unwrap_or_else(|| "Claude Code ターンが失敗しました".to_string());
            self.push_log(CodexLogKind::Error, message.clone());
            self.push_turn_complete_notice("Claude Code 失敗", message);
            // result に有効な session_id が含まれていれば site③ が復元フラグを解除済みなので、
            // ここでの drop は「session_id を伴わないエラー」時のみ発火する。
            self.drop_restored_thread_on_failure();
        } else {
            self.end_turn_work("応答完了");
            self.push_log(CodexLogKind::System, "Claude Code の応答が完了しました");
            self.push_turn_complete_notice("Claude Code 完了", "Claude Code の出力が完了しました");
        }

        // 常駐モードはプロセスが終了しないので、ここでキュー再開を駆動する
        // （ワンショットは update() のプロセス終了検知が駆動するため二重にしない）。
        if self.claude_resident.is_some() {
            self.current_turn_prompt = None;
            self.current_turn_retry_prompt = None;
            self.start_next_queued_turn_if_idle();
        }
    }

    fn set_claude_permission_decision(&mut self, denials: &[claude::ClaudeDenial]) {
        let summary = denials
            .iter()
            .map(|denial| match &denial.target {
                Some(target) => {
                    format!("{}（{}）", denial.tool_name, compact_one_line(target, 80))
                }
                None => denial.tool_name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", ");

        // ループガード: 直前の承認リトライで許可したのと同じ拒否だけが再発した場合、
        // 生成した許可ルールでは通せていない。バナーを再表示せずエラーで知らせる。
        let approved = std::mem::take(&mut self.claude_approved_denials);
        if !approved.is_empty() && denials.iter().all(|d| approved.contains(d)) {
            self.claude_pending_allowed_tools.clear();
            self.claude_pending_denials.clear();
            self.push_log(
                CodexLogKind::Error,
                format!(
                    "許可ルールでは通せませんでした: {summary}。F4 で permission-mode の変更を検討してください"
                ),
            );
            self.push_terminal_notice(
                "Claude Code 権限エラー",
                "許可ルールでは通せませんでした。F4 で permission-mode の変更を検討してください",
            );
            return;
        }

        // 拒否された具体的なツールだけを許可するルールを生成する。
        let rules = claude::allowed_tool_rules(denials);
        self.claude_pending_allowed_tools = rules.clone();
        self.claude_pending_denials = denials.to_vec();
        let allow = if rules.is_empty() {
            String::new()
        } else {
            format!(" 許可: {}", rules.join(", "))
        };
        let message =
            format!("ツール実行権限が拒否されました: {summary}。{allow} を許可して続行しますか？");
        self.set_pending_decision(CodexDecisionBanner::new(
            CodexDecisionKind::Permission,
            message,
        ));
    }

    /// 承認 Accept/Always 後に `--resume` で続行ターンを開始する。
    fn resolve_claude_decision(&mut self, response: CodexDecisionResponse, response_label: &str) {
        // 常駐モードの can_use_tool 応答はプロセスへ control_response を返してその場で続行する
        // （ワンショットのような resume 再実行はしない）。
        if self.claude_pending_tool.is_some() {
            self.resolve_claude_resident_tool(response, response_label);
            return;
        }
        self.set_work_action(format!("確認応答: {response_label}"));
        self.push_activity(format!("確認待ちに {response_label} で応答"));
        let rules = std::mem::take(&mut self.claude_pending_allowed_tools);
        let denials = std::mem::take(&mut self.claude_pending_denials);
        let rules_label = if rules.is_empty() {
            String::new()
        } else {
            format!("（{}）", rules.join(", "))
        };
        match response {
            CodexDecisionResponse::Accept => {
                // リトライ結果で同一の拒否が再発したらループとみなすため許可内容を控える。
                self.claude_approved_denials = denials;
                self.claude_one_shot_allowed_tools = rules;
                self.start_claude_approval_retry(&format!("今回だけ許可{rules_label}"));
            }
            CodexDecisionResponse::Always => {
                self.claude_approved_denials = denials;
                self.claude_settings.add_allowed_tools(&rules);
                self.start_claude_approval_retry(&format!("これからずっと許可{rules_label}"));
            }
            CodexDecisionResponse::Deny => {
                self.claude_approved_denials.clear();
                self.push_terminal_notice("Claude Code 確認応答", "拒否したため作業を中断します");
                self.push_log(CodexLogKind::System, "拒否したため入力待ちに戻ります");
                self.start_next_queued_turn_if_idle();
            }
        }
    }

    fn start_claude_approval_retry(&mut self, reason: &str) {
        if self.thread_id.is_none() {
            self.push_log(
                CodexLogKind::Error,
                "セッションIDが取得できていないため作業を続行できません",
            );
            return;
        }
        let prompt = "先ほど拒否されたツール実行を許可しました。同じ作業を続行してください。";
        self.set_work_action(format!("{reason}: 続行ターン"));
        self.push_log(
            CodexLogKind::System,
            format!("{reason}の設定で作業を続行します"),
        );
        self.push_log(CodexLogKind::User, prompt.to_string());
        self.input_state.record_submitted(prompt);
        self.start_turn(prompt);
    }

    /// 古い claude CLI 検出時: ストリーミング用フラグを sticky で無効化し、
    /// 現在のターンを一度だけ自動再試行する。再試行対象が無ければ次ターンから自然に無効になる。
    fn disable_claude_partial_messages_and_retry(&mut self) {
        self.claude_settings.disable_partial_messages();
        self.push_log(
            CodexLogKind::System,
            "お使いのclaude CLIはストリーミング表示に未対応のため無効化しました（アップデート推奨）",
        );
        // フラグは既に立っているので、再試行ターンの stderr では再発火しない（1 回だけ）。
        self.retry_current_turn_after_permission_change("ストリーミング無効化");
    }

    // -----------------------------------------------------------------------
    // 常駐（多ターン 1 プロセス）モード
    // -----------------------------------------------------------------------

    /// このターンを常駐経路で送るか（ClaudeCode かつ常駐有効）。
    fn claude_resident_active(&self) -> bool {
        self.kind == AgentKind::ClaudeCode && self.claude_resident_enabled
    }

    /// 常駐 claude プロセスを spawn する（stdout/stderr は既存の line reader で読む）。
    fn spawn_claude_resident(&mut self) -> Result<claude_resident::ResidentClient> {
        let mut cmd = Command::new(&self.codex_bin);
        let developer_instructions = self.composed_developer_instructions();
        for arg in claude::resident_args(
            self.thread_id.as_deref(),
            &self.claude_settings,
            self.claude_fork_next,
            &developer_instructions,
        ) {
            cmd.arg(arg);
        }
        cmd.current_dir(&self.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        self.apply_agent_env(&mut cmd);

        let mut child = cmd
            .spawn()
            .context("Claude Code 常駐プロセスの起動に失敗しました")?;
        let stdout = child
            .stdout
            .take()
            .context("Claude Code 常駐プロセス stdout の取得に失敗しました")?;
        let stderr = child
            .stderr
            .take()
            .context("Claude Code 常駐プロセス stderr の取得に失敗しました")?;
        spawn_line_reader(stdout, self.tx.clone(), false);
        spawn_line_reader(stderr, self.tx.clone(), true);
        claude_resident::ResidentClient::new(child)
    }

    /// 常駐モードでターンを開始する（既存プロセスがあれば再利用、無ければ spawn）。
    /// spawn 自体に失敗した場合はワンショットへフォールバックする。
    fn start_claude_resident_turn(&mut self, prompt: &str, display_prompt: &str) {
        self.esc_interrupt_armed = false;
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::System,
                "前の Claude Code ターンがまだ実行中です",
            );
            return;
        }

        // グレースフル終了中の常駐は使わず、確実に終了させてから作り直す。
        if let Some(resident) = self.claude_resident.as_mut()
            && resident.closing
        {
            resident.kill();
            self.claude_resident = None;
        }

        if self.claude_resident.is_none() {
            match self.spawn_claude_resident() {
                Ok(client) => {
                    self.claude_resident = Some(client);
                    self.claude_fork_next = false;
                    let resume = if self.thread_id.is_some() {
                        "（--resume で再接続）"
                    } else {
                        ""
                    };
                    self.push_log(
                        CodexLogKind::System,
                        format!("Claude Code 常駐プロセスを起動しました{resume}"),
                    );
                }
                Err(e) => {
                    // 常駐 spawn 失敗 → このセッションはワンショットへ退避する。
                    self.claude_resident_enabled = false;
                    self.push_log(
                        CodexLogKind::System,
                        format!("常駐プロセスの起動に失敗したためワンショットで実行します: {e}"),
                    );
                    self.start_oneshot_turn(prompt, display_prompt);
                    return;
                }
            }
        }

        let full_prompt = self.prompt_with_addness_context(prompt);
        let full_prompt = self.append_claude_image_paths(full_prompt);
        let sent = self
            .claude_resident
            .as_ref()
            .is_some_and(|resident| resident.send_user_message(&full_prompt));
        if !sent {
            // writer が死んでいる → プロセス死亡として扱い、次ターンで再接続する。
            self.push_log(
                CodexLogKind::Error,
                "常駐プロセスへの送信に失敗しました。次のターンで再接続します",
            );
            self.handle_claude_resident_death();
            return;
        }

        self.turn_running = true;
        self.turn_finished_by_event = false;
        self.pending_decision = None;
        self.diff_view = None;
        self.claude_interrupting = false;
        self.claude_interrupt_deadline = None;
        self.claude_one_shot_allowed_tools = Vec::new();
        self.current_turn_prompt = Some(display_prompt.to_string());
        self.current_turn_retry_prompt = Some(prompt.to_string());
        self.claude_resident_last_activity = Some(Instant::now());
        self.scroll_to_live();
    }

    /// 常駐 claude プロセスのポーリング（毎フレーム `update()` から呼ぶ）。
    /// 死亡検知・interrupt タイムアウト・設定変更タイムアウト・アイドル回収を扱う。
    fn poll_claude_resident(&mut self) -> bool {
        // 1. 終了検知（死亡 or グレースフルクローズ完了）。
        let waited = match self.claude_resident.as_mut() {
            Some(resident) => resident.try_wait(),
            None => return false,
        };
        match waited {
            Ok(Some(_status)) => {
                let closing = self
                    .claude_resident
                    .as_ref()
                    .map(|r| r.closing)
                    .unwrap_or(false);
                self.claude_resident = None;
                if closing {
                    self.push_log(
                        CodexLogKind::System,
                        "アイドルのため Claude Code 常駐プロセスを終了しました（次のターンで再接続）",
                    );
                } else {
                    self.handle_claude_resident_death();
                }
                return true;
            }
            Ok(None) => {}
            Err(e) => {
                self.claude_resident = None;
                self.push_log(
                    CodexLogKind::Error,
                    format!("Claude Code 常駐プロセスの状態確認に失敗しました: {e}"),
                );
                self.handle_claude_resident_death();
                return true;
            }
        }

        // 2. interrupt グレースフル待ちのタイムアウト → kill フォールバック。
        if let Some(deadline) = self.claude_interrupt_deadline
            && Instant::now() >= deadline
        {
            if let Some(resident) = self.claude_resident.as_mut() {
                resident.kill();
            }
            self.claude_resident = None;
            self.claude_interrupt_deadline = None;
            self.claude_interrupting = false;
            // kill フォールバックでも Addness へ作業メモを記録する（current_turn_prompt を消す前に）。
            self.queue_completed_turn_body_record();
            self.turn_running = false;
            self.current_command = None;
            self.current_command_started_at = None;
            self.streaming_assistant_index = None;
            self.pending_decision = None;
            self.claude_pending_tool = None;
            self.current_turn_prompt = None;
            self.current_turn_retry_prompt = None;
            self.push_log(
                CodexLogKind::System,
                "中断が2秒以内に完了しなかったため常駐プロセスを終了しました（次のターンで再接続）",
            );
            return true;
        }

        // 3. 設定変更（set_model / set_permission_mode）応答のタイムアウト → 再起動フォールバック。
        if let Some(change) = self.claude_pending_setting_change.as_ref()
            && Instant::now() >= change.deadline
        {
            let label = change.label.clone();
            self.claude_pending_setting_change = None;
            self.claude_resident_restart_pending = true;
            self.push_log(
                CodexLogKind::System,
                format!("{label}の応答がタイムアウトしました。次のアイドルで再起動します"),
            );
            return true;
        }

        // 4. アイドル時: 設定変更の再起動保留があればグレースフルに閉じる（次ターンで再 spawn）。
        if self.claude_resident_restart_pending
            && !self.turn_running
            && self.pending_decision.is_none()
            && self.claude_pending_tool.is_none()
        {
            self.claude_resident_restart_pending = false;
            if let Some(resident) = self.claude_resident.as_mut() {
                resident.begin_close();
            }
            self.push_log(
                CodexLogKind::System,
                "設定を反映するため常駐プロセスを再起動します（次のターンで再接続）",
            );
            return true;
        }

        // 5. アイドル回収（無操作が続いたらメモリ節約のため閉じる）。
        if !self.turn_running
            && self.pending_decision.is_none()
            && self.claude_pending_tool.is_none()
            && let Some(last) = self.claude_resident_last_activity
            && last.elapsed() >= CLAUDE_RESIDENT_IDLE_TIMEOUT
        {
            self.claude_resident_last_activity = None;
            if let Some(resident) = self.claude_resident.as_mut() {
                resident.begin_close();
            }
            return true;
        }

        false
    }

    /// 常駐プロセスが予期せず死んだときの処理。実行中ターンはエラー終了扱い。
    fn handle_claude_resident_death(&mut self) {
        self.claude_resident = None;
        self.claude_interrupt_deadline = None;
        self.claude_interrupting = false;
        self.claude_pending_setting_change = None;
        let was_running = self.turn_running;
        if was_running {
            // 実行中の死亡/切断でも Addness へ作業メモを記録する（current_turn_prompt を消す前に）。
            self.queue_completed_turn_body_record();
        }
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.pending_decision = None;
        self.claude_pending_tool = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        if was_running {
            self.refresh_current_turn_title();
        }
        let message = "Claude Code プロセスが終了しました。次のターンで再接続します";
        self.push_log(CodexLogKind::Error, message);
        self.push_terminal_notice("Claude Code 切断", message);
        // 復元した session_id が init で裏取りされる前に死んだ場合は resume 失敗とみなし、
        // 同じ --resume で死亡を繰り返さないよう新規セッションへフォールバックする。
        self.drop_restored_thread_on_failure();
        // 中断・死亡時はキューを自動開始しない（ユーザー判断を待つ）。
    }

    /// 常駐モードで実行中ターンへ interrupt（グレースフル中断）を送る。
    /// 送れたら 2 秒の完了待ちを開始し `true` を返す。
    fn request_claude_interrupt(&mut self) -> bool {
        if self.claude_resident.is_none() {
            return false;
        }
        let sent = self
            .claude_resident
            .as_mut()
            .and_then(|resident| resident.send_interrupt())
            .is_some();
        if !sent {
            return false;
        }
        self.claude_interrupting = true;
        self.claude_interrupt_deadline = Some(Instant::now() + CLAUDE_INTERRUPT_GRACE);
        self.claude_resident_last_activity = Some(Instant::now());
        self.push_log(
            CodexLogKind::System,
            "中断を要求しました（2秒以内に完了しなければ再起動します）",
        );
        true
    }

    /// 常駐モードの can_use_tool 制御要求を処理する。
    /// セッション許可ルールにマッチすればバナーを出さず自動許可、そうでなければバナー表示。
    fn handle_claude_control_request(&mut self, value: &Value) {
        let Some(claude_resident::ResidentControl::CanUseTool(request)) =
            claude_resident::parse_control(value)
        else {
            return;
        };
        self.claude_resident_last_activity = Some(Instant::now());

        // セッション内許可ルールにマッチ → 届いた時点でバナーなし自動許可。
        if claude_resident::tool_matches_rules(
            &request.tool_name,
            &request.input,
            self.claude_settings.sticky_allowed_tools(),
        ) {
            if let Some(resident) = self.claude_resident.as_ref() {
                resident.send_allow(&request.request_id, &request.input);
            }
            self.push_log(
                CodexLogKind::Tool,
                format!("{} を自動許可（セッション許可ルール）", request.tool_name),
            );
            return;
        }

        // バナー表示（既存 CodexDecisionBanner を流用）。
        let rules = claude_resident::rules_for_request(&request.tool_name, &request.input);
        let summary = claude::tool_use_summary(&request.tool_name, &request.input);
        let target = summary
            .as_deref()
            .map(|s| format!("（{}）", compact_one_line(s, 80)))
            .unwrap_or_default();
        let allow = if rules.is_empty() {
            String::new()
        } else {
            format!(" 常に許可の対象: {}", rules.join(", "))
        };
        let message = format!(
            "ツール実行の許可を求めています: {}{target}。{allow}",
            request.tool_name
        );
        self.claude_pending_tool = Some(ClaudePendingTool {
            request_id: request.request_id,
            input: request.input,
            rules,
        });
        self.set_pending_decision(CodexDecisionBanner::new(
            CodexDecisionKind::Permission,
            message,
        ));
    }

    /// 承認要求のキャンセル（中断時など）。該当バナーを閉じ、応答は送らない。
    fn handle_claude_control_cancel(&mut self, value: &Value) {
        let Some(claude_resident::ResidentControl::Cancel { request_id }) =
            claude_resident::parse_control(value)
        else {
            return;
        };
        if self
            .claude_pending_tool
            .as_ref()
            .is_some_and(|pending| pending.request_id == request_id)
        {
            self.claude_pending_tool = None;
            self.pending_decision = None;
            self.push_log(CodexLogKind::System, "ツール承認要求がキャンセルされました");
        }
    }

    /// 自前で送った control_request（set_model / set_permission_mode）への応答処理。
    fn handle_claude_control_response(&mut self, value: &Value) {
        let Some(claude_resident::ResidentControl::Response(response)) =
            claude_resident::parse_control(value)
        else {
            return;
        };
        let Some(change) = self.claude_pending_setting_change.as_ref() else {
            return;
        };
        if change.request_id != response.request_id {
            return;
        }
        let label = change.label.clone();
        self.claude_pending_setting_change = None;
        if response.success {
            self.push_log(CodexLogKind::System, format!("{label}を反映しました"));
        } else {
            let detail = response.error.unwrap_or_else(|| "エラー".to_string());
            self.claude_resident_restart_pending = true;
            self.push_log(
                CodexLogKind::System,
                format!("{label}の実行時反映に失敗（{detail}）。次のアイドルで再起動します"),
            );
        }
    }

    /// 常駐モードの承認バナーへの応答を control_response としてプロセスへ返す。
    /// ワンショットのような resume 再実行はせず、進行中ターンをその場で続行させる。
    fn resolve_claude_resident_tool(
        &mut self,
        response: CodexDecisionResponse,
        response_label: &str,
    ) {
        let Some(pending) = self.claude_pending_tool.take() else {
            return;
        };
        self.set_work_action(format!("確認応答: {response_label}"));
        self.push_activity(format!("ツール承認に {response_label} で応答"));
        self.claude_resident_last_activity = Some(Instant::now());
        let Some(resident) = self.claude_resident.as_ref() else {
            return;
        };
        match response {
            CodexDecisionResponse::Accept => {
                resident.send_allow(&pending.request_id, &pending.input);
                self.push_log(CodexLogKind::System, "今回だけ許可して続行します");
            }
            CodexDecisionResponse::Always => {
                resident.send_allow(&pending.request_id, &pending.input);
                let rules_label = if pending.rules.is_empty() {
                    String::new()
                } else {
                    format!("（{}）", pending.rules.join(", "))
                };
                self.claude_settings.add_allowed_tools(&pending.rules);
                self.push_log(
                    CodexLogKind::System,
                    format!("これからずっと許可{rules_label}して続行します"),
                );
            }
            CodexDecisionResponse::Deny => {
                resident.send_deny(&pending.request_id, "ユーザーが拒否しました");
                self.push_log(
                    CodexLogKind::System,
                    "拒否しました（Claude はこのツールなしで続行します）",
                );
            }
        }
    }

    /// F2（model）を常駐プロセスへ set_model control_request で反映する。
    /// 具体値が無い（config）場合は再起動フォールバックへ回す。
    fn push_claude_resident_model(&mut self) {
        if self.claude_resident.is_none() {
            return;
        }
        let Some(model) = self
            .claude_settings
            .effective_model_arg()
            .map(str::to_string)
        else {
            self.claude_resident_restart_pending = true;
            return;
        };
        let request_id = self
            .claude_resident
            .as_mut()
            .and_then(|resident| resident.send_set_model(&model));
        match request_id {
            Some(request_id) => {
                self.claude_pending_setting_change = Some(ClaudePendingSettingChange {
                    request_id,
                    deadline: Instant::now() + CLAUDE_SETTING_CHANGE_GRACE,
                    label: format!("model 変更（{model}）"),
                });
            }
            None => self.claude_resident_restart_pending = true,
        }
    }

    /// F4（permission-mode）を常駐プロセスへ set_permission_mode control_request で反映する。
    ///
    /// `--dangerously-skip-permissions` は起動時フラグでランタイム切替できないため、この variant
    /// へ/から切り替えた場合は control_request を送らず `claude_resident_restart_pending` を立てて
    /// 次のアイドルで常駐プロセスを再起動させる（`previous` は切替前のモード）。
    fn push_claude_resident_permission_mode(&mut self, previous: claude::ClaudePermissionMode) {
        if self.claude_resident.is_none() {
            return;
        }
        let current = self.claude_settings.permission_mode_choice();
        if current.is_dangerously_skip() || previous.is_dangerously_skip() {
            self.claude_resident_restart_pending = true;
            self.push_log(
                CodexLogKind::System,
                "skip-permissions は起動時フラグのため、常駐プロセスを次のアイドルで再起動して反映します",
            );
            return;
        }
        let Some(mode) = self
            .claude_settings
            .effective_permission_mode_arg()
            .map(str::to_string)
        else {
            self.claude_resident_restart_pending = true;
            return;
        };
        let request_id = self
            .claude_resident
            .as_mut()
            .and_then(|resident| resident.send_set_permission_mode(&mode));
        match request_id {
            Some(request_id) => {
                self.claude_pending_setting_change = Some(ClaudePendingSettingChange {
                    request_id,
                    deadline: Instant::now() + CLAUDE_SETTING_CHANGE_GRACE,
                    label: format!("permission-mode 変更（{mode}）"),
                });
            }
            None => self.claude_resident_restart_pending = true,
        }
    }

    // -----------------------------------------------------------------------
    // Codex app-server 常駐モード
    // -----------------------------------------------------------------------

    fn codex_appserver_active(&self) -> bool {
        self.kind == AgentKind::Codex && self.codex_appserver_enabled
    }

    /// 常駐 codex app-server を spawn する（stdout/stderr は既存の line reader で読む）。
    fn spawn_codex_appserver(&mut self) -> Result<codex_appserver::AppServerClient> {
        let mut cmd = Command::new(&self.codex_bin);
        cmd.arg("app-server");
        cmd.current_dir(&self.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        self.apply_agent_env(&mut cmd);

        let mut child = cmd
            .spawn()
            .context("codex app-server 常駐プロセスの起動に失敗しました")?;
        let stdout = child
            .stdout
            .take()
            .context("codex app-server 常駐プロセス stdout の取得に失敗しました")?;
        let stderr = child
            .stderr
            .take()
            .context("codex app-server 常駐プロセス stderr の取得に失敗しました")?;
        spawn_line_reader(stdout, self.tx.clone(), false);
        spawn_line_reader(stderr, self.tx.clone(), true);
        codex_appserver::AppServerClient::new(child)
    }

    /// 現在の CodexExecSettings から thread/start・thread/resume 用の設定を組み立てる。
    fn codex_appserver_thread_config(&self) -> codex_appserver::ThreadConfig {
        let settings = &self.exec_settings;
        // dangerously-bypass-approvals-and-sandbox 相当は never + danger-full-access で表す。
        let (approval, sandbox) = if settings.bypass_approvals_and_sandbox {
            (Some("never".to_string()), "danger-full-access".to_string())
        } else {
            (
                settings.approval.cli_arg().map(str::to_string),
                settings.sandbox.cli_arg().to_string(),
            )
        };
        codex_appserver::ThreadConfig {
            cwd: Some(self.cwd.clone()),
            model: settings.model_cli_arg().map(str::to_string),
            approval_policy: approval,
            sandbox: Some(sandbox),
            developer_instructions: Some(self.composed_developer_instructions()),
        }
    }

    /// turn/start に載せる reasoning effort（None は codex の既定）。
    fn codex_appserver_effort(&self) -> Option<String> {
        self.exec_settings
            .reasoning
            .config_value()
            .map(str::to_string)
    }

    /// 常駐モードでターンを開始する（既存プロセスがあれば再利用、無ければ spawn しハンドシェイク）。
    fn start_codex_appserver_turn(&mut self, prompt: &str, display_prompt: &str) {
        self.esc_interrupt_armed = false;
        if self.is_turn_running() {
            self.push_log(CodexLogKind::System, "前の Codex ターンがまだ実行中です");
            return;
        }
        // グレースフル終了中の常駐は使わず、確実に終了させてから作り直す。
        if let Some(client) = self.codex_appserver.as_mut()
            && client.closing
        {
            client.kill();
            self.codex_appserver = None;
            self.codex_appserver_phase = CodexAppServerPhase::Idle;
        }

        let full_prompt = self.prompt_with_addness_context(prompt);

        if self.codex_appserver.is_none() {
            match self.spawn_codex_appserver() {
                Ok(client) => {
                    self.codex_appserver = Some(client);
                    self.codex_appserver_phase = CodexAppServerPhase::Idle;
                    if !self.send_codex_appserver_initialize() {
                        // writer が死んでいる → ワンショットへ退避する。
                        self.codex_appserver = None;
                        self.codex_appserver_phase = CodexAppServerPhase::Idle;
                        self.codex_appserver_enabled = false;
                        self.push_log(
                            CodexLogKind::Error,
                            "常駐プロセスへの initialize 送信に失敗したためワンショットで実行します",
                        );
                        self.start_oneshot_turn(prompt, display_prompt);
                        return;
                    }
                    let resume = if self.thread_id.is_some() {
                        "（thread/resume で再接続）"
                    } else {
                        ""
                    };
                    self.push_log(
                        CodexLogKind::System,
                        format!("Codex app-server 常駐プロセスを起動しました{resume}"),
                    );
                }
                Err(e) => {
                    self.codex_appserver_enabled = false;
                    self.push_log(
                        CodexLogKind::System,
                        format!("常駐プロセスの起動に失敗したためワンショットで実行します: {e}"),
                    );
                    self.start_oneshot_turn(prompt, display_prompt);
                    return;
                }
            }
        }

        self.codex_appserver_pending_turn = Some(full_prompt);
        // 添付画像は turn/start へ渡すため退避してから入力欄をクリアする。
        self.codex_appserver_pending_images = std::mem::take(&mut self.exec_settings.image_paths);
        self.turn_running = true;
        self.turn_finished_by_event = false;
        self.pending_decision = None;
        self.diff_view = None;
        self.codex_appserver_interrupting = false;
        self.codex_appserver_interrupt_deadline = None;
        self.current_turn_prompt = Some(display_prompt.to_string());
        self.current_turn_retry_prompt = Some(prompt.to_string());
        self.codex_appserver_last_activity = Some(Instant::now());
        self.scroll_to_live();

        // すでに thread 確立済みなら即座に turn/start する。未確立ならハンドシェイク完了時に送る。
        if self.codex_appserver_phase == CodexAppServerPhase::Ready {
            self.flush_codex_appserver_pending_turn();
        }
    }

    /// initialize リクエストを送り、フェーズを Initializing にする。送信可否を返す。
    fn send_codex_appserver_initialize(&mut self) -> bool {
        let Some(client) = self.codex_appserver.as_mut() else {
            return false;
        };
        let id = client.next_id();
        let request =
            codex_appserver::initialize_request(id, "addness-tui", env!("CARGO_PKG_VERSION"));
        if client.send_value(&request) {
            self.codex_appserver_phase = CodexAppServerPhase::Initializing { request_id: id };
            self.codex_appserver_handshake_deadline =
                Some(Instant::now() + CODEX_APPSERVER_HANDSHAKE_TIMEOUT);
            true
        } else {
            false
        }
    }

    /// thread 確立後に保留していたターン本文を turn/start で送る。
    fn flush_codex_appserver_pending_turn(&mut self) {
        let Some(text) = self.codex_appserver_pending_turn.take() else {
            return;
        };
        let Some(thread_id) = self.thread_id.clone() else {
            self.handle_codex_appserver_death();
            return;
        };
        let effort = self.codex_appserver_effort();
        let images = std::mem::take(&mut self.codex_appserver_pending_images);
        let Some(client) = self.codex_appserver.as_mut() else {
            return;
        };
        let id = client.next_id();
        let request =
            codex_appserver::turn_start_request(id, &thread_id, &text, effort.as_deref(), &images);
        if client.send_value(&request) {
            self.codex_appserver_turn_req_id = Some(id);
            self.codex_appserver_last_activity = Some(Instant::now());
            self.codex_appserver_handshake_deadline =
                Some(Instant::now() + CODEX_APPSERVER_HANDSHAKE_TIMEOUT);
        } else {
            self.push_log(
                CodexLogKind::Error,
                "常駐プロセスへの turn/start 送信に失敗しました。次のターンで再接続します",
            );
            self.handle_codex_appserver_death();
        }
    }

    /// 常駐プロセスへ initialized 通知 → thread/start（or resume）を送る。
    fn send_codex_appserver_start_thread(&mut self) {
        let config = self.codex_appserver_thread_config();
        let resume_target = self.thread_id.clone();
        let Some(client) = self.codex_appserver.as_mut() else {
            return;
        };
        if !client.send_value(&codex_appserver::initialized_notification()) {
            self.handle_codex_appserver_death();
            return;
        }
        let id = client.next_id();
        let (request, resuming) = match resume_target.as_deref() {
            Some(thread_id) => (
                codex_appserver::thread_resume_request(id, thread_id, &config),
                true,
            ),
            None => (codex_appserver::thread_start_request(id, &config), false),
        };
        if client.send_value(&request) {
            self.codex_appserver_phase = CodexAppServerPhase::StartingThread {
                request_id: id,
                resuming,
            };
            self.codex_appserver_handshake_deadline =
                Some(Instant::now() + CODEX_APPSERVER_HANDSHAKE_TIMEOUT);
        } else {
            self.handle_codex_appserver_death();
        }
    }

    /// resume に失敗したとき、新規 thread/start で作り直す。
    fn send_codex_appserver_fresh_thread(&mut self) {
        let config = self.codex_appserver_thread_config();
        let Some(client) = self.codex_appserver.as_mut() else {
            return;
        };
        let id = client.next_id();
        let request = codex_appserver::thread_start_request(id, &config);
        if client.send_value(&request) {
            self.codex_appserver_phase = CodexAppServerPhase::StartingThread {
                request_id: id,
                resuming: false,
            };
            self.codex_appserver_handshake_deadline =
                Some(Instant::now() + CODEX_APPSERVER_HANDSHAKE_TIMEOUT);
        } else {
            self.handle_codex_appserver_death();
        }
    }

    /// 常駐 codex app-server の JSON-RPC メッセージを処理する。
    fn handle_codex_appserver_message(&mut self, value: &Value) {
        let Some(message) = codex_appserver::parse_message(value) else {
            return;
        };
        self.codex_appserver_last_activity = Some(Instant::now());
        match message {
            codex_appserver::ServerMessage::Response { id, result, error } => {
                self.handle_codex_appserver_response(id, result, error);
            }
            codex_appserver::ServerMessage::Approval(request) => {
                self.handle_codex_appserver_approval(request);
            }
            codex_appserver::ServerMessage::UnhandledRequest { id, method } => {
                if let Some(client) = self.codex_appserver.as_ref() {
                    client.send_value(&codex_appserver::error_response(
                        &id,
                        -32601,
                        "method not supported by addness-tui",
                    ));
                }
                self.push_log(
                    CodexLogKind::Event,
                    format!("Codex 未対応リクエストにエラー応答: {method}"),
                );
            }
            codex_appserver::ServerMessage::Notification(notification) => {
                self.handle_codex_appserver_notification(notification);
            }
        }
    }

    fn handle_codex_appserver_response(
        &mut self,
        id: u64,
        result: Option<Value>,
        error: Option<codex_appserver::JsonRpcError>,
    ) {
        // 1. initialize 応答。
        if let CodexAppServerPhase::Initializing { request_id } = self.codex_appserver_phase
            && request_id == id
        {
            if let Some(error) = error {
                self.push_log(
                    CodexLogKind::Error,
                    format!("initialize に失敗しました（{}）", error.message),
                );
                self.fallback_codex_appserver_to_oneshot();
                return;
            }
            self.send_codex_appserver_start_thread();
            return;
        }

        // 2. thread/start・thread/resume 応答。
        if let CodexAppServerPhase::StartingThread {
            request_id,
            resuming,
        } = self.codex_appserver_phase
            && request_id == id
        {
            if let Some(error) = error {
                if resuming {
                    self.push_log(
                        CodexLogKind::System,
                        format!(
                            "thread/resume に失敗（{}）。新規スレッドで開始します",
                            error.message
                        ),
                    );
                    self.send_codex_appserver_fresh_thread();
                } else {
                    self.push_log(
                        CodexLogKind::Error,
                        format!("thread/start に失敗しました（{}）", error.message),
                    );
                    self.fallback_codex_appserver_to_oneshot();
                }
                return;
            }
            if let Some(thread_id) = result
                .as_ref()
                .and_then(codex_appserver::thread_id_from_response)
            {
                self.set_thread_id(Some(thread_id));
            }
            self.codex_appserver_phase = CodexAppServerPhase::Ready;
            self.push_log(CodexLogKind::System, "Codex セッションを開始しました");
            self.flush_codex_appserver_pending_turn();
            return;
        }

        // 3. turn/start 応答（turn.id 確定）。ここでハンドシェイク/ターン開始の待ちが完了する。
        if self.codex_appserver_turn_req_id == Some(id) {
            self.codex_appserver_turn_req_id = None;
            self.codex_appserver_handshake_deadline = None;
            if let Some(error) = error {
                self.push_log(
                    CodexLogKind::Error,
                    format!("Codex ターンの開始に失敗しました（{}）", error.message),
                );
                self.finish_codex_appserver_turn_failed();
                return;
            }
            if let Some(turn_id) = result
                .as_ref()
                .and_then(codex_appserver::turn_id_from_response)
            {
                self.codex_appserver_turn_id = Some(turn_id);
            }
            return;
        }

        // 4. thread/settings/update 応答。
        if let Some(change) = self.codex_appserver_pending_setting.as_ref()
            && change.request_id == id
        {
            let label = change.label.clone();
            self.codex_appserver_pending_setting = None;
            if let Some(error) = error {
                self.codex_appserver_restart_pending = true;
                self.push_log(
                    CodexLogKind::System,
                    format!(
                        "{label}の実行時反映に失敗（{}）。次のアイドルで再起動します",
                        error.message
                    ),
                );
            } else {
                self.push_log(CodexLogKind::System, format!("{label}を反映しました"));
            }
        }

        // それ以外（interrupt の {} 応答など）は無視する。
    }

    fn handle_codex_appserver_notification(&mut self, notification: codex_appserver::Notification) {
        use codex_appserver::Notification as N;
        match notification {
            N::TurnStarted => self.begin_codex_appserver_turn(),
            N::TurnCompleted { status } => self.handle_codex_appserver_turn_completed(&status),
            N::AgentMessageDelta { delta } => self.append_assistant_delta(&delta),
            N::ReasoningDelta { text } => {
                if !text.trim().is_empty() {
                    self.push_log(
                        CodexLogKind::Event,
                        format!("(思考) {}", compact_one_line(&text, 2_000)),
                    );
                }
            }
            N::ItemStarted(item) => self.handle_codex_appserver_item_started(item),
            N::ItemCompleted(item) => self.handle_codex_appserver_item_completed(item),
            N::CommandOutputDelta { item_id, delta } => {
                self.record_codex_appserver_output(&item_id, &delta);
            }
            N::TokenUsage(usage) => self.record_codex_appserver_token_usage(usage),
            N::Error { message } => {
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice("Codex エラー", message);
            }
            N::Ignored => {}
        }
    }

    /// turn/started（ターン境界の始まり）で見出し行を出す。
    fn begin_codex_appserver_turn(&mut self) {
        if self.turn_count > 0 {
            self.collapsed_turns.insert(self.turn_count);
        }
        self.current_command = None;
        self.current_command_started_at = None;
        self.clear_recent_actions();
        self.begin_turn_work("依頼を確認中");
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

    fn handle_codex_appserver_turn_completed(&mut self, status: &str) {
        self.turn_running = false;
        self.turn_finished_by_event = true;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.codex_appserver_turn_id = None;
        self.codex_appserver_turn_req_id = None;
        self.codex_appserver_output.clear();
        self.codex_appserver_running_item = None;
        self.codex_appserver_last_activity = Some(Instant::now());
        self.record_turn_duration();

        // interrupt 要求中に来た完了は中断完了として扱い、キューを自動開始しない。
        if self.codex_appserver_interrupting || status == "interrupted" {
            self.codex_appserver_interrupting = false;
            self.codex_appserver_interrupt_deadline = None;
            self.pending_decision = None;
            self.codex_appserver_pending_approval = None;
            self.current_turn_prompt = None;
            self.current_turn_retry_prompt = None;
            self.end_turn_work("中断完了");
            self.refresh_current_turn_title();
            self.push_log(CodexLogKind::System, "ターンを中断しました");
            let queued = self.queued_prompts.len();
            if queued > 0 {
                self.push_log(
                    CodexLogKind::System,
                    format!("予約{queued}件は保留中（Enter で開始 / /stop queued で破棄）"),
                );
            }
            return;
        }

        self.pending_decision = None;
        self.codex_appserver_pending_approval = None;
        self.refresh_current_turn_title();
        self.queue_completed_turn_body_record();
        if status == "failed" {
            let message = "Codex ターンが失敗しました".to_string();
            self.push_log(CodexLogKind::Error, message.clone());
            self.push_turn_complete_notice("Codex 失敗", message);
        } else {
            self.end_turn_work("応答完了");
            self.push_log(CodexLogKind::System, "Codex の応答が完了しました");
            self.push_turn_complete_notice("Codex 完了", "Codex の出力が完了しました");
        }
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.start_next_queued_turn_if_idle();
    }

    /// turn/start 応答がエラーだったときにターンを失敗終了させる。
    fn finish_codex_appserver_turn_failed(&mut self) {
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.codex_appserver_turn_id = None;
        self.pending_decision = None;
        self.codex_appserver_pending_approval = None;
        self.codex_appserver_pending_turn = None;
        self.refresh_current_turn_title();
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.push_terminal_notice("Codex 失敗", "Codex ターンの開始に失敗しました");
        self.start_next_queued_turn_if_idle();
    }

    fn handle_codex_appserver_item_started(&mut self, item: codex_appserver::ThreadItemInfo) {
        use codex_appserver::ThreadItemKind as K;
        match item.kind {
            K::CommandExecution { command, .. } => {
                self.record_current_command(RecentActionKind::Command, compact_tool_text(&command));
                self.refresh_action_from_text(&command);
                self.codex_appserver_running_item = Some(item.id.clone());
                self.codex_appserver_output.insert(item.id, VecDeque::new());
                self.push_log(
                    CodexLogKind::Tool,
                    format!("$ {}", compact_tool_text(&command)),
                );
            }
            K::McpToolCall { server, tool, .. } => {
                let label = format!("{server}.{tool}");
                self.record_current_command(RecentActionKind::Mcp, label.clone());
                self.set_work_action(format!("ツール実行: {label}"));
                self.push_log(CodexLogKind::Tool, format!("MCP {label}"));
            }
            K::FileChange { changes, .. } => {
                let paths = codex_appserver_change_paths(&changes);
                let label = codex_appserver_paths_label(&paths);
                // 「今なにをしているか」表示へ反映する（Claude Code 経路と対称）。
                self.record_current_command(RecentActionKind::FileChange, label.clone());
                self.set_work_action(format!("ファイル変更: {label}"));
                self.push_log(CodexLogKind::Tool, format!("ファイル変更 {label}"));
            }
            // reasoning / agentMessage はデルタ・完了で扱う。
            _ => {}
        }
    }

    fn handle_codex_appserver_item_completed(&mut self, item: codex_appserver::ThreadItemInfo) {
        use codex_appserver::ThreadItemKind as K;
        match item.kind {
            K::CommandExecution {
                command,
                exit_code,
                duration_ms,
                ..
            } => {
                self.current_command = None;
                self.current_command_started_at = None;
                self.codex_appserver_output.remove(&item.id);
                if self.codex_appserver_running_item.as_deref() == Some(item.id.as_str()) {
                    self.codex_appserver_running_item = None;
                }
                let exit = exit_code
                    .map(|code| format!("exit {code}"))
                    .unwrap_or_else(|| "終了".to_string());
                let duration = duration_ms.map(|ms| format!(" {ms}ms")).unwrap_or_default();
                let state = if exit_code == Some(0) { "OK" } else { "FAIL" };
                self.push_log(
                    CodexLogKind::Tool,
                    format!("{state} {} ({exit}{duration})", compact_tool_text(&command)),
                );
            }
            K::AgentMessage { text, .. } => {
                // デルタで逐次表示済みならストリーミング行を確定するだけ。
                if self.streaming_assistant_index.is_some() {
                    self.streaming_assistant_index = None;
                } else if !text.trim().is_empty() {
                    self.push_log(CodexLogKind::Assistant, text);
                }
            }
            K::Reasoning { summary, content } => {
                let text = if !summary.is_empty() {
                    summary.join(" ")
                } else {
                    content.join(" ")
                };
                if !text.trim().is_empty() {
                    self.push_log(
                        CodexLogKind::Event,
                        format!("(思考) {}", compact_one_line(&text, 2_000)),
                    );
                }
            }
            K::FileChange { changes, status } => {
                self.current_command = None;
                self.current_command_started_at = None;
                let paths = codex_appserver_change_paths(&changes);
                let label = codex_appserver_paths_label(&paths);
                let status = status.unwrap_or_else(|| "完了".to_string());
                self.push_log(CodexLogKind::Tool, format!("ファイル変更 {status} {label}"));
                // unified diff を持つ変更は、見出し + 色付き差分プレビュー行として残す。
                if let Some(patch) = codex_filechange_patch_text(&changes) {
                    self.push_log(CodexLogKind::Tool, patch);
                }
            }
            K::McpToolCall {
                server,
                tool,
                status,
            } => {
                self.current_command = None;
                self.current_command_started_at = None;
                let status = status.unwrap_or_else(|| "完了".to_string());
                self.push_log(CodexLogKind::Tool, format!("MCP {server}.{tool} {status}"));
            }
            K::Other(_) => {}
        }
    }

    /// 実行中コマンドの stdout ストリーミングを末尾 N 行のリングバッファへ蓄積する。
    fn record_codex_appserver_output(&mut self, item_id: &str, delta: &str) {
        let buffer = self
            .codex_appserver_output
            .entry(item_id.to_string())
            .or_default();
        // delta は複数行を含みうる。行ごとにリングバッファへ push し、末尾 N 行だけ残す。
        for line in delta.split_inclusive('\n') {
            let line = line.trim_end_matches('\n');
            if line.is_empty() {
                continue;
            }
            let sanitized = sanitize_terminal_line(line);
            if sanitized.trim().is_empty() {
                continue;
            }
            buffer.push_back(compact_one_line(&sanitized, 200));
            while buffer.len() > CODEX_APPSERVER_OUTPUT_TAIL_LINES {
                buffer.pop_front();
            }
        }
        self.codex_appserver_running_item = Some(item_id.to_string());
    }

    /// 実行中コマンドのライブ出力（末尾 N 行）。描画側が実行中 Tool 行の下に薄色で出す。
    pub fn codex_appserver_live_output(&self) -> Vec<String> {
        self.codex_appserver_running_item
            .as_ref()
            .and_then(|item| self.codex_appserver_output.get(item))
            .map(|buffer| buffer.iter().cloned().collect())
            .unwrap_or_default()
    }

    fn record_codex_appserver_token_usage(&mut self, usage: codex_appserver::TokenUsageInfo) {
        let mut parts = Vec::new();
        if let Some(total) = usage.total_tokens {
            parts.push(format!("total {total}"));
        }
        if let Some(last) = usage.last_total_tokens {
            parts.push(format!("last {last}"));
        }
        if let Some(window) = usage.model_context_window {
            parts.push(format!("context {window}"));
        }
        if !parts.is_empty() {
            self.last_token_usage_label = Some(parts.join(" / "));
        }
        self.codex_appserver_token_usage = Some(usage);
    }

    /// サーバ発の承認リクエストを既存 CodexDecisionBanner で提示する。
    fn handle_codex_appserver_approval(&mut self, request: codex_appserver::ApprovalRequest) {
        use codex_appserver::ApprovalKind as K;
        let (heading, allow_hint) = match request.kind {
            K::Command => ("コマンド実行の許可を求めています", "許可すると実行します"),
            K::FileChange => ("ファイル変更の許可を求めています", "許可すると適用します"),
            K::Permissions => ("権限昇格の許可を求めています", "許可すると権限を付与します"),
        };
        let target = compact_one_line(&request.summary, 100);
        let session = if request.allows_session() {
            " l=このセッション中は許可"
        } else {
            ""
        };
        let message = format!("{heading}: {target}。{allow_hint}。{session}");
        self.codex_appserver_pending_approval = Some(request);
        self.codex_appserver_last_activity = Some(Instant::now());
        self.set_pending_decision(CodexDecisionBanner::new(
            CodexDecisionKind::Permission,
            message,
        ));
    }

    /// 承認バナーの応答を JSON-RPC result としてプロセスへ返す（進行中ターンをその場で続行させる）。
    fn resolve_codex_appserver_approval(
        &mut self,
        response: CodexDecisionResponse,
        response_label: &str,
    ) {
        use codex_appserver::ApprovalKind as K;
        let Some(request) = self.codex_appserver_pending_approval.take() else {
            return;
        };
        self.set_work_action(format!("確認応答: {response_label}"));
        self.push_activity(format!("ツール承認に {response_label} で応答"));
        self.codex_appserver_last_activity = Some(Instant::now());
        let allows_session = request.allows_session();
        let Some(client) = self.codex_appserver.as_ref() else {
            return;
        };
        let reply = match request.kind {
            K::Command | K::FileChange => {
                let decision = match response {
                    CodexDecisionResponse::Accept => "accept",
                    CodexDecisionResponse::Always if allows_session => "acceptForSession",
                    CodexDecisionResponse::Always => "accept",
                    CodexDecisionResponse::Deny => "decline",
                };
                codex_appserver::approval_result(&request.id, decision)
            }
            K::Permissions => {
                // permissions は decline が無く、付与プロファイル + scope で応答する。
                let (granted, scope) = match response {
                    CodexDecisionResponse::Accept => (
                        request
                            .requested_permissions
                            .clone()
                            .unwrap_or(serde_json::json!({})),
                        "turn",
                    ),
                    CodexDecisionResponse::Always => (
                        request
                            .requested_permissions
                            .clone()
                            .unwrap_or(serde_json::json!({})),
                        "session",
                    ),
                    // 拒否は「何も付与しない」で表す。
                    CodexDecisionResponse::Deny => (serde_json::json!({}), "turn"),
                };
                codex_appserver::permissions_result(&request.id, granted, scope)
            }
        };
        client.send_value(&reply);
        match response {
            CodexDecisionResponse::Accept => {
                self.push_log(CodexLogKind::System, "今回だけ許可して続行します")
            }
            CodexDecisionResponse::Always => {
                self.push_log(CodexLogKind::System, "このセッション中は許可して続行します")
            }
            CodexDecisionResponse::Deny => self.push_log(
                CodexLogKind::System,
                "拒否しました（Codex はこの操作なしで続行します）",
            ),
        }
    }

    /// 常駐モードで実行中ターンへ turn/interrupt（グレースフル中断）を送る。
    /// 送れたら 2 秒の完了待ちを開始し `true` を返す。
    fn request_codex_appserver_interrupt(&mut self) -> bool {
        let (Some(thread_id), Some(turn_id)) =
            (self.thread_id.clone(), self.codex_appserver_turn_id.clone())
        else {
            return false;
        };
        let Some(client) = self.codex_appserver.as_mut() else {
            return false;
        };
        let id = client.next_id();
        let request = codex_appserver::turn_interrupt_request(id, &thread_id, &turn_id);
        if !client.send_value(&request) {
            return false;
        }
        self.codex_appserver_interrupting = true;
        self.codex_appserver_interrupt_deadline =
            Some(Instant::now() + CODEX_APPSERVER_INTERRUPT_GRACE);
        self.codex_appserver_last_activity = Some(Instant::now());
        self.push_log(
            CodexLogKind::System,
            "中断を要求しました（2秒以内に完了しなければ再起動します）",
        );
        true
    }

    /// 常駐 codex app-server のポーリング（毎フレーム `update()` から呼ぶ）。
    fn poll_codex_appserver(&mut self) -> bool {
        // 1. 終了検知（死亡 or グレースフルクローズ完了）。
        let waited = match self.codex_appserver.as_mut() {
            Some(client) => client.try_wait(),
            None => return false,
        };
        match waited {
            Ok(Some(_status)) => {
                let closing = self
                    .codex_appserver
                    .as_ref()
                    .map(|c| c.closing)
                    .unwrap_or(false);
                self.codex_appserver = None;
                self.codex_appserver_phase = CodexAppServerPhase::Idle;
                if closing {
                    self.push_log(
                        CodexLogKind::System,
                        "アイドルのため Codex app-server 常駐プロセスを終了しました（次のターンで再接続）",
                    );
                } else {
                    self.handle_codex_appserver_death();
                }
                return true;
            }
            Ok(None) => {}
            Err(e) => {
                self.codex_appserver = None;
                self.codex_appserver_phase = CodexAppServerPhase::Idle;
                self.push_log(
                    CodexLogKind::Error,
                    format!("Codex app-server 常駐プロセスの状態確認に失敗しました: {e}"),
                );
                self.handle_codex_appserver_death();
                return true;
            }
        }

        // 2. interrupt グレースフル待ちのタイムアウト → kill フォールバック。
        if let Some(deadline) = self.codex_appserver_interrupt_deadline
            && Instant::now() >= deadline
        {
            if let Some(client) = self.codex_appserver.as_mut() {
                client.kill();
            }
            self.codex_appserver = None;
            self.codex_appserver_phase = CodexAppServerPhase::Idle;
            self.codex_appserver_interrupt_deadline = None;
            self.codex_appserver_interrupting = false;
            self.turn_running = false;
            self.current_command = None;
            self.current_command_started_at = None;
            self.streaming_assistant_index = None;
            self.pending_decision = None;
            self.codex_appserver_pending_approval = None;
            self.codex_appserver_turn_id = None;
            self.codex_appserver_pending_turn = None;
            self.current_turn_prompt = None;
            self.current_turn_retry_prompt = None;
            self.push_log(
                CodexLogKind::System,
                "中断が2秒以内に完了しなかったため常駐プロセスを終了しました（次のターンで再接続）",
            );
            return true;
        }

        // 3. ハンドシェイク / turn/start 応答のタイムアウト → ワンショットへフォールバック。
        // codex 0.142.5 が想定外の応答形を返して固まっても、ここで自動復旧して
        // 「考え中」フリーズを解消する。
        if let Some(deadline) = self.codex_appserver_handshake_deadline
            && Instant::now() >= deadline
        {
            self.codex_appserver_handshake_deadline = None;
            self.push_log(
                CodexLogKind::Error,
                "Codex app-server の応答が15秒以内に得られなかったためワンショットで実行します",
            );
            self.fallback_codex_appserver_to_oneshot();
            return true;
        }

        // 4. 設定変更応答のタイムアウト → 再起動フォールバック。
        if let Some(change) = self.codex_appserver_pending_setting.as_ref()
            && Instant::now() >= change.deadline
        {
            let label = change.label.clone();
            self.codex_appserver_pending_setting = None;
            self.codex_appserver_restart_pending = true;
            self.push_log(
                CodexLogKind::System,
                format!("{label}の応答がタイムアウトしました。次のアイドルで再起動します"),
            );
            return true;
        }

        // 5. アイドル時: 再起動保留があればグレースフルに閉じる（次ターンで再 spawn）。
        if self.codex_appserver_restart_pending
            && !self.turn_running
            && self.pending_decision.is_none()
            && self.codex_appserver_pending_approval.is_none()
        {
            self.codex_appserver_restart_pending = false;
            if let Some(client) = self.codex_appserver.as_mut() {
                client.begin_close();
            }
            self.push_log(
                CodexLogKind::System,
                "設定を反映するため常駐プロセスを再起動します（次のターンで再接続）",
            );
            return true;
        }

        // 6. アイドル回収（無操作が続いたら閉じる）。
        if !self.turn_running
            && self.pending_decision.is_none()
            && self.codex_appserver_pending_approval.is_none()
            && let Some(last) = self.codex_appserver_last_activity
            && last.elapsed() >= CODEX_APPSERVER_IDLE_TIMEOUT
        {
            self.codex_appserver_last_activity = None;
            if let Some(client) = self.codex_appserver.as_mut() {
                client.begin_close();
            }
            return true;
        }

        false
    }

    /// 常駐プロセスが予期せず死んだときの処理。実行中ターンはエラー終了扱い。
    fn handle_codex_appserver_death(&mut self) {
        self.codex_appserver = None;
        self.codex_appserver_phase = CodexAppServerPhase::Idle;
        self.codex_appserver_interrupt_deadline = None;
        self.codex_appserver_handshake_deadline = None;
        self.codex_appserver_interrupting = false;
        self.codex_appserver_pending_setting = None;
        self.codex_appserver_pending_approval = None;
        self.codex_appserver_pending_turn = None;
        self.codex_appserver_pending_images.clear();
        self.codex_appserver_turn_id = None;
        self.codex_appserver_turn_req_id = None;
        self.codex_appserver_restart_pending = false;
        self.codex_appserver_output.clear();
        self.codex_appserver_running_item = None;
        let was_running = self.turn_running;
        if was_running {
            // 実行中の死亡/切断でも Addness へ作業メモを記録する（current_turn_prompt を消す前に）。
            self.queue_completed_turn_body_record();
        }
        self.turn_running = false;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.pending_decision = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        if was_running {
            self.refresh_current_turn_title();
        }
        let message = "Codex プロセスが終了しました。次のターンで再接続します";
        self.push_log(CodexLogKind::Error, message);
        self.push_terminal_notice("Codex 切断", message);
    }

    /// initialize / thread/start に失敗したとき、常駐を諦めてワンショットで同じターンを実行する。
    fn fallback_codex_appserver_to_oneshot(&mut self) {
        if let Some(mut client) = self.codex_appserver.take() {
            client.kill();
        }
        self.codex_appserver_phase = CodexAppServerPhase::Idle;
        self.codex_appserver_enabled = false;
        self.codex_appserver_handshake_deadline = None;
        self.codex_appserver_pending_turn = None;
        self.codex_appserver_turn_id = None;
        self.codex_appserver_turn_req_id = None;
        let retry = self.current_turn_retry_prompt.clone();
        let display = self.current_turn_prompt.clone();
        // 進行中ターン状態をいったんクリアしてからワンショットへ切り替える。
        self.turn_running = false;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.push_log(
            CodexLogKind::System,
            "常駐初期化に失敗したためワンショットで実行します",
        );
        // 常駐 turn/start へ渡せなかった添付画像はワンショットの -i 引数へ戻す。
        if !self.codex_appserver_pending_images.is_empty() {
            self.exec_settings.image_paths =
                std::mem::take(&mut self.codex_appserver_pending_images);
        }
        if let Some(prompt) = retry {
            let display = display.unwrap_or_else(|| prompt.clone());
            self.start_oneshot_turn(&prompt, &display);
        }
    }

    /// 常駐 codex app-server を強制終了し状態をクリアする（`/stop` kill・ペイン切替など）。
    fn teardown_codex_appserver(&mut self) {
        if let Some(mut client) = self.codex_appserver.take() {
            client.kill();
        }
        self.codex_appserver_phase = CodexAppServerPhase::Idle;
        self.codex_appserver_interrupt_deadline = None;
        self.codex_appserver_handshake_deadline = None;
        self.codex_appserver_interrupting = false;
        self.codex_appserver_pending_setting = None;
        self.codex_appserver_pending_approval = None;
        self.codex_appserver_pending_turn = None;
        self.codex_appserver_pending_images.clear();
        self.codex_appserver_turn_id = None;
        self.codex_appserver_turn_req_id = None;
        self.codex_appserver_restart_pending = false;
        self.codex_appserver_output.clear();
        self.codex_appserver_running_item = None;
    }

    /// F2（model）を thread/settings/update で反映する。具体値が無ければ再起動フォールバック。
    fn push_codex_appserver_model(&mut self) {
        let value = self.exec_settings.model_cli_arg().map(str::to_string);
        let update = codex_appserver::SettingsUpdate {
            model: value.clone(),
            ..Default::default()
        };
        self.push_codex_appserver_setting(update, "model 変更", value.is_some());
    }

    /// F3（effort）を thread/settings/update で反映する。
    fn push_codex_appserver_effort(&mut self) {
        let value = self.codex_appserver_effort();
        let update = codex_appserver::SettingsUpdate {
            effort: value.clone(),
            ..Default::default()
        };
        self.push_codex_appserver_setting(update, "effort 変更", value.is_some());
    }

    /// F4（approval）を thread/settings/update で反映する。
    fn push_codex_appserver_approval(&mut self) {
        let value = if self.exec_settings.bypass_approvals_and_sandbox {
            Some("never".to_string())
        } else {
            self.exec_settings.approval.cli_arg().map(str::to_string)
        };
        let update = codex_appserver::SettingsUpdate {
            approval_policy: value.clone(),
            ..Default::default()
        };
        self.push_codex_appserver_setting(update, "approval 変更", value.is_some());
    }

    /// F5（sandbox）を thread/settings/update で反映する。
    fn push_codex_appserver_sandbox(&mut self) {
        let value = if self.exec_settings.bypass_approvals_and_sandbox {
            "danger-full-access".to_string()
        } else {
            self.exec_settings.sandbox.cli_arg().to_string()
        };
        let update = codex_appserver::SettingsUpdate {
            sandbox: Some(value),
            ..Default::default()
        };
        self.push_codex_appserver_setting(update, "sandbox 変更", true);
    }

    /// thread/settings/update を送る共通処理。具体値が無い（config）場合は再起動フォールバック。
    fn push_codex_appserver_setting(
        &mut self,
        update: codex_appserver::SettingsUpdate,
        label: &str,
        had_value: bool,
    ) {
        // 常駐が無い / thread 未確立なら次の thread/start が新設定を載せるので何もしない。
        if self.codex_appserver.is_none()
            || self.codex_appserver_phase != CodexAppServerPhase::Ready
            || self.thread_id.is_none()
        {
            return;
        }
        if !had_value {
            // config へ戻す変更は settings/update では表現できないため再起動で反映する。
            self.codex_appserver_restart_pending = true;
            return;
        }
        let thread_id = self.thread_id.clone().unwrap_or_default();
        let Some(client) = self.codex_appserver.as_mut() else {
            return;
        };
        let id = client.next_id();
        let Some(request) = codex_appserver::settings_update_request(id, &thread_id, &update)
        else {
            return;
        };
        if client.send_value(&request) {
            self.codex_appserver_pending_setting = Some(CodexAppServerSettingChange {
                request_id: id,
                deadline: Instant::now() + CODEX_APPSERVER_SETTING_CHANGE_GRACE,
                label: label.to_string(),
            });
        } else {
            self.codex_appserver_restart_pending = true;
        }
    }

    /// 起動する子プロセスへ Addness 文脈の環境変数を設定する（3 系統の spawn で共有）。
    fn apply_agent_env(&self, cmd: &mut Command) {
        // 後方互換: codex のときだけ従来の ADDNESS_TUI_CODEX を維持する。
        if self.kind == AgentKind::Codex {
            cmd.env("ADDNESS_TUI_CODEX", "1");
        }
        cmd.env("ADDNESS_TUI_BACKEND", self.kind.backend_env_value());
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
                    self.record_current_command(RecentActionKind::Tool, compact_tool_text(command));
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
                self.record_current_command(RecentActionKind::Tool, compact_tool_text(&text));
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
            self.set_work_action(label);
        }
    }

    fn set_pending_decision(&mut self, decision: CodexDecisionBanner) {
        // 新しいバナーは既定でペイン固有アクションを持たない（/undo だけが直後に立てる）。
        self.pending_decision_action = None;
        let message = decision.message.clone();
        self.pending_decision = Some(decision);
        // 承認待ち経過時間の起点をリセットする（リマインド通知の間隔もここから数える）。
        let now = Instant::now();
        self.pending_decision_started_at = Some(now);
        self.last_confirming_reminder_at = Some(now);
        let name = self.kind.display_name();
        self.push_terminal_notice(format!("{name} 確認待ち"), message);
    }

    fn resolve_pending_decision(
        &mut self,
        decision: CodexDecisionBanner,
        response: CodexDecisionResponse,
        auto: bool,
    ) {
        self.pending_decision = None;
        // Addness 独自アクション（/undo など）に紐づく確認は codex/claude 経路へ流さず個別処理する。
        if let Some(pane_action) = self.pending_decision_action.take() {
            self.resolve_pane_decision_action(pane_action, response);
            return;
        }
        let response_label = decision.label_for_response(response);
        if auto {
            self.set_work_action(format!("確認自動応答: {response_label}"));
            self.push_activity(format!("確認待ちに {response_label} で自動応答"));
            let name = self.kind.display_name();
            self.push_terminal_notice(
                format!("{name} 確認自動応答"),
                format!("{response_label} を自動選択しました"),
            );
            return;
        }

        if self.kind == AgentKind::ClaudeCode {
            self.resolve_claude_decision(response, response_label);
            return;
        }

        // 常駐 app-server の承認は JSON-RPC result を返してその場で続行する（resume 再実行しない）。
        if self.codex_appserver_pending_approval.is_some() {
            self.resolve_codex_appserver_approval(response, response_label);
            return;
        }

        self.set_work_action(format!("確認応答: {response_label}"));
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
        self.set_work_action(format!("{reason}: turn再実行中"));
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

    // ----- ターン単位チェックポイント / undo -----

    /// ターン開始時（ユーザープロンプト実行時）にチェックポイント作成を予約する。
    /// 実際の git 操作はホスト（App）が非同期ジョブで行う。
    fn request_checkpoint(&mut self) {
        let slug = checkpoint_slug(&self.goal_id);
        let seq = self.checkpoint_seq;
        self.checkpoint_seq = self.checkpoint_seq.saturating_add(1);
        let ref_name = checkpoint_ref_name(&slug, seq);
        // turn_count はターンが実際に始まると増える。ここでは次ターン番号を見込みで使う。
        let turn = self.turn_count.saturating_add(1);
        // 上限（4 件）を超えたら最古を捨てて無制限に溜まらないようにする。
        const MAX_PENDING_CHECKPOINTS: usize = 4;
        while self.pending_checkpoint_requests.len() >= MAX_PENDING_CHECKPOINTS {
            self.pending_checkpoint_requests.pop_front();
        }
        self.pending_checkpoint_requests
            .push_back(CheckpointRequest {
                cwd: self.cwd.clone(),
                ref_name,
                turn,
                message: format!("addness-checkpoint-turn-{turn}"),
            });
    }

    /// ホストがチェックポイント作成要求を取り出す（キュー先頭から 1 件ずつ）。
    pub fn take_checkpoint_request(&mut self) -> Option<CheckpointRequest> {
        self.pending_checkpoint_requests.pop_front()
    }

    /// ホストが undo（復元）要求を取り出す。
    pub fn take_undo_request(&mut self) -> Option<UndoRequest> {
        self.pending_undo_request.take()
    }

    /// 作成済みチェックポイント ref をスタックへ記録し、上限超過で掃除すべき ref 名を返す。
    pub fn record_checkpoint(&mut self, ref_name: String, turn: usize) -> Vec<String> {
        let evicted = push_checkpoint_with_evictions(
            &mut self.checkpoints,
            Checkpoint { ref_name, turn },
            CHECKPOINT_STACK_MAX,
        );
        evicted.into_iter().map(|cp| cp.ref_name).collect()
    }

    /// undo 結果をペインへ通知する（ホストの非同期ジョブ完了時に呼ぶ）。
    pub fn note_undo_result(&mut self, turn: usize, error: Option<String>) {
        match error {
            None => {
                self.set_work_action("undo 完了".to_string());
                self.push_log(
                    CodexLogKind::System,
                    format!(
                        "turn {turn} のチェックポイントへ作業ツリーを戻しました。チェックポイント以降に新規作成したファイルは残っています。"
                    ),
                );
            }
            Some(e) => {
                self.push_log(CodexLogKind::Error, format!("undo に失敗しました: {e}"));
            }
        }
    }

    /// `/undo` コマンド。直近チェックポイントへ戻す前に YesNo 確認バナーを出す。
    fn handle_undo_slash_command(&mut self) {
        if self.checkpoints.is_empty() {
            self.push_log(CodexLogKind::System, "チェックポイントがありません");
            return;
        }
        // 実行中のターンに割り込んで作業ツリーを書き換えると危険なので受け付けない。
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::System,
                "ターン実行中は /undo できません。完了を待つか /stop で中断してください",
            );
            return;
        }
        if self.pending_decision.is_some() {
            self.push_log(
                CodexLogKind::System,
                "確認応答の待機中です。先に応答してから /undo してください",
            );
            return;
        }
        let turn = self.checkpoints.last().map(|cp| cp.turn).unwrap_or(0);
        let message = format!(
            "turn {turn} のチェックポイントへこのペインの作業ディレクトリ配下を戻します（新規作成ファイルは残ります）。よろしいですか？"
        );
        // set_pending_decision がアクションをクリアするため、バナー設定後に立てる。
        self.set_pending_decision(CodexDecisionBanner::new(CodexDecisionKind::YesNo, message));
        self.pending_decision_action = Some(PaneDecisionAction::Undo);
    }

    /// YesNo 確認に対するペイン固有アクションの解決。
    fn resolve_pane_decision_action(
        &mut self,
        action: PaneDecisionAction,
        response: CodexDecisionResponse,
    ) {
        match action {
            PaneDecisionAction::Undo => match response {
                CodexDecisionResponse::Accept => self.request_undo(),
                _ => {
                    self.set_work_action("undo 取消".to_string());
                    self.push_log(CodexLogKind::System, "undo を取り消しました");
                }
            },
        }
    }

    /// スタック末尾のチェックポイントを取り出し、復元要求をホストへ渡す。
    fn request_undo(&mut self) {
        let Some(cp) = self.checkpoints.pop() else {
            self.push_log(CodexLogKind::System, "チェックポイントがありません");
            return;
        };
        self.push_log(
            CodexLogKind::System,
            format!("turn {} のチェックポイントへ復元を開始します", cp.turn),
        );
        self.pending_undo_request = Some(UndoRequest {
            cwd: self.cwd.clone(),
            ref_name: cp.ref_name,
            turn: cp.turn,
        });
    }

    /// 通知の文脈ラベル（ゴール名）。空なら None。
    fn notice_context(&self) -> Option<String> {
        let title = self.goal_title.trim();
        (!title.is_empty()).then(|| title.to_string())
    }

    fn push_terminal_notice(&mut self, title: impl Into<String>, message: impl Into<String>) {
        let context = self.notice_context();
        self.pending_notices.push_back(TerminalNotice {
            title: title.into(),
            message: message.into(),
            context,
            turn_elapsed_secs: None,
        });
    }

    /// ターン完了（正常/失敗）通知。ターン所要秒を添えて、短いターンの通知抑制に使う。
    fn push_turn_complete_notice(&mut self, title: impl Into<String>, message: impl Into<String>) {
        let context = self.notice_context();
        // turn_started_at はこの時点でまだ保持されている（manage_turn_timer が別途クリアする）。
        let elapsed = self.turn_started_at.map(|t| t.elapsed().as_secs());
        self.pending_notices.push_back(TerminalNotice {
            title: title.into(),
            message: message.into(),
            context,
            turn_elapsed_secs: elapsed,
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
            // Esc 経路と同様、常駐はまず graceful interrupt を試み、駄目なら kill に落とす。
            if self.claude_resident.is_some() && self.request_claude_interrupt() {
                return;
            }
            if self.codex_appserver.is_some() && self.request_codex_appserver_interrupt() {
                return;
            }
            self.kill_current_turn();
            self.start_next_queued_turn_if_idle();
            return;
        }

        if self.pending_decision.is_some() {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('c' | 'C'))
            {
                if self.claude_resident.is_some() && self.request_claude_interrupt() {
                    return;
                }
                if self.codex_appserver.is_some() && self.request_codex_appserver_interrupt() {
                    return;
                }
                self.kill_current_turn();
            }
            return;
        }

        // 通常の入力キーが来たら Esc 中断のアームは解除する。
        self.esc_interrupt_armed = false;
        let observed = self.input_state.observe_key(key);
        // 入力が変わったらパレットの選択を先頭へ戻す（↑↓/Tab は app 側で消費済み）。
        self.slash_palette_selected = 0;
        self.mention_palette_selected = 0;
        // 入力が動いたらメンションパレットの一時非表示（Esc）を解除して再表示できるようにする。
        self.mention_palette_dismissed = false;
        let Some(submitted) = observed else {
            return;
        };
        self.submit_user_line(&submitted);
    }

    pub fn paste_input(&mut self, text: &str) {
        if self.finished || self.pending_decision.is_some() {
            return;
        }
        let normalized = normalize_input_text(text);
        if paste_should_fold(&normalized) {
            // 長文は入力欄にプレースホルダだけ挿入し、全文はペイン側に保持する。
            self.paste_seq += 1;
            let line_count = paste_line_count(&normalized);
            let placeholder = paste_placeholder(self.paste_seq, &normalized);
            self.input_state.insert_text(&placeholder);
            self.pending_pastes.push(StoredPaste {
                placeholder,
                full: normalized,
            });
            self.set_work_action(format!("貼り付けを折り畳み: {line_count}行"));
        } else {
            self.input_state.insert_text(text);
        }
    }

    /// 送信直前に、入力行中のプレースホルダを保持中の全文へ展開する。
    /// ユーザーが編集して完全一致しなくなったプレースホルダはそのまま送る。
    /// 展開後は保持データと通し番号をクリアする。
    fn expand_pending_pastes(&mut self, line: String) -> String {
        if self.pending_pastes.is_empty() {
            return line;
        }
        let expanded = expand_paste_placeholders(&line, &self.pending_pastes);
        self.pending_pastes.clear();
        self.paste_seq = 0;
        expanded
    }

    /// Ctrl+V: クリップボードの画像を `~/.addness/attachments/` へ保存し、入力欄にパスを挿入する。
    /// macOS 以外は未対応。外部コマンド失敗はメッセージ表示のみ（パニック・ハングしない）。
    pub fn attach_clipboard_image(&mut self) {
        if self.finished || self.pending_decision.is_some() {
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let Some(dir) = self.attachments_dir.clone() else {
                self.push_log(CodexLogKind::Error, "画像の保存先ディレクトリが未設定です");
                return;
            };
            match capture_clipboard_image_to_dir(&dir) {
                Ok(Some(path)) => {
                    let path_str = path.display().to_string();
                    self.input_state.insert_text(&path_str);
                    self.set_work_action("クリップボード画像を添付".to_string());
                    self.push_log(
                        CodexLogKind::System,
                        format!("クリップボード画像を保存しパスを挿入しました: {path_str}"),
                    );
                }
                Ok(None) => {
                    self.push_log(CodexLogKind::System, "クリップボードに画像がありません");
                }
                Err(e) => {
                    self.push_log(
                        CodexLogKind::Error,
                        format!("クリップボード画像の取得に失敗しました: {e}"),
                    );
                }
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            self.push_log(
                CodexLogKind::System,
                "クリップボード画像の添付はこの環境では未対応です",
            );
        }
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

    /// 送信されたユーザー入力を入力履歴（メモリ＋ファイル）へ記録する。
    /// 直前と同一・空・`/exit` は記録しない。
    fn record_input_history(&mut self, submitted: &str) {
        if submitted == "/exit" {
            return;
        }
        if self.input_state.push_history(submitted.to_string())
            && let Some(path) = self.input_history_path.clone()
        {
            let _ = append_input_history(&path, submitted);
        }
    }

    fn submit_user_line(&mut self, submitted: &str) {
        let submitted = normalize_submitted_line(submitted);
        if submitted.is_empty() {
            return;
        }

        // 畳み込んだ長文ペーストのプレースホルダを全文へ展開してから履歴・実行へ渡す。
        let submitted = self.expand_pending_pastes(submitted);

        self.record_input_history(&submitted);

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
            self.set_work_action(format!("次ターン予約 {count}件"));
            let name = self.kind.display_name();
            self.push_log(
                CodexLogKind::System,
                format!("{name} 実行中のため次のターンに予約しました（待ち{count}件）"),
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

        // ターン開始（ユーザープロンプト送信）ごとにチェックポイントを予約する。
        self.request_checkpoint();
        let exec_prompt = self.prompt_with_goal_mode(&submitted);
        self.start_turn_with_display_prompt(&exec_prompt, &display_prompt);
    }

    fn finish_from_exit_command(&mut self) {
        self.finished = true;
        // 常駐/子プロセスを即座に落とす（アイドル回収を待たない）。turn_running も
        // false になり、should_close_after_exit_command が成立してクローズ経路へ進む。
        self.teardown_all_processes();
        self.queued_prompts.clear();
        self.pending_decision = None;
        self.current_command = None;
        self.current_command_started_at = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        let name = self.kind.display_name();
        self.push_log(
            CodexLogKind::System,
            format!("{name} セッションを終了します"),
        );
    }

    fn start_turn(&mut self, prompt: &str) {
        self.start_turn_with_display_prompt(prompt, prompt);
    }

    fn start_turn_with_display_prompt(&mut self, prompt: &str, display_prompt: &str) {
        // ClaudeCode の常駐モードは 1 プロセスで多ターンを回す（別経路）。
        if self.claude_resident_active() {
            self.start_claude_resident_turn(prompt, display_prompt);
            return;
        }
        // Codex の app-server 常駐モードも 1 プロセスで多ターンを回す（別経路）。
        if self.codex_appserver_active() {
            self.start_codex_appserver_turn(prompt, display_prompt);
            return;
        }
        self.start_oneshot_turn(prompt, display_prompt);
    }

    /// ワンショット（1 ターン 1 プロセス）でターンを開始する。codex 経路と、常駐を無効化した
    /// / 常駐 spawn に失敗した ClaudeCode 経路で使う。
    fn start_oneshot_turn(&mut self, prompt: &str, display_prompt: &str) {
        // 新しいターンでは前ターンの Esc 中断アームを持ち越さない。
        self.esc_interrupt_armed = false;
        let name = self.kind.display_name();
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::System,
                format!("前の {name} ターンがまだ実行中です"),
            );
            return;
        }

        let retry_prompt = prompt.to_string();
        let prompt = self.prompt_with_addness_context(prompt);
        let prompt = self.append_claude_image_paths(prompt);
        let command_result = self.spawn_exec_process(&prompt);
        self.one_shot_approval = None;
        self.claude_one_shot_allowed_tools = Vec::new();
        self.claude_fork_next = false;
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
                let message = format!("{name} の起動に失敗しました: {e}");
                self.push_log(CodexLogKind::Error, message.clone());
                self.push_terminal_notice(format!("{name} 起動失敗"), message);
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
        // パレット非表示の codex 専用コマンドは、手入力でも実行させない
        // （ClaudeCode のターンに反映されない設定を黙って変更しないため）。
        if !slash_command_visible_for_kind(&format!("/{command}"), self.kind) {
            self.push_log(
                CodexLogKind::System,
                format!("/{command} は codex 専用コマンドです。Claude Code では利用できません"),
            );
            return true;
        }
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
            "fork" => {
                if self.kind == AgentKind::ClaudeCode {
                    // Claude では bare /fork を /fork-last（引数なし）/ /fork-session（引数あり）へ委譲。
                    if args.is_empty() {
                        self.handle_fork_last_slash_command("", false);
                    } else {
                        self.handle_fork_session_slash_command(args, false);
                    }
                } else {
                    self.handle_root_session_command_slash_command("fork", args);
                }
                true
            }
            "codex-fork" => {
                // codex 専用（Claude では start_codex_subcommand のガードで拒否される）。
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
                    self.open_model_picker();
                } else {
                    self.set_model(args);
                }
                true
            }
            "reasoning" | "effort" => {
                self.handle_reasoning_slash_command(args);
                true
            }
            "lang" | "language" => {
                self.handle_language_slash_command(args);
                true
            }
            "approval" | "approvals" => {
                self.handle_approval_slash_command(args);
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
                self.handle_sandbox_slash_command(args);
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
                if args.is_empty() {
                    self.open_session_picker(20);
                } else {
                    self.handle_resume_session_slash_command(args, false);
                }
                true
            }
            "resume-memo" => {
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
            "undo" => {
                self.handle_undo_slash_command();
                true
            }
            "exit" | "quit" => {
                // ターン実行中でも即終了させる（キュー予約に回さない）。
                self.input_state.exit_command_sent = true;
                self.finish_from_exit_command();
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
        let developer_instructions = self.composed_developer_instructions();
        let args = codex_named_subcommand_args_with_settings(
            args,
            &self.exec_settings,
            &developer_instructions,
        );
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
        let name = self.kind.display_name();
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::Error,
                format!("{name} 実行中です。完了後に新しいセッションを開始してください"),
            );
            return;
        }
        // アイドル常駐が生きていると thread_id を捨てても旧セッションを継続してしまうため、
        // 新規セッション開始の前に常駐を落として次ターンを新 spawn で始める。
        self.teardown_claude_resident();
        self.teardown_codex_appserver();
        self.set_thread_id(None);
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.current_command = None;
        self.current_command_started_at = None;
        self.streaming_assistant_index = None;
        self.pending_decision = None;
        // 旧セッションの checkpoint/予約を持ち越さない（新セッションで /undo が旧 checkpoint へ
        // 巻き戻る事故を防ぐ。finished ペインのファクトリと同じ 3 フィールドを揃えてクリア）。
        self.checkpoints.clear();
        self.checkpoint_seq = 0;
        self.pending_checkpoint_requests.clear();
        self.queued_prompts.clear();
        self.set_status_note(format!("新しい{name}セッション"));
        self.push_log(
            CodexLogKind::System,
            format!("新しい {name} セッションを開始します。次の入力は新規セッションへ送信します"),
        );
    }

    fn handle_clear_log_slash_command(&mut self) {
        self.log.clear();
        self.collapsed_turns.clear();
        self.scrollback = 0;
        self.streaming_assistant_index = None;
        self.invalidate_rendered_history_metrics();
        let name = self.kind.display_name();
        self.push_log(
            CodexLogKind::System,
            format!("{name} 表示ログをクリアしました"),
        );
    }

    fn handle_stop_slash_command(&mut self, args: &str) {
        if self.is_turn_running() {
            if self.claude_resident.is_some() && self.request_claude_interrupt() {
                // graceful interrupt を送った。完了は result / タイムアウトで検知する。
            } else if self.codex_appserver.is_some() && self.request_codex_appserver_interrupt() {
                // graceful interrupt を送った。完了は turn/completed / タイムアウトで検知する。
            } else {
                self.kill_current_turn();
            }
        } else {
            let name = self.kind.display_name();
            self.push_log(
                CodexLogKind::System,
                format!("停止中の{name}ターンはありません"),
            );
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
            self.set_work_action(format!("作業分解を予約 {count}件"));
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
            self.set_work_action("子ゴール一覧を表示".to_string());
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
            self.set_work_action(format!("子ゴール着手を予約 {count}件"));
            self.push_log(
                CodexLogKind::System,
                format!("子ゴール着手を次のターンに予約しました（待ち{count}件）"),
            );
            return;
        }

        self.set_work_action(action);
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
            self.set_work_action(format!("子ゴール一括着手を予約 {added}件"));
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
        let name = self.kind.display_name();
        self.submit_system_line(&format!(
            "Use the {name} skill named `{args}` for the next response if it is available and relevant."
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
        let developer_instructions = self.composed_developer_instructions();
        let args = codex_named_subcommand_args_with_settings(
            args,
            &self.exec_settings,
            &developer_instructions,
        );
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_root_interactive_slash_command(&mut self, prompt: &str) {
        let developer_instructions = self.composed_developer_instructions();
        let args = codex_root_interactive_args(
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
        );
        let label = format!("codex {}", command_preview(&args));
        self.start_codex_subcommand(args, label);
    }

    fn handle_review_slash_command(&mut self, args: &str) {
        let developer_instructions = self.composed_developer_instructions();
        let args = match codex_review_args(
            args,
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
        ) {
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
        let developer_instructions = self.composed_developer_instructions();
        let args = match codex_exec_review_args(
            args,
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
        ) {
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
        if self.kind == AgentKind::ClaudeCode {
            self.push_log(
                CodexLogKind::System,
                "これは codex 専用コマンドです。Claude Code では利用できません",
            );
            return;
        }
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
                self.clear_recent_actions();
                self.record_current_command(RecentActionKind::Command, label.clone());
                self.child_process_label = Some(label.clone());
                self.child_process_output.clear();
                self.child_process_error_output.clear();
                self.set_work_action(format!("{category}: {label}"));
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

    /// ClaudeCode の cwd に対応するセッション候補を読み込む。
    fn load_claude_session_candidates(&self, limit: usize) -> Vec<CodexSessionCandidate> {
        match claude::config_dir() {
            Some(dir) => claude::load_session_candidates_from(&dir, &self.cwd, limit),
            None => Vec::new(),
        }
    }

    /// ClaudeCode の resume/fork をペイン内ターンとして開始する。
    fn start_claude_resume(&mut self, session_ref: Option<&str>, fork: bool, prompt_arg: &str) {
        if self.is_turn_running() {
            self.push_log(
                CodexLogKind::Error,
                "Claude Code 実行中です。完了後に再開してください",
            );
            return;
        }
        let session_id = match session_ref {
            None => match self.load_claude_session_candidates(1).into_iter().next() {
                Some(candidate) => candidate.id,
                None => {
                    self.push_log(
                        CodexLogKind::Error,
                        "このフォルダの Claude Code セッションが見つかりません",
                    );
                    return;
                }
            },
            Some(session_ref) => match self.resolve_claude_session_ref(session_ref) {
                Some(id) => id,
                None => return,
            },
        };
        // アイドル常駐が生きていると新 thread_id/--fork-session が無視されるため、
        // resume/fork の前に必ず常駐を落として新規 spawn で反映させる。
        self.teardown_claude_resident();
        // 旧セッションの承認バナー・予約・実行中ターン情報を持ち越さない（/new と揃える）。
        self.pending_decision = None;
        self.queued_prompts.clear();
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.set_thread_id(Some(session_id.clone()));
        self.claude_fork_next = fork;
        let verb = if fork { "fork" } else { "resume" };
        self.push_log(
            CodexLogKind::System,
            format!(
                "Claude Code セッションを {verb} します: {}",
                short_session_id(&session_id)
            ),
        );
        let prompt = if prompt_arg.trim().is_empty() {
            resume_prompt().to_string()
        } else {
            prompt_arg.trim().to_string()
        };
        self.record_and_run_user_line(prompt);
    }

    fn resolve_claude_session_ref(&mut self, session_ref: &str) -> Option<String> {
        if let Some(index) = parse_one_based_index(session_ref) {
            if self.indexed_sessions.is_empty() {
                self.indexed_sessions = self.load_claude_session_candidates(12);
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

    fn handle_sessions_slash_command(&mut self, args: &str) {
        let limit = args
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(12)
            .min(50);
        self.open_session_picker(limit);
    }

    fn handle_resume_last_slash_command(&mut self, args: &str, include_all: bool) {
        if self.kind == AgentKind::ClaudeCode {
            self.start_claude_resume(None, false, args);
            return;
        }
        let prompt = if args.trim().is_empty() {
            resume_prompt().to_string()
        } else {
            args.trim().to_string()
        };
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_exec_resume_args(
            None,
            true,
            include_all,
            &prompt,
            &self.exec_settings,
            &developer_instructions,
        );
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
        if self.kind == AgentKind::ClaudeCode {
            self.start_claude_resume(Some(&session_ref), false, &prompt);
            return;
        }
        let Some(session) = self.resolve_session_ref(&session_ref) else {
            return;
        };
        let prompt = if prompt.trim().is_empty() {
            resume_prompt().to_string()
        } else {
            prompt.trim().to_string()
        };
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_exec_resume_args(
            Some(&session),
            false,
            include_all,
            &prompt,
            &self.exec_settings,
            &developer_instructions,
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
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_root_resume_args(
            None,
            true,
            include_all,
            include_non_interactive,
            args.trim(),
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
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
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_root_resume_args(
            Some(&session),
            false,
            include_all,
            false,
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_root_session_command_slash_command(&mut self, command_name: &str, args: &str) {
        let developer_instructions = self.composed_developer_instructions();
        let command = match codex_root_session_command_args(
            command_name,
            args,
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
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
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_fork_args(
            Some(thread_id),
            false,
            false,
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
        );
        let label = format!("codex {}", command_preview(&command));
        self.start_codex_subcommand(command, label);
    }

    fn handle_fork_last_slash_command(&mut self, args: &str, include_all: bool) {
        if self.kind == AgentKind::ClaudeCode {
            self.start_claude_resume(None, true, args);
            return;
        }
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_fork_args(
            None,
            true,
            include_all,
            args.trim(),
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
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
        if self.kind == AgentKind::ClaudeCode {
            self.start_claude_resume(Some(&session_ref), true, &prompt);
            return;
        }
        let Some(session) = self.resolve_session_ref(&session_ref) else {
            return;
        };
        let developer_instructions = self.composed_developer_instructions();
        let command = codex_fork_args(
            Some(&session),
            false,
            include_all,
            prompt.trim(),
            &self.cwd,
            &self.exec_settings,
            &developer_instructions,
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
                self.set_status_note(format!("renamed: {title}"));
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
                self.set_status_note("local provider: config".to_string());
                self.push_log(CodexLogKind::System, "次回ターンの local provider: config");
            }
            "lmstudio" | "lm-studio" => {
                self.exec_settings.local_provider = CodexLocalProviderChoice::LmStudio;
                self.set_status_note("local provider: lmstudio".to_string());
                self.push_log(
                    CodexLogKind::System,
                    "次回ターンの local provider: lmstudio",
                );
            }
            "ollama" => {
                self.exec_settings.local_provider = CodexLocalProviderChoice::Ollama;
                self.set_status_note("local provider: ollama".to_string());
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
        self.set_status_note(format!("color: {value}"));
        self.push_log(CodexLogKind::System, format!("次回ターンの color: {value}"));
    }

    fn handle_permissions_slash_command(&mut self, args: &str) {
        let args = args.trim();
        if args.is_empty() {
            self.open_approval_picker();
            return;
        }
        if matches!(args, "show" | "status") {
            self.push_permissions_status();
            return;
        }

        if self.kind == AgentKind::ClaudeCode {
            if let Some(mode) = claude::parse_permission_mode(args) {
                self.set_claude_permission_mode(mode);
            } else {
                self.push_log(
                    CodexLogKind::Error,
                    "permissions は config / plan / acceptEdits / dontAsk / bypassPermissions / skip-permissions を指定してください",
                );
            }
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
        if self.kind == AgentKind::ClaudeCode {
            let allow = self.claude_settings.sticky_allowed_tools();
            let allow_label = if allow.is_empty() {
                "なし".to_string()
            } else {
                allow.join(", ")
            };
            self.push_log(
                CodexLogKind::System,
                format!(
                    "Permissions: permission-mode={} / 常に許可={allow_label}",
                    self.claude_settings.permission_label()
                ),
            );
            return;
        }
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
            self.set_work_action(format!("Addness記憶を予約 {count}件"));
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
            self.set_work_action(format!("Addness引き継ぎ保存を予約 {count}件"));
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
        self.set_status_note(format!("cwd: {}", compact_home_path(&path)));
        self.push_log(
            CodexLogKind::System,
            format!("次回ターンの cwd: {}", self.cwd),
        );
        if self.thread_id.is_some() {
            let name = self.kind.display_name();
            self.push_log(
                CodexLogKind::System,
                format!("既存{name}セッションの再開にはcwd変更は反映されません。新規セッションで有効です。"),
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
        if self.kind == AgentKind::ClaudeCode {
            let dir = args.trim();
            if dir.is_empty() || dir == "list" {
                self.push_log(
                    CodexLogKind::System,
                    "Claude Code: /add-dir <絶対パス> で次ターンの --add-dir を追加します",
                );
                return;
            }
            self.claude_settings.add_dir(dir.to_string());
            self.set_status_note(format!("add-dir: {dir}"));
            self.push_log(
                CodexLogKind::System,
                format!("次回ターンに --add-dir {dir} を渡します"),
            );
            return;
        }
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
            let add_dirs = if self.kind == AgentKind::ClaudeCode {
                self.claude_settings.additional_dirs()
            } else {
                &self.exec_settings.additional_dirs
            };
            lines.extend(numbered_or_none(add_dirs));
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
        self.set_status_note("sandbox preset".to_string());
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
                self.set_status_note("継続ゴール: 一時停止".to_string());
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
                self.set_status_note("継続ゴール: 有効".to_string());
                self.push_activity("Goal mode を再開しました".to_string());
                self.push_log(CodexLogKind::System, "Goal mode を再開しました");
            }
            "clear" => {
                self.goal_mode = CodexGoalMode::default();
                self.persist_goal_mode();
                self.set_status_note("継続ゴール: 解除".to_string());
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
        self.set_status_note("継続ゴール: 有効".to_string());
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
        self.push_log(CodexLogKind::System, slash_help_text(self.kind).to_string());
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

    /// codex ヘッダに常設表示する「コスト | ctx N%」相当のラベル。未取得なら None。
    pub fn usage_header_label(&self) -> Option<String> {
        match self.kind {
            AgentKind::ClaudeCode => {
                let mut parts = Vec::new();
                if let Some(cost) = self.claude_total_cost_usd {
                    parts.push(format!("${cost:.4}"));
                }
                if let Some(used) = self.claude_context_tokens {
                    let window =
                        claude_context_window_for_model(self.claude_active_model.as_deref());
                    if let Some(pct) = context_percent(used, window) {
                        parts.push(format!("ctx {pct}%"));
                    }
                }
                (!parts.is_empty()).then(|| parts.join(" | "))
            }
            AgentKind::Codex => {
                let usage = self.codex_appserver_token_usage.as_ref()?;
                let mut parts = Vec::new();
                if let Some(total) = usage.total_tokens {
                    parts.push(format!("{} tok", format_token_count(total)));
                    if let Some(window) = usage.model_context_window
                        && let Some(pct) = context_percent(total, window)
                    {
                        parts.push(format!("ctx {pct}%"));
                    }
                }
                (!parts.is_empty()).then(|| parts.join(" | "))
            }
        }
    }

    fn push_slash_usage(&mut self) {
        let name = self.kind.display_name();
        let mut lines = vec![format!("{name} 使用状況:")];
        match self.kind {
            AgentKind::ClaudeCode => {
                match self.claude_total_cost_usd {
                    Some(cost) => lines.push(format!("  累計コスト: ${cost:.4}")),
                    None => lines.push("  累計コスト: 未取得".to_string()),
                }
                if let Some(usage) = &self.claude_last_usage {
                    lines.push(format!("  直近usage: {usage}"));
                }
                match self.claude_context_tokens {
                    Some(used) => {
                        let window =
                            claude_context_window_for_model(self.claude_active_model.as_deref());
                        let pct = context_percent(used, window)
                            .map(|p| format!("{p}%"))
                            .unwrap_or_else(|| "-".to_string());
                        let remain = window.saturating_sub(used);
                        lines.push(format!(
                            "  コンテキスト: {} / {} ({pct}, 残り {})",
                            format_token_count(used),
                            format_token_count(window),
                            format_token_count(remain)
                        ));
                    }
                    None => lines.push("  コンテキスト: 未取得".to_string()),
                }
            }
            AgentKind::Codex => match &self.codex_appserver_token_usage {
                Some(usage) => {
                    if let Some(total) = usage.total_tokens {
                        lines.push(format!("  累計トークン: {}", format_token_count(total)));
                    }
                    if let Some(last) = usage.last_total_tokens {
                        lines.push(format!("  直近ターン: {}", format_token_count(last)));
                    }
                    if let (Some(total), Some(window)) =
                        (usage.total_tokens, usage.model_context_window)
                    {
                        let pct = context_percent(total, window)
                            .map(|p| format!("{p}%"))
                            .unwrap_or_else(|| "-".to_string());
                        let remain = window.saturating_sub(total);
                        lines.push(format!(
                            "  コンテキスト: {} / {} ({pct}, 残り {})",
                            format_token_count(total),
                            format_token_count(window),
                            format_token_count(remain)
                        ));
                    }
                }
                None => match &self.last_token_usage_label {
                    Some(usage) => lines.push(format!("  トークン: {usage}")),
                    None => lines.push(format!(
                        "  まだ取得できていません。{name} turn 実行後に表示されます。"
                    )),
                },
            },
        }
        self.push_log(CodexLogKind::System, lines.join("\n"));
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
                "設定詳細:\ncodex_bin={}\n作業フォルダ={}\ncodex_home={codex_home}\nセッション={thread}\nturn={}\n設定={}\n履歴={history}\nconfig_overrides:\n  - {config}{}",
                compact_home_path(&self.codex_bin),
                self.cwd,
                self.turn_count,
                self.settings_label(),
                self.claude_resident_debug_suffix(),
            ),
        );
    }

    /// `/debug-config` に付ける常駐 Claude Code の状態（保存済み cost/usage/model 等）。
    /// ClaudeCode 以外では空文字。
    fn claude_resident_debug_suffix(&self) -> String {
        if self.kind != AgentKind::ClaudeCode {
            return String::new();
        }
        let resident = if self.claude_resident.is_some() {
            "resident(alive)"
        } else if self.claude_resident_enabled {
            "resident(idle/none)"
        } else {
            "oneshot"
        };
        let model = self.claude_active_model.as_deref().unwrap_or("-");
        let mode = self.claude_active_permission_mode.as_deref().unwrap_or("-");
        let cost = self
            .claude_total_cost_usd
            .map(|c| format!("${c:.4}"))
            .unwrap_or_else(|| "-".to_string());
        let usage = self.claude_last_usage.as_deref().unwrap_or("-");
        format!(
            "\nclaude_mode={resident}\nactive_model={model}\nactive_permission={mode}\ntotal_cost={cost}\nlast_usage={usage}"
        )
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
        // 承認バナー待ちの間は予約ターンを勝手に開始しない（ユーザー判断を待つ）。
        if self.finished || self.is_turn_running() || self.pending_decision.is_some() {
            return false;
        }
        let Some(queued) = self.queued_prompts.pop_front() else {
            return false;
        };
        let remaining = self.queued_prompts.len();
        if let Some(active) = queued.active_work.clone() {
            self.active_work_package = Some(active.clone());
            if remaining > 0 {
                self.set_work_action(format!(
                    "子ゴール着手 #{}: {} 残り{}件",
                    active.ordinal,
                    compact_one_line(&active.title, 80),
                    remaining
                ));
            } else {
                self.set_work_action(format!(
                    "子ゴール着手 #{}: {}",
                    active.ordinal,
                    compact_one_line(&active.title, 80)
                ));
            }
        } else if remaining > 0 {
            self.set_work_action(format!("予約入力を実行中 残り{remaining}件"));
        } else {
            self.set_work_action("予約入力を実行中".to_string());
        }
        self.push_log(CodexLogKind::System, "予約した入力を実行します");
        if queued.apply_goal_mode {
            // run_submitted_line_with_display 側で request_checkpoint する。
            self.run_submitted_line_with_display(queued.submitted, queued.display_prompt);
        } else {
            self.request_checkpoint();
            self.start_turn_with_display_prompt(&queued.submitted, &queued.display_prompt);
        }
        true
    }

    fn spawn_exec_process(&self, prompt: &str) -> Result<Child> {
        let mut cmd = Command::new(&self.codex_bin);
        let developer_instructions = self.composed_developer_instructions();
        match self.kind {
            AgentKind::Codex => {
                let exec_settings = self.exec_settings_for_spawn();
                for arg in codex_exec_args(
                    self.thread_id.as_deref(),
                    &self.cwd,
                    &exec_settings,
                    &developer_instructions,
                ) {
                    cmd.arg(arg);
                }
            }
            AgentKind::ClaudeCode => {
                // session_id は codex の thread_id を共用する（2 ターン目以降 --resume）。
                for arg in claude::exec_args(
                    self.thread_id.as_deref(),
                    &self.claude_settings,
                    &self.claude_one_shot_allowed_tools,
                    self.claude_fork_next,
                    &developer_instructions,
                ) {
                    cmd.arg(arg);
                }
            }
        }

        cmd.current_dir(&self.cwd);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        self.apply_agent_env(&mut cmd);

        let name = self.kind.display_name();
        let mut child = cmd
            .spawn()
            .with_context(|| format!("{name} の起動に失敗しました"))?;

        if let Some(mut stdin) = child.stdin.take()
            && let Err(e) = stdin.write_all(prompt.as_bytes())
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e).context(format!("{name} へのプロンプト送信に失敗しました"));
        }

        let stdout = child
            .stdout
            .take()
            .with_context(|| format!("{name} stdout の取得に失敗しました"))?;
        let stderr = child
            .stderr
            .take()
            .with_context(|| format!("{name} stderr の取得に失敗しました"))?;

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
        self.apply_agent_env(&mut cmd);

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
        // 常駐 Claude Code プロセスも終了させる（次ターンで --resume 再接続する）。
        self.teardown_claude_resident();
        // 常駐 codex app-server プロセスも終了させる（次ターンで thread/resume 再接続する）。
        self.teardown_codex_appserver();
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
            self.push_log(
                CodexLogKind::System,
                format!("{} ターンを中断しました", self.kind.display_name()),
            );
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
        self.teardown_all_processes();
        self.current_command = None;
        self.current_command_started_at = None;
        self.pending_decision = None;
        self.current_turn_prompt = None;
        self.current_turn_retry_prompt = None;
        self.queued_prompts.clear();
    }

    /// 常駐/子プロセスをすべて終了させ、`turn_running` を落とす（プロセスの後始末のみ）。
    /// 状態クリアを伴う `kill()` と、`/exit`・`/quit` の即時終了経路で共有する。
    fn teardown_all_processes(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
        self.teardown_claude_resident();
        // 常駐 codex app-server プロセスも終了させる（kill_current_turn と同様に孤児化を防ぐ）。
        self.teardown_codex_appserver();
        self.turn_running = false;
    }

    /// 常駐 Claude Code プロセスを終了させ、常駐関連の一時状態をリセットする。
    fn teardown_claude_resident(&mut self) {
        if let Some(mut resident) = self.claude_resident.take() {
            resident.kill();
        }
        self.claude_interrupt_deadline = None;
        self.claude_interrupting = false;
        self.claude_pending_tool = None;
        self.claude_pending_setting_change = None;
        self.claude_resident_restart_pending = false;
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

fn slash_help_text(kind: AgentKind) -> &'static str {
    if kind == AgentKind::ClaudeCode {
        return claude_slash_help_text();
    }
    codex_slash_help_text()
}

fn claude_slash_help_text() -> &'static str {
    r#"Slash commands:
Claude Code CLI commands:
  /new - start the next prompt in a new Claude Code session
  /clear - clear the visible log
  /init [notes] - create or update AGENTS.md for future sessions
  /ide - show IDE context availability in this TUI
  /exec|/e <prompt> - send directly to Claude Code without Goal mode wrapping
  /skills [list|name] - local skill discovery
Claude Code sessions:
  /sessions [N] - list local Claude Code sessions for this cwd
  /resume-last [prompt], /resume-session <N|id> [prompt] - resume a session directly
  /fork-last [prompt], /fork-session <N|id> [prompt] - fork a session directly
Claude Code options for next turn:
  /settings, /cd <dir>, /model [name|config], /reasoning|/effort [level]
  /lang|/language [auto|ja|en|off] - エージェントの応答言語（既定 auto は LANG/LC_ALL から判定）
  /permissions|/approval [mode] - permission-mode: config/plan/acceptEdits/dontAsk/bypassPermissions/skip-permissions
  /add-dir <path|list|clear>
TUI helpers:
  /goal <目標>, /goal pause, /goal resume, /goal clear
  /organize|/team [task] - Addness子ゴールへ分解し、最初の実装単位へ進む
  /work [next|all|N|id|title] - 子ゴールを実装ワークパッケージとして着手/キュー化
  /remember|/memo <内容> - Addness bodyの作業メモへ保存
  /handoff [メモ] - 現在の会話をAddness bodyへ再開用に保存
  /diff, /history, /turn [picker|N|all|old|close N|toggle N], /rollout, /debug-config, /ps, /stop [all], /btw
  /undo - ターン開始時のチェックポイントへこのペインの作業ディレクトリ配下を戻す（新規作成ファイルは残る）
  /feedback [message], /test-approval [message], /compact [notes], /plan [task]
  /resume, /status, /usage, /help, /exit"#
}

fn codex_slash_help_text() -> &'static str {
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
  /lang|/language [auto|ja|en|off] - エージェントの応答言語（既定 auto は LANG/LC_ALL から判定）
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
  /undo - ターン開始時のチェックポイントへこのペインの作業ディレクトリ配下を戻す（新規作成ファイルは残る）
  /feedback [message], /test-approval [message], /compact [notes], /plan [task]
  /resume, /status, /usage, /help, /exit"#
}

#[derive(Default)]
struct CodexInputState {
    line: String,
    cursor: usize,
    exit_command_sent: bool,
    last_prompt: Option<String>,
    /// 送信済みプロンプトの履歴（古い順。末尾が最新）。↑↓で呼び戻す。
    history: Vec<String>,
    /// 履歴の閲覧位置。None=閲覧していない（下書き編集中）。Some(i)=history[i]を表示中。
    history_pos: Option<usize>,
    /// 履歴閲覧に入る前の編集中テキスト。閲覧を最新より先へ抜けたときに復元する。
    history_draft: Option<String>,
}

impl CodexInputState {
    fn record_submitted(&mut self, submitted: &str) {
        if !submitted.is_empty() && !is_exit_prompt(submitted) {
            self.last_prompt = Some(submitted.to_string());
        }
        if is_exit_prompt(submitted) {
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
            // 複数行入力中は行間移動、単一行でも履歴があれば↑で呼び戻せるよう捕捉する。
            KeyCode::Up => self.line.contains('\n') || !self.history.is_empty(),
            // ↓は複数行の行間移動か、履歴閲覧中の前進（新しい方/下書きへ）のときだけ捕捉する。
            KeyCode::Down => self.line.contains('\n') || self.history_pos.is_some(),
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
            KeyCode::Up => self.move_up(),
            KeyCode::Down => self.move_down(),
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
        self.detach_history();
        let normalized = normalize_input_text(text);
        self.line.insert_str(self.cursor, &normalized);
        self.cursor += normalized.len();
    }

    fn insert_char(&mut self, ch: char) {
        self.detach_history();
        self.line.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    fn delete_before_cursor(&mut self) {
        let Some(prev) = prev_char_boundary(&self.line, self.cursor) else {
            return;
        };
        self.detach_history();
        self.line.drain(prev..self.cursor);
        self.cursor = prev;
    }

    fn delete_at_cursor(&mut self) {
        let Some(next) = next_char_boundary(&self.line, self.cursor) else {
            return;
        };
        self.detach_history();
        self.line.drain(self.cursor..next);
    }

    /// カーソル手前の `from` からカーソルまでを `replacement` で置き換える（@メンション確定に使う）。
    fn replace_to_cursor(&mut self, from: usize, replacement: &str) {
        if from > self.cursor || !self.line.is_char_boundary(from) {
            return;
        }
        self.detach_history();
        self.line.replace_range(from..self.cursor, replacement);
        self.cursor = from + replacement.len();
    }

    /// 履歴閲覧を終了し、下書き退避を破棄する（編集で下書きが確定したとき）。
    fn detach_history(&mut self) {
        self.history_pos = None;
        self.history_draft = None;
    }

    /// 送信済みプロンプトを履歴末尾へ追加する。直前と同一なら追加しない。
    /// 追加したら true。上限を超えた古い分は捨てる。
    fn push_history(&mut self, entry: String) -> bool {
        if entry.is_empty() {
            return false;
        }
        if self.history.last().is_some_and(|last| last == &entry) {
            return false;
        }
        self.history.push(entry);
        if self.history.len() > INPUT_HISTORY_MAX {
            let overflow = self.history.len() - INPUT_HISTORY_MAX;
            self.history.drain(0..overflow);
        }
        true
    }

    /// ↑: 先頭行なら履歴を遡り、そうでなければ行間を上へ移動する。
    fn move_up(&mut self) {
        let (start, _) = self.current_line_bounds();
        if start == 0 {
            self.recall_history_prev();
        } else {
            self.move_vertical(true);
        }
    }

    /// ↓: 末尾行なら履歴を進め（最新の先で下書きへ）、そうでなければ行間を下へ移動する。
    fn move_down(&mut self) {
        let (_, end) = self.current_line_bounds();
        if end == self.line.len() {
            self.recall_history_next();
        } else {
            self.move_vertical(false);
        }
    }

    /// 履歴を 1 件古い方へ辿る。閲覧開始時は現在の入力を下書きへ退避する。
    fn recall_history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.history_pos {
            None => {
                self.history_draft = Some(self.line.clone());
                self.history.len() - 1
            }
            Some(0) => return,
            Some(pos) => pos - 1,
        };
        self.history_pos = Some(idx);
        self.set_line_from_history(idx);
    }

    /// 履歴を 1 件新しい方へ辿る。最新の先へ進むと退避した下書きへ戻る。
    fn recall_history_next(&mut self) {
        match self.history_pos {
            None => {}
            Some(pos) if pos + 1 < self.history.len() => {
                self.history_pos = Some(pos + 1);
                self.set_line_from_history(pos + 1);
            }
            Some(_) => {
                let draft = self.history_draft.take().unwrap_or_default();
                self.line = draft;
                self.cursor = self.line.len();
                self.history_pos = None;
            }
        }
    }

    fn set_line_from_history(&mut self, idx: usize) {
        if let Some(entry) = self.history.get(idx) {
            self.line = entry.clone();
            self.cursor = self.line.len();
        }
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
        self.history_pos = None;
        self.history_draft = None;
    }
}

fn normalize_input_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// 長文ペーストを入力欄で畳み込む閾値（行数）。これを超えるとプレースホルダ化する。
const PASTE_FOLD_LINES: usize = 10;
/// 長文ペーストを入力欄で畳み込む閾値（文字数）。これを超えるとプレースホルダ化する。
const PASTE_FOLD_CHARS: usize = 800;

/// ペーストの行数（改行区切りのセグメント数）。空文字列は 0 行。
fn paste_line_count(text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    text.split('\n').count()
}

/// ペーストを畳み込むべきか（10 行超 または 800 文字超）。
fn paste_should_fold(normalized: &str) -> bool {
    paste_line_count(normalized) > PASTE_FOLD_LINES || normalized.chars().count() > PASTE_FOLD_CHARS
}

/// 入力欄へ挿入するプレースホルダ文字列を作る（例: `[貼り付け#1: 120行]`）。
fn paste_placeholder(index: usize, normalized: &str) -> String {
    format!("[貼り付け#{index}: {}行]", paste_line_count(normalized))
}

/// 入力行中のプレースホルダを、保持中の全文へ展開する（純粋関数）。
/// プレースホルダがそのまま残っている（完全一致する）ものだけ差し替える。
/// ユーザーが編集して一致しなくなったものはそのまま残す。
fn expand_paste_placeholders(line: &str, pastes: &[StoredPaste]) -> String {
    let mut out = line.to_string();
    for paste in pastes {
        if out.contains(paste.placeholder.as_str()) {
            out = out.replace(paste.placeholder.as_str(), &paste.full);
        }
    }
    out
}

/// クリップボード画像の保存先（`~/.addness/attachments/`）。
fn attachments_dir_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".addness").join("attachments"))
}

/// `clip-<N>.png` の連番を解析する。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // クリップボード保存はmacOSのみ
fn parse_clip_index(name: &str) -> Option<usize> {
    name.strip_prefix("clip-")?
        .strip_suffix(".png")?
        .parse()
        .ok()
}

/// 既存ファイル名一覧から次の `clip-<N>.png` を決める（純粋関数）。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // クリップボード保存はmacOSのみ
fn next_clip_filename(existing: &[String]) -> String {
    let max = existing
        .iter()
        .filter_map(|name| parse_clip_index(name))
        .max()
        .unwrap_or(0);
    format!("clip-{}.png", max + 1)
}

/// ディレクトリ内の既存 `clip-*.png` を調べ、次の保存先パスを返す。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // クリップボード保存はmacOSのみ
fn next_clip_path(dir: &Path) -> PathBuf {
    let existing: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    dir.join(next_clip_filename(&existing))
}

/// macOS: クリップボードの PNG を `dir` 内の次の連番へ保存する。
/// 画像が無ければ `Ok(None)`、保存できたら `Ok(Some(path))`、外部コマンド失敗は `Err`。
#[cfg(target_os = "macos")]
fn capture_clipboard_image_to_dir(dir: &Path) -> std::io::Result<Option<PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let path = next_clip_path(dir);
    if run_osascript_write_clipboard_png(&path, std::time::Duration::from_secs(5))? {
        Ok(Some(path))
    } else {
        Ok(None)
    }
}

/// macOS: `osascript` でクリップボードの PNG をファイルへ書き出す。
/// 書き出せたら `Ok(true)`、画像が無ければ `Ok(false)`。タイムアウト付き（ハング防止）。
#[cfg(target_os = "macos")]
fn run_osascript_write_clipboard_png(
    path: &Path,
    timeout: std::time::Duration,
) -> std::io::Result<bool> {
    use std::io::Read;
    // パスに `"` や `\` が含まれても AppleScript 文字列リテラルを壊さないようエスケープする。
    let escaped_path = super::app::applescript_string_escape(&path.display().to_string());
    let script = format!(
        r#"set outFile to POSIX file "{escaped_path}"
try
    set pngData to (the clipboard as «class PNGf»)
on error
    return "NO_IMAGE"
end try
set fh to open for access outFile with write permission
set eof fh to 0
write pngData to fh
close access fh
return "OK""#
    );
    let mut child = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            let mut out = String::new();
            if let Some(mut so) = child.stdout.take() {
                let _ = so.read_to_string(&mut out);
            }
            if status.success() && out.trim() == "OK" && path.exists() {
                return Ok(true);
            }
            // 画像なし・書き込み失敗時は中途半端なファイルを残さない。
            let _ = std::fs::remove_file(path);
            return Ok(false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(path);
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "osascript がタイムアウトしました",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
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

/// fileChange の変更パス一覧を 1 行のラベルにまとめる（先頭数件 + 残り件数）。
fn codex_appserver_paths_label(paths: &[String]) -> String {
    if paths.is_empty() {
        return "(パス不明)".to_string();
    }
    let shown = paths.len().min(3);
    let mut label = paths[..shown].join(", ");
    if paths.len() > shown {
        label.push_str(&format!(" ほか{}件", paths.len() - shown));
    }
    compact_one_line(&label, 120)
}

/// fileChange の各変更からパスだけを取り出す。
fn codex_appserver_change_paths(changes: &[codex_appserver::FileChangeDetail]) -> Vec<String> {
    changes.iter().map(|c| c.path.clone()).collect()
}

/// apply_patch テキスト 1 セクションに含める差分本文行の上限。
/// 表示側（code_edit_diff_preview）でさらに 8 行へ切り詰めるが、
/// ログ・セッション記録の肥大化を防ぐためにここでも上限を設ける。
const CODEX_PATCH_SECTION_MAX_LINES: usize = 40;

/// codex 互換の apply_patch テキスト（`EDIT ` 接頭辞付き）を組み立てる。
/// `sections` は (ヘッダ行, 本文行) で、本文行は既に `+`/`-`/`@@` 接頭辞付き。
/// 空なら None。描画は既存の `code_edit_display_text` が担当する（色付き差分プレビュー）。
fn build_apply_patch_text(sections: &[(String, Vec<String>)]) -> Option<String> {
    if sections.is_empty() {
        return None;
    }
    let mut out = vec!["EDIT *** Begin Patch".to_string()];
    for (header, body) in sections {
        out.push(header.clone());
        for line in body {
            out.push(line.clone());
        }
    }
    out.push("*** End Patch".to_string());
    Some(out.join("\n"))
}

/// 生テキストを行分割し、各行へ `prefix` を付けて最大 `max` 行まで返す。
/// 空テキストは空の差分として扱う。
fn prefixed_diff_lines(text: &str, prefix: char, max: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.replace('\r', "")
        .split('\n')
        .take(max)
        .map(|line| format!("{prefix}{line}"))
        .collect()
}

/// Claude の Edit / MultiEdit / Write の tool_use input から差分プレビュー用の
/// apply_patch テキストを組み立てる純粋関数。対象外ツールや情報不足なら None。
fn claude_edit_patch_text(name: &str, input: &Value) -> Option<String> {
    let path = input
        .get("file_path")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())?;
    match name {
        "Write" => {
            let content = input.get("content").and_then(Value::as_str).unwrap_or("");
            let body = prefixed_diff_lines(content, '+', CODEX_PATCH_SECTION_MAX_LINES);
            build_apply_patch_text(&[(format!("*** Add File: {path}"), body)])
        }
        "Edit" => {
            let old = input
                .get("old_string")
                .and_then(Value::as_str)
                .unwrap_or("");
            let new = input
                .get("new_string")
                .and_then(Value::as_str)
                .unwrap_or("");
            let body = edit_pair_body(old, new);
            if body.is_empty() {
                return None;
            }
            build_apply_patch_text(&[(format!("*** Update File: {path}"), body)])
        }
        "MultiEdit" => {
            let edits = input.get("edits").and_then(Value::as_array)?;
            let mut body = Vec::new();
            for edit in edits {
                let old = edit.get("old_string").and_then(Value::as_str).unwrap_or("");
                let new = edit.get("new_string").and_then(Value::as_str).unwrap_or("");
                body.extend(edit_pair_body(old, new));
            }
            if body.is_empty() {
                return None;
            }
            build_apply_patch_text(&[(format!("*** Update File: {path}"), body)])
        }
        _ => None,
    }
}

/// 旧行を `-`、新行を `+` として並べた差分本文を作る（行単位 diff は取らない）。
fn edit_pair_body(old: &str, new: &str) -> Vec<String> {
    let half = CODEX_PATCH_SECTION_MAX_LINES / 2;
    let mut body = Vec::new();
    if !old.is_empty() {
        body.extend(prefixed_diff_lines(old, '-', half));
    }
    if !new.is_empty() {
        body.extend(prefixed_diff_lines(new, '+', half));
    }
    body
}

/// codex 常駐 fileChange の unified diff から apply_patch テキストを組み立てる。
fn codex_filechange_patch_text(changes: &[codex_appserver::FileChangeDetail]) -> Option<String> {
    let mut sections = Vec::new();
    for change in changes {
        let header = match change.change_type.as_str() {
            "add" => format!("*** Add File: {}", change.path),
            "delete" => format!("*** Delete File: {}", change.path),
            _ => format!("*** Update File: {}", change.path),
        };
        let body: Vec<String> = change
            .diff
            .replace('\r', "")
            .split('\n')
            .filter(|line| is_meaningful_diff_line(line))
            .take(CODEX_PATCH_SECTION_MAX_LINES)
            .map(str::to_string)
            .collect();
        if body.is_empty() {
            continue;
        }
        sections.push((header, body));
    }
    build_apply_patch_text(&sections)
}

/// unified diff からファイルヘッダ等のノイズ行を除き、`@@`/`+`/`-` の意味行だけ残す。
fn is_meaningful_diff_line(line: &str) -> bool {
    if line.starts_with("+++")
        || line.starts_with("---")
        || line.starts_with("diff ")
        || line.starts_with("index ")
        || line.starts_with("new file")
        || line.starts_with("deleted file")
        || line.starts_with("rename ")
        || line.starts_with("similarity ")
        || line.starts_with("\\ No newline")
    {
        return false;
    }
    line.starts_with("@@") || line.starts_with('+') || line.starts_with('-')
}

/// 経過秒を mm:ss（1時間以上は h:mm:ss）で整形する純粋関数。
fn format_elapsed(secs: u64) -> String {
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// トークン数を k / M で丸めた短い表示にする（例: 12711 → "12.7k"、1_500_000 → "1.5M"）。
fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// 使用トークン / コンテキスト長からパーセント（0-100、四捨五入）を返す。window が 0 なら None。
fn context_percent(used: u64, window: u64) -> Option<u8> {
    if window == 0 {
        return None;
    }
    let pct = (used as f64 / window as f64 * 100.0).round();
    Some(pct.clamp(0.0, 100.0) as u8)
}

/// モデル名から想定コンテキスト長を返す。既知マップに無ければ 200k と仮定する。
fn claude_context_window_for_model(model: Option<&str>) -> u64 {
    const DEFAULT_WINDOW: u64 = 200_000;
    let Some(model) = model else {
        return DEFAULT_WINDOW;
    };
    let lower = model.to_ascii_lowercase();
    // 1M コンテキストのモデル（例: `...[1m]` / `...-1m`）だけ別枠にする。
    if lower.contains("[1m]") || lower.contains("-1m") {
        1_000_000
    } else {
        DEFAULT_WINDOW
    }
}

/// goal_id からチェックポイント ref に使える slug を作る（英数と '-' のみ、他は '-'）。
/// 連続する '-' はまとめ、前後の '-' は落とす。空になった場合は "pane"。
fn checkpoint_slug(goal_id: &str) -> String {
    let mut slug = String::with_capacity(goal_id.len());
    let mut last_dash = false;
    for ch in goal_id.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    if trimmed.is_empty() {
        "pane".to_string()
    } else {
        trimmed.to_string()
    }
}

/// チェックポイント ref 名を組み立てる。
fn checkpoint_ref_name(slug: &str, seq: usize) -> String {
    format!("refs/addness/checkpoint-{slug}-{seq}")
}

/// `git status --porcelain` 出力に作業ツリー/インデックスの変更があるか。
pub(super) fn git_status_has_changes(porcelain: &str) -> bool {
    porcelain.lines().any(|line| !line.trim().is_empty())
}

/// チェックポイントをスタックへ push し、上限超過分（古い順）を返す（呼び出し側が ref を掃除する）。
fn push_checkpoint_with_evictions(
    stack: &mut Vec<Checkpoint>,
    checkpoint: Checkpoint,
    max: usize,
) -> Vec<Checkpoint> {
    stack.push(checkpoint);
    let mut evicted = Vec::new();
    while stack.len() > max.max(1) {
        evicted.push(stack.remove(0));
    }
    evicted
}

/// 端末出力から ANSI エスケープ列と制御文字を取り除く。ライブ出力の表示前に使う。
fn sanitize_terminal_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            match chars.peek() {
                Some('[') => {
                    // CSI: パラメータ・中間バイトを読み飛ばし、終端バイト(0x40-0x7E)まで消費。
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        chars.next();
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: BEL(0x07) か ST(ESC \) まで。
                    chars.next();
                    while let Some(&c) = chars.peek() {
                        if c == '\u{7}' {
                            chars.next();
                            break;
                        }
                        if c == '\u{1b}' {
                            chars.next();
                            if chars.peek() == Some(&'\\') {
                                chars.next();
                            }
                            break;
                        }
                        chars.next();
                    }
                }
                Some(_) => {
                    // ESC X（2バイトのエスケープ）。
                    chars.next();
                }
                None => {}
            }
            continue;
        }
        if ch == '\t' {
            out.push(' ');
        } else if !ch.is_control() {
            out.push(ch);
        }
    }
    out
}

fn spawn_line_reader<R>(reader: R, tx: Sender<CodexProcessEvent>, stderr: bool)
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
                CodexProcessEvent::Stderr(line)
            } else {
                CodexProcessEvent::Stdout(line)
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

fn agent_session_log_path(kind: AgentKind, goal_id: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|home| {
        home.join(".addness")
            .join(kind.session_history_dir())
            .join(format!("{}.jsonl", safe_path_component(goal_id)))
    })
}

/// 入力履歴の保存先（全ゴール共通。シェル履歴に相当）。
fn input_history_file_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".addness").join("input-history.jsonl"))
}

/// 入力履歴ファイルを読み込む（古い順。末尾が最新）。上限を超える古い分は捨てる。
fn load_input_history(path: &Path) -> Vec<String> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<String>(trimmed) {
            out.push(entry);
        }
    }
    if out.len() > INPUT_HISTORY_MAX {
        let overflow = out.len() - INPUT_HISTORY_MAX;
        out.drain(0..overflow);
    }
    out
}

/// 入力履歴を 1 件追記する（改行を含むプロンプトも JSON エスケープで 1 行に収まる）。
fn append_input_history(path: &Path, entry: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("入力履歴ディレクトリを作成できません: {}", parent.display())
        })?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("入力履歴ファイルを開けません: {}", path.display()))?;
    serde_json::to_writer(&mut file, entry).context("入力履歴のJSON化に失敗しました")?;
    writeln!(file).context("入力履歴の書き込みに失敗しました")?;
    Ok(())
}

/// 入力欄の `@` メンション候補 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MentionCandidate {
    /// 入力欄へ挿入する相対パス（ディレクトリは末尾 `/`）。
    insert: String,
    /// パレット表示に使う文字列（現状は insert と同じ）。
    pub display: String,
    pub is_dir: bool,
}

/// カーソル手前で入力中の `@` メンションを検出する。
/// 返り値は (`@` のバイト位置, `@` 以降の入力文字列)。
/// `@` の直前が英数字（メールアドレス等）の場合や、`@` 以降に空白を含む場合は None。
fn active_mention(line: &str, cursor: usize) -> Option<(usize, String)> {
    let before = line.get(..cursor)?;
    let at = before.rfind('@')?;
    let query = &before[at + '@'.len_utf8()..];
    if query.chars().any(char::is_whitespace) {
        return None;
    }
    // メールアドレス（foo@bar）の誤爆だけをガードしたいので、直前文字は ASCII 英数字と
    // ローカルパートで使われる記号に限定する。CJK など非 ASCII 直後（例:「見て@src」）は
    // メンションとして扱いパレットを出す。
    if at > 0
        && let Some(prev) = before[..at].chars().next_back()
        && (prev.is_ascii_alphanumeric() || matches!(prev, '.' | '_' | '-' | '+'))
    {
        return None;
    }
    Some((at, query.to_string()))
}

/// `@` メンションのクエリからファイル候補を作る。
/// `cwd` 基準で、クエリの末尾 `/` までをディレクトリ、残りを絞り込み接頭辞として扱う。
/// 前方一致を優先し、続けて部分一致を最大件数まで並べる。
fn mention_candidates(cwd: &Path, query: &str) -> Vec<MentionCandidate> {
    // cwd 基準の相対パスだけを列挙する。絶対パス（`/etc/...`）や `..` を含むクエリは
    // cwd 外へ出てしまうため候補を空にし、パレットを出さない。
    if query.starts_with('/') || query.split('/').any(|component| component == "..") {
        return Vec::new();
    }
    let (sub, prefix) = match query.rfind('/') {
        Some(idx) => (&query[..=idx], &query[idx + 1..]),
        None => ("", query),
    };
    let dir = if sub.is_empty() {
        cwd.to_path_buf()
    } else {
        cwd.join(sub)
    };
    let prefix_lower = prefix.to_lowercase();
    let mut prefix_hits = Vec::new();
    let mut substr_hits = Vec::new();
    for entry in super::file_picker::read_dir_entries(&dir) {
        let name_lower = entry.name.to_lowercase();
        let rel = format!("{sub}{}", entry.name);
        let insert = if entry.is_dir { format!("{rel}/") } else { rel };
        let candidate = MentionCandidate {
            display: insert.clone(),
            insert,
            is_dir: entry.is_dir,
        };
        if prefix_lower.is_empty() || name_lower.starts_with(&prefix_lower) {
            prefix_hits.push(candidate);
        } else if name_lower.contains(&prefix_lower) {
            substr_hits.push(candidate);
        }
    }
    prefix_hits.extend(substr_hits);
    prefix_hits.truncate(MENTION_PALETTE_MAX);
    prefix_hits
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
                thread_id: None,
            });
        }
        Err(e) => {
            return Err(e).with_context(|| format!("履歴ファイルを開けません: {}", path.display()));
        }
    };

    let mut log = Vec::new();
    let mut goal_mode = CodexGoalMode::default();
    let mut thread_id = None;
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
            CodexSessionRecord::ThreadId { id } => {
                // 最後の値を残す。tombstone（None）も最後ならそのまま反映する。
                thread_id = id;
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
        thread_id,
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
- Addnessの読み書きは必ずAddness CLI（`"$ADDNESS_BIN"`）で行う。`mcp__addness__*` のMCPツールが見えても使わない（認証・接続先がTUIと一致する保証がないため）。
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

/// 応答言語設定を解決した実効言語。`None` 相当（注入なし）は
/// `resolve_agent_language` の戻り値 `Option` で表す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedAgentLanguage {
    Japanese,
    English,
}

/// `LANG`（無ければ `LC_ALL`）の現在値。空文字は未設定として扱う。
/// auto 判定にのみ使う。テスト容易性のため env 読み取りはこの関数に閉じる。
fn current_lang_env() -> Option<String> {
    for key in ["LANG", "LC_ALL"] {
        if let Ok(value) = std::env::var(key)
            && !value.is_empty()
        {
            return Some(value);
        }
    }
    None
}

/// 言語設定と `LANG`/`LC_ALL` 相当の値から、注入すべき実効言語を解決する。
/// 環境変数を直接読まず引数で受け取るため、他テストと干渉せず検証できる。
fn resolve_agent_language(
    setting: AgentLanguage,
    lang_env: Option<&str>,
) -> Option<ResolvedAgentLanguage> {
    match setting {
        AgentLanguage::Off => None,
        AgentLanguage::Ja => Some(ResolvedAgentLanguage::Japanese),
        AgentLanguage::En => Some(ResolvedAgentLanguage::English),
        AgentLanguage::Auto => match lang_env {
            Some(value) if value.starts_with("ja") => Some(ResolvedAgentLanguage::Japanese),
            Some(value) if value.starts_with("en") => Some(ResolvedAgentLanguage::English),
            // 判定不能なら off と同じく注入しない。
            _ => None,
        },
    }
}

/// 実効言語に対応する 1 行の応答言語指示。
fn agent_language_instruction(language: ResolvedAgentLanguage) -> &'static str {
    match language {
        ResolvedAgentLanguage::Japanese => {
            "ユーザーへの応答・説明は必ず日本語で書く。コード・識別子・技術用語は原文のままでよい。"
        }
        ResolvedAgentLanguage::English => {
            "Always write your responses and explanations to the user in English. Code, identifiers, and technical terms may stay in their original form."
        }
    }
}

/// base 開発者指示に応答言語指示を合成する共通関数。
/// 全バックエンド（Claude の `--append-system-prompt` / Codex の developerInstructions・
/// ワンショット `-c`）の developer instructions 経路は必ずこの関数を通し、
/// 言語指示の注入漏れを防ぐ。
fn compose_developer_instructions(setting: AgentLanguage, lang_env: Option<&str>) -> String {
    let base = addness_tui_developer_instructions();
    match resolve_agent_language(setting, lang_env).map(agent_language_instruction) {
        Some(line) => format!("{base}\n\n応答言語:\n{line}"),
        None => base.to_string(),
    }
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
    use super::codex::{CodexConfigKey, codex_config_arg, load_codex_session_candidates_from};
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn modified_key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn agent_kind_labels_differ_per_backend() {
        assert_eq!(AgentKind::Codex.label(), "codex");
        assert_eq!(AgentKind::ClaudeCode.label(), "claude code");
        assert_eq!(AgentKind::Codex.display_name(), "Codex");
        assert_eq!(AgentKind::ClaudeCode.display_name(), "Claude Code");
        assert_eq!(AgentKind::Codex.backend_env_value(), "codex");
        assert_eq!(AgentKind::ClaudeCode.backend_env_value(), "claude");
    }

    #[test]
    fn session_log_path_splits_by_kind() {
        let codex = agent_session_log_path(AgentKind::Codex, "goal/x").unwrap();
        let claude = agent_session_log_path(AgentKind::ClaudeCode, "goal/x").unwrap();
        assert!(
            codex.to_string_lossy().contains("codex-sessions"),
            "{codex:?}"
        );
        assert!(
            claude.to_string_lossy().contains("claude-sessions"),
            "{claude:?}"
        );
        // ゴール ID 部分（ファイル名）は両バックエンドで同一の正規化になる。
        assert_eq!(codex.file_name(), claude.file_name());
    }

    #[test]
    fn claude_path_prefers_env_binary() {
        // 実在するファイルを指す ADDNESS_CLAUDE_BIN を最優先で返す。
        let tmp = std::env::temp_dir().join(format!("addness-claude-test-{}", std::process::id()));
        std::fs::write(&tmp, b"#!/bin/sh\n").unwrap();
        // SAFETY: テスト専用の一時的な環境変数操作。他テストは同変数を参照しない。
        unsafe {
            std::env::set_var("ADDNESS_CLAUDE_BIN", &tmp);
        }
        let resolved = claude_path();
        unsafe {
            std::env::remove_var("ADDNESS_CLAUDE_BIN");
        }
        let _ = std::fs::remove_file(&tmp);
        assert_eq!(resolved.as_deref(), Some(tmp.as_path()));
    }

    /// Claude Code バックエンドのテスト用ペイン。実ホームへ書き込まず（session_log_path: None）、
    /// 実在しないバイナリを使うので、ターン起動を試みても実プロセスは走らない。
    fn claude_pane() -> CodexPane {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut pane = CodexPane::spawn_inner(CodexPaneSpawnOptions {
            kind: AgentKind::ClaudeCode,
            codex_bin: Path::new("/nonexistent/addness-claude-test-bin"),
            cwd: &cwd,
            addness_bin: "addness",
            session_log_path: None,
            input_history_path: None,
            attachments_dir: None,
            goal_id: "goal/claude".to_string(),
            goal_title: "Claude goal".to_string(),
            dod: String::new(),
            status_label: "TEST".to_string(),
        })
        .unwrap();
        pane.log.clear();
        // 既定は常駐だが、プロセス起動を伴わないユニットテストではワンショット経路を検証する。
        // 常駐固有の遷移は claude_resident_pane() を使う専用テストで検証する。
        pane.claude_resident_enabled = false;
        pane
    }

    /// 常駐クライアントを差し込んだ ClaudeCode ペイン（実プロセスは起動しない）。
    /// `set_resident_for_test` でダミー `Child` を入れ、常駐固有の状態遷移を検証する。
    fn claude_resident_pane() -> CodexPane {
        let mut pane = claude_pane();
        pane.claude_resident_enabled = true;
        pane
    }

    #[test]
    fn claude_init_event_sets_session_id_and_turn() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "system", "subtype": "init", "session_id": "sid-1", "model": "opus"
        }));
        assert_eq!(pane.thread_id.as_deref(), Some("sid-1"));
        assert!(pane.log.iter().any(|line| line.kind == CodexLogKind::Turn));
    }

    #[test]
    fn claude_stderr_unknown_partial_flag_disables_streaming_once() {
        let mut pane = claude_pane();
        // 再試行対象プロンプトは無い状態にし、spawn せず判定・フラグ・ログのみ確認する。
        pane.current_turn_retry_prompt = None;
        assert!(!pane.claude_settings.no_partial_messages());

        pane.handle_stderr_line("error: unknown option '--include-partial-messages'");
        assert!(pane.claude_settings.no_partial_messages());
        let disabled_msg = |line: &CodexLogLine| line.text.contains("ストリーミング表示に未対応");
        assert_eq!(pane.log.iter().filter(|l| disabled_msg(l)).count(), 1);

        // 2 回目の同種 stderr ではフラグ済みなので再発火せず、無効化ログも増えない（無限ループ防止）。
        pane.handle_stderr_line("error: unknown option '--include-partial-messages'");
        assert_eq!(pane.log.iter().filter(|l| disabled_msg(l)).count(), 1);
    }

    #[test]
    fn claude_assistant_text_is_logged() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "こんにちは"}]}
        }));
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::Assistant && line.text == "こんにちは")
        );
    }

    #[test]
    fn claude_tool_use_bash_and_write_are_logged() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "name": "Bash", "input": {"command": "cargo test"}},
                {"type": "tool_use", "name": "Write", "input": {"file_path": "/tmp/x.rs"}}
            ]}
        }));
        assert!(pane.log.iter().any(|line| line.kind == CodexLogKind::Tool
            && line.text.contains("Bash")
            && line.text.contains("cargo test")));
        // Write は色付き差分プレビュー用の apply_patch 行として残る（Add File 見出し）。
        assert!(pane.log.iter().any(|line| line.kind == CodexLogKind::Tool
            && line.text.starts_with("EDIT ")
            && line.text.contains("*** Add File: /tmp/x.rs")));
    }

    #[test]
    fn claude_edit_tool_use_emits_colored_diff_line() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "name": "Edit", "input": {
                    "file_path": "src/x.rs",
                    "old_string": "let a = 1;",
                    "new_string": "let a = 2;"
                }}
            ]}
        }));
        let edit_line = pane
            .log
            .iter()
            .find(|line| line.kind == CodexLogKind::Tool && line.text.starts_with("EDIT "))
            .expect("EDIT 行が残る");
        assert!(edit_line.text.contains("*** Update File: src/x.rs"));
        assert!(edit_line.text.contains("-let a = 1;"));
        assert!(edit_line.text.contains("+let a = 2;"));
    }

    #[test]
    fn claude_tool_result_error_is_logged_as_error() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "boom", "is_error": true}
            ]}
        }));
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::Error && line.text.contains("boom"))
        );
    }

    #[test]
    fn claude_task_tool_use_is_detected_as_running_subagent() {
        let mut pane = claude_pane();
        assert_eq!(pane.subagents_len(), 0);
        assert_eq!(pane.subagent_running_count(), 0);

        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {
                    "type": "tool_use",
                    "id": "toolu_sub1",
                    "name": "Task",
                    "input": {
                        "description": "stream-json調査",
                        "subagent_type": "Explore"
                    }
                }
            ]}
        }));

        assert_eq!(pane.subagents_len(), 1);
        assert_eq!(pane.subagent_running_count(), 1);
        let lines = pane.subagent_status_lines(5);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("● stream-json調査"));
    }

    #[test]
    fn claude_agent_tool_use_without_task_name_is_not_detected() {
        // Bash / Write 等の通常ツールはサブエージェント扱いにならない。
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls"}}
            ]}
        }));
        assert_eq!(pane.subagents_len(), 0);
    }

    #[test]
    fn claude_subagent_transitions_to_completed_on_matching_tool_result() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {
                    "type": "tool_use",
                    "id": "toolu_sub1",
                    "name": "Task",
                    "input": {"description": "調査タスク", "subagent_type": "Explore"}
                }
            ]}
        }));
        assert_eq!(pane.subagent_running_count(), 1);

        pane.handle_json_event(serde_json::json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "toolu_sub1", "content": "完了しました", "is_error": false}
            ]}
        }));

        assert_eq!(pane.subagent_running_count(), 0);
        let lines = pane.subagent_status_lines(5);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("✔ 調査タスク"));
    }

    #[test]
    fn claude_subagent_transitions_to_failed_on_error_tool_result() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {
                    "type": "tool_use",
                    "id": "toolu_sub2",
                    "name": "Agent",
                    "input": {"description": "失敗するタスク"}
                }
            ]}
        }));
        pane.handle_json_event(serde_json::json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "toolu_sub2", "content": "boom", "is_error": true}
            ]}
        }));

        assert_eq!(pane.subagent_running_count(), 0);
        let lines = pane.subagent_status_lines(5);
        assert!(lines[0].starts_with("✖ 失敗するタスク"));
    }

    #[test]
    fn claude_subagent_tool_result_with_unrelated_id_does_not_resolve_running_entry() {
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {
                    "type": "tool_use",
                    "id": "toolu_sub3",
                    "name": "Task",
                    "input": {"description": "実行中タスク"}
                }
            ]}
        }));
        pane.handle_json_event(serde_json::json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "toolu_other", "content": "ok", "is_error": false}
            ]}
        }));

        // 無関係な tool_use_id では実行中のまま。
        assert_eq!(pane.subagent_running_count(), 1);
    }

    #[test]
    fn claude_subagent_history_survives_new_turn_start() {
        // ターンをまたいでも実行中エントリは clear_recent_actions の対象外として保持される。
        let mut pane = claude_pane();
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "tool_use", "id": "toolu_sub4", "name": "Task", "input": {"description": "長期タスク"}}
            ]}
        }));
        assert_eq!(pane.subagent_running_count(), 1);

        pane.handle_json_event(serde_json::json!({
            "type": "system", "subtype": "init", "session_id": "sid-2", "model": "opus"
        }));

        assert_eq!(pane.subagent_running_count(), 1);
        assert_eq!(pane.subagents_len(), 1);
    }

    #[test]
    fn claude_subagent_entries_are_capped_at_subagents_cap() {
        let mut pane = claude_pane();
        for i in 0..(SUBAGENTS_CAP + 3) {
            pane.record_subagent_launch(
                Some(format!("id-{i}")),
                format!("task-{i}"),
                Some("Explore".to_string()),
            );
        }
        assert_eq!(pane.subagents_len(), SUBAGENTS_CAP);
    }

    #[test]
    fn subagent_status_lines_prioritizes_running_and_limits_by_visible_count() {
        let mut pane = claude_pane();
        pane.record_subagent_launch(
            Some("id-1".to_string()),
            "完了済みタスクA".to_string(),
            None,
        );
        pane.resolve_subagent_result(Some("id-1"), false);
        pane.record_subagent_launch(Some("id-2".to_string()), "実行中タスクB".to_string(), None);
        pane.record_subagent_launch(
            Some("id-3".to_string()),
            "完了済みタスクC".to_string(),
            None,
        );
        pane.resolve_subagent_result(Some("id-3"), false);

        // limit=0 は空。
        assert!(pane.subagent_status_lines(0).is_empty());

        // limit=1 では実行中を優先。
        let lines = pane.subagent_status_lines(1);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("実行中タスクB"));

        // limit=2 では実行中 + 直近の完了（新しい方=タスクC）。
        let lines = pane.subagent_status_lines(2);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("実行中タスクB"));
        assert!(lines[1].contains("完了済みタスクC"));
    }

    #[test]
    fn claude_result_denials_raise_permission_banner() {
        let mut pane = claude_pane();
        pane.thread_id = Some("sid-1".to_string());
        pane.handle_json_event(serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done",
            "session_id": "sid-1",
            "permission_denials": [
                {"tool_name": "Write", "tool_use_id": "t1", "tool_input": {"file_path": "/a.rs"}}
            ]
        }));
        let banner = pane.pending_decision.as_ref().expect("banner");
        assert_eq!(banner.kind, CodexDecisionKind::Permission);
        assert!(banner.message.contains("Write"));
        // 拒否された Write ツールだけを許可するルールが用意される。
        assert_eq!(pane.claude_pending_allowed_tools, vec!["Write".to_string()]);
        assert!(banner.message.contains("許可: Write"));
    }

    #[test]
    fn claude_accept_starts_resume_turn_with_continue_prompt() {
        let mut pane = claude_pane();
        pane.thread_id = Some("sid-1".to_string());
        pane.handle_json_event(serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done",
            "session_id": "sid-1",
            "permission_denials": [
                {"tool_name": "Bash", "tool_use_id": "t1", "tool_input": {"command": "git push origin main"}}
            ]
        }));
        // Bash はコマンド先頭語のプレフィックスルールとして許可候補になる。
        assert_eq!(
            pane.claude_pending_allowed_tools,
            vec!["Bash(git push:*)".to_string()]
        );
        pane.handle_decision_key(key(KeyCode::Char('a')));
        // Accept で今回だけのルールが確定し、pending は消費される。
        assert!(pane.claude_pending_allowed_tools.is_empty());
        // 続行プロンプトがユーザー表示として積まれ、resume 対象の session_id が保持される。
        assert!(
            pane.log
                .iter()
                .any(|line| line.kind == CodexLogKind::User && line.text.contains("許可しました"))
        );
        assert_eq!(pane.thread_id.as_deref(), Some("sid-1"));
    }

    #[test]
    fn claude_always_adds_sticky_allowed_tool() {
        let mut pane = claude_pane();
        pane.thread_id = Some("sid-1".to_string());
        pane.handle_json_event(serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done",
            "session_id": "sid-1",
            "permission_denials": [
                {"tool_name": "Write", "tool_use_id": "t1", "tool_input": {"file_path": "/a.rs"}}
            ]
        }));
        // l（これからずっと許可）で sticky 許可リストへ入る。
        pane.handle_decision_key(key(KeyCode::Char('l')));
        assert!(pane.settings_label().contains("allow:1"));
        // 今回だけの一時ルールには入らない。
        assert!(pane.claude_one_shot_allowed_tools.is_empty());
    }

    // -- 常駐モード（実プロセスは cat のダミー）--------------------------------

    /// 常駐クライアント（ダミー）を差し込む。生成不可な環境では None を返す。
    fn with_dummy_resident(pane: &mut CodexPane) -> bool {
        match claude_resident::ResidentClient::spawn_dummy() {
            Some(client) => {
                pane.claude_resident = Some(client);
                true
            }
            None => false,
        }
    }

    fn can_use_tool_event(request_id: &str, tool: &str, input: serde_json::Value) -> Value {
        serde_json::json!({
            "type": "control_request",
            "request_id": request_id,
            "request": {"subtype": "can_use_tool", "tool_name": tool, "input": input}
        })
    }

    #[test]
    fn claude_resident_skip_permissions_switch_sets_restart_pending() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        // 他モード（acceptEdits）へ切替はランタイム反映のため再起動は立たない。
        pane.set_claude_permission_mode(claude::ClaudePermissionMode::AcceptEdits);
        assert!(!pane.claude_resident_restart_pending);
        // acceptEdits → skip-permissions は起動時フラグなので再起動保留が立つ。
        pane.set_claude_permission_mode(claude::ClaudePermissionMode::DangerouslySkipPermissions);
        assert!(pane.claude_resident_restart_pending);
        // 保留をクリアし、skip-permissions → 他モードの逆方向でも再起動保留が立つ。
        pane.claude_resident_restart_pending = false;
        pane.set_claude_permission_mode(claude::ClaudePermissionMode::Plan);
        assert!(pane.claude_resident_restart_pending);
    }

    #[test]
    fn claude_resident_can_use_tool_auto_allows_matching_rule() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        // セッション許可ルールに Edit を入れておく。
        pane.claude_settings
            .add_allowed_tools(&["Edit".to_string()]);
        pane.handle_json_event(can_use_tool_event(
            "req-a",
            "Edit",
            serde_json::json!({"file_path": "/a.rs"}),
        ));
        // マッチしたのでバナーを出さず自動許可（pending_tool は立たない）。
        assert!(pane.pending_decision.is_none());
        assert!(pane.claude_pending_tool.is_none());
        assert!(pane.log.iter().any(|l| l.text.contains("自動許可")));
    }

    #[test]
    fn claude_resident_can_use_tool_raises_banner_and_always_adds_sticky() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        pane.handle_json_event(can_use_tool_event(
            "req-b",
            "Bash",
            serde_json::json!({"command": "rm -rf x"}),
        ));
        // 未許可なのでバナーが立ち、応答用に pending_tool を保持する。
        let banner = pane.pending_decision.as_ref().expect("banner");
        assert_eq!(banner.kind, CodexDecisionKind::Permission);
        assert_eq!(
            pane.claude_pending_tool
                .as_ref()
                .map(|p| p.request_id.clone()),
            Some("req-b".to_string())
        );
        // l（これからずっと許可）で sticky に Bash(rm:*) が入り、バナー/pending が消える。
        pane.handle_decision_key(key(KeyCode::Char('l')));
        assert!(pane.pending_decision.is_none());
        assert!(pane.claude_pending_tool.is_none());
        assert!(
            pane.claude_settings
                .sticky_allowed_tools()
                .iter()
                .any(|r| r == "Bash(rm:*)")
        );
    }

    #[test]
    fn claude_resident_cancel_closes_banner_without_response() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        pane.handle_json_event(can_use_tool_event(
            "req-c",
            "Write",
            serde_json::json!({"file_path": "/a.rs"}),
        ));
        assert!(pane.pending_decision.is_some());
        // 同一 request_id のキャンセルでバナーを閉じ、pending をクリアする。
        pane.handle_json_event(serde_json::json!({
            "type": "control_cancel_request", "request_id": "req-c"
        }));
        assert!(pane.pending_decision.is_none());
        assert!(pane.claude_pending_tool.is_none());
    }

    #[test]
    fn claude_resident_esc_sends_graceful_interrupt() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        pane.turn_running = true;
        assert!(pane.interrupt_turn_by_esc());
        // graceful interrupt を送り、2 秒の完了待ちへ入る（まだ kill しない）。
        assert!(pane.claude_interrupting);
        assert!(pane.claude_interrupt_deadline.is_some());
        assert!(pane.claude_resident.is_some());
        assert!(pane.turn_running);
    }

    #[test]
    fn claude_resident_result_after_interrupt_completes_without_queue() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        pane.turn_running = true;
        pane.claude_interrupting = true;
        pane.claude_interrupt_deadline = Some(Instant::now() + CLAUDE_INTERRUPT_GRACE);
        pane.queued_prompts
            .push_back(QueuedPrompt::user("次の依頼".to_string()));
        pane.handle_json_event(serde_json::json!({
            "type": "result", "subtype": "error_during_execution",
            "is_error": true, "result": null, "terminal_reason": "aborted_streaming",
            "session_id": "sid-1"
        }));
        // 中断完了として扱い、キューは自動開始しない（保留）。
        assert!(!pane.claude_interrupting);
        assert!(pane.claude_interrupt_deadline.is_none());
        assert!(!pane.turn_running);
        assert_eq!(pane.queued_prompts.len(), 1);
        assert!(pane.log.iter().any(|l| l.text.contains("中断しました")));
    }

    #[test]
    fn claude_resident_death_marks_turn_error() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        pane.turn_running = true;
        pane.handle_claude_resident_death();
        assert!(pane.claude_resident.is_none());
        assert!(!pane.turn_running);
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Error && l.text.contains("プロセスが終了"))
        );
    }

    #[test]
    fn claude_resident_normal_result_starts_queued_turn() {
        let mut pane = claude_resident_pane();
        if !with_dummy_resident(&mut pane) {
            return;
        }
        pane.thread_id = Some("sid-1".to_string());
        pane.turn_running = true;
        // 実プロセスは cat なので次ターンの実起動はしないが、キューが消費されることを確認する。
        pane.queued_prompts
            .push_back(QueuedPrompt::direct("依頼".to_string(), "依頼".to_string()));
        pane.handle_json_event(serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done",
            "session_id": "sid-1", "total_cost_usd": 0.0088,
            "usage": {"input_tokens": 10, "output_tokens": 5}
        }));
        // cost/usage を保存する。
        assert_eq!(pane.claude_total_cost_usd, Some(0.0088));
        assert!(pane.claude_last_usage.is_some());
        // キューが消費されて次ターンへ進む（実プロセスは cat のため送信のみ）。
        assert_eq!(pane.queued_prompts.len(), 0);
    }

    // -- Codex app-server 常駐モード（実プロセスは cat のダミー）--------------

    /// app-server 常駐（ダミー・Ready 相当）を差し込んだ Codex ペインを作る。
    /// 生成不可な環境では None。
    fn codex_appserver_pane() -> Option<CodexPane> {
        let mut pane = CodexPane::test_with_output(10, 80, 0, "");
        pane.finished = false;
        pane.kind = AgentKind::Codex;
        pane.codex_appserver_enabled = true;
        let client = codex_appserver::AppServerClient::spawn_dummy()?;
        pane.codex_appserver = Some(client);
        pane.codex_appserver_phase = CodexAppServerPhase::Ready;
        pane.thread_id = Some("th-1".to_string());
        Some(pane)
    }

    #[test]
    fn codex_appserver_routes_jsonrpc_to_resident_handler() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        // turn/started で見出し行が立つ。
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "turn/started", "params": {"threadId": "th-1"}
        }));
        assert!(pane.log.iter().any(|l| l.kind == CodexLogKind::Turn));
    }

    #[test]
    fn codex_appserver_command_item_updates_running_command() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/started",
            "params": {"item": {"id": "call_1", "type": "commandExecution", "command": "cargo build"}}
        }));
        assert!(pane.current_command.is_some());
        // ライブ出力デルタが末尾行バッファに載る。
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/commandExecution/outputDelta",
            "params": {"itemId": "call_1", "delta": "compiling...\n"}
        }));
        assert_eq!(
            pane.codex_appserver_live_output(),
            vec!["compiling...".to_string()]
        );
        // 完了で current_command とバッファを解放し、確定行を残す。
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/completed",
            "params": {"item": {"id": "call_1", "type": "commandExecution", "command": "cargo build", "exitCode": 0, "durationMs": 900, "status": "completed"}}
        }));
        assert!(pane.current_command.is_none());
        assert!(pane.codex_appserver_live_output().is_empty());
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Tool && l.text.contains("OK"))
        );
    }

    #[test]
    fn codex_appserver_filechange_item_updates_current_activity() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        // started で「今」表示（current_command / action）へ対象パスが載る。
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/started",
            "params": {"item": {"id": "fc_1", "type": "fileChange", "changes": [
                {"path": "src/tui/ui.rs", "kind": {"type": "update"}}
            ]}}
        }));
        assert_eq!(pane.current_command.as_deref(), Some("src/tui/ui.rs"));
        assert!(pane.current_command_started_at.is_some());
        assert_eq!(pane.work_action(), Some("ファイル変更: src/tui/ui.rs"));
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Tool
                    && l.text.contains("ファイル変更 src/tui/ui.rs"))
        );
        // completed でクリアし、確定行を残す。
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/completed",
            "params": {"item": {"id": "fc_1", "type": "fileChange", "status": "completed", "changes": [
                {"path": "src/tui/ui.rs", "kind": {"type": "update"}}
            ]}}
        }));
        assert!(pane.current_command.is_none());
        assert!(pane.current_command_started_at.is_none());
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Tool && l.text.contains("completed"))
        );
    }

    #[test]
    fn codex_appserver_filechange_multiple_paths_show_rest_count() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/started",
            "params": {"item": {"id": "fc_2", "type": "fileChange", "changes": [
                {"path": "a.rs", "kind": {"type": "update"}},
                {"path": "b.rs", "kind": {"type": "add"}},
                {"path": "c.rs", "kind": {"type": "update"}},
                {"path": "d.rs", "kind": {"type": "delete"}}
            ]}}
        }));
        let command = pane.current_command.as_deref().unwrap();
        assert!(command.contains("a.rs"));
        assert!(command.contains("ほか1件"));
    }

    #[test]
    fn codex_appserver_agent_message_delta_streams_and_completes() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/agentMessage/delta",
            "params": {"itemId": "m1", "delta": "答えは"}
        }));
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/agentMessage/delta",
            "params": {"itemId": "m1", "delta": "2です"}
        }));
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "item/completed",
            "params": {"item": {"id": "m1", "type": "agentMessage", "text": "答えは2です", "phase": "final_answer"}}
        }));
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Assistant && l.text.contains("答えは2です"))
        );
    }

    #[test]
    fn codex_appserver_turn_completed_finishes_turn() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.codex_appserver_turn_id = Some("turn-1".to_string());
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "turn/completed",
            "params": {"threadId": "th-1", "turn": {"id": "turn-1", "status": "completed", "items": []}}
        }));
        // キューが無ければターンは終了状態になる。
        assert!(!pane.turn_running);
        assert!(pane.codex_appserver_turn_id.is_none());
        assert!(pane.log.iter().any(|l| l.text.contains("応答が完了")));
    }

    #[test]
    fn codex_appserver_turn_completed_consumes_queue() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.codex_appserver_turn_id = Some("turn-1".to_string());
        pane.queued_prompts
            .push_back(QueuedPrompt::direct("次".to_string(), "次".to_string()));
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "turn/completed",
            "params": {"threadId": "th-1", "turn": {"id": "turn-1", "status": "completed", "items": []}}
        }));
        // 通常完了はキューを消費して次ターンへ進む（ダミーは送信のみ）。
        assert_eq!(pane.queued_prompts.len(), 0);
    }

    #[test]
    fn codex_appserver_esc_sends_graceful_interrupt() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.codex_appserver_turn_id = Some("turn-1".to_string());
        assert!(pane.interrupt_turn_by_esc());
        assert!(pane.codex_appserver_interrupting);
        assert!(pane.codex_appserver_interrupt_deadline.is_some());
        assert!(pane.turn_running);
    }

    #[test]
    fn codex_appserver_interrupted_completion_holds_queue() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.codex_appserver_interrupting = true;
        pane.codex_appserver_interrupt_deadline =
            Some(Instant::now() + CODEX_APPSERVER_INTERRUPT_GRACE);
        pane.queued_prompts
            .push_back(QueuedPrompt::direct("次".to_string(), "次".to_string()));
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "turn/completed",
            "params": {"threadId": "th-1", "turn": {"id": "turn-1", "status": "interrupted", "items": []}}
        }));
        assert!(!pane.codex_appserver_interrupting);
        assert!(!pane.turn_running);
        // 中断はキューを自動開始しない（保留）。
        assert_eq!(pane.queued_prompts.len(), 1);
        assert!(pane.log.iter().any(|l| l.text.contains("中断しました")));
    }

    #[test]
    fn codex_appserver_command_approval_replies_with_decision() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "id": 0, "method": "item/commandExecution/requestApproval",
            "params": {"itemId": "call_1", "command": "rm -rf x", "availableDecisions": ["accept", "acceptForSession", "decline"]}
        }));
        let banner = pane.pending_decision.as_ref().expect("banner");
        assert_eq!(banner.kind, CodexDecisionKind::Permission);
        assert!(pane.codex_appserver_pending_approval.is_some());
        // a（許可）で応答し、バナーと pending を解消する。
        pane.handle_decision_key(key(KeyCode::Char('a')));
        assert!(pane.pending_decision.is_none());
        assert!(pane.codex_appserver_pending_approval.is_none());
    }

    #[test]
    fn codex_appserver_token_usage_saved() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.handle_json_event(serde_json::json!({
            "jsonrpc": "2.0", "method": "thread/tokenUsage/updated",
            "params": {"threadId": "th-1", "tokenUsage": {"total": {"totalTokens": 100}, "last": {"totalTokens": 20}, "modelContextWindow": 4096}}
        }));
        assert!(pane.codex_appserver_token_usage.is_some());
        assert!(
            pane.last_token_usage_label
                .as_deref()
                .unwrap()
                .contains("total 100")
        );
    }

    #[test]
    fn codex_appserver_death_marks_turn_error() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.turn_running = true;
        pane.handle_codex_appserver_death();
        assert!(pane.codex_appserver.is_none());
        assert!(!pane.turn_running);
        assert_eq!(pane.codex_appserver_phase, CodexAppServerPhase::Idle);
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Error && l.text.contains("プロセスが終了"))
        );
    }

    #[test]
    fn codex_appserver_settings_change_sends_update_when_ready() {
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        // 具体値のある model 変更は settings/update を送り、応答待ちになる。
        pane.exec_settings.model = CodexModelChoice::Gpt5;
        pane.push_codex_appserver_model();
        assert!(pane.codex_appserver_pending_setting.is_some());
        // config（値なし）へ戻す approval 変更は settings/update では表せず再起動保留になる。
        pane.exec_settings.approval = CodexApprovalChoice::Config;
        pane.codex_appserver_pending_setting = None;
        pane.push_codex_appserver_approval();
        assert!(pane.codex_appserver_restart_pending);
    }

    #[test]
    fn codex_appserver_initialize_response_without_jsonrpc_advances_handshake() {
        // codex 0.142.5 は initialize 応答に "jsonrpc":"2.0" を付けない。
        // jsonrpc 無しの `{"id":N,"result":{...}}` でも Initializing→StartingThread へ進むこと。
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.codex_appserver_phase = CodexAppServerPhase::Initializing { request_id: 5 };
        pane.handle_json_event(serde_json::json!({
            "id": 5, "result": {"userAgent": "codex"}
        }));
        // initialized 通知 → thread/resume（thread_id あり）を送り StartingThread へ遷移する。
        assert!(matches!(
            pane.codex_appserver_phase,
            CodexAppServerPhase::StartingThread { .. }
        ));
    }

    #[test]
    fn codex_appserver_notification_without_jsonrpc_is_routed() {
        // jsonrpc 無しの通知 `{"method":"turn/started",...}` が常駐経路で処理されること。
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        pane.handle_json_event(serde_json::json!({
            "method": "turn/started", "params": {"threadId": "th-1"}
        }));
        assert!(pane.log.iter().any(|l| l.kind == CodexLogKind::Turn));
    }

    #[test]
    fn codex_appserver_handshake_timeout_falls_back_to_oneshot() {
        // ハンドシェイク応答が来ないまま deadline を過ぎたら、ワンショットへフォールバックして
        // 「考え中」フリーズを解消する。
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        // フォールバックのワンショット再実行で実 codex を起動しないよう、実在しないパスにする。
        pane.codex_bin = PathBuf::from("/nonexistent/addness-codex-test-bin");
        pane.codex_appserver_phase = CodexAppServerPhase::Initializing { request_id: 1 };
        pane.turn_running = true;
        pane.current_turn_prompt = Some("依頼".to_string());
        pane.current_turn_retry_prompt = Some("依頼".to_string());
        // deadline を過去にして poll を呼ぶ。
        pane.codex_appserver_handshake_deadline = Some(Instant::now() - Duration::from_secs(1));
        assert!(pane.poll_codex_appserver());
        // 常駐は破棄され、フォールバックのため deadline も enabled も落ちる。
        assert!(pane.codex_appserver.is_none());
        assert!(pane.codex_appserver_handshake_deadline.is_none());
        assert!(!pane.codex_appserver_enabled);
        assert!(
            pane.log
                .iter()
                .any(|l| l.kind == CodexLogKind::Error && l.text.contains("15秒以内"))
        );
    }

    #[test]
    fn codex_appserver_ready_idle_has_no_handshake_deadline() {
        // Ready かつ turn 応答待ちでもない平常時に deadline が残らないこと（誤フォールバック防止）。
        let Some(mut pane) = codex_appserver_pane() else {
            return;
        };
        assert_eq!(pane.codex_appserver_phase, CodexAppServerPhase::Ready);
        assert!(pane.codex_appserver_handshake_deadline.is_none());
        // poll を回してもフォールバックは起きない。
        assert!(!pane.poll_codex_appserver());
        assert!(pane.codex_appserver.is_some());
    }

    #[test]
    fn claude_stream_text_delta_shown_once() {
        let mut pane = claude_pane();
        // 部分メッセージのトークンを 2 つ流す。
        pane.handle_json_event(serde_json::json!({
            "type": "stream_event",
            "event": {"type": "content_block_delta", "index": 0,
                      "delta": {"type": "text_delta", "text": "こん"}}
        }));
        pane.handle_json_event(serde_json::json!({
            "type": "stream_event",
            "event": {"type": "content_block_delta", "index": 0,
                      "delta": {"type": "text_delta", "text": "にちは"}}
        }));
        // 完成形の assistant イベントが後から来る。
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "こんにちは"}]}
        }));
        let assistant_lines: Vec<_> = pane
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Assistant)
            .collect();
        // 逐次表示した 1 行だけが残り、二重表示されない。
        assert_eq!(assistant_lines.len(), 1);
        assert_eq!(assistant_lines[0].text, "こんにちは");
    }

    #[test]
    fn claude_assistant_multiple_text_blocks_are_not_merged() {
        let mut pane = claude_pane();
        // まず 1 ブロック目をストリーミング表示する。
        pane.handle_json_event(serde_json::json!({
            "type": "stream_event",
            "event": {"type": "content_block_delta", "index": 0,
                      "delta": {"type": "text_delta", "text": "最初のブロック"}}
        }));
        // 1 イベントに Text ブロックが 2 つ同梱される（防御ケース）。
        pane.handle_json_event(serde_json::json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "text", "text": "最初のブロック"},
                {"type": "text", "text": "二番目のブロック"}
            ]}
        }));
        let assistant_lines: Vec<_> = pane
            .log
            .iter()
            .filter(|line| line.kind == CodexLogKind::Assistant)
            .collect();
        // 1 ブロック目はストリーミング行を上書き、2 ブロック目は別行に。連結・欠落しない。
        assert_eq!(assistant_lines.len(), 2);
        assert_eq!(assistant_lines[0].text, "最初のブロック");
        assert_eq!(assistant_lines[1].text, "二番目のブロック");
    }

    #[test]
    fn claude_repeated_denial_after_approval_shows_error_not_banner() {
        let mut pane = claude_pane();
        pane.thread_id = Some("sid-1".to_string());
        let denial_event = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done",
            "session_id": "sid-1",
            "permission_denials": [
                {"tool_name": "Bash", "tool_use_id": "t1", "tool_input": {"command": "rm -rf x"}}
            ]
        });
        // 1 回目: バナーが出るので Accept で承認しリトライする。
        pane.handle_json_event(denial_event.clone());
        assert!(pane.pending_decision.is_some());
        pane.handle_decision_key(key(KeyCode::Char('a')));
        assert!(pane.pending_decision.is_none());

        // リトライ結果で同一の拒否が再発 → バナーを再表示せずエラーで知らせる（ループ回避）。
        pane.handle_json_event(denial_event);
        assert!(pane.pending_decision.is_none());
        assert!(
            pane.log.iter().any(|l| l.kind == CodexLogKind::Error
                && l.text.contains("許可ルールでは通せませんでした"))
        );
    }

    #[test]
    fn claude_f_keys_cycle_claude_settings() {
        let mut pane = claude_pane();
        pane.cycle_model();
        assert!(pane.settings_label().contains("model:fable"));
        pane.cycle_reasoning();
        assert!(pane.settings_label().contains("effort:low"));
        pane.cycle_approval();
        assert!(pane.settings_label().contains("permission:plan"));
        // F5（sandbox）は Claude では F4 へ案内する通知だけ。
        pane.cycle_sandbox();
        assert!(
            pane.log
                .iter()
                .any(|line| line.text.contains("F4") && line.text.contains("permission-mode"))
        );
    }

    #[test]
    fn claude_codex_subcommand_is_rejected() {
        let mut pane = claude_pane();
        pane.start_codex_subcommand(vec!["doctor".to_string()], "codex doctor".to_string());
        assert!(
            pane.log
                .iter()
                .any(|line| line.text.contains("codex 専用コマンド"))
        );
        assert!(!pane.is_turn_running());
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

    #[test]
    fn paste_fold_threshold_boundaries() {
        // 10 行ちょうど・800 文字ちょうどは畳み込まない（超えたら畳み込む）。
        let ten_lines = (0..10)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(paste_line_count(&ten_lines), 10);
        assert!(!paste_should_fold(&ten_lines));
        let eleven_lines = (0..11)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(paste_line_count(&eleven_lines), 11);
        assert!(paste_should_fold(&eleven_lines));

        let exactly_800 = "a".repeat(800);
        assert!(!paste_should_fold(&exactly_800));
        let over_800 = "a".repeat(801);
        assert!(paste_should_fold(&over_800));
    }

    #[test]
    fn paste_placeholder_reports_line_count() {
        let text = "l1\nl2\nl3";
        assert_eq!(paste_placeholder(1, text), "[貼り付け#1: 3行]");
        assert_eq!(paste_placeholder(2, "solo"), "[貼り付け#2: 1行]");
    }

    #[test]
    fn expand_paste_placeholders_replaces_matching_and_keeps_edited() {
        let pastes = vec![
            StoredPaste {
                placeholder: "[貼り付け#1: 3行]".to_string(),
                full: "l1\nl2\nl3".to_string(),
            },
            StoredPaste {
                placeholder: "[貼り付け#2: 2行]".to_string(),
                full: "a\nb".to_string(),
            },
        ];
        let line = "前 [貼り付け#1: 3行] 中 [貼り付け#2: 2行] 後";
        assert_eq!(
            expand_paste_placeholders(line, &pastes),
            "前 l1\nl2\nl3 中 a\nb 後"
        );
        // 編集されて一致しないプレースホルダは展開せずそのまま残す。
        let edited = "[貼り付け#1: 9行] のこり";
        assert_eq!(expand_paste_placeholders(edited, &pastes), edited);
    }

    #[test]
    fn paste_input_folds_long_and_expands_on_submit() {
        let mut pane = live_pane();
        let long = (0..15)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        pane.paste_input(&long);
        assert_eq!(pane.input_line(), "[貼り付け#1: 15行]");
        assert_eq!(pane.pending_pastes.len(), 1);

        // 2 件目は #2 として区別する。
        pane.paste_input(&long);
        assert!(pane.input_line().contains("[貼り付け#2: 15行]"));
        assert_eq!(pane.pending_pastes.len(), 2);

        // 送信時展開: プレースホルダが全文へ戻り、保持データはクリアされる。
        let line = pane.input_line().to_string();
        let expanded = pane.expand_pending_pastes(line);
        assert!(expanded.contains("line0\nline1"));
        assert!(!expanded.contains("[貼り付け#"));
        assert!(pane.pending_pastes.is_empty());
        assert_eq!(pane.paste_seq, 0);
    }

    #[test]
    fn paste_input_short_inserts_directly() {
        let mut pane = live_pane();
        pane.paste_input("short paste");
        assert_eq!(pane.input_line(), "short paste");
        assert!(pane.pending_pastes.is_empty());
    }

    #[test]
    fn next_clip_filename_increments_past_existing() {
        assert_eq!(next_clip_filename(&[]), "clip-1.png");
        let existing = vec![
            "clip-1.png".to_string(),
            "clip-3.png".to_string(),
            "other.png".to_string(),
        ];
        assert_eq!(next_clip_filename(&existing), "clip-4.png");
    }

    #[test]
    fn next_clip_path_uses_injected_dir() {
        let dir = std::env::temp_dir().join(format!(
            "addness-clip-test-{}-{}",
            std::process::id(),
            "nextpath"
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip-1.png"), b"x").unwrap();
        std::fs::write(dir.join("clip-3.png"), b"x").unwrap();
        let path = next_clip_path(&dir);
        assert_eq!(path.parent().unwrap(), dir.as_path());
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "clip-4.png");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attach_clipboard_image_on_non_macos_reports_unsupported() {
        // macOS 以外では未対応メッセージを出す（osascript は呼ばない）。
        #[cfg(not(target_os = "macos"))]
        {
            let mut pane = live_pane();
            pane.attach_clipboard_image();
            assert!(pane.log.iter().any(|line| line.text.contains("未対応")));
        }
    }

    #[test]
    fn append_claude_image_paths_appends_and_clears_for_claude() {
        let mut pane = claude_pane();
        pane.exec_settings.image_paths = vec!["/tmp/a.png".to_string(), "/tmp/b.png".to_string()];
        let out = pane.append_claude_image_paths("元のプロンプト".to_string());
        assert!(out.starts_with("元のプロンプト"));
        assert!(out.contains("添付画像（Readツールで内容を確認してください）:"));
        assert!(out.contains("- /tmp/a.png"));
        assert!(out.contains("- /tmp/b.png"));
        // 消費後はリストがクリアされる。
        assert!(pane.exec_settings.image_paths.is_empty());
    }

    #[test]
    fn append_claude_image_paths_noop_for_codex() {
        let mut pane = live_pane();
        assert_eq!(pane.kind, AgentKind::Codex);
        pane.exec_settings.image_paths = vec!["/tmp/a.png".to_string()];
        let out = pane.append_claude_image_paths("元のプロンプト".to_string());
        // Codex ではプロンプト不変・リスト保持（--image 引数で消費するため）。
        assert_eq!(out, "元のプロンプト");
        assert_eq!(
            pane.exec_settings.image_paths,
            vec!["/tmp/a.png".to_string()]
        );
    }

    #[test]
    fn image_slash_command_visible_for_claude() {
        assert!(slash_command_visible_for_kind(
            "/image",
            AgentKind::ClaudeCode
        ));
        assert!(slash_command_visible_for_kind(
            "/attachments",
            AgentKind::ClaudeCode
        ));
        assert!(slash_command_visible_for_kind(
            "/fork",
            AgentKind::ClaudeCode
        ));
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
    fn record_submitted_marks_exit_for_quit_and_exit_aliases() {
        for cmd in ["/quit", "/exit"] {
            let mut state = CodexInputState::default();
            state.record_submitted(cmd);
            assert!(
                state.exit_command_sent,
                "{cmd} should mark exit_command_sent"
            );
            assert!(
                state.last_prompt.is_none(),
                "{cmd} should not be recorded as last_prompt"
            );
        }
    }

    #[test]
    fn dispatch_exit_finishes_immediately_even_when_turn_running() {
        let mut pane = claude_pane();
        pane.turn_running = true;
        pane.queued_prompts
            .push_back(QueuedPrompt::user("queued".to_string()));

        let handled = pane.handle_local_slash_command("/exit");

        assert!(handled, "/exit should be handled locally");
        assert!(pane.finished);
        assert!(pane.queued_prompts.is_empty());
        assert!(pane.input_state.exit_command_sent);
        assert!(!pane.turn_running);
        assert!(pane.should_close_after_exit_command());
    }

    #[test]
    fn dispatch_quit_alias_finishes_and_marks_exit() {
        let mut pane = claude_pane();

        let handled = pane.handle_local_slash_command("/quit");

        assert!(handled, "/quit should be handled locally");
        assert!(pane.finished);
        assert!(pane.input_state.exit_command_sent);
        assert!(pane.should_close_after_exit_command());
    }

    #[test]
    fn new_thread_clears_checkpoints_and_queued_prompts() {
        let mut pane = claude_pane();
        pane.checkpoints.push(Checkpoint {
            ref_name: "refs/addness/checkpoint-x-0".to_string(),
            turn: 1,
        });
        pane.checkpoint_seq = 3;
        pane.pending_checkpoint_requests
            .push_back(CheckpointRequest {
                cwd: ".".to_string(),
                ref_name: "refs/addness/checkpoint-x-1".to_string(),
                turn: 2,
                message: "cp".to_string(),
            });
        pane.queued_prompts
            .push_back(QueuedPrompt::user("queued".to_string()));

        pane.handle_new_thread_slash_command();

        assert!(pane.checkpoints.is_empty());
        assert_eq!(pane.checkpoint_seq, 0);
        assert!(pane.pending_checkpoint_requests.is_empty());
        assert!(pane.queued_prompts.is_empty());
    }

    #[test]
    fn start_claude_resume_clears_pending_decision_and_queued_prompts() {
        let mut pane = claude_pane();
        pane.pending_decision = Some(CodexDecisionBanner::new(
            CodexDecisionKind::Approval,
            "承認待ち".to_string(),
        ));
        pane.queued_prompts
            .push_back(QueuedPrompt::user("queued".to_string()));

        pane.start_claude_resume(Some("session-123"), false, "続きをお願いします");

        assert!(pane.pending_decision.is_none());
        assert!(pane.queued_prompts.is_empty());
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
    fn input_history_dedupes_consecutive_and_caps_length() {
        let mut state = CodexInputState::default();
        assert!(state.push_history("a".to_string()));
        assert!(!state.push_history("a".to_string()));
        assert!(state.push_history("b".to_string()));
        assert!(!state.push_history(String::new()));
        assert_eq!(state.history, vec!["a".to_string(), "b".to_string()]);

        let mut capped = CodexInputState::default();
        for i in 0..(INPUT_HISTORY_MAX + 20) {
            capped.push_history(format!("cmd{i}"));
        }
        assert_eq!(capped.history.len(), INPUT_HISTORY_MAX);
        assert_eq!(capped.history.first().unwrap(), "cmd20");
        assert_eq!(
            capped.history.last().unwrap(),
            &format!("cmd{}", INPUT_HISTORY_MAX + 19)
        );
    }

    #[test]
    fn input_history_up_recalls_saving_draft_and_down_restores_it() {
        let mut state = CodexInputState::default();
        state.push_history("first".to_string());
        state.push_history("second".to_string());
        // 下書きを入力してから履歴を遡る。
        state.insert_text("draft");
        state.observe_key(key(KeyCode::Up));
        assert_eq!(state.line, "second");
        assert_eq!(state.cursor, "second".len());
        state.observe_key(key(KeyCode::Up));
        assert_eq!(state.line, "first");
        // 最古より先へは遡れない。
        state.observe_key(key(KeyCode::Up));
        assert_eq!(state.line, "first");
        // 下方向で新しい方へ戻り、最後に下書きへ復帰する。
        state.observe_key(key(KeyCode::Down));
        assert_eq!(state.line, "second");
        state.observe_key(key(KeyCode::Down));
        assert_eq!(state.line, "draft");
        assert!(state.history_pos.is_none());
    }

    #[test]
    fn input_history_edit_while_browsing_detaches() {
        let mut state = CodexInputState::default();
        state.push_history("recalled".to_string());
        state.observe_key(key(KeyCode::Up));
        assert_eq!(state.line, "recalled");
        assert!(state.history_pos.is_some());
        // 閲覧中に編集すると、その内容が新たな編集対象になる（履歴閲覧から離脱）。
        state.observe_key(key(KeyCode::Char('!')));
        assert_eq!(state.line, "recalled!");
        assert!(state.history_pos.is_none());
        assert!(state.history_draft.is_none());
    }

    #[test]
    fn input_history_multiline_moves_between_lines_before_recall() {
        let mut state = CodexInputState::default();
        state.push_history("older".to_string());
        state.insert_text("top\nbottom");
        // カーソルは末尾（bottom 行）。↑ はまず行間移動する。
        state.observe_key(key(KeyCode::Up));
        assert_eq!(state.line, "top\nbottom");
        assert!(state.history_pos.is_none());
        // 先頭行から更に↑で履歴へ。
        state.observe_key(key(KeyCode::Up));
        assert_eq!(state.line, "older");
    }

    #[test]
    fn active_mention_detects_and_rejects_email_like() {
        assert_eq!(
            active_mention("見て @src/ma", "見て @src/ma".len()),
            Some(("見て ".len(), "src/ma".to_string()))
        );
        // 行頭の @ も有効。
        assert_eq!(
            active_mention("@main", "@main".len()),
            Some((0, "main".to_string()))
        );
        // CJK 直後の @（メールアドレスではない）は候補を出す。
        assert_eq!(
            active_mention("見て@src", "見て@src".len()),
            Some(("見て".len(), "src".to_string()))
        );
        // 直前が ASCII 英数字（メールアドレス等）は候補を出さない。
        assert_eq!(active_mention("user@host", "user@host".len()), None);
        // 直前がローカルパート記号（.）でも候補を出さない。
        assert_eq!(active_mention("foo.bar@host", "foo.bar@host".len()), None);
        // @ 以降に空白があれば確定済みとみなす。
        assert_eq!(active_mention("@src file", "@src file".len()), None);
        // @ が無ければ None。
        assert_eq!(active_mention("plain", "plain".len()), None);
    }

    #[test]
    fn mention_candidates_prefix_before_substring_and_dir_marker() {
        let dir = std::env::temp_dir().join(format!(
            "addness-mention-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("srcbin")).unwrap();
        std::fs::write(dir.join("main.rs"), "").unwrap();
        std::fs::write(dir.join("mainframe.txt"), "").unwrap();
        std::fs::write(dir.join("README.md"), "").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();

        let cands = mention_candidates(&dir, "main");
        let inserts: Vec<&str> = cands.iter().map(|c| c.insert.as_str()).collect();
        // 前方一致（main.rs / mainframe.txt）が並ぶ。隠しファイルは出ない。
        assert!(inserts.contains(&"main.rs"));
        assert!(inserts.contains(&"mainframe.txt"));
        assert!(!inserts.iter().any(|i| i.contains(".hidden")));

        // 空クエリはディレクトリ優先。ディレクトリは末尾 '/'。
        let all = mention_candidates(&dir, "");
        assert_eq!(all.first().unwrap().insert, "srcbin/");
        assert!(all.first().unwrap().is_dir);

        // サブディレクトリへ潜る場合は相対パスを保つ。
        std::fs::write(dir.join("srcbin").join("lib.rs"), "").unwrap();
        let sub = mention_candidates(&dir, "srcbin/li");
        assert_eq!(sub.first().unwrap().insert, "srcbin/lib.rs");

        // 絶対パス（/ 始まり）や `..` を含むクエリは cwd 外に出るため候補を空にする。
        assert!(mention_candidates(&dir, "/etc/pas").is_empty());
        assert!(mention_candidates(&dir, "..").is_empty());
        assert!(mention_candidates(&dir, "../").is_empty());
        assert!(mention_candidates(&dir, "srcbin/../..").is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mention_palette_accepts_file_and_descends_into_dir() {
        let dir = std::env::temp_dir().join(format!(
            "addness-mention-pane-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs").join("guide.md"), "").unwrap();

        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.finished = false;
        pane.cwd = dir.display().to_string();

        for ch in "見て @do".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        assert!(pane.mention_palette_active());
        // ディレクトリ確定は潜って絞り込みを続ける（末尾 '/'）。
        pane.accept_mention_palette_selection();
        assert_eq!(pane.input_line(), "見て @docs/");
        assert!(pane.mention_palette_active());
        // ファイル確定は末尾に空白を付けてパレットを閉じる。
        pane.accept_mention_palette_selection();
        assert_eq!(pane.input_line(), "見て @docs/guide.md ");
        assert!(!pane.mention_palette_active());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mention_palette_esc_dismisses_until_next_input() {
        let dir = std::env::temp_dir().join(format!(
            "addness-mention-esc-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("alpha.txt"), "").unwrap();

        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.finished = false;
        pane.cwd = dir.display().to_string();
        for ch in "@al".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        assert!(pane.mention_palette_active());
        pane.dismiss_mention_palette();
        assert!(!pane.mention_palette_active());
        // 追加入力で再表示される。
        pane.input(key(KeyCode::Char('p')));
        assert!(pane.mention_palette_active());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn esc_interrupt_requires_two_presses() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        assert!(!pane.take_esc_interrupt_armed());
        pane.arm_esc_interrupt();
        assert!(pane.log.iter().any(|l| l.text.contains("もう一度 Esc")));
        assert!(pane.take_esc_interrupt_armed());
        assert!(!pane.take_esc_interrupt_armed());

        assert!(pane.interrupt_turn_by_esc());
        assert!(!pane.is_turn_running());
        assert!(
            pane.log
                .iter()
                .any(|l| l.text.contains("Esc でターンを中断しました"))
        );
    }

    #[test]
    fn esc_interrupt_does_not_auto_start_queued_turn() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.finished = false;
        pane.turn_running = true;
        // 予約ターンを 1 件積んでおく。
        pane.queued_prompts
            .push_back(QueuedPrompt::user("次の依頼".to_string()));

        assert!(pane.interrupt_turn_by_esc());
        assert!(!pane.is_turn_running());
        // /stop と同様、予約ターンは自動開始されず保留される。
        assert_eq!(pane.queued_prompts.len(), 1);
        assert!(pane.log.iter().any(|l| l.text.contains("予約1件は保留中")));
    }

    #[test]
    fn input_history_persists_to_file_and_reloads() {
        let path = std::env::temp_dir().join(format!(
            "addness-input-history-{}-{}.jsonl",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_file(&path);

        append_input_history(&path, "最初の依頼").unwrap();
        append_input_history(&path, "複数行\n依頼").unwrap();
        let loaded = load_input_history(&path);
        assert_eq!(
            loaded,
            vec!["最初の依頼".to_string(), "複数行\n依頼".to_string()]
        );

        let _ = std::fs::remove_file(&path);
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
    fn exit_finishes_immediately_even_during_running_turn() {
        // `/exit` はターン実行中でも即終了させる（キュー予約に回さない）。
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.finished = false;
        pane.turn_running = true;

        for ch in "/exit".chars() {
            pane.input(key(KeyCode::Char(ch)));
        }
        pane.input(key(KeyCode::Enter));

        assert!(pane.finished);
        assert_eq!(pane.queued_prompt_count(), 0);
        assert!(!pane.turn_running);
        assert!(pane.input_state.exit_command_sent);
        assert!(pane.should_close_after_exit_command());
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
        assert_eq!(pane.work_action(), Some("確認応答: これからずっと許可"));
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
        assert_eq!(pane.work_action(), Some("依頼を確認中"));

        assert!(pane.handle_decision_key(key(KeyCode::Char('y'))));

        assert!(pane.decision_banner().is_none());
        assert_eq!(pane.work_action(), Some("確認応答: Yes"));
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
        assert_eq!(pane.work_action(), Some("依頼を確認中"));
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
    fn conversation_filter_mixes_in_live_tool_lines_while_turn_running() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.push_log(CodexLogKind::Turn, "Turn 1");
        pane.push_log(CodexLogKind::User, "cargo test して");
        pane.push_log(CodexLogKind::Tool, "old turn tool line");

        // ターンが実行中でなければ、従来通り会話フィルタは Tool/Event を含まない。
        let idle = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(!idle.contains(&"old turn tool line"));
        assert_eq!(pane.log_filter_display_label(), "会話");

        // ターン実行中は直近ターンの Tool/Event 行を会話フィルタでも混ぜて表示する。
        pane.turn_running = true;
        pane.push_log(CodexLogKind::Turn, "Turn 2");
        pane.push_log(CodexLogKind::Assistant, "確認します");
        pane.push_log(CodexLogKind::Tool, "exec_command_begin: cargo test");
        pane.push_log(CodexLogKind::Event, "file changed: src/lib.rs");

        let running = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(running.contains(&"exec_command_begin: cargo test"));
        assert!(running.contains(&"file changed: src/lib.rs"));
        // 完了済みの前ターンの Tool 行は引き続き隠れたまま。
        assert!(!running.contains(&"old turn tool line"));
        assert_eq!(pane.log_filter_display_label(), "会話+実行中");

        // ターンが終われば従来の会話フィルタ挙動に戻る（ログは汚さない）。
        pane.turn_running = false;
        let finished = pane
            .filtered_log_lines()
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>();
        assert!(!finished.contains(&"exec_command_begin: cargo test"));
        assert!(!finished.contains(&"file changed: src/lib.rs"));
        assert_eq!(pane.log_filter_display_label(), "会話");
    }

    #[test]
    fn live_tool_mixing_only_applies_to_conversation_filter() {
        let mut pane = CodexPane::test_with_output(8, 40, 0, "");
        pane.turn_running = true;
        pane.push_log(CodexLogKind::Turn, "Turn 1");
        pane.push_log(CodexLogKind::Tool, "exec_command_begin: ls");

        // 実行フィルタでは元々 Tool 行が見えるため、混在ロジックは表示件数へ影響しない。
        pane.cycle_log_filter();
        assert_eq!(pane.log_filter, CodexLogFilter::Tools);
        let tools = pane.filtered_log_lines();
        assert!(
            tools
                .iter()
                .any(|line| line.text == "exec_command_begin: ls")
        );
        assert_eq!(pane.log_filter_display_label(), "実行");

        // 失敗フィルタは対象外（Tool行は依然として現れない）。
        pane.cycle_log_filter();
        assert_eq!(pane.log_filter, CodexLogFilter::Errors);
        let errors = pane.filtered_log_lines();
        assert!(
            !errors
                .iter()
                .any(|line| line.text == "exec_command_begin: ls")
        );
        assert_eq!(pane.log_filter_display_label(), "失敗");
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
    fn confirming_reminder_is_not_sent_before_interval_elapses() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? yes/no"}"#);
        // 初回の通知（set_pending_decision 時点）を消費しておく。
        pane.take_terminal_notice();

        // まだリマインド間隔（5分）が経っていないので追加通知は来ない。
        pane.remind_confirming_if_stale();
        assert!(pane.take_terminal_notice().is_none());
    }

    #[test]
    fn confirming_reminder_repeats_while_still_pending() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? yes/no"}"#);
        pane.take_terminal_notice();

        // リマインド間隔が経過した状態を作る（実時間を待たずにテストするための時刻巻き戻し）。
        pane.last_confirming_reminder_at = Some(Instant::now() - Duration::from_secs(5 * 60 + 1));
        pane.remind_confirming_if_stale();

        let reminder = pane
            .take_terminal_notice()
            .expect("承認待ちが続いていればリマインド通知が来るはず");
        assert!(reminder.title.contains("承認待ち継続中"));
        assert!(reminder.message.contains("yes/no"));

        // 直後にもう一度呼んでも、間隔内なので再送されない。
        pane.remind_confirming_if_stale();
        assert!(pane.take_terminal_notice().is_none());
    }

    #[test]
    fn confirming_reminder_stops_once_decision_resolved() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(r#"{"type":"approval_requested","message":"Run command? yes/no"}"#);
        pane.take_terminal_notice();

        pane.resolve_pending_decision(
            pane.pending_decision.clone().unwrap(),
            CodexDecisionResponse::Accept,
            false,
        );
        pane.take_terminal_notice(); // 応答時の通知を消費

        pane.last_confirming_reminder_at = Some(Instant::now() - Duration::from_secs(5 * 60 + 1));
        pane.remind_confirming_if_stale();
        assert!(pane.take_terminal_notice().is_none());
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
                kind: AgentKind::Codex,
                codex_bin: Path::new("codex"),
                cwd: &cwd,
                addness_bin: "addness",
                goal_id: "goal/history".to_string(),
                goal_title: "History goal".to_string(),
                dod: String::new(),
                status_label: "TEST".to_string(),
                session_log_path: Some(history_path),
                input_history_path: None,
                attachments_dir: None,
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
            kind: AgentKind::Codex,
            codex_bin: Path::new("codex"),
            cwd: &cwd,
            addness_bin: "addness",
            goal_id: "goal/history".to_string(),
            goal_title: "History goal".to_string(),
            dod: String::new(),
            status_label: "TEST".to_string(),
            session_log_path: Some(history_path),
            input_history_path: None,
            attachments_dir: None,
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

    /// テスト用の一時 JSONL パス。
    fn thread_id_temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "addness-thread-id-{tag}-{}-{}.jsonl",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ))
    }

    /// session_log_path を注入した Codex ペイン（実プロセスは起動しない）。
    fn codex_pane_with_log(path: PathBuf) -> CodexPane {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        CodexPane::spawn_inner(CodexPaneSpawnOptions {
            kind: AgentKind::Codex,
            codex_bin: Path::new("codex"),
            cwd: &cwd,
            addness_bin: "addness",
            goal_id: "goal/thread".to_string(),
            goal_title: "Thread goal".to_string(),
            dod: String::new(),
            status_label: "TEST".to_string(),
            session_log_path: Some(path),
            input_history_path: None,
            attachments_dir: None,
        })
        .unwrap()
    }

    fn count_thread_id_records(path: &Path) -> usize {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .filter(|line| line.contains(r#""record":"thread_id""#))
            .count()
    }

    #[test]
    fn load_codex_session_restores_last_thread_id() {
        let path = thread_id_temp_path("load-last");
        let _ = std::fs::remove_file(&path);
        std::fs::write(
            &path,
            "{\"record\":\"thread_id\",\"id\":\"t1\"}\n{\"record\":\"thread_id\",\"id\":\"t2\"}\n",
        )
        .unwrap();

        let loaded = load_codex_session(&path).unwrap();
        assert_eq!(loaded.thread_id.as_deref(), Some("t2"));

        // 末尾に tombstone(None) を書くと復元値も None になる。
        std::fs::write(
            &path,
            "{\"record\":\"thread_id\",\"id\":\"t1\"}\n{\"record\":\"thread_id\",\"id\":null}\n",
        )
        .unwrap();
        let loaded = load_codex_session(&path).unwrap();
        assert_eq!(loaded.thread_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_codex_session_skips_unknown_records() {
        let path = thread_id_temp_path("unknown");
        let _ = std::fs::remove_file(&path);
        // 未知レコード（旧バイナリ互換）を挟んでも読込は壊れず、既知レコードは反映される。
        std::fs::write(
            &path,
            "{\"record\":\"log\",\"kind\":\"System\",\"text\":\"hi\"}\n{\"record\":\"future_variant\",\"foo\":123}\n{\"record\":\"thread_id\",\"id\":\"t9\"}\n",
        )
        .unwrap();

        let loaded = load_codex_session(&path).unwrap();
        assert_eq!(loaded.thread_id.as_deref(), Some("t9"));
        assert!(
            loaded
                .log
                .iter()
                .any(|line| line.kind == CodexLogKind::System && line.text == "hi")
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_thread_id_dedupes_and_persists_only_changes() {
        let path = thread_id_temp_path("dedupe");
        let _ = std::fs::remove_file(&path);
        let mut pane = codex_pane_with_log(path.clone());
        assert_eq!(count_thread_id_records(&path), 0);

        pane.set_thread_id(Some("t1".to_string()));
        pane.set_thread_id(Some("t1".to_string())); // 同値: 記録しない
        assert_eq!(count_thread_id_records(&path), 1);

        pane.set_thread_id(Some("t2".to_string())); // 変化: 1件追記
        assert_eq!(count_thread_id_records(&path), 2);

        pane.set_thread_id(None); // tombstone: 1件追記
        assert_eq!(count_thread_id_records(&path), 3);
        assert_eq!(pane.thread_id, None);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drop_restored_thread_on_failure_only_when_restored() {
        let path = thread_id_temp_path("drop");
        let _ = std::fs::remove_file(&path);
        let mut pane = codex_pane_with_log(path.clone());

        // 復元フラグが立っていなければ何もしない。
        pane.thread_id = Some("live-1".to_string());
        pane.thread_id_restored = false;
        pane.drop_restored_thread_on_failure();
        assert_eq!(pane.thread_id.as_deref(), Some("live-1"));
        assert_eq!(count_thread_id_records(&path), 0);

        // 復元フラグが立っていれば thread_id をクリアし tombstone を書く。
        pane.thread_id = Some("restored-1".to_string());
        pane.thread_id_restored = true;
        pane.drop_restored_thread_on_failure();
        assert_eq!(pane.thread_id, None);
        assert!(!pane.thread_id_restored);
        assert_eq!(count_thread_id_records(&path), 1);

        let _ = std::fs::remove_file(&path);
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

        assert_eq!(pane.work_action(), Some("ゴール文脈を読込中"));
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
    fn begin_turn_work_resets_previous_work_action() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.set_work_action("ツール実行: Bash");

        pane.begin_turn_work("依頼を確認中");

        assert_eq!(pane.work_action(), Some("依頼を確認中"));
    }

    #[test]
    fn end_turn_work_resets_accumulated_work_action() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.begin_turn_work("依頼を確認中");
        pane.set_work_action("ツール実行: Bash");

        pane.end_turn_work("応答完了");

        assert_eq!(pane.work_action(), Some("応答完了"));
    }

    #[test]
    fn status_note_and_work_action_are_independent() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");

        pane.set_status_note("model: gpt-5");
        assert_eq!(pane.status_note(), Some("model: gpt-5"));
        assert_eq!(pane.work_action(), None);

        pane.begin_turn_work("依頼を確認中");
        pane.set_work_action("ツール実行: Bash");
        assert_eq!(pane.status_note(), Some("model: gpt-5"));

        pane.end_turn_work("応答完了");
        assert_eq!(pane.status_note(), Some("model: gpt-5"));
        assert_eq!(pane.work_action(), Some("応答完了"));

        pane.set_status_note("effort: high");
        assert_eq!(pane.work_action(), Some("応答完了"));
        assert_eq!(pane.status_note(), Some("effort: high"));
    }

    #[test]
    fn turn_started_event_resets_work_action_via_begin_turn_work() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.set_work_action("前ターンの残骸");

        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);
        assert_eq!(pane.work_action(), Some("依頼を確認中"));

        pane.handle_stdout_line(r#"{"type":"turn.completed"}"#);
        assert_eq!(pane.work_action(), Some("応答完了"));
    }

    #[test]
    fn record_current_command_pushes_recent_action_breadcrumb() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        assert_eq!(pane.recent_actions_len(), 0);

        pane.record_current_command(RecentActionKind::Command, "cargo test".to_string());

        assert_eq!(pane.current_command(), Some("cargo test"));
        assert_eq!(pane.recent_actions_len(), 1);
        let breadcrumbs = pane.recent_action_breadcrumbs();
        assert_eq!(breadcrumbs, vec!["▶ cargo test #1".to_string()]);
    }

    #[test]
    fn recent_action_breadcrumbs_keep_oldest_to_newest_order_with_turn_seq() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");

        pane.record_current_command(RecentActionKind::Command, "cargo test".to_string());
        pane.record_current_command(RecentActionKind::Mcp, "addness.get_goal".to_string());
        pane.record_current_command(RecentActionKind::FileChange, "src/main.rs".to_string());

        let breadcrumbs = pane.recent_action_breadcrumbs();
        assert_eq!(
            breadcrumbs,
            vec![
                "▶ cargo test #1".to_string(),
                "◆ addness.get_goal #2".to_string(),
                "✎ src/main.rs #3".to_string(),
            ]
        );
    }

    #[test]
    fn recent_actions_buffer_is_capped_at_recent_actions_cap() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");

        for i in 0..(RECENT_ACTIONS_CAP + 2) {
            pane.record_current_command(RecentActionKind::Tool, format!("action-{i}"));
        }

        assert_eq!(pane.recent_actions_len(), RECENT_ACTIONS_CAP);
        let breadcrumbs = pane.recent_action_breadcrumbs();
        assert_eq!(breadcrumbs.len(), RECENT_ACTIONS_CAP);
        // 古い順で保持されるので、末尾の要素が直近の action-(N-1) になる。
        assert!(
            breadcrumbs
                .last()
                .unwrap()
                .contains(&format!("action-{}", RECENT_ACTIONS_CAP + 1))
        );
        // 先頭 2 件は溢れて消えている（action-0, action-1 は含まれない）。
        assert!(!breadcrumbs.iter().any(|b| b.contains("action-0 ")));
        assert!(!breadcrumbs.iter().any(|b| b.contains("action-1 ")));
    }

    #[test]
    fn clear_recent_actions_resets_buffer_and_sequence() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.record_current_command(RecentActionKind::Command, "cargo test".to_string());
        pane.record_current_command(RecentActionKind::Command, "cargo build".to_string());
        assert_eq!(pane.recent_actions_len(), 2);

        pane.clear_recent_actions();
        assert_eq!(pane.recent_actions_len(), 0);
        assert!(pane.recent_action_breadcrumbs().is_empty());

        // クリア後に積み直すと通し番号は 1 から振り直される。
        pane.record_current_command(RecentActionKind::Command, "cargo fmt".to_string());
        assert_eq!(
            pane.recent_action_breadcrumbs(),
            vec!["▶ cargo fmt #1".to_string()]
        );
    }

    #[test]
    fn turn_started_clears_recent_action_history() {
        let mut pane = CodexPane::test_with_output(8, 20, 0, "");
        pane.handle_stdout_line(
            r#"{"type":"exec_command_begin","parsed_cmd":"cargo check","codex_cwd":"/repo"}"#,
        );
        assert_eq!(pane.recent_actions_len(), 1);

        // 新しいターンが始まると、前ターンのパンくずは残さずクリアする。
        pane.handle_stdout_line(r#"{"type":"turn.started"}"#);

        assert_eq!(pane.recent_actions_len(), 0);
        assert!(pane.recent_action_breadcrumbs().is_empty());
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
    fn resolve_agent_language_auto_uses_lang_env_prefix() {
        assert_eq!(
            resolve_agent_language(AgentLanguage::Auto, Some("ja_JP.UTF-8")),
            Some(ResolvedAgentLanguage::Japanese)
        );
        assert_eq!(
            resolve_agent_language(AgentLanguage::Auto, Some("en_US.UTF-8")),
            Some(ResolvedAgentLanguage::English)
        );
        // 判定不能・未設定は off と同じく注入しない。
        assert_eq!(resolve_agent_language(AgentLanguage::Auto, Some("C")), None);
        assert_eq!(resolve_agent_language(AgentLanguage::Auto, None), None);
    }

    #[test]
    fn resolve_agent_language_explicit_ignores_env() {
        assert_eq!(
            resolve_agent_language(AgentLanguage::Ja, Some("en_US.UTF-8")),
            Some(ResolvedAgentLanguage::Japanese)
        );
        assert_eq!(
            resolve_agent_language(AgentLanguage::En, Some("ja_JP.UTF-8")),
            Some(ResolvedAgentLanguage::English)
        );
        assert_eq!(
            resolve_agent_language(AgentLanguage::Off, Some("ja_JP.UTF-8")),
            None
        );
    }

    #[test]
    fn compose_developer_instructions_injects_language_line() {
        let base = addness_tui_developer_instructions();

        let ja = compose_developer_instructions(AgentLanguage::Ja, None);
        assert!(ja.starts_with(base));
        assert!(ja.contains("ユーザーへの応答・説明は必ず日本語で書く"));

        let en = compose_developer_instructions(AgentLanguage::En, None);
        assert!(en.contains("Always write your responses and explanations to the user in English"));

        // auto + ja 環境は日本語指示、auto + 判定不能 は注入なし（base と一致）。
        let auto_ja = compose_developer_instructions(AgentLanguage::Auto, Some("ja_JP.UTF-8"));
        assert!(auto_ja.contains("必ず日本語で書く"));
        let auto_unknown = compose_developer_instructions(AgentLanguage::Auto, Some("C"));
        assert_eq!(auto_unknown, base);
    }

    #[test]
    fn compose_developer_instructions_off_omits_language_line() {
        let base = addness_tui_developer_instructions();
        let off = compose_developer_instructions(AgentLanguage::Off, Some("ja_JP.UTF-8"));
        assert_eq!(off, base);
        assert!(!off.contains("応答言語:"));
    }

    #[test]
    fn lang_slash_command_without_args_opens_picker_with_current_marker() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        assert!(pane.list_picker().is_none());

        assert!(pane.handle_local_slash_command("/lang"));
        let picker = pane.list_picker().expect("picker opened");
        assert_eq!(picker.action, CodexListPickerAction::SetLanguage);
        let values = picker
            .items
            .iter()
            .map(|item| item.value.as_str())
            .collect::<Vec<_>>();
        assert_eq!(values, vec!["auto", "ja", "en", "off"]);
        // 既定は auto なので auto のみ current。
        assert!(picker.items[0].current);
        assert!(picker.items.iter().skip(1).all(|item| !item.current));
    }

    #[test]
    fn lang_alias_language_is_dispatched() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        assert!(pane.handle_local_slash_command("/language"));
        assert_eq!(
            pane.list_picker().map(|picker| picker.action),
            Some(CodexListPickerAction::SetLanguage)
        );
    }

    #[test]
    fn agent_language_parse_maps_slash_argument() {
        assert_eq!(AgentLanguage::parse("ja"), Some(AgentLanguage::Ja));
        assert_eq!(AgentLanguage::parse("en"), Some(AgentLanguage::En));
        assert_eq!(AgentLanguage::parse("auto"), Some(AgentLanguage::Auto));
        assert_eq!(AgentLanguage::parse("off"), Some(AgentLanguage::Off));
        assert_eq!(AgentLanguage::parse("bogus"), None);
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

        // 引数なしはピッカーを開くだけで設定は変えない。
        submit_line(&mut pane, "/model");
        assert!(pane.list_picker_open());
        pane.close_list_picker();
        assert_eq!(
            pane.settings_label().split(' ').next(),
            Some("model:config")
        );

        // 引数ありは従来どおり直接指定で反映される。
        submit_line(&mut pane, "/model gpt-5.5");
        submit_line(&mut pane, "/reasoning low");
        submit_line(&mut pane, "/approval untrusted");
        submit_line(&mut pane, "/sandbox danger-full-access");
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
        let text = slash_help_text(AgentKind::Codex);

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
    fn slash_help_text_for_claude_code_omits_codex_only_commands() {
        let text = slash_help_text(AgentKind::ClaudeCode);

        assert!(text.contains("Claude Code CLI commands:"));
        assert!(text.contains("/model [name|config]"));
        assert!(text.contains("/reasoning|/effort [level]"));
        assert!(text.contains("/permissions|/approval [mode]"));
        assert!(text.contains("/resume-last [prompt]"));
        assert!(text.contains("/resume-session <N|id> [prompt]"));
        assert!(text.contains("/fork-last [prompt]"));
        assert!(text.contains("/fork-session <N|id> [prompt]"));
        assert!(text.contains("/add-dir <path|list|clear>"));
        assert!(text.contains("/organize|/team [task]"));
        assert!(text.contains("/remember|/memo <内容>"));
        assert!(!text.contains("/review"));
        assert!(!text.contains("/apply"));
        assert!(!text.contains("/cloud"));
        assert!(!text.contains("codex apply"));
        assert!(!text.contains("/hooks"));
        assert!(!text.contains("/mcp"));
        assert!(!text.contains("/login"));
        assert!(!text.contains("/profile"));
    }

    #[test]
    fn slash_palette_hides_codex_only_commands_for_claude_code() {
        let mut pane = claude_pane();
        pane.input_state.line = "/".to_string();
        pane.input_state.cursor = pane.input_state.line.len();
        let names: Vec<&str> = pane
            .slash_palette_suggestions()
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        assert!(names.contains(&"/model"));
        assert!(names.contains(&"/reasoning"));
        assert!(names.contains(&"/permissions"));
        assert!(names.contains(&"/sessions"));
        assert!(names.contains(&"/resume-last"));
        assert!(names.contains(&"/add-dir"));
        for excluded in CODEX_ONLY_SLASH_COMMANDS {
            assert!(
                !names.contains(excluded),
                "{excluded} should be hidden from the ClaudeCode palette"
            );
        }
    }

    #[test]
    fn codex_only_slash_commands_are_rejected_at_runtime_for_claude_code() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/theme dark"));
        assert!(
            pane.log
                .iter()
                .any(|line| line.text.contains("/theme は codex 専用コマンドです")),
            "guard log missing: {:?}",
            pane.log.iter().map(|l| &l.text).collect::<Vec<_>>()
        );
        // 設定サイクル等の claude 対応コマンドはガードされない。
        assert!(pane.handle_local_slash_command("/model opus"));
        assert!(
            pane.log
                .iter()
                .any(|line| line.text.contains("次回ターンの model: opus"))
        );
    }

    #[test]
    fn slash_palette_keeps_codex_only_commands_for_codex() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.input_state.line = "/review".to_string();
        pane.input_state.cursor = pane.input_state.line.len();
        let names: Vec<&str> = pane
            .slash_palette_suggestions()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, vec!["/review"]);
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
        assert_eq!(pane.work_action(), Some("作業分解を予約 1件"));
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
        assert_eq!(pane.work_action(), Some("子ゴール一覧を表示"));
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
        assert_eq!(pane.work_action(), Some("子ゴール着手を予約 1件"));
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
        assert_eq!(pane.work_action(), Some("子ゴール一括着手を予約 2件"));
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

        let args = codex_exec_args(
            pane.thread_id.as_deref(),
            &pane.cwd,
            &pane.exec_settings,
            "DEV",
        );
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

        let args = codex_exec_args(None, "/repo", &pane.exec_settings, "DEV");
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
        let args = codex_exec_args(None, &pane.cwd, &pane.exec_settings, "DEV");
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

    #[test]
    fn format_elapsed_formats_minutes_and_hours() {
        assert_eq!(format_elapsed(0), "0:00");
        assert_eq!(format_elapsed(45), "0:45");
        assert_eq!(format_elapsed(83), "1:23");
        assert_eq!(format_elapsed(3600), "1:00:00");
        assert_eq!(format_elapsed(3723), "1:02:03");
    }

    #[test]
    fn format_token_count_rounds_k_and_m() {
        assert_eq!(format_token_count(0), "0");
        assert_eq!(format_token_count(999), "999");
        assert_eq!(format_token_count(12_711), "12.7k");
        assert_eq!(format_token_count(1_000), "1.0k");
        assert_eq!(format_token_count(1_500_000), "1.5M");
    }

    #[test]
    fn context_percent_computes_and_guards_zero_window() {
        assert_eq!(context_percent(45_000, 100_000), Some(45));
        assert_eq!(context_percent(0, 200_000), Some(0));
        assert_eq!(context_percent(200_000, 200_000), Some(100));
        // 上限 100% でクランプする。
        assert_eq!(context_percent(300_000, 200_000), Some(100));
        assert_eq!(context_percent(10, 0), None);
    }

    #[test]
    fn claude_context_window_maps_default_and_1m() {
        assert_eq!(claude_context_window_for_model(None), 200_000);
        assert_eq!(
            claude_context_window_for_model(Some("claude-sonnet-4-5")),
            200_000
        );
        assert_eq!(
            claude_context_window_for_model(Some("claude-opus-4-8[1m]")),
            1_000_000
        );
    }

    #[test]
    fn usage_header_label_claude_shows_cost_and_ctx() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.kind = AgentKind::ClaudeCode;
        assert_eq!(pane.usage_header_label(), None);
        pane.claude_total_cost_usd = Some(0.0123);
        pane.claude_context_tokens = Some(90_000);
        pane.claude_active_model = Some("claude-sonnet-4-5".to_string());
        assert_eq!(
            pane.usage_header_label().as_deref(),
            Some("$0.0123 | ctx 45%")
        );
    }

    #[test]
    fn checkpoint_slug_sanitizes_goal_id() {
        assert_eq!(checkpoint_slug("abc123"), "abc123");
        assert_eq!(
            checkpoint_slug("11111111-2222-3333-4444-555555555555"),
            "11111111-2222-3333-4444-555555555555"
        );
        assert_eq!(checkpoint_slug("Foo/Bar_Baz"), "foo-bar-baz");
        assert_eq!(checkpoint_slug("--@@--"), "pane");
        assert_eq!(checkpoint_slug(""), "pane");
    }

    #[test]
    fn checkpoint_ref_name_builds_namespaced_ref() {
        assert_eq!(
            checkpoint_ref_name("goal-1", 3),
            "refs/addness/checkpoint-goal-1-3"
        );
    }

    #[test]
    fn git_status_has_changes_detects_nonblank_lines() {
        assert!(!git_status_has_changes(""));
        assert!(!git_status_has_changes("\n  \n"));
        assert!(git_status_has_changes(" M src/main.rs"));
        assert!(git_status_has_changes("?? new.txt\n"));
    }

    #[test]
    fn push_checkpoint_evicts_oldest_over_limit() {
        let mut stack = Vec::new();
        let mut evicted_total = Vec::new();
        for i in 0..12 {
            let evicted = push_checkpoint_with_evictions(
                &mut stack,
                Checkpoint {
                    ref_name: format!("refs/addness/checkpoint-p-{i}"),
                    turn: i,
                },
                CHECKPOINT_STACK_MAX,
            );
            evicted_total.extend(evicted);
        }
        // 上限 10 件を保持、古い 2 件（turn 0,1）が押し出される。
        assert_eq!(stack.len(), CHECKPOINT_STACK_MAX);
        assert_eq!(stack.first().unwrap().turn, 2);
        assert_eq!(stack.last().unwrap().turn, 11);
        assert_eq!(
            evicted_total.iter().map(|c| c.turn).collect::<Vec<_>>(),
            vec![0, 1]
        );
    }

    #[test]
    fn undo_slash_without_checkpoint_reports_missing() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        submit_line(&mut pane, "/undo");
        assert!(pane.log.iter().any(|line| {
            line.kind == CodexLogKind::System && line.text.contains("チェックポイントがありません")
        }));
        assert!(pane.decision_banner().is_none());
    }

    #[test]
    fn undo_slash_with_checkpoint_prompts_and_pops_on_accept() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.record_checkpoint("refs/addness/checkpoint-p-0".to_string(), 1);
        pane.record_checkpoint("refs/addness/checkpoint-p-1".to_string(), 2);

        submit_line(&mut pane, "/undo");
        let banner = pane.decision_banner().expect("確認バナーが出る");
        assert_eq!(banner.kind, CodexDecisionKind::YesNo);

        // Accept で undo 要求が積まれ、最新チェックポイント（turn 2）が pop される。
        pane.handle_decision_key(KeyEvent::from(KeyCode::Char('y')));
        let req = pane.take_undo_request().expect("undo 要求が積まれる");
        assert_eq!(req.turn, 2);
        assert_eq!(req.ref_name, "refs/addness/checkpoint-p-1");
        assert_eq!(pane.checkpoints.len(), 1);
    }

    #[test]
    fn undo_slash_deny_keeps_checkpoint() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.finished = false;
        pane.record_checkpoint("refs/addness/checkpoint-p-0".to_string(), 1);
        submit_line(&mut pane, "/undo");
        pane.handle_decision_key(KeyEvent::from(KeyCode::Char('n')));
        assert!(pane.take_undo_request().is_none());
        assert_eq!(pane.checkpoints.len(), 1);
    }

    #[test]
    fn undo_appears_in_slash_command_list() {
        assert!(SLASH_COMMANDS.iter().any(|(name, _)| *name == "/undo"));
        // /undo は codex 専用ではない（両バックエンドで使える）。
        assert!(!CODEX_ONLY_SLASH_COMMANDS.contains(&"/undo"));
    }

    #[test]
    fn checkpoint_requests_queue_and_drain_in_order() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        // ジョブ消化前に複数のチェックポイントが予約されても取りこぼさない。
        pane.request_checkpoint();
        pane.request_checkpoint();
        pane.request_checkpoint();
        let first = pane.take_checkpoint_request().expect("1件目");
        let second = pane.take_checkpoint_request().expect("2件目");
        let third = pane.take_checkpoint_request().expect("3件目");
        // 先入れ先出しで seq（turn 見込み番号）が単調増加する。
        assert!(first.turn <= second.turn && second.turn <= third.turn);
        assert!(pane.take_checkpoint_request().is_none());
    }

    #[test]
    fn checkpoint_requests_are_capped() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        // 上限 4 件を超える予約は最古が捨てられ、無制限には溜まらない。
        for _ in 0..8 {
            pane.request_checkpoint();
        }
        let mut count = 0;
        while pane.take_checkpoint_request().is_some() {
            count += 1;
        }
        assert_eq!(count, 4);
    }

    #[test]
    fn usage_header_label_codex_shows_tokens_and_ctx() {
        let mut pane = CodexPane::test_with_output(8, 80, 0, "");
        pane.kind = AgentKind::Codex;
        assert_eq!(pane.usage_header_label(), None);
        pane.codex_appserver_token_usage = Some(codex_appserver::TokenUsageInfo {
            total_tokens: Some(12_711),
            last_total_tokens: Some(42),
            model_context_window: Some(258_400),
        });
        assert_eq!(
            pane.usage_header_label().as_deref(),
            Some("12.7k tok | ctx 5%")
        );
    }

    #[test]
    fn sanitize_terminal_line_strips_ansi_and_control() {
        // CSI カラーコード・OSC・制御文字を除去し、タブは空白へ。
        let input = "\u{1b}[31mred\u{1b}[0m\ttext\u{7}end\u{1b}]0;title\u{7}!";
        assert_eq!(sanitize_terminal_line(input), "red textend!");
        assert_eq!(sanitize_terminal_line("plain"), "plain");
    }

    #[test]
    fn claude_edit_patch_text_builds_update_patch() {
        let input = serde_json::json!({
            "file_path": "src/x.rs",
            "old_string": "old1\nold2",
            "new_string": "new1"
        });
        let patch = claude_edit_patch_text("Edit", &input).unwrap();
        assert!(patch.starts_with("EDIT *** Begin Patch"));
        assert!(patch.contains("*** Update File: src/x.rs"));
        assert!(patch.contains("-old1"));
        assert!(patch.contains("-old2"));
        assert!(patch.contains("+new1"));
        assert!(patch.contains("*** End Patch"));
    }

    #[test]
    fn claude_edit_patch_text_builds_add_patch_for_write() {
        let input = serde_json::json!({
            "file_path": "src/new.rs",
            "content": "line1\nline2"
        });
        let patch = claude_edit_patch_text("Write", &input).unwrap();
        assert!(patch.contains("*** Add File: src/new.rs"));
        assert!(patch.contains("+line1"));
        assert!(patch.contains("+line2"));
        // Edit/Write 以外や file_path 無しは対象外。
        assert!(claude_edit_patch_text("Bash", &input).is_none());
        assert!(claude_edit_patch_text("Edit", &serde_json::json!({})).is_none());
    }

    #[test]
    fn claude_edit_patch_text_caps_long_content() {
        let content = (0..100)
            .map(|i| format!("l{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let input = serde_json::json!({ "file_path": "big.rs", "content": content });
        let patch = claude_edit_patch_text("Write", &input).unwrap();
        // 本文行はセクション上限（40行）までに抑える（全文は出さない）。
        let plus_lines = patch.lines().filter(|l| l.starts_with('+')).count();
        assert_eq!(plus_lines, CODEX_PATCH_SECTION_MAX_LINES);
    }

    #[test]
    fn codex_filechange_patch_text_from_unified_diff() {
        let changes = vec![
            codex_appserver::FileChangeDetail {
                path: "src/a.rs".to_string(),
                change_type: "update".to_string(),
                diff: "--- a/src/a.rs\n+++ b/src/a.rs\n@@ -1,2 +1,2 @@\n-old\n+new\n".to_string(),
            },
            codex_appserver::FileChangeDetail {
                path: "src/b.rs".to_string(),
                change_type: "add".to_string(),
                diff: "+added line\n".to_string(),
            },
        ];
        let patch = codex_filechange_patch_text(&changes).unwrap();
        assert!(patch.contains("*** Update File: src/a.rs"));
        assert!(patch.contains("*** Add File: src/b.rs"));
        assert!(patch.contains("@@ -1,2 +1,2 @@"));
        assert!(patch.contains("-old"));
        assert!(patch.contains("+new"));
        assert!(patch.contains("+added line"));
        // ファイルヘッダ（--- / +++）はノイズとして除去する。
        assert!(!patch.contains("--- a/src/a.rs"));
        assert!(!patch.contains("+++ b/src/a.rs"));
    }

    #[test]
    fn codex_filechange_patch_text_empty_without_diff() {
        let changes = vec![codex_appserver::FileChangeDetail {
            path: "src/a.rs".to_string(),
            change_type: "update".to_string(),
            diff: String::new(),
        }];
        assert!(codex_filechange_patch_text(&changes).is_none());
    }

    #[test]
    fn model_slash_without_args_opens_picker_with_current_marker_claude() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/model"));
        let picker = pane.list_picker().expect("model picker");
        assert_eq!(picker.action, CodexListPickerAction::SetModel);
        let labels = picker
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["config", "fable", "opus", "sonnet", "haiku"]);
        assert!(picker.items[0].current, "既定は config が現在値");
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn model_picker_accept_applies_selection_claude() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/model"));
        pane.move_list_picker_selection(2); // config -> opus
        pane.accept_list_picker(false);
        assert!(pane.list_picker().is_none());
        assert_eq!(pane.claude_settings.effective_model_arg(), Some("opus"));
    }

    #[test]
    fn model_picker_lists_codex_choices_and_applies_selection() {
        let mut pane = live_pane();
        pane.exec_settings.model = CodexModelChoice::Gpt5;
        assert!(pane.handle_local_slash_command("/model"));
        let picker = pane.list_picker().expect("model picker");
        let labels = picker
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, ["config", "gpt-5.5", "gpt-5", "o3"]);
        assert!(picker.items[2].current, "現在値 gpt-5 にマーカー");
        assert_eq!(picker.selected, 2, "初期選択は現在値");
        pane.move_list_picker_selection(-1); // gpt-5 -> gpt-5.5
        pane.accept_list_picker(false);
        assert_eq!(pane.exec_settings.model, CodexModelChoice::Gpt55);
        assert!(pane.exec_settings.model_override.is_none());
    }

    #[test]
    fn model_picker_shows_override_in_title_without_current_marker() {
        let mut pane = live_pane();
        pane.exec_settings.model_override = Some("my-custom-model".to_string());
        assert!(pane.handle_local_slash_command("/model"));
        let picker = pane.list_picker().expect("model picker");
        assert!(picker.title.contains("my-custom-model"));
        assert!(picker.items.iter().all(|item| !item.current));
        assert_eq!(picker.selected, 0);
    }

    #[test]
    fn reasoning_picker_accept_sets_claude_effort() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/effort"));
        let picker = pane.list_picker().expect("effort picker");
        assert_eq!(picker.action, CodexListPickerAction::SetReasoning);
        pane.move_list_picker_selection(3); // config -> high
        pane.accept_list_picker(false);
        assert!(pane.list_picker().is_none());
        assert!(pane.claude_settings.label().contains("effort:high"));
    }

    #[test]
    fn permissions_slash_without_args_opens_permission_picker_claude() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/permissions"));
        let picker = pane.list_picker().expect("permission picker");
        assert_eq!(picker.action, CodexListPickerAction::SetApproval);
        let labels = picker
            .items
            .iter()
            .map(|item| item.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            labels,
            [
                "config",
                "plan",
                "acceptEdits",
                "dontAsk",
                "bypassPermissions",
                "skip-permissions（危険・全許可）"
            ]
        );
        pane.move_list_picker_selection(1); // config -> plan
        pane.accept_list_picker(false);
        assert_eq!(pane.claude_settings.permission_label(), "plan");
    }

    #[test]
    fn sandbox_picker_accept_sets_codex_sandbox() {
        let mut pane = live_pane();
        assert!(pane.handle_local_slash_command("/sandbox"));
        let picker = pane.list_picker().expect("sandbox picker");
        assert_eq!(picker.action, CodexListPickerAction::SetSandbox);
        assert_eq!(picker.selected, 1, "既定 workspace-write が現在値");
        pane.move_list_picker_selection(-1); // -> read-only
        pane.accept_list_picker(false);
        assert_eq!(pane.exec_settings.sandbox, CodexSandboxChoice::ReadOnly);
    }

    #[test]
    fn list_picker_fork_accept_ignored_for_settings_picker() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/model"));
        pane.accept_list_picker(true);
        assert!(
            pane.list_picker().is_some(),
            "ResumeSession 以外では f を無視して開いたままにする"
        );
    }

    #[test]
    fn session_picker_items_mark_current_thread() {
        let candidates = vec![
            CodexSessionCandidate {
                id: "11111111-2222-3333-4444-555555555555".to_string(),
                title: "first".to_string(),
                updated_at: "2026-07-08T10:00:00Z".to_string(),
                cwd: None,
            },
            CodexSessionCandidate {
                id: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
                title: "second".to_string(),
                updated_at: "2026-07-07T09:00:00Z".to_string(),
                cwd: None,
            },
        ];
        let items = session_picker_items(&candidates, Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
        assert_eq!(items[0].label, "11111111");
        assert!(!items[0].current);
        assert!(items[1].current);
        assert_eq!(items[1].value, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert!(items[0].detail.contains("first"));
        assert!(items[0].detail.contains("2026-07-08"));
        assert_eq!(list_picker_initial_selection(&items), 1);
    }

    #[test]
    fn bare_resume_opens_session_picker_instead_of_memo_prompt() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/resume"));
        // 旧挙動の定型プロンプト送信はしない。
        assert!(
            !pane
                .log
                .iter()
                .any(|line| line.text.contains("前回の続きから再開してください"))
        );
        // 候補があればセッションピッカー、無ければ案内ログのみ（実行環境のセッション有無に依存）。
        match pane.list_picker() {
            Some(picker) => assert_eq!(picker.action, CodexListPickerAction::ResumeSession),
            None => assert!(
                pane.log
                    .iter()
                    .any(|line| line.text.contains("セッションがありません"))
            ),
        }
    }

    #[test]
    fn resume_memo_sends_legacy_resume_prompt() {
        let mut pane = claude_pane();
        assert!(pane.handle_local_slash_command("/resume-memo"));
        assert!(pane.list_picker().is_none());
        assert!(
            pane.log
                .iter()
                .any(|line| line.text.contains("前回の続きから再開してください"))
        );
    }

    #[test]
    fn opening_list_picker_closes_turn_picker() {
        let mut pane = claude_pane();
        pane.turn_picker = Some(CodexTurnPicker { selected_turn: 1 });
        assert!(pane.handle_local_slash_command("/model"));
        assert!(!pane.turn_picker_open());
        assert!(pane.list_picker_open());
    }
}
