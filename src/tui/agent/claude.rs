//! Claude Code CLI（`claude -p --output-format stream-json`）を駆動する backend。
//!
//! codex とは CLI 体系・イベントスキーマが別物なので、`turn_args` と
//! `parse_lifecycle_event` を Claude 用に実装する。プロンプトは codex の
//! stdin 方式ではなく位置引数として渡す（[`PromptDelivery::Arg`]）。
//!
//! 本モジュールは backend の標準出力（JSONL の stream-json）の**ライフサイクル**
//! 正規化までを担う。app.rs への配線（起動導線・メニュー・DoD 判定の分岐）は
//! 後続ステップで行う。
//!
//! NOTE: `ClaudeBackend` は現時点で app.rs へ未配線（Step6 で `CodexPane` 側へ
//! 差し込む）。それまで production ビルドでは未使用となるため、モジュール単位で
//! dead_code を許可する。配線が済んだらこの許可を外す。
#![allow(dead_code)]

use serde_json::Value;

use super::{AgentBackend, AgentEvent, PromptDelivery, first_text_field, string_at_any};

/// Claude Code の権限モード（`--permission-mode`）。
/// codex の approval/sandbox からの読み替え規則は app.rs 配線時に定める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudePermissionMode {
    /// 自動（既定）。
    Auto,
    /// ファイル編集は自動承認、それ以外は確認。
    AcceptEdits,
    /// プランモード（読み取り中心）。
    Plan,
    /// 全権限をバイパス（サンドボックス前提）。
    BypassPermissions,
    /// 各操作を都度確認。
    Manual,
    /// 確認を出さない。
    DontAsk,
}

impl ClaudePermissionMode {
    /// `--permission-mode` に渡す CLI 文字列。
    pub(crate) fn cli_arg(self) -> &'static str {
        match self {
            ClaudePermissionMode::Auto => "auto",
            ClaudePermissionMode::AcceptEdits => "acceptEdits",
            ClaudePermissionMode::Plan => "plan",
            ClaudePermissionMode::BypassPermissions => "bypassPermissions",
            ClaudePermissionMode::Manual => "manual",
            ClaudePermissionMode::DontAsk => "dontAsk",
        }
    }
}

/// Claude Code backend の実行設定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClaudeSettings {
    /// 使用モデル（`--model`）。`None` なら CLI 既定に委ねる。
    pub(crate) model: Option<String>,
    /// 権限モード（`--permission-mode`）。
    pub(crate) permission_mode: ClaudePermissionMode,
    /// 既定システムプロンプトへ追記する Addness 文脈（`--append-system-prompt`）。
    pub(crate) append_system_prompt: Option<String>,
}

impl Default for ClaudeSettings {
    fn default() -> Self {
        Self {
            model: None,
            permission_mode: ClaudePermissionMode::Auto,
            append_system_prompt: None,
        }
    }
}

/// Claude Code CLI（`claude -p --output-format stream-json`）を駆動する backend。
pub(crate) struct ClaudeBackend;

impl AgentBackend for ClaudeBackend {
    type Settings = ClaudeSettings;

    fn turn_args(
        &self,
        session: Option<&str>,
        cwd: &str,
        settings: &Self::Settings,
    ) -> Vec<String> {
        // ヘッドレス（`-p`）＋ リアルタイム JSONL 出力。stream-json は --verbose 必須。
        let mut args = vec![
            "-p".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ];

        // ツールアクセスを対象作業ディレクトリへ明示付与する。
        args.push("--add-dir".to_string());
        args.push(cwd.to_string());

        args.push("--permission-mode".to_string());
        args.push(settings.permission_mode.cli_arg().to_string());

        if let Some(model) = &settings.model {
            args.push("--model".to_string());
            args.push(model.clone());
        }

        if let Some(system) = &settings.append_system_prompt {
            args.push("--append-system-prompt".to_string());
            args.push(system.clone());
        }

        // 2 ターン目以降は同一セッションを resume する。
        if let Some(session) = session {
            args.push("--resume".to_string());
            args.push(session.to_string());
        }

        // プロンプト本体は PromptDelivery::Arg として呼び出し側が末尾へ付与する。
        args
    }

    fn prompt_delivery(&self) -> PromptDelivery {
        // claude -p は位置引数（または stdin）でプロンプトを受ける。ここでは引数方式。
        PromptDelivery::Arg
    }

    fn session_id_from_event(&self, value: &Value) -> Option<String> {
        string_at_any(value, &["session_id", "sessionId"])
    }

    fn parse_lifecycle_event(&self, value: &Value) -> AgentEvent {
        let event_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            // `{"type":"system","subtype":"init","session_id":...}` でセッション開始。
            "system" => {
                let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("");
                if subtype == "init" {
                    AgentEvent::SessionStarted(self.session_id_from_event(value))
                } else {
                    AgentEvent::Other
                }
            }
            // `{"type":"result","subtype":"success"|"error_*","is_error":bool,...}` でターン終了。
            "result" => {
                let is_error = value
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let subtype = value.get("subtype").and_then(Value::as_str).unwrap_or("");
                if is_error || subtype != "success" {
                    let message = string_at_any(value, &["result", "error"])
                        .or_else(|| first_text_field(value))
                        .unwrap_or_else(|| "Claude ターンが失敗しました".to_string());
                    AgentEvent::TurnFailed(message)
                } else {
                    AgentEvent::TurnCompleted
                }
            }
            // assistant / user(tool_result) など本文・ツール系は表層処理へ委ねる。
            _ => AgentEvent::Other,
        }
    }

    fn help_text(&self) -> &'static str {
        concat!(
            "Claude Code コマンド:\n",
            "  /model <name>       使用モデルを切り替える\n",
            "  /permissions <mode> 権限モードを切り替える (auto/acceptEdits/plan/bypassPermissions/manual/dontAsk)\n",
            "  /resume             直近セッションを再開する\n",
            "  /stop               実行中のターンを中断する\n",
            "  /clear              会話履歴をクリアする\n",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_turn_args_start_new_stream_json_turn() {
        let backend = ClaudeBackend;
        let settings = ClaudeSettings::default();
        let args = backend.turn_args(None, "/repo", &settings);

        assert_eq!(args.first().map(String::as_str), Some("-p"));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--output-format", "stream-json"])
        );
        assert!(args.iter().any(|a| a == "--verbose"));
        assert!(args.windows(2).any(|pair| pair == ["--add-dir", "/repo"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--permission-mode", "auto"])
        );
        // 新規ターンでは resume を含めない。
        assert!(!args.iter().any(|a| a == "--resume"));
    }

    #[test]
    fn claude_turn_args_resume_and_overrides() {
        let backend = ClaudeBackend;
        let settings = ClaudeSettings {
            model: Some("opus".to_string()),
            permission_mode: ClaudePermissionMode::AcceptEdits,
            append_system_prompt: Some("Addness 文脈".to_string()),
        };
        let args = backend.turn_args(Some("sess-1"), "/repo", &settings);

        assert!(args.windows(2).any(|pair| pair == ["--resume", "sess-1"]));
        assert!(args.windows(2).any(|pair| pair == ["--model", "opus"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--permission-mode", "acceptEdits"])
        );
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--append-system-prompt", "Addness 文脈"])
        );
    }

    #[test]
    fn claude_backend_normalizes_lifecycle_events() {
        let backend = ClaudeBackend;

        let init = serde_json::json!({
            "type": "system", "subtype": "init", "session_id": "sess-abc"
        });
        assert!(matches!(
            backend.parse_lifecycle_event(&init),
            AgentEvent::SessionStarted(Some(id)) if id == "sess-abc"
        ));
        assert_eq!(
            backend.session_id_from_event(&init).as_deref(),
            Some("sess-abc")
        );

        let success = serde_json::json!({
            "type": "result", "subtype": "success", "is_error": false, "result": "done"
        });
        assert!(matches!(
            backend.parse_lifecycle_event(&success),
            AgentEvent::TurnCompleted
        ));

        let failure = serde_json::json!({
            "type": "result", "subtype": "error_max_turns", "is_error": true,
            "result": "max turns reached"
        });
        assert!(matches!(
            backend.parse_lifecycle_event(&failure),
            AgentEvent::TurnFailed(msg) if msg == "max turns reached"
        ));

        // assistant 本文や system(非init) は表層処理へ委ねる。
        assert!(matches!(
            backend.parse_lifecycle_event(&serde_json::json!({"type": "assistant"})),
            AgentEvent::Other
        ));
        assert!(matches!(
            backend
                .parse_lifecycle_event(&serde_json::json!({"type": "system", "subtype": "other"})),
            AgentEvent::Other
        ));

        assert_eq!(backend.prompt_delivery(), PromptDelivery::Arg);
    }
}
