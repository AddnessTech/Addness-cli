//! Codex owns its history storage. Query app-server instead of depending on
//! legacy rollout files, which do not describe every paginated thread.

use std::collections::HashSet;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

use super::CodexSessionCandidate;
use super::codex_appserver::{self, AppServerClient, ServerMessage};

const HISTORY_TIMEOUT: Duration = Duration::from_secs(10);

struct HistoryClient {
    client: AppServerClient,
    rx: Receiver<Value>,
    deadline: Instant,
}

impl HistoryClient {
    fn connect(bin: &Path, cwd: &str, home: Option<&Path>) -> Result<Self> {
        let mut command = Command::new(bin);
        command
            .arg("app-server")
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(home) = home {
            command.env("CODEX_HOME", home);
        }
        let mut child = command.spawn().context("Codex 履歴取得の起動に失敗")?;
        let stdout = child
            .stdout
            .take()
            .context("Codex 履歴 stdout がありません")?;
        let client = AppServerClient::new(child)?;
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if let Ok(value) = serde_json::from_str(&line)
                    && tx.send(value).is_err()
                {
                    break;
                }
            }
        });
        let mut connection = Self {
            client,
            rx,
            deadline: Instant::now() + HISTORY_TIMEOUT,
        };
        let id = connection.client.next_id();
        connection.request(codex_appserver::initialize_request(
            id,
            "addness-history",
            env!("CARGO_PKG_VERSION"),
        ))?;
        if !connection
            .client
            .send_value(&codex_appserver::initialized_notification())
        {
            bail!("Codex 履歴接続が閉じられました");
        }
        Ok(connection)
    }

    fn request(&mut self, request: Value) -> Result<Value> {
        let request_id = request["id"].as_u64().context("request id がありません")?;
        if !self.client.send_value(&request) {
            bail!("Codex 履歴リクエストの送信に失敗");
        }
        loop {
            let remaining = self.deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("Codex 履歴取得がタイムアウトしました");
            }
            let value = self
                .rx
                .recv_timeout(remaining)
                .context("Codex 履歴応答を受信できません")?;
            match codex_appserver::parse_message(&value) {
                Some(ServerMessage::Response { id, result, error }) if id == request_id => {
                    if let Some(error) = error {
                        bail!("Codex 履歴: {} ({})", error.message, error.code);
                    }
                    return result.context("Codex 履歴応答に result がありません");
                }
                Some(ServerMessage::UnhandledRequest { id, method }) => {
                    self.client.send_value(&json!({
                        "id": id, "error": {"code": -32601, "message": format!("Unsupported history request: {method}")}
                    }));
                }
                _ => {}
            }
        }
    }
}

fn thread_list_request(id: u64, cursor: Option<&str>, limit: usize) -> Value {
    json!({
        "id": id,
        "method": "thread/list",
        "params": {
            "cursor": cursor,
            "limit": limit.min(100),
            "sortKey": "updated_at",
            "sortDirection": "desc",
            "modelProviders": [],
            // The default only includes cli/vscode, omitting Addness's own threads.
            "sourceKinds": ["cli", "vscode", "exec", "appServer", "subAgent",
                "subAgentReview", "subAgentCompact", "subAgentThreadSpawn", "subAgentOther", "unknown"],
            "archived": false
        }
    })
}

fn collect_sessions(
    limit: usize,
    mut fetch: impl FnMut(Option<&str>, usize) -> Result<Value>,
) -> Result<Vec<CodexSessionCandidate>> {
    let mut sessions = Vec::new();
    let mut ids = HashSet::new();
    let mut cursors = HashSet::new();
    let mut cursor: Option<String> = None;
    while sessions.len() < limit {
        let page = fetch(cursor.as_deref(), limit - sessions.len())?;
        let data = page
            .get("data")
            .and_then(Value::as_array)
            .context("Codex 履歴応答に data がありません")?;
        for thread in data {
            let Some(id) = thread
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
            else {
                continue;
            };
            if !ids.insert(id.to_string()) {
                continue;
            }
            let title = thread
                .get("name")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .or_else(|| {
                    thread
                        .get("preview")
                        .and_then(Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or("untitled")
                .to_string();
            let updated_at = thread
                .get("updatedAt")
                .and_then(Value::as_i64)
                .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
                .map(|date| date.to_rfc3339())
                .unwrap_or_default();
            sessions.push(CodexSessionCandidate {
                id: id.to_string(),
                title,
                updated_at,
                cwd: thread
                    .get("cwd")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });
            if sessions.len() == limit {
                break;
            }
        }
        cursor = page
            .get("nextCursor")
            .and_then(Value::as_str)
            .map(str::to_string);
        let Some(next) = &cursor else { break };
        if !cursors.insert(next.clone()) {
            bail!("Codex 履歴のページカーソルが循環しています");
        }
    }
    Ok(sessions)
}

pub(super) fn load_sessions(
    bin: &Path,
    cwd: &str,
    home: Option<&Path>,
    limit: usize,
) -> Result<Vec<CodexSessionCandidate>> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut connection = HistoryClient::connect(bin, cwd, home)?;
    collect_sessions(limit, |cursor, remaining| {
        let id = connection.client.next_id();
        connection.request(thread_list_request(id, cursor, remaining))
    })
}

pub(super) fn rename_session(bin: &Path, cwd: &str, thread_id: &str, title: &str) -> Result<()> {
    let mut connection = HistoryClient::connect(bin, cwd, None)?;
    let id = connection.client.next_id();
    connection.request(json!({"id": id, "method": "thread/name/set", "params": {"threadId": thread_id, "name": title}}))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginated_history_includes_all_sources_and_follows_opaque_cursors() {
        let mut calls = 0;
        let sessions = collect_sessions(3, |cursor, remaining| {
            calls += 1;
            if calls == 1 {
                assert_eq!(cursor, None);
                assert_eq!(remaining, 3);
                Ok(json!({"data": [{"id": "new", "name": "Named", "updatedAt": 10, "cwd": "/repo"}], "nextCursor": "opaque/+=="}))
            } else {
                assert_eq!(cursor, Some("opaque/+=="));
                assert_eq!(remaining, 2);
                Ok(json!({"data": [{"id": "new"}, {"id": "old", "preview": "Preview"}, {"id": "third"}], "nextCursor": null}))
            }
        }).unwrap();
        assert_eq!(calls, 2);
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.title.as_str())
                .collect::<Vec<_>>(),
            ["Named", "Preview", "untitled"]
        );
        assert_eq!(sessions[0].cwd.as_deref(), Some("/repo"));
        assert_eq!(sessions[0].updated_at, "1970-01-01T00:00:10+00:00");
        let request = thread_list_request(1, None, 200);
        assert_eq!(request["params"]["limit"], 100);
        assert!(
            request["params"]["sourceKinds"]
                .as_array()
                .unwrap()
                .contains(&json!("appServer"))
        );
        assert!(
            request["params"]["sourceKinds"]
                .as_array()
                .unwrap()
                .contains(&json!("exec"))
        );
    }

    #[test]
    fn history_stops_at_limit_and_rejects_repeated_cursors() {
        let sessions = collect_sessions(1, |_, _| {
            Ok(json!({"data": [{"id": "one"}, {"id": "two"}], "nextCursor": "next"}))
        })
        .unwrap();
        assert_eq!(sessions.len(), 1);
        assert!(collect_sessions(2, |_, _| Ok(json!({"data": [], "nextCursor": "same"}))).is_err());
        assert!(collect_sessions(1, |_, _| Ok(json!({}))).is_err());
    }
    struct ProbeDirectory(std::path::PathBuf);
    impl ProbeDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("addness-history-probe-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for ProbeDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    #[ignore = "実 codex が必要。認証不要、外部モデルへのリクエストなし"]
    fn upstream_probe_codex_current_model_catalog() {
        let root = ProbeDirectory::new();
        let bin = std::env::var("ADDNESS_PROBE_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
        let mut connection =
            HistoryClient::connect(Path::new(&bin), root.0.to_str().unwrap(), Some(&root.0))
                .unwrap();
        let id = connection.client.next_id();
        let models = connection
            .request(json!({"id": id, "method": "model/list", "params": {}}))
            .unwrap();
        let astra = models["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["model"] == "gpt-6-astra")
            .expect("Astra is missing from the current Codex catalog");
        for effort in ["max", "ultra"] {
            assert!(
                astra["supportedReasoningEfforts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["reasoningEffort"] == effort)
            );
        }
    }

    #[test]
    #[ignore = "実 codex が必要。認証不要、モデルへのリクエストなし"]
    fn upstream_probe_codex_thread_approval_policies() {
        let root = ProbeDirectory::new();
        let bin = std::env::var("ADDNESS_PROBE_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
        let mut connection =
            HistoryClient::connect(Path::new(&bin), root.0.to_str().unwrap(), Some(&root.0))
                .unwrap();
        for policy in [None, Some("untrusted"), Some("on-request"), Some("never")] {
            let id = connection.client.next_id();
            let config = codex_appserver::ThreadConfig {
                cwd: Some(root.0.to_string_lossy().into_owned()),
                approval_policy: policy.map(str::to_string),
                sandbox: Some("read-only".to_string()),
                ..Default::default()
            };
            let mut request = codex_appserver::thread_start_request(id, &config);
            request["params"]["ephemeral"] = json!(true);
            let response = connection.request(request).unwrap();
            if let Some(policy) = policy {
                assert_eq!(response["approvalPolicy"], policy);
            }
        }
    }

    #[test]
    #[ignore = "実 codex が必要。モデル通信はローカル HTTP fixture のみ"]
    fn upstream_probe_codex_paginated_history_and_rename() {
        use std::io::{Read, Write};
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        };
        // Reject model requests locally. The user turn is still persisted by Codex,
        // allowing us to exercise both storage contracts without credentials/usage.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        struct ServerGuard {
            stop: Arc<AtomicBool>,
            worker: Option<std::thread::JoinHandle<()>>,
        }
        impl Drop for ServerGuard {
            fn drop(&mut self) {
                self.stop.store(true, Ordering::Relaxed);
                if let Some(worker) = self.worker.take() {
                    let _ = worker.join();
                }
            }
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let _server = ServerGuard {
            stop,
            worker: Some(std::thread::spawn(move || {
                while !stopped.load(Ordering::Relaxed) {
                    if let Ok((mut stream, _)) = listener.accept() {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let mut buffer = [0; 8192];
                        let _ = stream.read(&mut buffer);
                        let body = r#"{"error":{"message":"local compatibility fixture","type":"invalid_request_error"}}"#;
                        let _ = write!(
                            stream,
                            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: application/json\r\n\r\n{body}",
                            body.len()
                        );
                    } else {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                }
            })),
        };
        let root = ProbeDirectory::new();
        let bin = std::env::var("ADDNESS_PROBE_CODEX_BIN").unwrap_or_else(|_| "codex".to_string());
        let cwd = root.0.to_str().unwrap();
        let mut connection = HistoryClient::connect(Path::new(&bin), cwd, Some(&root.0)).unwrap();
        let mut expected_ids = HashSet::new();
        for mode in ["legacy", "paginated"] {
            let id = connection.client.next_id();
            let created = connection.request(json!({"id": id, "method": "thread/start", "params": {
                "cwd": cwd, "historyMode": mode, "approvalPolicy": "never", "sandbox": "read-only", "model": "probe",
                "config": {"model_provider": "addness_probe", "model_providers.addness_probe": {
                    "name": "Local fixture", "base_url": format!("http://{address}"), "wire_api": "responses",
                    "request_max_retries": 0, "stream_max_retries": 0, "requires_openai_auth": false
                }}
            }})).unwrap();
            let thread_id = created["thread"]["id"].as_str().unwrap().to_string();
            assert_eq!(created["thread"]["historyMode"], mode);
            expected_ids.insert(thread_id.clone());
            let id = connection.client.next_id();
            connection
                .request(codex_appserver::turn_start_request(
                    id,
                    &thread_id,
                    "offline fixture",
                    None,
                    &[],
                ))
                .unwrap();
            loop {
                let event = connection
                    .rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("local fixture turn did not complete");
                if event["method"] == "turn/completed" {
                    break;
                }
            }
            let id = connection.client.next_id();
            connection.request(json!({"id": id, "method": "thread/name/set", "params": {"threadId": thread_id, "name": format!("fixture {mode}")}})).unwrap();
        }
        let sessions = collect_sessions(2, |cursor, _| {
            let id = connection.client.next_id();
            connection.request(thread_list_request(id, cursor, 1))
        })
        .unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.id.clone())
                .collect::<HashSet<_>>(),
            expected_ids
        );
        assert!(sessions.iter().any(|s| s.title == "fixture paginated"));
        drop(connection);
        // A fresh connection sees the persisted names and both history formats.
        let sessions = load_sessions(Path::new(&bin), cwd, Some(&root.0), 2).unwrap();
        assert_eq!(
            sessions
                .iter()
                .map(|s| s.id.clone())
                .collect::<HashSet<_>>(),
            expected_ids
        );
        assert!(sessions.iter().any(|s| s.title == "fixture legacy"));
    }
}
