mod activity;
mod assignment;
mod chat;
mod comment;
mod consent;
mod core_values;
mod deliverable;
mod desktop_auth;
mod diagnosis;
mod goal;
mod goal_execution;
mod goalreport;
mod inlinemedia;
mod invitation;
mod issue;
mod master_plan;
mod meeting;
mod member;
mod notification;
mod org;
mod personal;
mod referral;
mod search;
mod streak;
mod user;

pub use activity::*;
pub use assignment::*;
pub use chat::*;
pub use comment::*;
pub use consent::*;
pub use core_values::*;
pub use deliverable::*;
pub use desktop_auth::*;
pub use diagnosis::*;
pub use goal::*;
pub use goal_execution::*;
pub use goalreport::*;
pub use inlinemedia::*;
pub use invitation::*;
pub use issue::*;
pub use master_plan::*;
pub use meeting::*;
pub use member::*;
pub use notification::*;
pub use org::*;
pub use personal::*;
pub use referral::*;
pub use search::*;
pub use streak::*;
pub use user::*;

use serde::{Deserialize, Serialize};

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
