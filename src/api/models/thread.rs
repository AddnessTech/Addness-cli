use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// AIスレッド (Thread) — legacy V1 "AI エージェント" レイヤーのスレッドCRUD・
// チャット・アクショントレース・共有リンク・質問応答・ツール実行承認応答。
// — presentation/handlers/ai/{thread_handler,action_trace_handler,
//   question_handler,tool_confirmation_handler}.go
// — application/{requests,resources}/ai/*.go
// — presentation/routes/api.go の `/api/v1/team/ai/threads...`
//   (`auth := team.Group("")` → `ai := auth.Group("/ai")`)
//
// Go側ソースの冒頭コメントに "Deprecated. Use internal/aigoalchat /
// internal/aitodochat for new implementations." とあるが、goal-chat 等の
// ジェネリックチャットハンドラ (`internal/chat/handler`) が持たない機能
// （アクショントレース・取消、共有リンク、質問応答、ツール実行承認）を提供する
// 唯一の現行ルートであり、フロントエンドからも呼ばれている。
//
// レスポンスは（`traces` を除き）`{"data": ...}` エンベロープなしで直接
// JSONエンコードされる（`c.JSON(http.StatusOK, response)` — Go側で確認済み）。
// `GET .../threads/:id/traces` のみ `{"data": {"traces": [...]}}` で包まれる。
//
// SSE (`chat`/`edit-and-regenerate`) は goal decompose と同じ
// `infra/ai/streaming.SSEWriter` を使うため、`event:` 行を送らず
// `data: {"type": "...", ...}` のみを流す形式（goal-chat 等の
// ジェネリックチャットとは異なる）。イベント語彙も goal decompose と同じ
// `pkg/ai/streaming.EventType`（graph-run エージェントループ、~30種）。
// ---------------------------------------------------------------------------

/// `POST /api/v1/team/ai/threads` のリクエストボディ。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadCreateRequest {
    #[serde(default)]
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ai_agent_id: Option<String>,
}

/// `PATCH /api/v1/team/ai/threads/:id` のリクエストボディ。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadUpdateRequest {
    #[serde(default)]
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// `POST /api/v1/team/ai/threads/:id/chat` および
/// `PUT /api/v1/team/ai/threads/:threadId/messages/:messageId/edit-and-regenerate`
/// のリクエストボディ（後者は `content`/`mode`/mention系フィールドのみ使う
/// `MessageEditRequest` 相当）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadChatRequest {
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_objective_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_member_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_skill_ids: Vec<String>,
}

/// `PUT /api/v1/team/ai/threads/:threadId/messages/:messageId/edit-and-regenerate`
/// のリクエストボディ（`application/requests/ai/message_edit_request.go`）。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageEditRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_objective_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub mentioned_member_ids: Vec<String>,
}

/// 実行中Run情報（`Thread.Status == "busy"` の場合のみ付与）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveRunResponse {
    pub run_id: String,
    pub status: String,
    pub started_at: String,
}

/// Thread情報のAPIレスポンス（`application/resources/ai/thread_resource.go`
/// の `ThreadResponse`）。エンベロープなしで直接返る。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadResponse {
    pub id: String,
    #[serde(default)]
    pub organization_member_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub last_message_at: Option<String>,
    #[serde(default)]
    pub share_token: Option<String>,
    #[serde(default)]
    pub is_public: bool,
    #[serde(default)]
    pub message_count: i64,
    #[serde(default)]
    pub unread_count: i64,
    #[serde(default)]
    pub active_run: Option<ActiveRunResponse>,
    #[serde(default)]
    pub ai_agent_id: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// `GET /api/v1/team/ai/threads` のレスポンス（エンベロープなし）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadListResponse {
    #[serde(default)]
    pub threads: Vec<ThreadResponse>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

/// `POST /api/v1/team/ai/threads/:id/share` のレスポンス。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadShareLinkResponse {
    pub thread_id: String,
    pub share_token: String,
    pub is_public: bool,
    #[serde(default)]
    pub share_url: Option<String>,
}

/// Message情報のAPIレスポンス（`application/resources/ai/thread_resource.go`
/// の `MessageResponse`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessageResponse {
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Value,
    #[serde(default)]
    pub message_index: i64,
    #[serde(default)]
    pub tool_calls: Option<Value>,
    #[serde(default)]
    pub model_version: Option<String>,
    #[serde(default)]
    pub token_count_in: i64,
    #[serde(default)]
    pub token_count_out: i64,
    #[serde(default)]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
}

/// `GET /api/v1/team/ai/threads/:id/messages` のレスポンス（エンベロープなし）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadMessageListResponse {
    #[serde(default)]
    pub messages: Vec<ThreadMessageResponse>,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
    #[serde(default)]
    pub token_count_in: i64,
    #[serde(default)]
    pub token_count_out: i64,
}

/// アクショントレース1件（`presentation/handlers/ai/action_trace_types.go`
/// の `ActionTraceResponse`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTraceResponse {
    pub id: String,
    #[serde(default)]
    pub thread_id: String,
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub execution_time_ms: i64,
    #[serde(default)]
    pub reverted_at: Option<String>,
    #[serde(default)]
    pub reverted_by: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub is_revertible: bool,
}

/// `GET /api/v1/team/ai/threads/:id/traces` の内側（`{"data": {"traces":
/// [...]}}` — `ApiResponse<ActionTraceListResponse>` として受ける）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionTraceListResponse {
    #[serde(default)]
    pub traces: Vec<ActionTraceResponse>,
}

/// `POST /api/v1/team/ai/threads/:id/question/respond` のリクエストボディ。
/// `answer`（単一選択）か `answers`（複数選択）のどちらか一方を指定する。
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionRespondRequest {
    pub request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<String>,
}

/// `question/respond` / `tool-confirmation/respond` に共通のシンプルな
/// `{"success": bool, "message": "..."}` レスポンス。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadActionResultResponse {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub message: String,
}

/// `POST /api/v1/team/ai/threads/:id/tool-confirmation/respond` のリクエスト
/// ボディ。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolConfirmationRespondRequest {
    pub request_id: String,
    pub approved: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        MessageEditRequest, QuestionRespondRequest, ThreadChatRequest, ThreadCreateRequest,
        ThreadUpdateRequest, ToolConfirmationRespondRequest,
    };

    #[test]
    fn create_request_omits_absent_optionals() {
        let req = ThreadCreateRequest {
            title: "New chat".to_string(),
            metadata: None,
            ai_agent_id: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json, serde_json::json!({"title": "New chat"}));
    }

    #[test]
    fn create_request_includes_present_optionals() {
        let req = ThreadCreateRequest {
            title: "New chat".to_string(),
            metadata: Some(serde_json::json!({"k": "v"})),
            ai_agent_id: Some("agent-1".to_string()),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "title": "New chat",
                "metadata": {"k": "v"},
                "aiAgentId": "agent-1",
            })
        );
    }

    #[test]
    fn update_request_serializes_camel_case() {
        let req = ThreadUpdateRequest {
            title: "Renamed".to_string(),
            metadata: None,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json, serde_json::json!({"title": "Renamed"}));
    }

    #[test]
    fn chat_request_omits_empty_mention_lists() {
        let req = ThreadChatRequest {
            message: "hello".to_string(),
            mode: None,
            model: None,
            mentioned_objective_ids: vec![],
            mentioned_member_ids: vec![],
            mentioned_skill_ids: vec![],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json, serde_json::json!({"message": "hello"}));
    }

    #[test]
    fn chat_request_includes_present_fields() {
        let req = ThreadChatRequest {
            message: "hello".to_string(),
            mode: Some("hearing_mode".to_string()),
            model: Some("gpt-5.4".to_string()),
            mentioned_objective_ids: vec!["o1".to_string()],
            mentioned_member_ids: vec![],
            mentioned_skill_ids: vec![],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "message": "hello",
                "mode": "hearing_mode",
                "model": "gpt-5.4",
                "mentionedObjectiveIds": ["o1"],
            })
        );
    }

    #[test]
    fn message_edit_request_serializes_camel_case() {
        let req = MessageEditRequest {
            content: "edited".to_string(),
            mode: Some("hearing_mode".to_string()),
            mentioned_objective_ids: vec![],
            mentioned_member_ids: vec![],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"content": "edited", "mode": "hearing_mode"})
        );
    }

    #[test]
    fn question_respond_request_uses_answer_for_single_choice() {
        let req = QuestionRespondRequest {
            request_id: "q1".to_string(),
            answer: Some("yes".to_string()),
            answers: vec![],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"requestId": "q1", "answer": "yes"})
        );
    }

    #[test]
    fn question_respond_request_uses_answers_for_multi_choice() {
        let req = QuestionRespondRequest {
            request_id: "q1".to_string(),
            answer: None,
            answers: vec!["a".to_string(), "b".to_string()],
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"requestId": "q1", "answers": ["a", "b"]})
        );
    }

    #[test]
    fn tool_confirmation_request_serializes_camel_case() {
        let req = ToolConfirmationRespondRequest {
            request_id: "t1".to_string(),
            approved: true,
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"requestId": "t1", "approved": true})
        );
    }
}
