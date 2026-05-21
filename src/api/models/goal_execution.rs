use serde::{Deserialize, Serialize};

use super::{GoalStatus, Owner};

/// Response for the todays-goals endpoint
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaysGoalsData {
    pub nodes: Vec<TodaysGoalNode>,
    pub auto_generated_count: i32,
    pub collapsed_goal_ids: Vec<String>,
}

/// A single goal node in the today's goals response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaysGoalNode {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: i32,
    pub title: String,
    pub status: String,
    pub completed_at: Option<String>,
    pub order_no: f64,
    pub is_leaf: bool,
    pub has_recurring: bool,
    pub is_recurring: bool,
    pub kind: String,
    pub execution: Option<ExecutionRecord>,
    pub owner: Option<Owner>,
    pub is_direct_assignment: bool,
    #[serde(rename = "isAIRunning")]
    pub is_ai_running: Option<bool>,
}

impl TodaysGoalNode {
    /// Parse status string to GoalStatus enum
    pub fn parsed_status(&self) -> Option<GoalStatus> {
        match self.status.as_str() {
            "NONE" => Some(GoalStatus::None),
            "IN_PROGRESS" => Some(GoalStatus::InProgress),
            "CANCELLED" => Some(GoalStatus::Cancelled),
            "" => Some(GoalStatus::None),
            _ => None,
        }
    }

    /// Check if this goal is completed (has completed_at timestamp)
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}

/// Execution record attached to a goal
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRecord {
    pub id: String,
    pub objective_id: String,
    pub date: String, // YYYY-MM-DD
    pub status: GoalStatus,
    pub completed_at: Option<String>,
    pub created_at: Option<String>,
    pub due_at: Option<String>,
}

impl ExecutionRecord {
    /// Check if this execution is completed
    #[allow(dead_code)]
    pub fn is_completed(&self) -> bool {
        self.completed_at.is_some()
    }
}
