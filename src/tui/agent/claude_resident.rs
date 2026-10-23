//! Claude Code 常駐プロセスモードのクライアント。
//!
//! ワンショット（1 ターン 1 プロセス）に対し、常駐モードは 1 プロセスで多ターンを回す。
//! ここには以下を置く（TUI 状態は持たない = ユニットテスト可能な純粋関数/薄いラッパのみ）:
//! - stdin へ書き込む専用 writer スレッド + mpsc（不正 JSON でプロセス即死するため、必ず
//!   `serde_json` でシリアライズしたものだけを書く）
//! - `control_request` / `control_response` の生成（interrupt / set_model /
//!   set_permission_mode / can_use_tool への allow・deny 応答）
//! - stdout に来る `control_request`（can_use_tool）/ `control_cancel_request` /
//!   `control_response` のパース
//! - セッション内許可ルールに基づく can_use_tool の自動許可マッチ
//!
//! TUI 側の状態更新は `agent/mod.rs`（`CodexPane`）が行う。

use std::io::Write;
use std::process::{Child, ExitStatus};
use std::sync::mpsc::{self, Sender};

use anyhow::{Context, Result};
use serde_json::{Value, json};

use super::claude::{self, ClaudeDenial};

// ---------------------------------------------------------------------------
// 送信メッセージ生成（純粋関数・serde_json 経由でのみ stdin へ書く）
// ---------------------------------------------------------------------------

/// ユーザーターンの stream-json メッセージ。ターン送信 = これを stdin へ 1 行書く。
pub(super) fn user_message(text: &str) -> Value {
    json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{"type": "text", "text": text}]
        }
    })
}

/// interrupt（グレースフル中断）control_request。
pub(super) fn interrupt_request(request_id: &str) -> Value {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "interrupt"}
    })
}

/// 実行時のモデル変更 control_request。
pub(super) fn set_model_request(request_id: &str, model: &str) -> Value {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "set_model", "model": model}
    })
}

/// 実行時の permission-mode 変更 control_request。
pub(super) fn set_permission_mode_request(request_id: &str, mode: &str) -> Value {
    json!({
        "type": "control_request",
        "request_id": request_id,
        "request": {"subtype": "set_permission_mode", "mode": mode}
    })
}

/// can_use_tool への許可応答。`updated_input` には要求の input をそのまま返す。
pub(super) fn allow_response(request_id: &str, updated_input: &Value) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": {"behavior": "allow", "updatedInput": updated_input}
        }
    })
}

/// can_use_tool への拒否応答。
pub(super) fn deny_response(request_id: &str, message: &str) -> Value {
    json!({
        "type": "control_response",
        "response": {
            "subtype": "success",
            "request_id": request_id,
            "response": {"behavior": "deny", "message": message}
        }
    })
}

// ---------------------------------------------------------------------------
// 受信パース（stdout の control 系イベント）
// ---------------------------------------------------------------------------

/// その場承認要求（`--permission-prompt-tool stdio` 指定時に届く）。
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CanUseToolRequest {
    pub(super) request_id: String,
    pub(super) tool_name: String,
    pub(super) input: Value,
}

/// 自前で送った control_request への応答。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ControlResponse {
    pub(super) request_id: String,
    pub(super) success: bool,
    pub(super) error: Option<String>,
}

/// 常駐プロセスが stdout に流す制御イベント。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum ResidentControl {
    /// その場承認要求（バナー表示 or 自動許可）。
    CanUseTool(CanUseToolRequest),
    /// 承認要求のキャンセル（中断時など）。バナーを閉じ応答は送らない。
    Cancel { request_id: String },
    /// 自前 control_request への応答。
    Response(ControlResponse),
}

/// control 系イベントをパースする。制御イベントでなければ `None`。
pub(super) fn parse_control(value: &Value) -> Option<ResidentControl> {
    match value.get("type").and_then(Value::as_str)? {
        "control_request" => {
            let request_id = value.get("request_id").and_then(Value::as_str)?.to_string();
            let request = value.get("request")?;
            match request.get("subtype").and_then(Value::as_str)? {
                "can_use_tool" => {
                    let tool_name = request
                        .get("tool_name")
                        .and_then(Value::as_str)
                        .unwrap_or("tool")
                        .to_string();
                    let input = request.get("input").cloned().unwrap_or(Value::Null);
                    Some(ResidentControl::CanUseTool(CanUseToolRequest {
                        request_id,
                        tool_name,
                        input,
                    }))
                }
                _ => None,
            }
        }
        "control_cancel_request" => {
            let request_id = value.get("request_id").and_then(Value::as_str)?.to_string();
            Some(ResidentControl::Cancel { request_id })
        }
        "control_response" => {
            let response = value.get("response")?;
            let request_id = response
                .get("request_id")
                .and_then(Value::as_str)?
                .to_string();
            let success = response.get("subtype").and_then(Value::as_str) == Some("success");
            let error = if success {
                None
            } else {
                response
                    .get("error")
                    .and_then(Value::as_str)
                    .or_else(|| response.get("message").and_then(Value::as_str))
                    .map(str::to_string)
            };
            Some(ResidentControl::Response(ControlResponse {
                request_id,
                success,
                error,
            }))
        }
        _ => None,
    }
}

/// can_use_tool 要求から `--allowedTools` 形式の許可ルール候補を作る
/// （「これからずっと許可」で sticky に入れる値）。
pub(super) fn rules_for_request(tool_name: &str, input: &Value) -> Vec<String> {
    let target = claude::tool_use_summary(tool_name, input);
    claude::allowed_tool_rules_for_denial(&ClaudeDenial {
        tool_name: tool_name.to_string(),
        target,
    })
}

/// セッション内許可ルール（sticky）に照らして can_use_tool を自動許可できるか。
/// 要求から生成した許可ルールがすべて sticky に含まれていれば自動許可する。
pub(super) fn tool_matches_rules(tool_name: &str, input: &Value, sticky: &[String]) -> bool {
    let rules = rules_for_request(tool_name, input);
    !rules.is_empty() && rules.iter().all(|rule| sticky.iter().any(|s| s == rule))
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

/// 常駐 claude プロセスのクライアント。`Child` を所有し、stdin は writer スレッドへ委譲する。
/// stdout/stderr は呼び出し側（`CodexPane`）が `spawn_line_reader` で読む。
pub(super) struct ResidentClient {
    child: Child,
    writer_tx: Sender<WriterMsg>,
    next_request_id: u64,
    /// stdin クローズ済み（終了待ち）なら、その理由。
    pub(super) closing: Option<CloseReason>,
}

impl ResidentClient {
    /// spawn 済みの `Child`（stdout/stderr は取得済みで良い）から常駐クライアントを作る。
    /// stdin を writer スレッドへ移す。
    pub(super) fn new(mut child: Child) -> Result<Self> {
        let mut stdin = child
            .stdin
            .take()
            .context("Claude Code 常駐プロセス stdin の取得に失敗しました")?;
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

    fn next_id(&mut self) -> String {
        let id = self.next_request_id;
        self.next_request_id = self.next_request_id.saturating_add(1);
        format!("req-{id}")
    }

    /// serde_json でシリアライズして writer へ渡す。失敗（シリアライズ or writer 切断）は `false`。
    fn send_value(&self, value: &Value) -> bool {
        match serde_json::to_string(value) {
            Ok(line) => self.writer_tx.send(WriterMsg::Line(line)).is_ok(),
            Err(_) => false,
        }
    }

    /// ユーザーターンを送信する。成功可否を返す。
    pub(super) fn send_user_message(&self, text: &str) -> bool {
        self.send_value(&user_message(text))
    }

    /// interrupt を送る。送信できたら発行した request_id を返す。
    pub(super) fn send_interrupt(&mut self) -> Option<String> {
        let id = self.next_id();
        self.send_value(&interrupt_request(&id)).then_some(id)
    }

    /// set_model を送る。成功で request_id を返す。
    pub(super) fn send_set_model(&mut self, model: &str) -> Option<String> {
        let id = self.next_id();
        self.send_value(&set_model_request(&id, model))
            .then_some(id)
    }

    /// set_permission_mode を送る。成功で request_id を返す。
    pub(super) fn send_set_permission_mode(&mut self, mode: &str) -> Option<String> {
        let id = self.next_id();
        self.send_value(&set_permission_mode_request(&id, mode))
            .then_some(id)
    }

    /// can_use_tool へ許可応答を送る。
    pub(super) fn send_allow(&self, request_id: &str, updated_input: &Value) -> bool {
        self.send_value(&allow_response(request_id, updated_input))
    }

    /// can_use_tool へ拒否応答を送る。
    pub(super) fn send_deny(&self, request_id: &str, message: &str) -> bool {
        self.send_value(&deny_response(request_id, message))
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

    /// テスト用: 実 claude を起動せず、stdin を捨てるだけの生存プロセス（`cat`）で
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

impl Drop for ResidentClient {
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
    use serde_json::json;

    #[test]
    fn user_message_shape() {
        let v = user_message("1+1は?");
        assert_eq!(v["type"], "user");
        assert_eq!(v["message"]["role"], "user");
        assert_eq!(v["message"]["content"][0]["type"], "text");
        assert_eq!(v["message"]["content"][0]["text"], "1+1は?");
    }

    #[test]
    fn interrupt_request_shape() {
        let v = interrupt_request("my-int-1");
        assert_eq!(v["type"], "control_request");
        assert_eq!(v["request_id"], "my-int-1");
        assert_eq!(v["request"]["subtype"], "interrupt");
    }

    #[test]
    fn set_model_and_permission_request_shape() {
        let m = set_model_request("sm-1", "claude-sonnet-4-5");
        assert_eq!(m["request"]["subtype"], "set_model");
        assert_eq!(m["request"]["model"], "claude-sonnet-4-5");
        let p = set_permission_mode_request("spm-1", "acceptEdits");
        assert_eq!(p["request"]["subtype"], "set_permission_mode");
        assert_eq!(p["request"]["mode"], "acceptEdits");
    }

    #[test]
    fn allow_response_echoes_input() {
        let input = json!({"command": "mkdir x", "description": "d"});
        let v = allow_response("cffb", &input);
        assert_eq!(v["response"]["subtype"], "success");
        assert_eq!(v["response"]["request_id"], "cffb");
        assert_eq!(v["response"]["response"]["behavior"], "allow");
        assert_eq!(v["response"]["response"]["updatedInput"], input);
    }

    #[test]
    fn deny_response_shape() {
        let v = deny_response("cffb", "ユーザーが拒否しました");
        assert_eq!(v["response"]["response"]["behavior"], "deny");
        assert_eq!(
            v["response"]["response"]["message"],
            "ユーザーが拒否しました"
        );
    }

    #[test]
    fn parse_can_use_tool_request() {
        let v = json!({
            "type": "control_request",
            "request_id": "cffb9ad9",
            "request": {
                "subtype": "can_use_tool",
                "tool_name": "Bash",
                "input": {"command": "mkdir probe"}
            }
        });
        let parsed = parse_control(&v).expect("parsed");
        match parsed {
            ResidentControl::CanUseTool(req) => {
                assert_eq!(req.request_id, "cffb9ad9");
                assert_eq!(req.tool_name, "Bash");
                assert_eq!(req.input["command"], "mkdir probe");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_cancel_request() {
        let v = json!({"type": "control_cancel_request", "request_id": "cffb"});
        assert_eq!(
            parse_control(&v),
            Some(ResidentControl::Cancel {
                request_id: "cffb".to_string()
            })
        );
    }

    #[test]
    fn parse_control_response_success_and_error() {
        let ok = json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": "sm-1", "response": {"mode": "acceptEdits"}}
        });
        assert_eq!(
            parse_control(&ok),
            Some(ResidentControl::Response(ControlResponse {
                request_id: "sm-1".to_string(),
                success: true,
                error: None,
            }))
        );
        let err = json!({
            "type": "control_response",
            "response": {"subtype": "error", "request_id": "sm-2", "error": "boom"}
        });
        match parse_control(&err) {
            Some(ResidentControl::Response(resp)) => {
                assert!(!resp.success);
                assert_eq!(resp.error.as_deref(), Some("boom"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_control_ignores_non_control_events() {
        let v = json!({"type": "assistant", "message": {"content": []}});
        assert_eq!(parse_control(&v), None);
        let r = json!({"type": "result", "subtype": "success"});
        assert_eq!(parse_control(&r), None);
    }

    #[test]
    fn auto_allow_matches_bash_prefix_rule() {
        let input = json!({"command": "git push origin main"});
        // 完全一致するプレフィックスルールがあれば自動許可。
        assert!(tool_matches_rules(
            "Bash",
            &input,
            &["Bash(git push:*)".to_string()]
        ));
        // ルールが無ければ自動許可しない。
        assert!(!tool_matches_rules("Bash", &input, &[]));
        // 別コマンドのルールでは通らない。
        assert!(!tool_matches_rules(
            "Bash",
            &input,
            &["Bash(rm:*)".to_string()]
        ));
    }

    #[test]
    fn auto_allow_matches_compound_bash_only_when_all_present() {
        let input = json!({"command": "cd sub && rm -rf x"});
        // 片方のサブコマンドしか許可されていなければ自動許可しない。
        assert!(!tool_matches_rules(
            "Bash",
            &input,
            &["Bash(cd sub:*)".to_string()]
        ));
        // 両方揃えば自動許可。
        assert!(tool_matches_rules(
            "Bash",
            &input,
            &["Bash(cd sub:*)".to_string(), "Bash(rm:*)".to_string()]
        ));
    }

    #[test]
    fn auto_allow_matches_plain_tool_name() {
        let input = json!({"file_path": "/a.rs"});
        assert!(tool_matches_rules("Edit", &input, &["Edit".to_string()]));
        assert!(!tool_matches_rules("Edit", &input, &["Write".to_string()]));
    }

    #[test]
    fn drop_kills_child_process() {
        let Some(client) = ResidentClient::spawn_dummy() else {
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
}
