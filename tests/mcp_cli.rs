use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Stdio};

use serde_json::{Value, json};

#[test]
fn mcp_cli_stdin_preserves_json_error_and_exits_unsuccessfully() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let mut calls = Vec::new();
        for index in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let (end, size) = loop {
                let mut chunk = [0; 4096];
                let count = socket.read(&mut chunk).unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let size = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap();
                    if bytes.len() >= end + 4 + size {
                        break (end + 4, size);
                    }
                }
            };
            let request: Value = serde_json::from_slice(&bytes[end..end + size]).unwrap();
            calls.push(request.clone());
            let (status, response) = match index {
                0 => ("200 OK", json!({"jsonrpc":"2.0","id":request["id"],"result":{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"fixture","version":"1"}}}).to_string()),
                1 => ("202 Accepted", String::new()),
                _ => ("200 OK", json!({"jsonrpc":"2.0","id":request["id"],"result":{"isError":true,"content":[{"type":"text","text":"revision conflict"}],"structuredContent":{"expectedRevision":4}}}).to_string()),
            };
            write!(socket,"HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}",response.len()).unwrap();
        }
        calls
    });
    let mut process = Command::new(env!("CARGO_BIN_EXE_addness"))
        .args(["mcp", "call", "future_tool", "--args-file", "-", "--json"])
        .env("ADDNESS_API_TOKEN", "test-token")
        .env("ADDNESS_API_URL", url)
        .env("ADDNESS_ORG_ID", "test-org")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    process
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"revision":3,"enabled":false,"items":[1,null]}"#)
        .unwrap();
    let output = process.wait_with_output().unwrap();
    assert!(!output.status.success());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["isError"], true);
    assert_eq!(result["structuredContent"]["expectedRevision"], 4);
    let calls = server.join().unwrap();
    assert_eq!(
        calls[2]["params"]["arguments"],
        json!({"revision":3,"enabled":false,"items":[1,null]})
    );
    assert_eq!(calls[2]["params"]["name"], "future_tool");
}

#[test]
fn mcp_cli_invalid_arguments_fail_before_network_access() {
    for invalid in ["[]", "null", "{broken"] {
        let output = Command::new(env!("CARGO_BIN_EXE_addness"))
            .args(["mcp", "call", "future_tool", "--args", invalid, "--json"])
            .env("ADDNESS_API_TOKEN", "test-token")
            .env("ADDNESS_API_URL", "http://127.0.0.1:1")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("JSON"), "{error}");
        assert!(!error.contains("MCP request failed"), "{error}");
    }
}
