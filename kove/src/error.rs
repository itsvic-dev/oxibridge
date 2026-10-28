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
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] Box<tokio_tungstenite::tungstenite::Error>),
    #[error("TLS setup failed: {0}")]
    Tls(#[from] rustls::Error),
    /// The gateway closed the connection with a code that a reconnect cannot fix, like `4004` for a bad token.
    #[error("the gateway closed the connection with code {0}")]
    GatewayClosed(u16),
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

impl From<tokio_tungstenite::tungstenite::Error> for Error {
    fn from(value: tokio_tungstenite::tungstenite::Error) -> Self {
        Self::WebSocket(Box::new(value))
    }
}
