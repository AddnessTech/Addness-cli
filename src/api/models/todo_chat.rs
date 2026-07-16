use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AI今日のToDoチャット (Todo Chat) — 「今日のToDoを一緒に作る壁打ち」でのAI対話
// — internal/chat/handler/{chat,threads}.go, internal/aitodochat/wire.go
// — presentation/routes/api.go の `/api/v2/ai-todo-chat/...`（v2Auth配下）
//
// goal-chat/core-values/master-plan と同一のジェネリックハンドラ
// （internal/chat/handler）を共有するが、Todo Chatは以下の点で固有:
//   - `openGoalId` は不要（単一ゴールに紐づかない。送っても無視される）
//   - `opening: true` を新規スレッドに対して送ると「口火」（パネルを開いた
//     直後の自動起動）を実行できる。message は空でよい。opening は既存
//     スレッド（threadId指定）とは併用不可
//   - SSEに `goal` イベントは流れない（ゴールスコープではないため）
//   - `GoalScopeAuthorizer` は無く、encouragement 相当の追加エンドポイントも無い
//   - `/stream` にAIチャット用レート制限ミドルウェアが付く
//   - `threads` はページング未対応（runtime.Agent が `ThreadPageLister` を
//     実装していない）。`page` クエリを送っても常にレガシーな配列形式
//     `{"data": [...]}` が返る（本番での実測で確認済み）。goal-chat の
//     `{"data": {"threads":[...],"meta":{...}}}` 形式とは異なる
// ---------------------------------------------------------------------------

/// `POST /api/v2/ai-todo-chat/stream` のリクエストボディ。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoChatStreamRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub message: String,
    pub opening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_from_message_id: Option<String>,
}

/// `GET /api/v2/ai-todo-chat/threads` の1件。レスポンス本体は
/// `{"data": [TodoChatThread, ...]}`（ページングなしの単純配列）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoChatThread {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_message_at: Option<String>,
}

/// `GET /api/v2/ai-todo-chat/threads/:threadId/messages` の1件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodoChatMessage {
    pub id: String,
    /// `"user"` または `"assistant"`。
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub created_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{TodoChatMessage, TodoChatStreamRequest};

    #[test]
    fn stream_request_omits_absent_optionals_but_keeps_opening() {
        let req = TodoChatStreamRequest {
            reasoning: None,
            thread_id: None,
            message: String::new(),
            opening: true,
            edit_from_message_id: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "message": "",
                "opening": true,
            })
        );
    }

    #[test]
    fn stream_request_includes_present_optionals() {
        let req = TodoChatStreamRequest {
            reasoning: Some("high".to_string()),
            thread_id: Some("thread-1".to_string()),
            message: "hello".to_string(),
            opening: false,
            edit_from_message_id: Some("msg-1".to_string()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "reasoning": "high",
                "threadId": "thread-1",
                "message": "hello",
                "opening": false,
                "editFromMessageId": "msg-1",
            })
        );
    }

    #[test]
    fn message_deserializes_extra_created_at_field() {
        let message: TodoChatMessage = serde_json::from_value(serde_json::json!({
            "id": "m-1",
            "role": "assistant",
            "content": "hi",
            "createdAt": "2026-07-04T06:25:14.06415Z",
        }))
        .unwrap();
        assert_eq!(
            message.created_at.as_deref(),
            Some("2026-07-04T06:25:14.06415Z")
        );
    }

    #[test]
    fn message_deserializes_without_created_at() {
        let message: TodoChatMessage = serde_json::from_value(serde_json::json!({
            "id": "m-1",
            "role": "user",
            "content": "hi",
        }))
        .unwrap();
        assert_eq!(message.created_at, None);
    }
}
