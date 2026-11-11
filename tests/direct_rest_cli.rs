use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::process::{Command, Output};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use serde_json::{Value, json};

const ID: &str = "11111111-2222-4333-8444-555555555555";

struct Fixture {
    url: String,
    cache: std::path::PathBuf,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    server: Option<std::thread::JoinHandle<()>>,
}

impl Fixture {
    fn new(replies: Vec<(&'static str, Value)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let finished = stop.clone();
        let server = std::thread::spawn(move || {
            let mut replies = VecDeque::from(replies);
            while !finished.load(Ordering::SeqCst) {
                let (mut socket, _) = match listener.accept() {
                    Ok(socket) => socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2));
                        continue;
                    }
                    Err(error) => panic!("{error}"),
                };
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = Vec::new();
                loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                recorded
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(bytes).unwrap());
                let (status, body) = replies
                    .pop_front()
                    .unwrap_or(("500 Unexpected request", json!({})));
                let body = body.to_string();
                write!(socket, "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        Self {
            url,
            requests,
            stop,
            server: Some(server),
            cache: std::env::temp_dir().join(format!("addness-rest-{}", uuid::Uuid::new_v4())),
        }
    }

    fn run(&self, token: &str, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_addness"))
            .args(args)
            .arg("--json")
            .env("ADDNESS_API_TOKEN", token)
            .env("ADDNESS_API_URL", &self.url)
            .env("ADDNESS_ORG_ID", ID)
            .env("ADDNESS_CACHE_DIR", &self.cache)
            .output()
            .unwrap()
    }

    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.lines().next().unwrap().to_string())
            .collect()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let result = self.server.take().unwrap().join();
        if !std::thread::panicking() {
            result.unwrap();
        }
        let _ = std::fs::remove_dir_all(&self.cache);
    }
}

#[test]
fn native_reads_make_one_rest_request_each_and_preserve_full_json_and_cursors() {
    let detail = json!({"data":{"id":ID,"body":"長".repeat(12_000),"version":"v1"}});
    let rows = json!({"data":{"rows":[{"name":"日本語"}],"nextCursor":"opaque+/="}});
    let fixture = Fixture::new(vec![
        ("200 OK", detail.clone()),
        ("200 OK", rows.clone()),
        ("200 OK", json!({"total":101,"items":[{"id":ID}]})),
        ("200 OK", json!({"data":{"list":{"id":ID}}})),
        ("200 OK", json!({"data":{"lists":[]}})),
    ]);
    let output = fixture.run("test-token", &["goal", "detail", ID]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        detail
    );
    let output = fixture.run(
        "test-token",
        &["crm", "rows", ID, "--cursor", "opaque+/=", "--limit", "20"],
    );
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        rows
    );
    assert!(
        fixture
            .run(
                "test-token",
                &[
                    "member",
                    "objectives",
                    ID,
                    "--offset",
                    "100",
                    "--limit",
                    "20",
                    "--org",
                    ID
                ]
            )
            .status
            .success()
    );
    assert!(
        fixture
            .run("test-token", &["crm", "get", ID])
            .status
            .success()
    );
    assert!(
        fixture
            .run(
                "test-token",
                &[
                    "crm",
                    "list",
                    "--query",
                    "商談 & X",
                    "--folder-path",
                    "",
                    "--org",
                    ID
                ]
            )
            .status
            .success()
    );
    assert_eq!(
        fixture.paths(),
        vec![
            format!("GET /api/v2/objectives/{ID}?include_version=true HTTP/1.1"),
            format!(
                "GET /api/v2/crm/lists/{ID}/rows?limit=20&filter=all&cursor=opaque%2B%2F%3D HTTP/1.1"
            ),
            format!(
                "GET /api/v2/members/{ID}/objectives?limit=20&offset=100&include_completed=false&include_counts=false HTTP/1.1"
            ),
            format!("GET /api/v2/crm/lists/{ID} HTTP/1.1"),
            "GET /api/v2/crm/lists?limit=50&q=%E5%95%86%E8%AB%87+%26+X&folder_path= HTTP/1.1"
                .to_string(),
        ]
    );
    assert!(
        fixture
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.contains(&format!("x-organization-id: {ID}")))
    );
}

#[test]
fn quick_memo_list_preserves_status_and_cursor_with_one_scoped_rest_request() {
    let page = json!({
        "items": [{"id": ID, "content": "日本語のメモ", "status": "ready", "revision": 2,
                   "recommendations": [{"goal_id": ID}]}],
        "next_cursor": "next+/="
    });
    let fixture = Fixture::new(vec![("200 OK", json!({"data": page.clone()}))]);
    let organization = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let output = fixture.run(
        "test-token",
        &[
            "quick-memo",
            "list",
            "--cursor",
            "opaque+/=",
            "--limit",
            "5",
            "--org",
            organization,
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        page
    );
    assert_eq!(
        fixture.paths(),
        vec!["GET /api/v2/quick-memos?limit=5&cursor=opaque%2B%2F%3D HTTP/1.1"]
    );
    assert!(
        fixture.requests.lock().unwrap()[0].contains(&format!("x-organization-id: {organization}"))
    );
}

#[test]
fn a_429_blocks_later_processes_before_network_but_not_other_api_keys() {
    let fixture = Fixture::new(vec![
        (
            "429 Too Many Requests\r\nRetry-After: 120",
            json!({"error":"rate limited"}),
        ),
        ("200 OK", json!({"data":{"id":ID}})),
    ]);
    let first = fixture.run("limited-token", &["crm", "get", ID]);
    assert!(!first.status.success());
    assert!(String::from_utf8_lossy(&first.stderr).contains("120s"));
    for _ in 0..3 {
        let next = fixture.run("limited-token", &["goal", "detail", ID]);
        assert!(!next.status.success());
        assert!(String::from_utf8_lossy(&next.stderr).contains("No request was sent"));
    }
    assert_eq!(fixture.paths().len(), 1);
    assert!(
        fixture
            .run("other-token", &["goal", "detail", ID])
            .status
            .success()
    );
    assert_eq!(fixture.paths().len(), 2);
}

#[test]
fn parallel_cli_processes_stop_successful_polling_before_the_server_limits_them() {
    let fixture = Fixture::new(vec![("200 OK", json!({"data":{"id":ID}})); 81]);
    let outputs = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let fixture = &fixture;
                scope.spawn(move || {
                    (0..10)
                        .map(|_| fixture.run("polling-token", &["goal", "detail", ID]))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(
        outputs
            .iter()
            .filter(|output| output.status.success())
            .count(),
        60
    );
    for output in outputs.iter().filter(|output| !output.status.success()) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("Local request budget"), "{stderr}");
        assert!(stderr.contains("No request was sent"), "{stderr}");
    }
    assert_eq!(fixture.paths().len(), 60);
    // コマンドを変えても同じ上限が適用され、別キーは巻き込まない。
    assert!(
        !fixture
            .run("polling-token", &["crm", "get", ID])
            .status
            .success()
    );
    assert_eq!(fixture.paths().len(), 60);
    assert!(
        fixture
            .run("other-token", &["crm", "get", ID])
            .status
            .success()
    );
    assert_eq!(fixture.paths().len(), 61);
}

#[test]
fn cache_failures_stop_the_cli_before_network() {
    let fixture = Fixture::new(vec![]);
    std::fs::write(&fixture.cache, "not a directory").unwrap();
    assert!(
        !fixture
            .run("test-token", &["goal", "detail", ID])
            .status
            .success()
    );
    assert!(fixture.paths().is_empty());
    std::fs::remove_file(&fixture.cache).unwrap();
}

#[test]
fn removed_mcp_and_invalid_native_arguments_fail_without_network() {
    let fixture = Fixture::new(vec![]);
    for args in [
        vec!["mcp", "call", "get_goal"],
        vec!["crm", "get", "bad-id"],
        vec!["crm", "rows", ID, "--limit", "0"],
        vec!["quick-memo", "list", "--limit", "0"],
        vec!["quick-memo", "list", "--limit", "51"],
        vec!["member", "objectives", ID, "--created-after", "yesterday"],
    ] {
        assert!(!fixture.run("test-token", &args).status.success());
    }
    assert!(fixture.paths().is_empty());
}

#[test]
fn attachment_download_keeps_api_credentials_off_the_file_host_and_preserves_existing_files() {
    let body = json!({"attachment":"日本語の添付"});
    let assets = Fixture::new(vec![("200 OK", body.clone())]);
    let api = Fixture::new(vec![(
        "200 OK",
        json!({"data":{"url":format!("{}/image", assets.url)}}),
    )]);
    std::fs::create_dir_all(&api.cache).unwrap();
    let output_path = api.cache.join("image.png");
    let args = [
        "chat",
        "attachment",
        "--room",
        ID,
        "--message",
        ID,
        "--attachment",
        ID,
        "--output",
        output_path.to_str().unwrap(),
    ];
    let output = api.run("private-api-token", &args);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        body.to_string().as_bytes()
    );
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["bytes"], body.to_string().len());
    assert_eq!(
        api.paths(),
        [format!(
            "GET /api/v2/chat/rooms/{ID}/messages/{ID}/attachments/{ID}/url HTTP/1.1"
        )]
    );
    let asset_request = &assets.requests.lock().unwrap()[0];
    assert!(!asset_request.contains("authorization:"));
    assert!(!asset_request.contains("x-organization-id:"));
    assert!(!api.run("private-api-token", &args).status.success());
    assert_eq!(api.paths().len(), 1);
    assert_eq!(
        std::fs::read(&output_path).unwrap(),
        body.to_string().as_bytes()
    );
}

#[test]
fn attachment_failure_removes_the_incomplete_file() {
    let api = Fixture::new(vec![("404 Not Found", json!({"error":"not found"}))]);
    std::fs::create_dir_all(&api.cache).unwrap();
    let output_path = api.cache.join("image.png");
    let output = api.run(
        "test-token",
        &[
            "chat",
            "attachment",
            "--room",
            ID,
            "--message",
            ID,
            "--attachment",
            ID,
            "--output",
            output_path.to_str().unwrap(),
        ],
    );
    assert!(!output.status.success());
    assert!(!output_path.exists());
}
