use serde_json::{Value, json};

use super::mcp::tests::server;
use super::{ListAllCommentsParams, SearchQueryParams};

fn reply(value: Value) -> (&'static str, String) {
    (
        "200 OK\r\nContent-Type: application/json",
        value.to_string(),
    )
}

fn comment() -> Value {
    json!({"id":"comment-1","content":"body","commentableType":"objective",
        "commentableId":"goal-1","author":{"id":"member-1","name":"Tester","isAIAgent":true},
        "createdAt":"2026-10-11T00:00:00Z","updatedAt":"2026-10-11T00:00:00Z"})
}

fn node() -> Value {
    json!({"id":"node-1","name":"PR #1","kind":"link","url":"https://example.test/pr/1",
        "parentId":"folder-1","goals":[{"objectiveId":"goal-1"}]})
}

fn body(request: &str) -> Value {
    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap()
}

#[tokio::test]
async fn current_comments_use_v2_bare_responses_and_preserve_long_text_and_mentions() {
    let (client, task) = server(vec![
        reply(comment()),
        reply(comment()),
        reply(comment()),
        reply(comment()),
        reply(json!({"comments":[comment()],"totalCount":1})),
    ]);
    let content = "長".repeat(4_001);
    let created = client
        .create_comment_with_options(
            "goal-1",
            &content,
            Some("parent-1".to_string()),
            vec!["member-1".to_string()],
        )
        .await
        .unwrap();
    assert_eq!(created.id(), "comment-1");
    assert!(created.author.is_ai_agent);
    client.get_comment("comment-1").await.unwrap();
    client
        .update_comment("comment-1", "updated", vec!["member-2".to_string()])
        .await
        .unwrap();
    client.resolve_comment("comment-1").await.unwrap();
    let page = client
        .list_all_comments(ListAllCommentsParams {
            author_id: "member-1",
            resolved: Some(false),
            limit: Some(20),
            offset: Some(40),
        })
        .await
        .unwrap();
    assert_eq!(page.total_count, 1);
    let requests = task.join().unwrap();
    assert!(requests[0].starts_with("POST /api/v2/objectives/goal-1/comments "));
    assert_eq!(body(&requests[0])["content"], content);
    assert_eq!(body(&requests[0])["mentions"], json!(["member-1"]));
    assert_eq!(body(&requests[0])["parentId"], "parent-1");
    assert!(body(&requests[0]).get("commentableId").is_none());
    assert!(requests[1].starts_with("GET /api/v2/comments/comment-1 "));
    assert!(requests[2].starts_with("PUT /api/v2/comments/comment-1 "));
    assert!(requests[3].starts_with("PATCH /api/v2/comments/comment-1/resolve "));
    assert!(
        requests[4].starts_with(
            "GET /api/v2/comments?author_id=member-1&resolved=false&limit=20&offset=40 "
        )
    );
}

#[tokio::test]
async fn current_meeting_bot_preserves_drive_output_and_uses_recording_stop() {
    let response = json!({"id":"job-1","meetingUrl":"https://meet.example.test/meeting",
        "botName":"Addness","status":"pending","createdAt":"2026-10-11T00:00:00Z",
        "updatedAt":"2026-10-11T00:00:00Z","driveFolderId":"folder-1","videoStatus":"pending"});
    let (client, task) = server(vec![reply(response), ("202 Accepted", String::new())]);
    let created = client
        .create_meeting_bot_job(&crate::api::MeetingBotJobCreateRequest {
            meeting_url: "https://meet.example.test/meeting".to_string(),
            meeting_title: Some("Review".to_string()),
            drive_folder_id: Some("folder-1".to_string()),
            objective_id: None,
            record_video: Some(false),
        })
        .await
        .unwrap();
    assert_eq!(created.id, "job-1");
    assert_eq!(created.details["driveFolderId"], "folder-1");
    assert_eq!(created.details["videoStatus"], "pending");
    client.stop_meeting_bot_job("job-1").await.unwrap();
    let requests = task.join().unwrap();
    assert_eq!(
        body(&requests[0]),
        json!({"meetingUrl":"https://meet.example.test/meeting",
        "meetingTitle":"Review","driveFolderId":"folder-1","recordVideo":false})
    );
    assert!(requests[1].starts_with("POST /api/v1/team/meeting-bot/jobs/job-1/stop "));
}

#[tokio::test]
async fn current_pr_links_use_drive_goal_folder_and_return_created_id() {
    let (client, task) = server(vec![
        reply(json!({"nodeId":"folder-1"})),
        reply({
            let mut created = node();
            created["goals"] = Value::Null;
            created
        }),
        reply(json!({"items":[node()],"total":1})),
    ]);
    let created = client
        .create_link_deliverable("goal-1", "https://example.test/pr/1", "org/repo#1")
        .await
        .unwrap();
    assert_eq!(created.data.id, "node-1");
    assert_eq!(created.data.objective_id, "goal-1");
    let list = client.get_goal_deliverables("goal-1").await.unwrap();
    assert_eq!(
        list.data.deliverables[0].link_url.as_deref(),
        Some("https://example.test/pr/1")
    );
    let requests = task.join().unwrap();
    assert!(requests[0].starts_with("POST /api/v2/drive/nodes/goal-folder "));
    assert_eq!(body(&requests[0]), json!({"objectiveId":"goal-1"}));
    assert!(requests[1].starts_with("POST /api/v2/drive/nodes/links "));
    assert_eq!(
        body(&requests[1]),
        json!({"name":"org／repo#1","url":"https://example.test/pr/1","parentId":"folder-1"})
    );
    assert!(
        requests[2]
            .starts_with("GET /api/v2/drive/nodes/search?objectiveId=goal-1&limit=100&page=1 ")
    );
}

#[tokio::test]
async fn current_drive_lists_all_pages_and_rejects_stalled_pagination() {
    let mut second = node();
    second["id"] = json!("node-2");
    let (client, task) = server(vec![
        reply(json!({"items":[node()],"total":2})),
        reply(json!({"items":[second],"total":2})),
    ]);
    let result = client.get_goal_deliverables("goal-1").await.unwrap();
    assert_eq!(result.data.deliverables.len(), 2);
    assert_eq!(result.data.deliverables[1].id, "node-2");
    assert!(task.join().unwrap()[1].contains("&page=2 "));
    let (client, task) = server(vec![
        reply(json!({"items":[node()],"total":2})),
        reply(json!({"items":[],"total":2})),
    ]);
    assert!(
        client
            .get_goal_deliverables("goal-1")
            .await
            .unwrap_err()
            .to_string()
            .contains("before all")
    );
    task.join().unwrap();
}

#[tokio::test]
async fn timed_out_writes_are_not_replayed_while_reads_remain_retryable() {
    use std::net::TcpListener;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;
    for (method, expected) in [(reqwest::Method::POST, 1), (reqwest::Method::GET, 3)] {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let finished = Arc::new(AtomicBool::new(false));
        let stop = finished.clone();
        let server = std::thread::spawn(move || {
            let mut requests = Vec::new();
            loop {
                match listener.accept() {
                    Ok((socket, _)) => requests.push(socket), // Accept the write but never acknowledge it.
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if stop.load(Ordering::SeqCst) {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(error) => panic!("{error}"),
                }
            }
            requests.len()
        });
        let request = reqwest::Client::new()
            .request(method.clone(), &url)
            .timeout(Duration::from_millis(100))
            .json(&json!({"value": 1}));
        let result = super::ApiClient::send_request(request, &url).await;
        finished.store(true, Ordering::SeqCst);
        let received = server.join().unwrap();
        assert!(result.is_err());
        assert_eq!(received, expected, "{method}");
    }
}

#[tokio::test]
async fn current_drive_rejects_wrong_goal_before_mutation() {
    let (client, task) = server(vec![reply(node())]);
    let error = client
        .delete_deliverable("other-goal", "node-1")
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not linked"));
    assert_eq!(task.join().unwrap().len(), 1);
}

#[tokio::test]
async fn current_drive_trashes_only_after_all_targets_are_verified() {
    let (client, task) = server(vec![reply(node()), reply(json!({"affectedCount":1}))]);
    client.delete_deliverable("goal-1", "node-1").await.unwrap();
    let requests = task.join().unwrap();
    assert!(requests[0].starts_with("GET /api/v2/drive/nodes/node-1 "));
    assert!(requests[1].starts_with("POST /api/v2/drive/nodes/trash "));
    assert_eq!(body(&requests[1]), json!({"nodeIds":["node-1"]}));
}

#[tokio::test]
async fn current_share_reads_wrapped_v2_response() {
    let (client, task) = server(vec![reply(
        json!({"data":{"shareUrl":"https://example.test/shared","publicId":"public-1"}}),
    )]);
    let result = client.create_share_link("goal-1").await.unwrap();
    assert_eq!(result.public_id.as_deref(), Some("public-1"));
    assert!(task.join().unwrap()[0].starts_with("POST /api/v2/objectives/goal-1/share "));
}

#[tokio::test]
async fn current_search_uses_authorized_v2_route() {
    // 現行検索は既存の data wrapper を保つ。
    let (client, task) = server(vec![reply(
        json!({"data":{"items":[{"type":"objective","data":{"id":"goal-1"}}],"hasMore":true}}),
    )]);
    let result = client
        .unified_search(SearchQueryParams {
            query: "hello",
            organization_id: "test-org",
            limit: None,
            offset: None,
        })
        .await
        .unwrap();
    assert!(result.has_more);
    assert_eq!(result.items[0].kind, "objective");
    let requests = task.join().unwrap();
    assert!(requests[0].starts_with("GET /api/v2/search?q=hello&organizationId=test-org "));
    assert!(requests[0].contains("x-organization-id: test-org"));
}
