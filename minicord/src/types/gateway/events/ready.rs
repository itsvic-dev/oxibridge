use serde::{Deserialize, Serialize};

use crate::types::User;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Ready {
    /// API version
    pub v: u8,

    /// Information about the user including email
    pub user: User,

    /// Used for resuming sessions
    pub session_id: String,

    /// Gateway URL for resuming connections
    pub resume_gateway_url: String,
}
