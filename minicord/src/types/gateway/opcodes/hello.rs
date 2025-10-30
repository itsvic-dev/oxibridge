use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Hello {
    // How often to send heartbeat packets, in milliseconds.
    pub heartbeat_interval: u64,
}
