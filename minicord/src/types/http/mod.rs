use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum APIResponse<T> {
    Success(T),
    Error {
        /// error code
        code: usize,
        /// human-readable message of the error
        message: String,
    },
}
