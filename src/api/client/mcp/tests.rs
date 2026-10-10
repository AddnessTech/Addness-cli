use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

use serde_json::{Value, json};

use super::{ApiClient, McpScope};

// 本番データを触らず、HTTPの経路・認証・JSON-RPC・ページ送りを一緒に検証する。
pub(crate) fn server(
    replies: Vec<(&'static str, String)>,
) -> (ApiClient, thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = thread::spawn(move || {
        let mut requests = Vec::new();
        for (headers, body) in replies {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = stream.read(&mut buffer).unwrap();
                assert!(count > 0, "request ended early");
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length: ")
                                .and_then(|s| s.parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            write!(
                stream,
                "HTTP/1.1 {headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        requests
    });
    (
        ApiClient::new("test-token", &url)
            .unwrap()
            .with_org_id(Some("test-org".to_string())),
        task,
    )
}

fn response(id: u64, result: Value) -> (&'static str, String) {
    (
        "200 OK\r\nContent-Type: application/json",
        json!({"jsonrpc":"2.0", "id": id, "result":result}).to_string(),
    )
}

fn initialize() -> (&'static str, String) {
    response(
        1,
        json!({"protocolVersion":"2025-06-18", "capabilities":{"tools":{}}, "serverInfo":{"name":"test","version":"1"}}),
    )
}

fn notification() -> (&'static str, String) {
    ("202 Accepted", String::new())
}

#[tokio::test]
async fn mcp_discovers_future_tools_and_all_pages() {
    let (client, task) = server(vec![
        initialize(),
        notification(),
        response(
            2,
            json!({"tools":[{"name":"future_tool","inputSchema":{"type":"object"}}], "nextCursor":"page 2"}),
        ),
        response(3, json!({"tools":[{"name":"another_tool"}]})),
    ]);
    let mut session = client.connect_mcp(McpScope::Organization).await.unwrap();
    let result = session.list_tools().await.unwrap();
    assert_eq!(result["tools"].as_array().unwrap().len(), 2);
    let requests = task.join().unwrap();
    for request in &requests {
        assert!(request.starts_with("POST /mcp HTTP/1.1"));
        assert!(request.contains("authorization: Bearer test-token"));
        assert!(request.contains("x-organization-id: test-org"));
        assert!(request.contains("accept: application/json, text/event-stream"));
    }
    assert!(requests[3].contains("\"cursor\":\"page 2\""));
}

#[tokio::test]
async fn mcp_preserves_sse_structured_errors_and_session_headers() {
    let init = initialize().1;
    let result = json!({"isError":true,"content":[{"type":"text","text":"revision conflict"}],"structuredContent":{"revision":3}});
    let sse = format!(
        "event: message\ndata: {{\"jsonrpc\":\"2.0\",\"method\":\"notifications/progress\",\"params\":{{}}}}\n\nevent: message\ndata: {}\n\n",
        response(2, result.clone()).1
    );
    let (client, task) = server(vec![
        (
            "200 OK\r\nContent-Type: application/json\r\nMcp-Session-Id: session-1",
            init,
        ),
        notification(),
        ("200 OK\r\nContent-Type: text/event-stream", sse),
        ("204 No Content", String::new()),
    ]);
    let mut session = client.connect_mcp(McpScope::Personal).await.unwrap();
    let actual = session
        .call_tool("future_write", json!({"body":"日本語\n本文"}))
        .await
        .unwrap();
    assert_eq!(actual, result);
    session.close().await;
    let requests = task.join().unwrap();
    assert!(requests[0].starts_with("POST /mcp/personal "));
    assert!(!requests[0].contains("x-organization-id"));
    assert!(requests[2].contains("mcp-session-id: session-1"));
    assert!(requests[2].contains("mcp-protocol-version: 2025-06-18"));
    assert!(requests[3].starts_with("DELETE /mcp/personal "));
}

#[tokio::test]
async fn mcp_rejects_repeated_cursor_and_mismatched_response_id() {
    let (client, task) = server(vec![
        initialize(),
        notification(),
        response(2, json!({"tools":[],"nextCursor":"same"})),
        response(3, json!({"tools":[],"nextCursor":"same"})),
    ]);
    let mut session = client.connect_mcp(McpScope::Organization).await.unwrap();
    assert!(
        session
            .list_tools()
            .await
            .unwrap_err()
            .to_string()
            .contains("did not advance")
    );
    task.join().unwrap();
    let (client, task) = server(vec![
        initialize(),
        notification(),
        response(100, json!({"content":[]})),
    ]);
    let mut session = client.connect_mcp(McpScope::Organization).await.unwrap();
    assert!(
        session
            .call_tool("test", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("ID did not match")
    );
    task.join().unwrap();
}

#[tokio::test]
async fn mcp_http_and_rpc_failures_are_errors() {
    let (client, task) = server(vec![
        initialize(),
        notification(),
        (
            "403 Forbidden\r\nContent-Type: text/html",
            "<html>blocked</html>".to_string(),
        ),
    ]);
    let mut session = client.connect_mcp(McpScope::Organization).await.unwrap();
    let error = format!(
        "{:#}",
        session.call_tool("test", json!({})).await.unwrap_err()
    );
    assert!(error.contains("WAF"));
    assert!(!error.contains("<html>"));
    assert_eq!(task.join().unwrap().len(), 3);
    let (client, task) = server(vec![
        initialize(),
        notification(),
        (
            "200 OK\r\nContent-Type: application/json",
            json!({"jsonrpc":"2.0", "id":2,"error":{"code":-32602,"message":"invalid arguments"}})
                .to_string(),
        ),
    ]);
    let mut session = client.connect_mcp(McpScope::Organization).await.unwrap();
    assert!(
        session
            .call_tool("test", json!({}))
            .await
            .unwrap_err()
            .to_string()
            .contains("invalid arguments")
    );
    task.join().unwrap();
}
