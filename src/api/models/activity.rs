use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// GET /api/v1/team/organizations/:id/activity-logs/by-member
// GET /api/v1/team/organizations/:id/activity-logs/objectives/:goalId
// レスポンスは `{"data": ..., "message": "success"}` でラップされる
// （presentation/handlers/team.BaseHandler.RespondWithJSON がラップするため）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogListResponse {
    #[serde(default)]
    pub items: Vec<ActivityLogItem>,
    #[serde(default)]
    pub total_count: i64,
    #[serde(default)]
    pub limit: i32,
    #[serde(default)]
    pub offset: i32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogItem {
    pub id: String,
    pub event_type: String,
    pub event_category: String,
    pub occurred_at: String,
    pub actor: ActivityLogActor,
    pub target: ActivityLogTarget,
    #[serde(default)]
    pub goal_info: Option<ActivityLogGoalInfo>,
    #[serde(default)]
    pub kpi_info: Option<ActivityLogKpiInfo>,
    #[serde(default)]
    pub deliverable_info: Option<ActivityLogDeliverableInfo>,
    #[serde(default)]
    pub ai_info: Option<ActivityLogAiInfo>,
    #[serde(default)]
    pub value_change: Option<ActivityLogValueChange>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogActor {
    pub id: String,
    pub organization_member_id: String,
    pub name: String,
    #[serde(default)]
    pub avatar_url: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogTarget {
    pub entity_type: String,
    pub entity_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogGoalInfo {
    pub goal_id: String,
    pub goal_title: String,
    #[serde(default)]
    pub parent_goal_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogKpiInfo {
    pub kpi_id: String,
    pub kpi_title: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogDeliverableInfo {
    pub deliverable_id: String,
    pub deliverable_title: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogAiInfo {
    pub session_id: String,
    #[serde(default)]
    pub message_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogValueChange {
    pub before_value: String,
    pub after_value: String,
}

// GET /api/v1/team/organizations/:id/activity-logs/summary
// `{"data": ..., "message": "success"}` でラップされる。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogSummaryResponse {
    #[serde(default)]
    pub total_count: i64,
    #[serde(default)]
    pub count_by_category: HashMap<String, i64>,
    #[serde(default)]
    pub most_active_members: Vec<ActivityLogMemberActivity>,
    #[serde(default)]
    pub recent_activities: Vec<ActivityLogItem>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityLogMemberActivity {
    pub member_id: String,
    pub member_name: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub count: i64,
}

// GET /api/v2/organizations/:id/activity-logs/objectives/:goalId/summary
// レスポンスは `{"data": ..., "message": "success"}` でラップされる
// （internal/activitylogパッケージのcommonhttp.BaseHandler経由のため）。
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalActivitySummaryResponse {
    #[serde(default)]
    pub members: Vec<GoalActivityMemberSummary>,
    #[serde(default)]
    pub created_total: i64,
    #[serde(default)]
    pub completed_total: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalActivityMemberSummary {
    pub member_id: String,
    pub member_name: String,
    #[serde(default)]
    pub avatar_url: String,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub completed: i64,
}

#[cfg(test)]
mod tests {
    use super::{ActivityLogListResponse, ActivityLogSummaryResponse, GoalActivitySummaryResponse};
    use crate::api::ApiResponse;

    #[test]
    fn activity_log_list_response_parses_wrapped_envelope() {
        let json = serde_json::json!({
            "data": {
                "items": [{
                    "id": "log-1",
                    "eventType": "objective.create",
                    "eventCategory": "objective",
                    "occurredAt": "2026-07-15T10:00:00Z",
                    "actor": {
                        "id": "user-1",
                        "organizationMemberId": "member-1",
                        "name": "Alice",
                        "avatarUrl": ""
                    },
                    "target": {"entityType": "objective", "entityId": "goal-1"},
                    "goalInfo": {"goalId": "goal-1", "goalTitle": "テストゴール"},
                    "description": "created",
                    "createdAt": "2026-07-15T10:00:00Z"
                }],
                "totalCount": 1,
                "limit": 50,
                "offset": 0
            },
            "message": "success"
        });

        let resp: ApiResponse<ActivityLogListResponse> = serde_json::from_value(json).unwrap();

        assert_eq!(resp.data.total_count, 1);
        assert_eq!(resp.data.items.len(), 1);
        let item = &resp.data.items[0];
        assert_eq!(item.event_type, "objective.create");
        assert_eq!(item.actor.name, "Alice");
        assert_eq!(item.goal_info.as_ref().unwrap().goal_title, "テストゴール");
        assert!(item.kpi_info.is_none());
    }

    #[test]
    fn activity_log_summary_response_parses_categories_and_members() {
        let json = serde_json::json!({
            "totalCount": 12,
            "countByCategory": {"objective": 10, "kpi": 2},
            "mostActiveMembers": [{
                "memberId": "member-1",
                "memberName": "Alice",
                "avatarUrl": "",
                "count": 8
            }],
            "recentActivities": []
        });

        let resp: ActivityLogSummaryResponse = serde_json::from_value(json).unwrap();

        assert_eq!(resp.total_count, 12);
        assert_eq!(resp.count_by_category.get("objective"), Some(&10));
        assert_eq!(resp.most_active_members[0].count, 8);
    }

    #[test]
    fn goal_activity_summary_response_parses_member_counts() {
        let json = serde_json::json!({
            "members": [{
                "memberId": "member-1",
                "memberName": "Alice",
                "avatarUrl": "",
                "created": 3,
                "completed": 1
            }],
            "createdTotal": 3,
            "completedTotal": 1
        });

        let resp: GoalActivitySummaryResponse = serde_json::from_value(json).unwrap();

        assert_eq!(resp.created_total, 3);
        assert_eq!(resp.members[0].completed, 1);
    }
}
