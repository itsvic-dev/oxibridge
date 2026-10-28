use crate::model::Problem;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{} {}: {}", .0.status, .0.code, .0.title)]
    Api(Box<Problem>),
    /// An error response without a Problem Details body, for example from a proxy.
    #[error("unexpected HTTP status {0}")]
    Status(u16),
    #[error("the file is larger than {0} bytes")]
    TooLarge(u64),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
}

impl Error {
    /// The stable error code of an API error, like `UNKNOWN_MESSAGE`.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Api(problem) => Some(&problem.code),
            _ => None,
        }
    }

    /// The HTTP status of a failed request.
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api(problem) => Some(problem.status),
            Self::Status(status) => Some(*status),
            Self::Http(e) => e.status().map(|status| status.as_u16()),
            _ => None,
        }
    }
}
