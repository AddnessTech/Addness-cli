mod comment;
mod deliverable;
mod goal;
mod member;
mod org;

pub use comment::*;
pub use deliverable::*;
pub use goal::*;
pub use member::*;
pub use org::*;

use serde::{Deserialize, Serialize};

// Generic API response wrapper: { "data": T, "message": "..." }
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
