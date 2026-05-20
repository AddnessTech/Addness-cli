mod assignment;
mod comment;
mod deliverable;
mod goal;
mod invitation;
mod kpi;
mod member;
mod org;

pub use assignment::*;
pub use comment::*;
pub use deliverable::*;
pub use goal::*;
pub use invitation::*;
pub use kpi::*;
pub use member::*;
pub use org::*;

use serde::{Deserialize, Serialize};

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
