mod activity;
mod assignment;
mod chat;
mod codex_job;
mod comment;
mod deliverable;
mod diagnosis;
mod goal;
mod goal_execution;
mod goalreport;
mod inlinemedia;
mod invitation;
mod invoice;
mod issue;
mod kpi;
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
mod tool;
mod user;

pub use activity::*;
pub use assignment::*;
pub use chat::*;
pub use codex_job::*;
pub use comment::*;
pub use deliverable::*;
pub use diagnosis::*;
pub use goal::*;
pub use goal_execution::*;
pub use goalreport::*;
pub use inlinemedia::*;
pub use invitation::*;
pub use invoice::*;
pub use issue::*;
pub use kpi::*;
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
pub use tool::*;
pub use user::*;

use serde::{Deserialize, Serialize};

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
