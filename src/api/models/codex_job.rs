use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

// ---------------------------------------------------------------------------
// Codexジョブ (クラウドCodexエージェントセッション)
// — internal/codex/handler/handler.go, internal/codex/usecase/{service,types}.go
//
// v1 (`/api/v1/codex/jobs`) と v2 (`/api/v2/codex/jobs`) は同一Handler/DTOを
// 共有しており、差異はv2にのみ `Delete`（論理削除）ルートが追加登録されている点
// のみ（presentation/routes/api.go）。v2はv1の完全な上位互換のため、本CLIは
// v2エンドポイントのみを実装する（v1は対象外リストの「v1レガシー重複」扱い）。
//
// 成功レスポンスはすべて `BaseHandler.JSON` 経由で
// `{"data": ..., "message": "success"}` にラップされる
// （internal/common/http/base_handler.go）。202/204系（Input/Close/Cancel/
// Delete）はボディなし。
// ---------------------------------------------------------------------------

/// ジョブの実行ステータス（`internal/codex/usecase/types.go`）。
/// `queued`/`running`/`idle`/`cancel_requested` はアクティブ（未終了）、
/// それ以外はterminal。将来値の追加に備え `Other` でフォワード互換を保つ
/// （serdeの`#[serde(untagged)]`はコンテナ単位の属性でバリアント単位には
/// 使えないため、素の文字列との相互変換を手動実装する）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexJobStatus {
    Queued,
    Running,
    Idle,
    Succeeded,
    Failed,
    CancelRequested,
    Cancelled,
    Closed,
    Other(String),
}

impl CodexJobStatus {
    /// アクティブ（未終了）なジョブか（`isActiveJobStatus`と同じ判定）。
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            CodexJobStatus::Queued
                | CodexJobStatus::Running
                | CodexJobStatus::Idle
                | CodexJobStatus::CancelRequested
        )
    }

    fn as_str(&self) -> &str {
        match self {
            CodexJobStatus::Queued => "queued",
            CodexJobStatus::Running => "running",
            CodexJobStatus::Idle => "idle",
            CodexJobStatus::Succeeded => "succeeded",
            CodexJobStatus::Failed => "failed",
            CodexJobStatus::CancelRequested => "cancel_requested",
            CodexJobStatus::Cancelled => "cancelled",
            CodexJobStatus::Closed => "closed",
            CodexJobStatus::Other(raw) => raw,
        }
    }
}

impl From<&str> for CodexJobStatus {
    fn from(raw: &str) -> Self {
        match raw {
            "queued" => CodexJobStatus::Queued,
            "running" => CodexJobStatus::Running,
            "idle" => CodexJobStatus::Idle,
            "succeeded" => CodexJobStatus::Succeeded,
            "failed" => CodexJobStatus::Failed,
            "cancel_requested" => CodexJobStatus::CancelRequested,
            "cancelled" => CodexJobStatus::Cancelled,
            "closed" => CodexJobStatus::Closed,
            other => CodexJobStatus::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for CodexJobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for CodexJobStatus {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CodexJobStatus {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(CodexJobStatus::from(raw.as_str()))
    }
}

/// `POST /api/v2/codex/jobs` (`createJobRequest`)。
/// `prompt` は必須タグなし（空なら backend 側で "Codex session" にデフォルト）。
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexJobCreateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_scope: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_job_id: Option<String>,
}

/// `POST /api/v2/codex/jobs/:id/input` (`inputRequest`)。`prompt` は
/// `binding:"required"` — 必須。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexJobInputRequest {
    pub prompt: String,
}

/// `jobResponse`（`GET/POST .../jobs`, `.../jobs/:id`, `.../resume` の共通形）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexJob {
    pub id: String,
    pub organization_id: String,
    pub requested_by_user_id: String,
    pub requested_by_member_id: String,
    pub status: CodexJobStatus,
    #[serde(default)]
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_scope: Option<Map<String, Value>>,
    #[serde(default)]
    pub runner_id: String,
    #[serde(default)]
    pub error_message: String,
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    pub updated_at: String,
}

/// `jobsResponse` (`GET /api/v2/codex/jobs`)。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodexJobListResponse {
    #[serde(default)]
    pub jobs: Vec<CodexJob>,
}

#[cfg(test)]
mod tests {
    use super::CodexJobStatus;

    #[test]
    fn status_round_trips_known_values() {
        for (raw, status, active) in [
            ("queued", CodexJobStatus::Queued, true),
            ("running", CodexJobStatus::Running, true),
            ("idle", CodexJobStatus::Idle, true),
            ("succeeded", CodexJobStatus::Succeeded, false),
            ("failed", CodexJobStatus::Failed, false),
            ("cancel_requested", CodexJobStatus::CancelRequested, true),
            ("cancelled", CodexJobStatus::Cancelled, false),
            ("closed", CodexJobStatus::Closed, false),
        ] {
            let parsed = CodexJobStatus::from(raw);
            assert_eq!(parsed, status);
            assert_eq!(parsed.is_active(), active);
            assert_eq!(serde_json::to_string(&parsed).unwrap(), format!("{raw:?}"));
            let deserialized: CodexJobStatus = serde_json::from_str(&format!("{raw:?}")).unwrap();
            assert_eq!(deserialized, status);
        }
    }

    #[test]
    fn status_falls_back_to_other_for_unknown_values() {
        let parsed = CodexJobStatus::from("paused");
        assert_eq!(parsed, CodexJobStatus::Other("paused".to_string()));
        assert!(!parsed.is_active());
        assert_eq!(serde_json::to_string(&parsed).unwrap(), "\"paused\"");
    }
}
