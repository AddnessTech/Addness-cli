//! Claude Code バックエンド固有ロジック（`agent/mod.rs` から enum ディスパッチで呼ばれる）。
//!
//! ここには純粋関数だけを置く（TUI 状態を持たない）:
//! - `claude -p --output-format stream-json` の起動引数ビルダー
//! - stream-json イベントの防御的パーサ（`serde_json::Value` ベース）
//! - `~/.claude/projects/<cwd スラッグ>/*.jsonl` のセッション候補探索
//! - 次ターン設定 `ClaudeExecSettings`
//!
//! `CodexPane`（`agent/mod.rs`）側で状態を更新する。codex 経路のヒューリスティック
//! （`handle_generic_json_event` / `tool_display` 等）は Claude 経路では一切使わない。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::CodexSessionCandidate;

// ---------------------------------------------------------------------------
// 設定（F2/F3/F4 と /model /effort /permissions で巡回・指定する値）
// ---------------------------------------------------------------------------

/// F2 で巡回するモデル。`config` は `--model` を付けない（claude 側の既定に従う）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudeModelChoice {
    Config,
    Fable,
    Opus,
    Sonnet,
    Haiku,
}

impl ClaudeModelChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Fable,
            Self::Fable => Self::Opus,
            Self::Opus => Self::Sonnet,
            Self::Sonnet => Self::Haiku,
            Self::Haiku => Self::Config,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Fable => "fable",
            Self::Opus => "opus",
            Self::Sonnet => "sonnet",
            Self::Haiku => "haiku",
        }
    }

    /// `--model` に渡す値。`config` は None。
    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Fable => Some("fable"),
            Self::Opus => Some("opus"),
            Self::Sonnet => Some("sonnet"),
            Self::Haiku => Some("haiku"),
        }
    }
}

pub(super) fn parse_model_choice(value: &str) -> Option<ClaudeModelChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(ClaudeModelChoice::Config),
        "fable" => Some(ClaudeModelChoice::Fable),
        "opus" => Some(ClaudeModelChoice::Opus),
        "sonnet" => Some(ClaudeModelChoice::Sonnet),
        "haiku" => Some(ClaudeModelChoice::Haiku),
        _ => None,
    }
}

/// F3 で巡回する effort レベル。`--effort` に渡す。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudeEffortChoice {
    Config,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ClaudeEffortChoice {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::XHigh,
            Self::XHigh => Self::Max,
            Self::Max => Self::Config,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Low => Some("low"),
            Self::Medium => Some("medium"),
            Self::High => Some("high"),
            Self::XHigh => Some("xhigh"),
            Self::Max => Some("max"),
        }
    }
}

pub(super) fn parse_effort_choice(value: &str) -> Option<ClaudeEffortChoice> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(ClaudeEffortChoice::Config),
        "low" => Some(ClaudeEffortChoice::Low),
        "medium" | "med" => Some(ClaudeEffortChoice::Medium),
        "high" => Some(ClaudeEffortChoice::High),
        "xhigh" | "extra-high" | "extra_high" => Some(ClaudeEffortChoice::XHigh),
        "max" => Some(ClaudeEffortChoice::Max),
        _ => None,
    }
}

/// F4 で巡回する permission-mode。`config` は `--permission-mode` を付けない。
///
/// `DangerouslySkipPermissions` だけは `--permission-mode` の値ではなく、独立した起動フラグ
/// `--dangerously-skip-permissions`（原則として権限チェックをバイパス）を使う。
/// Claude Code 2.1.208 以降は壊滅的な削除など一部の操作では確認が残る。起動時フラグなので
/// 常駐プロセスのランタイム切替（`set_permission_mode` control_request）はできず、この variant
/// へ/から切り替える場合は常駐プロセスの再起動で反映する（`mod.rs` 側で処理）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaudePermissionMode {
    Config,
    Plan,
    AcceptEdits,
    DontAsk,
    BypassPermissions,
    DangerouslySkipPermissions,
}

impl ClaudePermissionMode {
    fn next(self) -> Self {
        match self {
            Self::Config => Self::Plan,
            Self::Plan => Self::AcceptEdits,
            Self::AcceptEdits => Self::DontAsk,
            Self::DontAsk => Self::BypassPermissions,
            Self::BypassPermissions => Self::DangerouslySkipPermissions,
            Self::DangerouslySkipPermissions => Self::Config,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Plan => "plan",
            Self::AcceptEdits => "acceptEdits",
            Self::DontAsk => "dontAsk",
            Self::BypassPermissions => "bypassPermissions",
            Self::DangerouslySkipPermissions => "skip-permissions（危険・原則許可）",
        }
    }

    /// `--permission-mode` に渡す値。`config` と `skip-permissions` は None。
    /// `skip-permissions` は `--permission-mode` の値ではなく独立フラグ
    /// `--dangerously-skip-permissions` を使う（`push_permission_mode_args` 参照）。
    fn cli_arg(self) -> Option<&'static str> {
        match self {
            Self::Config => None,
            Self::Plan => Some("plan"),
            Self::AcceptEdits => Some("acceptEdits"),
            Self::DontAsk => Some("dontAsk"),
            Self::BypassPermissions => Some("bypassPermissions"),
            Self::DangerouslySkipPermissions => None,
        }
    }

    /// 独立起動フラグ `--dangerously-skip-permissions` を使う variant か。
    pub(super) fn is_dangerously_skip(self) -> bool {
        matches!(self, Self::DangerouslySkipPermissions)
    }
}

pub(super) fn parse_permission_mode(value: &str) -> Option<ClaudePermissionMode> {
    match value.to_ascii_lowercase().as_str() {
        "config" | "default" | "clear" => Some(ClaudePermissionMode::Config),
        "plan" => Some(ClaudePermissionMode::Plan),
        "acceptedits" | "accept-edits" | "accept" => Some(ClaudePermissionMode::AcceptEdits),
        "dontask" | "dont-ask" | "auto" => Some(ClaudePermissionMode::DontAsk),
        "bypasspermissions" | "bypass" | "bypass-permissions" => {
            Some(ClaudePermissionMode::BypassPermissions)
        }
        // ラベル（ピッカーの value）自身も受け付け、選択→parse の往復を成立させる。
        "skip"
        | "skip-permissions"
        | "dangerously-skip-permissions"
        | "skip-permissions（危険・原則許可）"
        // 2.1.207 以前の表示値も、設定復元との互換性のため受け付ける。
        | "skip-permissions（危険・全許可）" => {
            Some(ClaudePermissionMode::DangerouslySkipPermissions)
        }
        _ => None,
    }
}

/// 次回 `claude -p` 起動時に使う設定。codex の `CodexExecSettings` と並置する。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeExecSettings {
    model: ClaudeModelChoice,
    /// `/model <任意文字列>` で指定したフルネーム。set すると `model` は Config に戻す。
    model_override: Option<String>,
    effort: ClaudeEffortChoice,
    permission_mode: ClaudePermissionMode,
    /// 承認バナーで「これからずっと許可」を選んだツールルール（`--allowedTools`）。
    /// 以後の全ターンに付与する（セッションレベルの sticky 許可リスト）。
    sticky_allowed_tools: Vec<String>,
    additional_dirs: Vec<String>,
    /// 古い claude CLI が `--include-partial-messages` 未対応と判明した場合に立てる sticky フラグ。
    /// 立つと以後の全ターンで同フラグを付けず、ストリーミングなしのブロック表示へ退化する。
    no_partial_messages: bool,
}

impl Default for ClaudeExecSettings {
    fn default() -> Self {
        Self {
            model: ClaudeModelChoice::Config,
            model_override: None,
            effort: ClaudeEffortChoice::Config,
            permission_mode: ClaudePermissionMode::Config,
            sticky_allowed_tools: Vec::new(),
            additional_dirs: Vec::new(),
            no_partial_messages: false,
        }
    }
}

impl ClaudeExecSettings {
    pub(super) fn label(&self) -> String {
        let model = self
            .model_override
            .as_deref()
            .unwrap_or_else(|| self.model.label());
        let mut parts = vec![
            format!("model:{model}"),
            format!("effort:{}", self.effort.label()),
            format!("permission:{}", self.permission_mode.label()),
        ];
        if !self.sticky_allowed_tools.is_empty() {
            parts.push(format!("allow:{}", self.sticky_allowed_tools.len()));
        }
        if !self.additional_dirs.is_empty() {
            parts.push(format!("add-dir:{}", self.additional_dirs.len()));
        }
        parts.join(" ")
    }

    pub(super) fn permission_label(&self) -> String {
        self.permission_mode.label().to_string()
    }

    /// `--model` に相当する現在の実効値。`config`（既定）なら None。
    /// 常駐モードの `set_model` control_request 送信可否の判定に使う。
    pub(super) fn effective_model_arg(&self) -> Option<&str> {
        self.model_override
            .as_deref()
            .or_else(|| self.model.cli_arg())
    }

    /// `--permission-mode` に相当する現在の実効値。`config`（既定）と `skip-permissions` は None。
    /// 常駐モードの `set_permission_mode` control_request 送信可否の判定に使う（None なら再起動）。
    /// `skip-permissions` は起動時フラグでランタイム切替不可のため None を返す。
    pub(super) fn effective_permission_mode_arg(&self) -> Option<&str> {
        self.permission_mode.cli_arg()
    }

    /// sticky 許可リストを一覧表示用に返す。
    pub(super) fn sticky_allowed_tools(&self) -> &[String] {
        &self.sticky_allowed_tools
    }

    /// 現在のビルトインモデル選択（ピッカーの current マーカー用）。
    pub(super) fn model_choice(&self) -> ClaudeModelChoice {
        self.model
    }

    /// `/model <任意文字列>` の override 名（設定中なら Some）。
    pub(super) fn model_override(&self) -> Option<&str> {
        self.model_override.as_deref()
    }

    /// 現在の effort 選択（ピッカーの current マーカー用）。
    pub(super) fn effort_choice(&self) -> ClaudeEffortChoice {
        self.effort
    }

    /// 現在の permission-mode 選択（ピッカーの current マーカー用）。
    pub(super) fn permission_mode_choice(&self) -> ClaudePermissionMode {
        self.permission_mode
    }

    pub(super) fn cycle_model(&mut self) -> &'static str {
        self.model_override = None;
        self.model = self.model.next();
        self.model.label()
    }

    /// フリーテキスト / ビルトイン名でモデルを設定し、表示用ラベルを返す。
    pub(super) fn set_model(&mut self, value: &str) -> String {
        if let Some(model) = parse_model_choice(value) {
            self.model = model;
            self.model_override = None;
        } else {
            self.model = ClaudeModelChoice::Config;
            self.model_override = Some(value.to_string());
        }
        self.model_override
            .as_deref()
            .unwrap_or_else(|| self.model.label())
            .to_string()
    }

    pub(super) fn cycle_effort(&mut self) -> &'static str {
        self.effort = self.effort.next();
        self.effort.label()
    }

    pub(super) fn set_effort(&mut self, value: ClaudeEffortChoice) -> &'static str {
        self.effort = value;
        self.effort.label()
    }

    pub(super) fn cycle_permission_mode(&mut self) -> String {
        self.permission_mode = self.permission_mode.next();
        self.permission_mode.label().to_string()
    }

    pub(super) fn set_permission_mode(&mut self, value: ClaudePermissionMode) -> String {
        self.permission_mode = value;
        self.permission_mode.label().to_string()
    }

    /// Always（sticky）承認の適用。拒否ツールごとの `--allowedTools` ルールを
    /// セッションレベルの許可リストへ追加する（重複は無視）。
    pub(super) fn add_allowed_tools(&mut self, rules: &[String]) {
        for rule in rules {
            if !self.sticky_allowed_tools.iter().any(|r| r == rule) {
                self.sticky_allowed_tools.push(rule.clone());
            }
        }
    }

    pub(super) fn add_dir(&mut self, dir: String) {
        if !self.additional_dirs.iter().any(|d| d == &dir) {
            self.additional_dirs.push(dir);
        }
    }

    /// 次ターンに渡す `--add-dir` の一覧（`/attachments` の list 表示用）。
    pub(super) fn additional_dirs(&self) -> &[String] {
        &self.additional_dirs
    }

    /// `--include-partial-messages` を今後付けないか（古い CLI 検出後の sticky フラグ）。
    pub(super) fn no_partial_messages(&self) -> bool {
        self.no_partial_messages
    }

    /// 古い CLI 検出時に呼び、以後のターンでストリーミング用フラグを無効化する。
    pub(super) fn disable_partial_messages(&mut self) {
        self.no_partial_messages = true;
    }
}

// ---------------------------------------------------------------------------
// 起動引数ビルダー
// ---------------------------------------------------------------------------

/// `claude -p` の起動引数を組み立てる。プロンプトは stdin へ渡すので引数には含めない。
///
/// * `session_id` — 2 ターン目以降の `--resume <id>`（初回ターンは None）。
/// * `one_shot_allowed_tools` — 承認「今回だけ許可」で付与する `--allowedTools` ルール。
///   sticky 許可リストと合わせて 1 回だけ有効。
/// * `fork` — resume 時にセッションを複製する（`--fork-session`）。
/// * `developer_instructions` — 毎ターン `--append-system-prompt` で注入する Addness 手順。
pub(super) fn exec_args(
    session_id: Option<&str>,
    settings: &ClaudeExecSettings,
    one_shot_allowed_tools: &[String],
    fork: bool,
    developer_instructions: &str,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        OsString::from("-p"),
        OsString::from("--output-format"),
        OsString::from("stream-json"),
        OsString::from("--verbose"),
    ];
    push_partial_messages_flag(&mut args, settings);
    push_resume_args(&mut args, session_id, fork);
    push_model_and_effort_args(&mut args, settings);

    // 承認直後の続行ターン（one-shot allowedTools を消費）で permission_mode が Plan の場合は、
    // plan が実行を抑止して承認済みでも再拒否ループに陥るのを避けるため、このターンだけ
    // --permission-mode を default（フラグ省略）へ落とす。sticky 許可のみのターンでは通常どおり
    // settings の permission_mode を尊重する。ワンショット専用のヒューリスティック。
    let effective_mode = if !one_shot_allowed_tools.is_empty()
        && settings.permission_mode == ClaudePermissionMode::Plan
    {
        ClaudePermissionMode::Config
    } else {
        settings.permission_mode
    };
    push_permission_mode_args(&mut args, effective_mode);

    // 権限: sticky 許可リスト + 今回だけの許可ルールを `--allowedTools` へ（重複除去）。
    push_allowed_tools_args(
        &mut args,
        settings
            .sticky_allowed_tools
            .iter()
            .chain(one_shot_allowed_tools.iter()),
    );
    push_add_dir_args(&mut args, settings);
    push_system_prompt_args(&mut args, developer_instructions);
    args
}

/// 常駐（多ターン 1 プロセス）モードの起動引数を組み立てる。
/// exec_args と共通する部分（resume/model/effort/allowedTools/add-dir/system-prompt）は
/// 同じヘルパを共有し、常駐固有の入力ストリームと承認プロンプトフラグだけを足す。
///
/// * `session_id` — プロセス死亡・アイドル回収後の再接続に使う `--resume <id>`。
/// * `fork` — resume 時にセッションを複製する（`--fork-session`）。
///
/// 承認はプロセス実行中に can_use_tool でその場処理するため、one-shot allowedTools は取らず
/// sticky 許可リストのみを付与する。plan-drop ヒューリスティックも使わない。
pub(super) fn resident_args(
    session_id: Option<&str>,
    settings: &ClaudeExecSettings,
    fork: bool,
    developer_instructions: &str,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        OsString::from("-p"),
        OsString::from("--input-format"),
        OsString::from("stream-json"),
        OsString::from("--output-format"),
        OsString::from("stream-json"),
        OsString::from("--verbose"),
    ];
    push_partial_messages_flag(&mut args, settings);
    // その場承認（can_use_tool）を stdio 経由で受けるための隠しフラグ。
    args.push(OsString::from("--permission-prompt-tool"));
    args.push(OsString::from("stdio"));
    push_resume_args(&mut args, session_id, fork);
    push_model_and_effort_args(&mut args, settings);
    push_permission_mode_args(&mut args, settings.permission_mode);
    // 常駐では sticky 許可リストのみを付与する（今回だけの許可は can_use_tool 応答で処理）。
    push_allowed_tools_args(&mut args, settings.sticky_allowed_tools.iter());
    push_add_dir_args(&mut args, settings);
    push_system_prompt_args(&mut args, developer_instructions);
    args
}

/// トークン単位のストリーミング表示のため部分メッセージを受け取る。
/// 古い claude CLI が未対応と判明した場合（sticky フラグ）は付けず、ブロック表示へ退化する。
fn push_partial_messages_flag(args: &mut Vec<OsString>, settings: &ClaudeExecSettings) {
    if !settings.no_partial_messages {
        args.push(OsString::from("--include-partial-messages"));
    }
}

fn push_resume_args(args: &mut Vec<OsString>, session_id: Option<&str>, fork: bool) {
    if let Some(session_id) = session_id {
        args.push(OsString::from("--resume"));
        args.push(OsString::from(session_id));
        if fork {
            args.push(OsString::from("--fork-session"));
        }
    }
}

/// permission-mode を args へ追加する。`skip-permissions` は `--permission-mode` の値ではなく
/// 独立起動フラグ `--dangerously-skip-permissions` を出力する。`config` は何も付けない。
fn push_permission_mode_args(args: &mut Vec<OsString>, mode: ClaudePermissionMode) {
    if mode.is_dangerously_skip() {
        args.push(OsString::from("--dangerously-skip-permissions"));
    } else if let Some(mode) = mode.cli_arg() {
        args.push(OsString::from("--permission-mode"));
        args.push(OsString::from(mode));
    }
}

fn push_model_and_effort_args(args: &mut Vec<OsString>, settings: &ClaudeExecSettings) {
    if let Some(model) = settings.effective_model_arg() {
        args.push(OsString::from("--model"));
        args.push(OsString::from(model));
    }
    if let Some(effort) = settings.effort.cli_arg() {
        args.push(OsString::from("--effort"));
        args.push(OsString::from(effort));
    }
}

fn push_allowed_tools_args<'a>(args: &mut Vec<OsString>, rules: impl Iterator<Item = &'a String>) {
    let mut allowed: Vec<&str> = Vec::new();
    for rule in rules {
        if !allowed.contains(&rule.as_str()) {
            allowed.push(rule.as_str());
        }
    }
    if !allowed.is_empty() {
        args.push(OsString::from("--allowedTools"));
        for rule in allowed {
            args.push(OsString::from(rule));
        }
    }
}

fn push_add_dir_args(args: &mut Vec<OsString>, settings: &ClaudeExecSettings) {
    for dir in &settings.additional_dirs {
        args.push(OsString::from("--add-dir"));
        args.push(OsString::from(dir));
    }
}

fn push_system_prompt_args(args: &mut Vec<OsString>, developer_instructions: &str) {
    args.push(OsString::from("--append-system-prompt"));
    args.push(OsString::from(developer_instructions));
}

/// 古い claude CLI が `--include-partial-messages` を未対応で拒否した stderr かどうか。
/// unknown option / unexpected argument 系のメッセージにフラグ名が含まれるかで判定する。
pub(super) fn stderr_indicates_no_partial_messages(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    if !lower.contains("--include-partial-messages") {
        return false;
    }
    lower.contains("unknown option")
        || lower.contains("unknown argument")
        || lower.contains("unexpected argument")
        || lower.contains("unknown flag")
        || lower.contains("unrecognized")
        || lower.contains("invalid option")
}

// ---------------------------------------------------------------------------
// stream-json イベントパーサ
// ---------------------------------------------------------------------------

/// `type` フィールドを取り出す。
pub(super) fn event_type(value: &Value) -> &str {
    value.get("type").and_then(Value::as_str).unwrap_or("")
}

pub(super) fn event_subtype(value: &Value) -> Option<&str> {
    value.get("subtype").and_then(Value::as_str)
}

/// `--include-partial-messages` の `stream_event` から text_delta のテキストを取り出す。
/// content_block_delta 以外・thinking_delta / input_json_delta 等は逐次表示しないので None。
pub(super) fn stream_text_delta(value: &Value) -> Option<&str> {
    if event_type(value) != "stream_event" {
        return None;
    }
    let event = value.get("event")?;
    if event.get("type").and_then(Value::as_str) != Some("content_block_delta") {
        return None;
    }
    let delta = event.get("delta")?;
    if delta.get("type").and_then(Value::as_str) != Some("text_delta") {
        return None;
    }
    delta.get("text").and_then(Value::as_str)
}

/// system/init の session_id を取り出す。stream-json は snake_case、
/// 永続 jsonl は camelCase なので両方見る。
pub(super) fn session_id(value: &Value) -> Option<String> {
    value
        .get("session_id")
        .or_else(|| value.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// assistant メッセージの content ブロック。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ClaudeBlock {
    Text(String),
    Thinking(String),
    ToolUse {
        name: String,
        summary: Option<String>,
        /// Edit/Write/MultiEdit のとき、色付き差分プレビュー用の apply_patch テキスト。
        edit_patch: Option<String>,
    },
}

/// assistant イベントの content を順序どおりに分解する。
pub(super) fn assistant_blocks(value: &Value) -> Vec<ClaudeBlock> {
    let Some(content) = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut blocks = Vec::new();
    for item in content {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "text" => {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    let text = text.trim();
                    if !text.is_empty() {
                        blocks.push(ClaudeBlock::Text(text.to_string()));
                    }
                }
            }
            "thinking" => {
                if let Some(text) = item.get("thinking").and_then(Value::as_str) {
                    let text = text.trim();
                    if !text.is_empty() {
                        blocks.push(ClaudeBlock::Thinking(text.to_string()));
                    }
                }
            }
            "tool_use" => {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let input = item.get("input");
                let summary = input.and_then(|input| tool_use_summary(&name, input));
                let edit_patch =
                    input.and_then(|input| super::claude_edit_patch_text(&name, input));
                blocks.push(ClaudeBlock::ToolUse {
                    name,
                    summary,
                    edit_patch,
                });
            }
            _ => {}
        }
    }
    blocks
}

/// tool_use の input を 1 行に要約する。Bash はコマンド、ファイル系はパス、
/// それ以外は input の JSON を短縮表示する。
pub(super) fn tool_use_summary(name: &str, input: &Value) -> Option<String> {
    let str_field = |key: &str| input.get(key).and_then(Value::as_str).map(str::to_string);
    match name {
        "Bash" | "BashOutput" => str_field("command").or_else(|| str_field("description")),
        "Read" | "Write" | "Edit" | "MultiEdit" => str_field("file_path"),
        "NotebookEdit" => str_field("notebook_path").or_else(|| str_field("file_path")),
        "Glob" => str_field("pattern"),
        "Grep" => str_field("pattern"),
        "WebFetch" => str_field("url"),
        "WebSearch" => str_field("query"),
        "Task" => str_field("description").or_else(|| str_field("subagent_type")),
        _ => match input {
            Value::Null => None,
            Value::Object(map) if map.is_empty() => None,
            other => Some(other.to_string()),
        },
    }
}

/// user イベント（tool_result）の出力 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeToolResult {
    pub(super) text: String,
    pub(super) is_error: bool,
}

/// user イベントの tool_result ブロックを取り出す。
pub(super) fn tool_results(value: &Value) -> Vec<ClaudeToolResult> {
    let Some(content) = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let mut results = Vec::new();
    for item in content {
        if item.get("type").and_then(Value::as_str) != Some("tool_result") {
            continue;
        }
        let is_error = item
            .get("is_error")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let text = tool_result_text(item.get("content"));
        results.push(ClaudeToolResult { text, is_error });
    }
    results
}

/// tool_result の content（文字列 or ブロック配列）を平文へ落とす。
fn tool_result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.trim().to_string(),
        Some(Value::Array(items)) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(Value::as_str) {
                    parts.push(text.trim().to_string());
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

/// 承認拒否 1 件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeDenial {
    pub(super) tool_name: String,
    pub(super) target: Option<String>,
}

/// result イベントの要約。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeResult {
    pub(super) text: Option<String>,
    pub(super) is_error: bool,
    pub(super) subtype: Option<String>,
    pub(super) usage_label: Option<String>,
    pub(super) denials: Vec<ClaudeDenial>,
}

/// result イベントを解析する。
pub(super) fn parse_result(value: &Value) -> ClaudeResult {
    let text = value
        .get("result")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string);
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let subtype = value
        .get("subtype")
        .and_then(Value::as_str)
        .map(str::to_string);
    let usage_label = usage_summary(value);
    let denials = value
        .get("permission_denials")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().map(parse_denial).collect())
        .unwrap_or_default();
    ClaudeResult {
        text,
        is_error,
        subtype,
        usage_label,
        denials,
    }
}

fn parse_denial(value: &Value) -> ClaudeDenial {
    let tool_name = value
        .get("tool_name")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_string();
    let target = value
        .get("tool_input")
        .and_then(|input| tool_use_summary(&tool_name, input));
    ClaudeDenial { tool_name, target }
}

/// 単一（複合でない）Bash コマンドから `--allowedTools` のプレフィックスルールを 1 件作る。
/// 先頭のプログラム名と、`-` で始まらない次のサブコマンドまでを対象にする。
/// 例: `git push origin main` → `Bash(git push:*)` / `rm -rf x` → `Bash(rm:*)`。
fn bash_single_prefix_rule(command: &str) -> String {
    let mut tokens = command.split_whitespace();
    let Some(prog) = tokens.next() else {
        return "Bash".to_string();
    };
    let prefix = match tokens.next() {
        Some(sub) if !sub.starts_with('-') => format!("{prog} {sub}"),
        _ => prog.to_string(),
    };
    format!("Bash({prefix}:*)")
}

/// シェルコマンドを演算子 `&&` `||` `;` `|` で分割する。引用符（`'` `"`）内の演算子は
/// 分割対象にしない。引用符が閉じられていない場合は分割不能とみなし空 Vec を返す。
fn split_shell_subcommands(command: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut chars = command.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' => {
                quote = Some(ch);
                current.push(ch);
            }
            '&' | '|' => {
                // `&&` `||` は 2 文字演算子、`|` はパイプ。単独 `&`（バックグラウンド）は
                // サブコマンドの区切りとしては扱わずそのまま残す。
                if chars.peek() == Some(&ch) {
                    chars.next();
                    parts.push(std::mem::take(&mut current));
                } else if ch == '|' {
                    parts.push(std::mem::take(&mut current));
                } else {
                    current.push(ch);
                }
            }
            ';' => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    if quote.is_some() {
        // 引用符が閉じていない → 分割を諦める。
        return Vec::new();
    }
    parts.push(current);
    parts
}

/// 複合 Bash コマンドをサブコマンドごとに分割し、それぞれのプレフィックスルールを生成する。
/// Claude Code は `&&`/`||`/`;`/`|` で分割して各サブコマンドを個別照合するため、
/// `cd sub && rm -rf x` のような複合コマンドは先頭語だけのルールでは通らない。
/// 分割できない（引用符が閉じていない等）場合はコマンド全体を 1 サブコマンドとして扱う。
fn bash_prefix_rules(command: &str) -> Vec<String> {
    let subs = split_shell_subcommands(command);
    let subs = if subs.is_empty() {
        vec![command.to_string()]
    } else {
        subs
    };
    let mut rules: Vec<String> = Vec::new();
    for sub in subs {
        let sub = sub.trim();
        if sub.is_empty() {
            continue;
        }
        let rule = bash_single_prefix_rule(sub);
        if !rules.contains(&rule) {
            rules.push(rule);
        }
    }
    if rules.is_empty() {
        rules.push("Bash".to_string());
    }
    rules
}

/// 拒否 1 件から `--allowedTools` ルール一覧を生成する。
/// Bash は複合コマンドをサブコマンドごとに分割したプレフィックスルール群、
/// それ以外はツール名そのもの 1 件。
pub(super) fn allowed_tool_rules_for_denial(denial: &ClaudeDenial) -> Vec<String> {
    match denial.tool_name.as_str() {
        "Bash" | "BashOutput" => match denial.target.as_deref() {
            Some(command) if !command.trim().is_empty() => bash_prefix_rules(command),
            _ => vec!["Bash".to_string()],
        },
        other => vec![other.to_string()],
    }
}

/// 拒否群から重複を除いた `--allowedTools` ルール一覧を生成する。
pub(super) fn allowed_tool_rules(denials: &[ClaudeDenial]) -> Vec<String> {
    let mut rules: Vec<String> = Vec::new();
    for denial in denials {
        for rule in allowed_tool_rules_for_denial(denial) {
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    }
    rules
}

/// result / assistant の usage からトークン・コスト要約を作る。
fn usage_summary(value: &Value) -> Option<String> {
    let usage = value.get("usage")?;
    let field = |key: &str| usage.get(key).and_then(Value::as_u64);
    let mut parts = Vec::new();
    if let Some(input) = field("input_tokens") {
        parts.push(format!("input={input}"));
    }
    let cached = field("cache_read_input_tokens").unwrap_or(0)
        + field("cache_creation_input_tokens").unwrap_or(0);
    if cached > 0 {
        parts.push(format!("cached={cached}"));
    }
    if let Some(output) = field("output_tokens") {
        parts.push(format!("output={output}"));
    }
    if let Some(cost) = value.get("total_cost_usd").and_then(Value::as_f64) {
        parts.push(format!("cost=${cost:.4}"));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// result の usage からコンテキストを占めるトークン数（input + cache_read + cache_creation）を返す。
/// output_tokens は含めない（次リクエストのコンテキストには input として積まれるため二重計上しない）。
pub(super) fn context_tokens(value: &Value) -> Option<u64> {
    let usage = value.get("usage")?;
    let field = |key: &str| usage.get(key).and_then(Value::as_u64).unwrap_or(0);
    let total = field("input_tokens")
        + field("cache_read_input_tokens")
        + field("cache_creation_input_tokens");
    (total > 0).then_some(total)
}

// ---------------------------------------------------------------------------
// セッション候補探索（~/.claude/projects/<cwd スラッグ>/*.jsonl）
// ---------------------------------------------------------------------------

/// claude 設定ディレクトリ。`CLAUDE_CONFIG_DIR` があればそれ、無ければ `~/.claude`。
pub(super) fn config_dir() -> Option<PathBuf> {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".claude")))
}

/// cwd 絶対パスを projects ディレクトリのスラッグへ変換する。
/// 実ファイルで検証済み: `/`, `.`, `_` を `-` に置換する（例:
/// `/Users/x/dev/foo` → `-Users-x-dev-foo`）。
pub(super) fn cwd_slug(cwd: &str) -> String {
    cwd.chars()
        .map(|c| match c {
            '/' | '.' | '_' => '-',
            other => other,
        })
        .collect()
}

/// 指定 config ディレクトリ配下から、cwd に対応するセッション候補を mtime 降順で返す。
pub(super) fn load_session_candidates_from(
    config_dir: &Path,
    cwd: &str,
    limit: usize,
) -> Vec<CodexSessionCandidate> {
    let dir = config_dir.join("projects").join(cwd_slug(cwd));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(OsStr::to_str) != Some("jsonl") {
            continue;
        }
        let mtime = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        files.push((path, mtime));
    }
    files.sort_by_key(|(_, mtime)| std::cmp::Reverse(*mtime));
    files.truncate(limit);
    files
        .into_iter()
        .filter_map(|(path, _)| session_candidate_from_file(&path, cwd))
        .collect()
}

/// 指定 config ディレクトリ配下に、cwd スラッグに対応する `<session_id>.jsonl` があるか判定する。
/// テスト容易性のため config ディレクトリを引数注入する内部関数。
pub(super) fn session_file_exists_in(config_dir: &Path, cwd: &str, session_id: &str) -> bool {
    config_dir
        .join("projects")
        .join(cwd_slug(cwd))
        .join(format!("{session_id}.jsonl"))
        .is_file()
}

/// 本番用ラッパ。`CLAUDE_CONFIG_DIR` or `~/.claude` を解決してセッション実体の有無を判定する。
/// config ディレクトリを解決できない場合は判定不能とみなし、存在扱い（復元を維持）する。
pub(super) fn session_file_exists(cwd: &Path, session_id: &str) -> bool {
    match config_dir() {
        Some(dir) => session_file_exists_in(&dir, &cwd.display().to_string(), session_id),
        None => true,
    }
}

/// jsonl 1 ファイルから候補を作る。id=ファイル名、title=最初の人間 user メッセージ。
pub(super) fn session_candidate_from_file(path: &Path, cwd: &str) -> Option<CodexSessionCandidate> {
    let id = path.file_stem()?.to_str()?.to_string();
    let content = std::fs::read_to_string(path).ok()?;
    let mut title = "untitled".to_string();
    let mut updated_at = String::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        // queue-operation 等の非 user 行はスキップ。
        if value.get("type").and_then(Value::as_str) != Some("user") {
            continue;
        }
        let Some(text) = first_user_text(&value) else {
            continue;
        };
        title = text;
        updated_at = value
            .get("timestamp")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        break;
    }
    Some(CodexSessionCandidate {
        id,
        title,
        updated_at,
        cwd: Some(cwd.to_string()),
    })
}

/// user レコードの message.content から人間が入力したテキストを取り出す。
/// tool_result のみの行（content が配列で text ブロックを含まない）は None。
fn first_user_text(value: &Value) -> Option<String> {
    let content = value.get("message").and_then(|m| m.get("content"))?;
    let text = match content {
        Value::String(text) => text.trim().to_string(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if item.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(text) = item.get("text").and_then(Value::as_str)
                {
                    parts.push(text.trim().to_string());
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    };
    let text = text.trim();
    if text.is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn os(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn exec_args_first_turn_has_stream_json_and_system_prompt() {
        let settings = ClaudeExecSettings::default();
        let args = os(&exec_args(None, &settings, &[], false, "INSTRUCTIONS"));
        assert_eq!(
            &args[0..5],
            &[
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages"
            ]
        );
        assert!(!args.iter().any(|a| a == "--resume"));
        // config 既定なのでモデル・effort・permission は付かない。
        assert!(!args.iter().any(|a| a == "--model"));
        assert!(!args.iter().any(|a| a == "--effort"));
        assert!(!args.iter().any(|a| a == "--permission-mode"));
        let idx = args
            .iter()
            .position(|a| a == "--append-system-prompt")
            .unwrap();
        assert_eq!(args[idx + 1], "INSTRUCTIONS");
    }

    #[test]
    fn exec_args_resume_adds_session_id() {
        let settings = ClaudeExecSettings::default();
        let args = os(&exec_args(Some("sess-123"), &settings, &[], false, "x"));
        let idx = args.iter().position(|a| a == "--resume").unwrap();
        assert_eq!(args[idx + 1], "sess-123");
        assert!(!args.iter().any(|a| a == "--fork-session"));
    }

    #[test]
    fn exec_args_fork_adds_fork_flag() {
        let settings = ClaudeExecSettings::default();
        let args = os(&exec_args(Some("sess-123"), &settings, &[], true, "x"));
        assert!(args.iter().any(|a| a == "--fork-session"));
    }

    #[test]
    fn exec_args_include_model_effort_permission() {
        let mut settings = ClaudeExecSettings::default();
        settings.set_model("opus");
        settings.set_effort(ClaudeEffortChoice::High);
        settings.set_permission_mode(ClaudePermissionMode::AcceptEdits);
        let args = os(&exec_args(None, &settings, &[], false, "x"));
        let m = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[m + 1], "opus");
        let e = args.iter().position(|a| a == "--effort").unwrap();
        assert_eq!(args[e + 1], "high");
        let p = args.iter().position(|a| a == "--permission-mode").unwrap();
        assert_eq!(args[p + 1], "acceptEdits");
    }

    #[test]
    fn exec_args_model_override_takes_precedence() {
        let mut settings = ClaudeExecSettings::default();
        settings.set_model("claude-opus-4-8");
        let args = os(&exec_args(None, &settings, &[], false, "x"));
        let m = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[m + 1], "claude-opus-4-8");
    }

    #[test]
    fn exec_args_one_shot_allowed_tools_added_after_flag() {
        let settings = ClaudeExecSettings::default();
        let one_shot = vec!["Bash(git push:*)".to_string(), "Edit".to_string()];
        let args = os(&exec_args(Some("s"), &settings, &one_shot, false, "x"));
        let idx = args.iter().position(|a| a == "--allowedTools").unwrap();
        assert_eq!(args[idx + 1], "Bash(git push:*)");
        assert_eq!(args[idx + 2], "Edit");
        // 自動での全許可昇格は廃止済み。
        assert!(!args.iter().any(|a| a == "--dangerously-skip-permissions"));
    }

    #[test]
    fn exec_args_sticky_and_one_shot_allowed_tools_dedup() {
        let mut settings = ClaudeExecSettings::default();
        settings.add_allowed_tools(&["Edit".to_string()]);
        let one_shot = vec!["Edit".to_string(), "Bash(cargo test:*)".to_string()];
        let args = os(&exec_args(None, &settings, &one_shot, false, "x"));
        // sticky の Edit と one-shot の Edit は 1 つに統合される。
        assert_eq!(args.iter().filter(|a| *a == "Edit").count(), 1);
        assert!(args.iter().any(|a| a == "Bash(cargo test:*)"));
    }

    #[test]
    fn exec_args_no_allowed_tools_flag_when_empty() {
        let settings = ClaudeExecSettings::default();
        let args = os(&exec_args(None, &settings, &[], false, "x"));
        assert!(!args.iter().any(|a| a == "--allowedTools"));
    }

    #[test]
    fn exec_args_omits_partial_messages_when_disabled() {
        let mut settings = ClaudeExecSettings::default();
        assert!(!settings.no_partial_messages());
        settings.disable_partial_messages();
        assert!(settings.no_partial_messages());
        let args = os(&exec_args(None, &settings, &[], false, "x"));
        assert!(!args.iter().any(|a| a == "--include-partial-messages"));
        // 他の基本フラグは維持される。
        assert_eq!(
            &args[0..4],
            &["-p", "--output-format", "stream-json", "--verbose"]
        );
    }

    #[test]
    fn stderr_no_partial_messages_detection() {
        assert!(stderr_indicates_no_partial_messages(
            "error: unknown option '--include-partial-messages'"
        ));
        assert!(stderr_indicates_no_partial_messages(
            "error: unexpected argument '--include-partial-messages' found"
        ));
        assert!(stderr_indicates_no_partial_messages(
            "Unknown Flag: --include-partial-messages"
        ));
        // フラグ名を含まない一般的なエラーは対象外。
        assert!(!stderr_indicates_no_partial_messages(
            "error: unknown option '--foo'"
        ));
        // フラグ名を含むが unknown/unexpected 系でない出力は対象外。
        assert!(!stderr_indicates_no_partial_messages(
            "using --include-partial-messages for streaming"
        ));
    }

    #[test]
    fn exec_args_add_dir_repeats() {
        let mut settings = ClaudeExecSettings::default();
        settings.add_dir("/a".to_string());
        settings.add_dir("/b".to_string());
        let args = os(&exec_args(None, &settings, &[], false, "x"));
        assert_eq!(args.iter().filter(|a| *a == "--add-dir").count(), 2);
    }

    #[test]
    fn model_cycle_wraps() {
        let mut s = ClaudeExecSettings::default();
        assert_eq!(s.cycle_model(), "fable");
        assert_eq!(s.cycle_model(), "opus");
        assert_eq!(s.cycle_model(), "sonnet");
        assert_eq!(s.cycle_model(), "haiku");
        assert_eq!(s.cycle_model(), "config");
    }

    #[test]
    fn effort_cycle_reaches_max() {
        let mut s = ClaudeExecSettings::default();
        assert_eq!(s.cycle_effort(), "low");
        assert_eq!(s.cycle_effort(), "medium");
        assert_eq!(s.cycle_effort(), "high");
        assert_eq!(s.cycle_effort(), "xhigh");
        assert_eq!(s.cycle_effort(), "max");
        assert_eq!(s.cycle_effort(), "config");
    }

    #[test]
    fn permission_cycle_wraps() {
        let mut s = ClaudeExecSettings::default();
        assert_eq!(s.cycle_permission_mode(), "plan");
        assert_eq!(s.cycle_permission_mode(), "acceptEdits");
        assert_eq!(s.cycle_permission_mode(), "dontAsk");
        assert_eq!(s.cycle_permission_mode(), "bypassPermissions");
        assert_eq!(
            s.cycle_permission_mode(),
            "skip-permissions（危険・原則許可）"
        );
        assert_eq!(s.cycle_permission_mode(), "config");
    }

    #[test]
    fn exec_args_skip_permissions_uses_dedicated_flag() {
        let mut settings = ClaudeExecSettings::default();
        settings.set_permission_mode(ClaudePermissionMode::DangerouslySkipPermissions);
        let args = os(&exec_args(None, &settings, &[], false, "x"));
        // 独立フラグを出力し、--permission-mode は付けない。
        assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
        assert!(!args.iter().any(|a| a == "--permission-mode"));
        assert!(
            !args
                .iter()
                .any(|a| a == "skip-permissions（危険・原則許可）")
        );
    }

    #[test]
    fn resident_args_skip_permissions_uses_dedicated_flag() {
        let mut settings = ClaudeExecSettings::default();
        settings.set_permission_mode(ClaudePermissionMode::DangerouslySkipPermissions);
        let args = os(&resident_args(None, &settings, false, "x"));
        assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));
        assert!(!args.iter().any(|a| a == "--permission-mode"));
    }

    #[test]
    fn parse_permission_mode_skip_aliases() {
        for alias in [
            "skip",
            "skip-permissions",
            "dangerously-skip-permissions",
            "SKIP-PERMISSIONS",
            "skip-permissions（危険・原則許可）",
            "skip-permissions（危険・全許可）",
        ] {
            assert_eq!(
                parse_permission_mode(alias),
                Some(ClaudePermissionMode::DangerouslySkipPermissions),
                "alias={alias}"
            );
        }
    }

    #[test]
    fn init_session_id_parsed() {
        let value = json!({"type":"system","subtype":"init","session_id":"abc","model":"opus"});
        assert_eq!(event_type(&value), "system");
        assert_eq!(event_subtype(&value), Some("init"));
        assert_eq!(session_id(&value).as_deref(), Some("abc"));
    }

    #[test]
    fn assistant_blocks_text_thinking_tool() {
        let value = json!({
            "type": "assistant",
            "message": {"content": [
                {"type": "thinking", "thinking": "考える"},
                {"type": "text", "text": "こんにちは"},
                {"type": "tool_use", "id": "t1", "name": "Bash", "input": {"command": "ls -la"}},
                {"type": "tool_use", "id": "t2", "name": "Write", "input": {"file_path": "/tmp/x.rs"}}
            ]}
        });
        let blocks = assistant_blocks(&value);
        assert_eq!(blocks[0], ClaudeBlock::Thinking("考える".to_string()));
        assert_eq!(blocks[1], ClaudeBlock::Text("こんにちは".to_string()));
        assert_eq!(
            blocks[2],
            ClaudeBlock::ToolUse {
                name: "Bash".to_string(),
                summary: Some("ls -la".to_string()),
                edit_patch: None,
            }
        );
        assert_eq!(
            blocks[3],
            ClaudeBlock::ToolUse {
                name: "Write".to_string(),
                summary: Some("/tmp/x.rs".to_string()),
                // content 未指定の Write は空の Add パッチになる。
                edit_patch: Some(
                    "EDIT *** Begin Patch\n*** Add File: /tmp/x.rs\n*** End Patch".to_string()
                ),
            }
        );
    }

    #[test]
    fn tool_results_flags_error() {
        let value = json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "boom", "is_error": true}
            ]}
        });
        let results = tool_results(&value);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_error);
        assert_eq!(results[0].text, "boom");
    }

    #[test]
    fn parse_result_extracts_denials_and_usage() {
        let value = json!({
            "type": "result",
            "subtype": "success",
            "is_error": false,
            "result": "done",
            "session_id": "s1",
            "total_cost_usd": 0.0123,
            "usage": {"input_tokens": 100, "output_tokens": 50, "cache_read_input_tokens": 10},
            "permission_denials": [
                {"tool_name": "Write", "tool_use_id": "t1", "tool_input": {"file_path": "/a.rs"}}
            ]
        });
        let result = parse_result(&value);
        assert_eq!(result.text.as_deref(), Some("done"));
        assert!(!result.is_error);
        assert_eq!(result.denials.len(), 1);
        assert_eq!(result.denials[0].tool_name, "Write");
        assert_eq!(result.denials[0].target.as_deref(), Some("/a.rs"));
        let usage = result.usage_label.unwrap();
        assert!(usage.contains("input=100"));
        assert!(usage.contains("output=50"));
        assert!(usage.contains("cost=$0.0123"));
    }

    #[test]
    fn session_file_exists_in_checks_cwd_slug_path() {
        let root = std::env::temp_dir().join(format!(
            "addness-claude-session-exists-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let cwd = "/Users/x/dev/foo";
        let dir = root.join("projects").join(cwd_slug(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("sess-1.jsonl"), "{}\n").unwrap();

        assert!(session_file_exists_in(&root, cwd, "sess-1"));
        assert!(!session_file_exists_in(&root, cwd, "missing"));
        // cwd が異なればスラッグが変わり実体は見つからない。
        assert!(!session_file_exists_in(&root, "/other/dir", "sess-1"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn allowed_tool_rule_bash_prefix_and_edit() {
        let git = ClaudeDenial {
            tool_name: "Bash".to_string(),
            target: Some("git push origin main".to_string()),
        };
        assert_eq!(
            allowed_tool_rules_for_denial(&git),
            vec!["Bash(git push:*)"]
        );
        let rm = ClaudeDenial {
            tool_name: "Bash".to_string(),
            target: Some("rm -rf x".to_string()),
        };
        assert_eq!(allowed_tool_rules_for_denial(&rm), vec!["Bash(rm:*)"]);
        let write = ClaudeDenial {
            tool_name: "Write".to_string(),
            target: Some("/a.rs".to_string()),
        };
        assert_eq!(allowed_tool_rules_for_denial(&write), vec!["Write"]);
    }

    #[test]
    fn allowed_tool_rules_split_compound_bash_command() {
        // 複合コマンドはサブコマンドごとにルール化する。
        let denial = ClaudeDenial {
            tool_name: "Bash".to_string(),
            target: Some("cd sub && rm -rf x".to_string()),
        };
        assert_eq!(
            allowed_tool_rules_for_denial(&denial),
            vec!["Bash(cd sub:*)", "Bash(rm:*)"]
        );

        // パイプ・セミコロンも分割対象。
        let piped = ClaudeDenial {
            tool_name: "Bash".to_string(),
            target: Some("cat a.txt | grep foo; echo done".to_string()),
        };
        assert_eq!(
            allowed_tool_rules_for_denial(&piped),
            vec!["Bash(cat a.txt:*)", "Bash(grep foo:*)", "Bash(echo done:*)"]
        );

        // 引用符内の演算子では分割しない。
        let quoted = ClaudeDenial {
            tool_name: "Bash".to_string(),
            target: Some("echo \"a && b\"".to_string()),
        };
        assert_eq!(
            allowed_tool_rules_for_denial(&quoted),
            vec!["Bash(echo \"a:*)"]
        );
    }

    #[test]
    fn exec_args_plan_mode_dropped_on_approval_retry() {
        let mut settings = ClaudeExecSettings::default();
        settings.set_permission_mode(ClaudePermissionMode::Plan);
        // sticky 許可のみ（one-shot 空）なら plan を維持する。
        let sticky = exec_args(Some("s1"), &settings, &[], false, "instr");
        assert!(sticky.iter().any(|a| a == "plan"));
        // 承認直後の続行ターン（one-shot allowedTools あり）では plan を落とす。
        let retry = exec_args(
            Some("s1"),
            &settings,
            &["Bash(rm:*)".to_string()],
            false,
            "instr",
        );
        assert!(!retry.iter().any(|a| a == "--permission-mode"));
        assert!(!retry.iter().any(|a| a == "plan"));
    }

    #[test]
    fn allowed_tool_rules_dedup() {
        let denials = vec![
            ClaudeDenial {
                tool_name: "Bash".to_string(),
                target: Some("git push origin main".to_string()),
            },
            ClaudeDenial {
                tool_name: "Bash".to_string(),
                target: Some("git push --force".to_string()),
            },
            ClaudeDenial {
                tool_name: "Write".to_string(),
                target: None,
            },
        ];
        // 同じ git push プレフィックスは 1 つに畳まれる。
        assert_eq!(
            allowed_tool_rules(&denials),
            vec!["Bash(git push:*)".to_string(), "Write".to_string()]
        );
    }

    #[test]
    fn stream_text_delta_extracts_only_text_delta() {
        let text = json!({
            "type": "stream_event",
            "event": {"type": "content_block_delta", "index": 0,
                      "delta": {"type": "text_delta", "text": "こん"}}
        });
        assert_eq!(stream_text_delta(&text), Some("こん"));
        let thinking = json!({
            "type": "stream_event",
            "event": {"type": "content_block_delta", "index": 0,
                      "delta": {"type": "thinking_delta", "thinking": "考える"}}
        });
        assert_eq!(stream_text_delta(&thinking), None);
        let start = json!({
            "type": "stream_event",
            "event": {"type": "content_block_start", "index": 0,
                      "content_block": {"type": "text", "text": ""}}
        });
        assert_eq!(stream_text_delta(&start), None);
        let assistant = json!({"type": "assistant", "message": {"content": []}});
        assert_eq!(stream_text_delta(&assistant), None);
    }

    #[test]
    fn cwd_slug_replaces_separators() {
        assert_eq!(cwd_slug("/Users/x/dev/foo"), "-Users-x-dev-foo");
        assert_eq!(cwd_slug("/a/.claude-worktrees/b"), "-a--claude-worktrees-b");
        assert_eq!(cwd_slug("/a/my_dir"), "-a-my-dir");
    }

    #[test]
    fn load_session_candidates_reads_first_user_message() {
        let tmp = std::env::temp_dir().join(format!("claude-sess-test-{}", std::process::id()));
        let cwd = "/Users/x/dev/proj";
        let dir = tmp.join("projects").join(cwd_slug(cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("11111111-1111-1111-1111-111111111111.jsonl");
        let lines = [
            json!({"type":"mode","mode":"normal"}).to_string(),
            json!({"type":"queue-operation","operation":"enqueue","content":"skip me"}).to_string(),
            json!({"type":"user","message":{"role":"user","content":"最初の依頼です"},"timestamp":"2026-07-06T09:00:00Z"}).to_string(),
        ];
        std::fs::write(&file, lines.join("\n")).unwrap();

        let candidates = load_session_candidates_from(&tmp, cwd, 12);
        std::fs::remove_dir_all(&tmp).ok();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].id, "11111111-1111-1111-1111-111111111111");
        assert_eq!(candidates[0].title, "最初の依頼です");
        assert_eq!(candidates[0].cwd.as_deref(), Some(cwd));
    }

    #[test]
    fn session_candidate_skips_tool_result_only_user_lines() {
        let value = json!({
            "type": "user",
            "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": "output"}
            ]}
        });
        assert!(first_user_text(&value).is_none());
    }

    // -----------------------------------------------------------------------
    // 上流プローブ（#[ignore]・実 claude バイナリ相手）
    //
    // 通常の `cargo test` では走らない。CI の upstream-sync が新バージョンの実
    // バイナリをインストールした上で `--ignored` 付きで実行し、チェンジログに
    // 現れないフラグの改名・廃止を実測検知する。
    // ローカル実行: `cargo test upstream_probe_ -- --ignored`。
    // バイナリは `ADDNESS_PROBE_CLAUDE_BIN`（未設定時 `claude`）で差し替え可能。
    // -----------------------------------------------------------------------

    /// 実 `claude --help` に、本リポジトリが `resident_args` / `exec_args` で渡す
    /// フラグが存在することを確認する。
    #[test]
    #[ignore = "実 claude バイナリが必要（CI の upstream-sync が --ignored で実行）"]
    fn upstream_probe_claude_cli_flags() {
        let bin =
            std::env::var("ADDNESS_PROBE_CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());
        let output = std::process::Command::new(&bin)
            .arg("--help")
            .output()
            .unwrap_or_else(|e| panic!("{bin} --help の実行に失敗しました: {e}"));
        let mut help = String::from_utf8_lossy(&output.stdout).into_owned();
        help.push_str(&String::from_utf8_lossy(&output.stderr));

        // resident_args / exec_args / push_* ヘルパが渡すフラグ。
        // 隠しフラグ（--permission-prompt-tool）は help に出ないため対象外。
        const REQUIRED: &[&str] = &[
            "--print",                        // -p 非対話モード
            "--output-format",                // stream-json 出力
            "--input-format",                 // 常駐（双方向）stream-json 入力
            "--include-partial-messages",     // トークン単位ストリーミング
            "--verbose",                      // stream-json 全イベント出力
            "--permission-mode",              // 権限モード
            "--dangerously-skip-permissions", // 全権限スキップ（起動時フラグ）
            "--resume",                       // セッション継続
            "--fork-session",                 // resume 時のセッション複製
            "--model",                        // モデル指定
            "--effort",                       // effort 指定
            "--add-dir",                      // 書込許可ディレクトリ追加
            "--append-system-prompt",         // Addness 手順注入
        ];
        let missing: Vec<&str> = REQUIRED
            .iter()
            .copied()
            .filter(|flag| !help.contains(flag))
            .collect();
        assert!(
            missing.is_empty(),
            "claude --help に存在しないフラグ: {missing:?}（bin={bin}）"
        );
    }
}
