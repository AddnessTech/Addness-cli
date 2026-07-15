mod activity;
mod assignment;
mod comment;
mod deliverable;
mod goal;
mod goal_execution;
mod invitation;
mod kpi;
mod member;
mod notification;
mod org;
mod streak;

pub use activity::*;
pub use assignment::*;
pub use comment::*;
pub use deliverable::*;
pub use goal::*;
pub use goal_execution::*;
pub use invitation::*;
pub use kpi::*;
pub use member::*;
pub use notification::*;
pub use org::*;
pub use streak::*;

use serde::{Deserialize, Serialize};

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
