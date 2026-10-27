use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::Command;

use serde_json::{Value, json};

const ORG: &str = "79e5df38-ffbb-4a95-9d41-f5968427c653";
const OTHER: &str = "11111111-2222-4333-8444-555555555555";
const RUN: &str = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";

fn mock(status: &'static str, response: Value) -> (String, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 4096];
            let count = socket.read(&mut chunk).unwrap();
            assert!(count > 0);
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("content-length: ")
                            .and_then(|s| s.parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let body = response.to_string();
        write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        String::from_utf8(bytes).unwrap()
    });
    (url, task)
}

#[test]
fn explicit_org_scopes_list_and_preserves_json() {
    let (url, task) = mock(
        "200 OK",
        json!({"remaining":1,"challenges":[{"key":"crm_list"}]}),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_addness"))
        .args(["challenge", "list", "--org", ORG, "--json"])
        .env("ADDNESS_API_TOKEN", "test-token")
        .env("ADDNESS_API_URL", url)
        .env("ADDNESS_ORG_ID", OTHER)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["challenges"][0]["key"], "crm_list");
    let request = task.join().unwrap();
    assert!(request.starts_with(&format!(
        "GET /api/v2/organizations/{ORG}/challenges?platform=pc&defs=3 "
    )));
    assert!(request.contains(&format!("x-organization-id: {ORG}")));
}

#[test]
fn complete_propagates_unsatisfied_steps_as_failure() {
    let (url, task) = mock(
        "409 Conflict",
        json!({"error":"まだ満たされていない手順があります","unsatisfiedSteps":["record"]}),
    );
    let output = Command::new(env!("CARGO_BIN_EXE_addness"))
        .args([
            "challenge",
            "complete",
            "recording_capture",
            RUN,
            "--manual-step",
            "see_library",
            "--json",
        ])
        .env("ADDNESS_API_TOKEN", "test-token")
        .env("ADDNESS_API_URL", url)
        .env("ADDNESS_ORG_ID", ORG)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("record"));
    let request = task.join().unwrap();
    let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body, json!({"manualSteps":["see_library"]}));
}
