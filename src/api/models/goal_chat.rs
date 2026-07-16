use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AIゴールチャット (Goal Chat) — ゴール文脈でのAIエージェント対話
// — internal/chat/handler/{chat,threads}.go, internal/aigoalchat/{wire,encouragement}.go
// — presentation/routes/api.go の `/api/v2/ai-goal-chat/...`（v2Auth配下）
//
// todo-chat/core-values/master-plan と同一のジェネリックハンドラ
// （internal/chat/handler）を共有するが、Goal Chatは以下の点で固有:
//   - `openGoalId` が必須（対象ゴールをスコープする）
//   - `opening` は常に false（トップレベルの「開始」演出は他系統専用）
//   - SSEに `goal` イベント（{id,title}）が追加で流れる
//   - `GoalScopeAuthorizer` によるゴール可視性チェックがあり、権限がなければ404
//   - `/stream` にAIレート制限ミドルウェアが付いていない（他3系統にはある）
// `encouragement` はGoal Chat専用の追加エンドポイントで、通常のJSON応答
// （SSEではない）。
// ---------------------------------------------------------------------------

/// `POST /api/v2/ai-goal-chat/stream` のリクエストボディ。
/// `opening` はGoal Chatでは常に `false` を送る仕様（バックエンドの
/// `binding` 上は必須フィールドではないが、明示的に送ることで
/// 他系統向けの「開始」演出を誤って起動しないようにする）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalChatStreamRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    pub open_goal_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    pub message: String,
    pub opening: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit_from_message_id: Option<String>,
}

/// `GET /api/v2/ai-goal-chat/encouragement` のレスポンスペイロード
/// (`{"data": {"message": "..."}}"`)。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalChatEncouragement {
    pub message: String,
}

/// `GET /api/v2/ai-goal-chat/threads` の1件（ページ形式レスポンスの要素）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalChatThread {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub last_message_at: Option<String>,
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub goal_title: Option<String>,
}

/// `GET /api/v2/ai-goal-chat/threads` のページネーションメタ情報。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoalChatThreadsMeta {
    #[serde(default)]
    pub more: bool,
    #[serde(default)]
    pub remaining_count: i64,
}

/// `GET /api/v2/ai-goal-chat/threads?page=...` のデータ本体。
/// 本CLIは常に `page` クエリを付けてリクエストするため、レガシーな
/// 配列形式（`page` 省略時のみ）は扱わない。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GoalChatThreadsData {
    #[serde(default)]
    pub threads: Vec<GoalChatThread>,
    #[serde(default)]
    pub meta: GoalChatThreadsMeta,
}

/// `GET /api/v2/ai-goal-chat/threads/:threadId/messages` の1件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalChatMessage {
    pub id: String,
    /// `"user"` または `"assistant"`。
    pub role: String,
    #[serde(default)]
    pub content: String,
}

#[cfg(test)]
mod tests {
    use super::{GoalChatStreamRequest, GoalChatThreadsData};

    #[test]
    fn stream_request_omits_absent_optionals_but_keeps_opening() {
        let req = GoalChatStreamRequest {
            reasoning: None,
            open_goal_id: "goal-1".to_string(),
            thread_id: None,
            message: "hello".to_string(),
            opening: false,
            edit_from_message_id: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "openGoalId": "goal-1",
                "message": "hello",
                "opening": false,
            })
        );
    }

    #[test]
    fn stream_request_includes_present_optionals() {
        let req = GoalChatStreamRequest {
            reasoning: Some("high".to_string()),
            open_goal_id: "goal-1".to_string(),
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
                "openGoalId": "goal-1",
                "threadId": "thread-1",
                "message": "hello",
                "opening": false,
                "editFromMessageId": "msg-1",
            })
        );
    }

    #[test]
    fn threads_data_defaults_to_empty() {
        let data: GoalChatThreadsData = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(data.threads.is_empty());
        assert!(!data.meta.more);
        assert_eq!(data.meta.remaining_count, 0);
    }
}
