mod activity;
mod api_key;
mod assignment;
mod chat;
mod codex_job;
mod comment;
mod consent;
mod core_values;
mod deliverable;
mod desktop_auth;
mod diagnosis;
mod goal;
mod goal_chat;
mod goal_execution;
mod goalreport;
mod inlinemedia;
mod invitation;
mod invoice;
mod issue;
mod kpi;
mod master_plan;
mod meeting;
mod member;
mod notification;
mod org;
mod personal;
mod referral;
mod search;
mod sharetree;
mod skill;
mod streak;
mod thread;
mod todo_chat;
mod tool;
mod user;

pub use activity::*;
pub use api_key::*;
pub use assignment::*;
pub use chat::*;
pub use codex_job::*;
pub use comment::*;
pub use consent::*;
pub use core_values::*;
pub use deliverable::*;
pub use desktop_auth::*;
pub use diagnosis::*;
pub use goal::*;
pub use goal_chat::*;
pub use goal_execution::*;
pub use goalreport::*;
pub use inlinemedia::*;
pub use invitation::*;
pub use invoice::*;
pub use issue::*;
pub use kpi::*;
pub use master_plan::*;
pub use meeting::*;
pub use member::*;
pub use notification::*;
pub use org::*;
pub use personal::*;
pub use referral::*;
pub use search::*;
pub use sharetree::*;
pub use skill::*;
pub use streak::*;
pub use thread::*;
pub use todo_chat::*;
pub use tool::*;
pub use user::*;

use serde::{Deserialize, Serialize};

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
