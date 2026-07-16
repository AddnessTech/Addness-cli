use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AIマスタープランチャット (Master Plan Chat) — 中長期の方針・優先順位を壁打ちする
// エージェントとの対話
// — internal/chat/handler/{chat,threads}.go, internal/aimasterplan/{wire,chat}.go
// — presentation/routes/api.go の `/api/v2/ai-master-plan/...`（v2Auth配下）
//
// goal-chat/todo-chat/core-values と同一のジェネリックハンドラ
// （internal/chat/handler）を共有する。core-valuesとリクエスト/レスポンス形は
// 完全に同一（Go側 `internal/aimasterplan/chat/types.go` の `Input` に
// `OpenGoalID` は無く、`internal/chat/handler/handler.go` の
// `requireObjectiveID` も master-plan では false = ゴール非紐づき）。
// core-valuesとの差異は以下の点のみ:
//   - `opening: true` を新規スレッドに対して送ると「口火」を実行できる
//     （core-valuesと同様。message は空でよい）
//   - `RuntimeAgent`（`internal/aimasterplan/chat/agent.go`）も
//     `runtime.ThreadPageLister` を実装していない。`threads` はページング
//     未対応で、常にレガシーな配列形式 `{"data": [...]}` が返る
//     （Go側実装で確認済み、core-values/todo-chatと同一の教訓）
// ---------------------------------------------------------------------------

/// `POST /api/v2/ai-master-plan/stream` のリクエストボディ。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterPlanChatStreamRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub message: String,
    pub opening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_from_message_id: Option<String>,
}

/// `GET /api/v2/ai-master-plan/threads` の1件。レスポンス本体は
/// `{"data": [MasterPlanThread, ...]}`（ページングなしの単純配列）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterPlanThread {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_message_at: Option<String>,
}

/// `GET /api/v2/ai-master-plan/threads/:threadId/messages` の1件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MasterPlanMessage {
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
    use super::{MasterPlanChatStreamRequest, MasterPlanMessage};

    #[test]
    fn stream_request_omits_absent_optionals_but_keeps_opening() {
        let req = MasterPlanChatStreamRequest {
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
        let req = MasterPlanChatStreamRequest {
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
        let message: MasterPlanMessage = serde_json::from_value(serde_json::json!({
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
        let message: MasterPlanMessage = serde_json::from_value(serde_json::json!({
            "id": "m-1",
            "role": "user",
            "content": "hi",
        }))
        .unwrap();
        assert_eq!(message.created_at, None);
    }
}
