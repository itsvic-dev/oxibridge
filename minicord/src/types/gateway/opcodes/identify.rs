use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Identify {
    pub token: String,
    pub properties: IdentifyProperties,
    pub intents: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct IdentifyProperties {
    pub os: String,
    pub browser: String,
    pub device: String,
}
