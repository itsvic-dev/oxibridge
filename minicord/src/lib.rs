use serde::{Deserialize, Serialize};

use crate::types::Snowflake;

pub mod types;

#[derive(Debug, Serialize, Deserialize)]
pub struct Webhook {
    id: Snowflake,
}
