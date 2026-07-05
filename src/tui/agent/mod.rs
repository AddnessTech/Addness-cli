//! TUI から起動できるコーディングエージェントの抽象。
//!
//! 同一 TUI から複数のエージェント（codex / Claude Code 等）を呼べるように
//! するための継ぎ目。CLI 引数生成・プロンプト受け渡し方式・イベント正規化など
//! backend 固有の差分をこのモジュール配下に閉じ込め、`codex_pane` 側は
//! backend 非依存のロジックへ寄せていく。
//!
//! - [`AgentBackend`] — backend の trait 境界
//! - [`codex::CodexBackend`] — codex CLI（`codex exec --json`）実装
//! - [`claude::ClaudeBackend`] — Claude Code CLI（`claude -p --output-format stream-json`）実装

use serde_json::Value;

pub(crate) mod claude;
pub(crate) mod codex;

/// プロンプトを子プロセスへ渡す方式。backend ごとに異なる。
/// codex は stdin へ書き込み、引数末尾に sentinel `-` を置く（`Stdin`）。
/// Claude Code は `--input-format stream-json` で stdin へ JSON を流す方式もあるが、
/// 単発プロンプトは引数として渡す（`Arg`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptDelivery {
    /// stdin へ書き込む（codex exec 方式）。
    Stdin,
    /// コマンドライン引数として渡す（`claude -p "<prompt>"` 方式）。
    Arg,
}

/// backend 非依存に正規化したターンライフサイクルイベント。
///
/// codex の `thread.started` / `turn.*` / `error` や Claude の
/// `system(init)` / `result` といった型名の違いをこの共通形へマップし、
/// `CodexPane` 側は型名文字列ではなくこの enum で分岐する。ツール実行や
/// アシスタント本文など表層に近いイベントは、当面 `handle_generic_json_event`
/// 側で扱うため `Other` に集約する。
pub(crate) enum AgentEvent {
    /// セッション開始（codex: `thread.started` / Claude: `system` init）。
    /// 値はセッション/スレッド ID。
    SessionStarted(Option<String>),
    /// ターン開始（codex: `turn.started`）。
    TurnStarted,
    /// ターン完了（codex: `turn.completed` / `turn.finished` / Claude: `result` 成功）。
    TurnCompleted,
    /// ターン失敗（codex: `turn.failed` / Claude: `result` エラー）。値は表示用メッセージ。
    TurnFailed(String),
    /// エラー通知（codex: `error`）。値は表示用メッセージ。
    Error(String),
    /// 上記以外。ツール実行・本文など、backend 共通の表層処理へ委ねる。
    Other,
}

/// TUI から起動できるコーディングエージェントの継ぎ目。
///
/// 設定型は関連型 `Settings` に閉じ込め、ライフサイクルイベントの型名解釈
/// （`parse_lifecycle_event` / `session_id_from_event`）を backend 側へ寄せる。
/// バイナリ解決・resume 可否・ツール/本文イベントの正規化は、呼び出し側
/// （app.rs）の抽象化に合わせ後続で加える。
pub(crate) trait AgentBackend {
    /// backend 固有の実行設定型。
    type Settings;

    /// 通常ターンの CLI 引数を生成する（新規 / resume を `session` で分岐）。
    fn turn_args(&self, session: Option<&str>, cwd: &str, settings: &Self::Settings)
    -> Vec<String>;

    /// プロンプトの受け渡し方式。
    fn prompt_delivery(&self) -> PromptDelivery;

    /// JSON イベント 1 件を backend 非依存のライフサイクルイベントへ正規化する。
    /// ツール/本文など表層イベントは [`AgentEvent::Other`] に集約する。
    fn parse_lifecycle_event(&self, value: &Value) -> AgentEvent;

    /// セッション開始イベントからセッション/スレッド ID を抽出する。
    fn session_id_from_event(&self, value: &Value) -> Option<String>;

    /// スラッシュコマンドのヘルプ本文。
    fn help_text(&self) -> &'static str;
}

/// JSON オブジェクトから、指定キー群のうち最初に見つかった文字列値を返す。
pub(crate) fn string_at_any(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::to_string)
}

/// `error.message`（無ければ `error` 自体の文字列）を取り出す。
pub(crate) fn nested_error_message(value: &Value) -> Option<String> {
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

/// ネストした JSON から、表示に使えそうな最初のテキストフィールドを再帰探索する。
pub(crate) fn first_text_field(value: &Value) -> Option<String> {
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
