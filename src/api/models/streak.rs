use serde::{Deserialize, Serialize};

// GET /api/v2/organizations/:id/members/:memberId/streak
// レスポンスは `{"data": ..., "message": "success"}` でラップされる
// （internal/memberパッケージのcommonhttp.BaseHandler経由のため）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Streak {
    #[serde(default)]
    pub streak_count: i32,
    #[serde(default)]
    pub total_working_days: i32,
    #[serde(default)]
    pub days: Vec<StreakDay>,
    /// 自分自身のストリークを見ている場合のみ意味を持つ（他人の場合は常にfalse）。
    #[serde(default)]
    pub revive_available: bool,
    /// 自分自身のストリークを見ている場合のみ意味を持つ（他人の場合は常にfalse）。
    #[serde(default)]
    pub revivable: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StreakDay {
    pub date: String,
    pub state: StreakDayState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreakDayState {
    None,
    Completed,
    Frozen,
}

impl StreakDayState {
    pub fn as_str(self) -> &'static str {
        match self {
            StreakDayState::None => "none",
            StreakDayState::Completed => "completed",
            StreakDayState::Frozen => "frozen",
        }
    }
}

// GET /api/v2/organizations/:id/members/:memberId/streak/share
// POST /api/v2/organizations/:id/members/:memberId/streak/share
// レスポンスは `{"data": ..., "message": "success"}` でラップされる
// （presentation/handlers/team.BaseHandler.RespondWithJSON がラップするため）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreakShareStatus {
    pub member_id: String,
    #[serde(default)]
    pub share_token: Option<String>,
    #[serde(default)]
    pub is_public: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StreakShareLink {
    pub member_id: String,
    pub share_token: String,
    #[serde(default)]
    pub is_public: bool,
}

// POST /api/v2/organizations/:id/members/:memberId/streak/freeze
// `{"data": ..., "message": "success"}` でラップされる。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FreezeResult {
    pub member_id: String,
    pub date: String,
    #[serde(default)]
    pub frozen: bool,
}

// POST /api/v2/organizations/:id/members/:memberId/streak/revive
// `{"data": ..., "message": "success"}` でラップされる。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviveResult {
    pub member_id: String,
    pub revived_date: String,
    #[serde(default)]
    pub revived: bool,
}

// GET /api/v1/public/streaks/:token — 認証不要（共有トークンのみ）。
// 素のJSON（presentation/handlers/public経由でラップされない）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicStreakResponse {
    #[serde(default)]
    pub streak_count: i32,
    #[serde(default)]
    pub total_working_days: i32,
    #[serde(default)]
    pub week_days: Vec<WeekDayStatus>,
    #[serde(default)]
    pub member_name: String,
    #[serde(default)]
    pub avatar_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WeekDayStatus {
    pub date: String,
    #[serde(default)]
    pub completed: bool,
    #[serde(default)]
    pub frozen: bool,
}

#[cfg(test)]
mod tests {
    use super::{PublicStreakResponse, Streak, StreakDayState, StreakShareStatus};
    use crate::api::ApiResponse;

    #[test]
    fn streak_parses_wrapped_envelope_with_day_states() {
        let json = serde_json::json!({
            "data": {
                "streakCount": 3,
                "totalWorkingDays": 59,
                "days": [
                    {"date": "2026-07-13", "state": "completed"},
                    {"date": "2026-07-14", "state": "frozen"},
                    {"date": "2026-07-15", "state": "none"}
                ],
                "reviveAvailable": true,
                "revivable": false
            },
            "message": "success"
        });

        let resp: ApiResponse<Streak> = serde_json::from_value(json).unwrap();

        assert_eq!(resp.data.streak_count, 3);
        assert_eq!(resp.data.total_working_days, 59);
        assert_eq!(resp.data.days[0].state, StreakDayState::Completed);
        assert_eq!(resp.data.days[1].state, StreakDayState::Frozen);
        assert_eq!(resp.data.days[2].state, StreakDayState::None);
        assert!(resp.data.revive_available);
        assert!(!resp.data.revivable);
    }

    #[test]
    fn streak_share_status_parses_null_token() {
        let json = serde_json::json!({
            "memberId": "member-1",
            "shareToken": null,
            "isPublic": false
        });

        let status: StreakShareStatus = serde_json::from_value(json).unwrap();

        assert_eq!(status.member_id, "member-1");
        assert!(status.share_token.is_none());
        assert!(!status.is_public);
    }

    #[test]
    fn public_streak_response_parses_bare_json() {
        let json = serde_json::json!({
            "streakCount": 5,
            "totalWorkingDays": 40,
            "weekDays": [
                {"date": "2026-07-14", "completed": true, "frozen": false},
                {"date": "2026-07-15", "completed": false, "frozen": true}
            ],
            "memberName": "Alice",
            "avatarUrl": ""
        });

        let resp: PublicStreakResponse = serde_json::from_value(json).unwrap();

        assert_eq!(resp.streak_count, 5);
        assert_eq!(resp.member_name, "Alice");
        assert!(resp.week_days[0].completed);
        assert!(resp.week_days[1].frozen);
    }
}
