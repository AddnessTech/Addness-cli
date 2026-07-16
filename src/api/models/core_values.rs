use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AIコアバリュー診断チャット (Core Values Chat) — ツールなし純対話のヒアリング
// エージェントとの対話（15〜20分のコアバリュー診断ワーク）
// — internal/chat/handler/{chat,threads}.go, internal/aicorevalues/wire.go
// — presentation/routes/api.go の `/api/v2/ai-core-values/...`（v2Auth配下）
//
// goal-chat/todo-chat/master-plan と同一のジェネリックハンドラ
// （internal/chat/handler）を共有する。todo-chatとの差異は無く、以下の点で
// goal-chatとは異なる（todo-chatと同一の振る舞い）:
//   - `openGoalId` は不要（単一ゴールに紐づかない。送っても無視される）
//   - `opening: true` を新規スレッドに対して送ると「口火」（パネルを開いた
//     直後の自動起動）を実行できる。message は空でよい。opening は既存
//     スレッド（threadId指定）とは併用不可
//   - SSEに `goal` イベントは流れない（ゴールスコープではないため）
//   - `GoalScopeAuthorizer` は無く、encouragement 相当の追加エンドポイントも無い
//   - `/stream` にAIチャット用レート制限ミドルウェアが付く
//   - `threads` はページング未対応（`internal/aicorevalues/chat/agent.go`の
//     `RuntimeAgent` が `runtime.ThreadPageLister` を実装していない。
//     `Kind`/`Stream`/`ValidateTurnInput`/`ListThreads`/`Messages` のみ）。
//     `page` クエリを送っても常にレガシーな配列形式 `{"data": [...]}` が
//     返る（Go側実装で確認済み、todo-chatと同一の教訓）
// ---------------------------------------------------------------------------

/// `POST /api/v2/ai-core-values/stream` のリクエストボディ。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreValuesChatStreamRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub message: String,
    pub opening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_from_message_id: Option<String>,
}

/// `GET /api/v2/ai-core-values/threads` の1件。レスポンス本体は
/// `{"data": [CoreValuesThread, ...]}`（ページングなしの単純配列）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreValuesThread {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_message_at: Option<String>,
}

/// `GET /api/v2/ai-core-values/threads/:threadId/messages` の1件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreValuesMessage {
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
    use super::{CoreValuesChatStreamRequest, CoreValuesMessage};

    #[test]
    fn stream_request_omits_absent_optionals_but_keeps_opening() {
        let req = CoreValuesChatStreamRequest {
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
        let req = CoreValuesChatStreamRequest {
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
        let message: CoreValuesMessage = serde_json::from_value(serde_json::json!({
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
        let message: CoreValuesMessage = serde_json::from_value(serde_json::json!({
            "id": "m-1",
            "role": "user",
            "content": "hi",
        }))
        .unwrap();
        assert_eq!(message.created_at, None);
    }
}
