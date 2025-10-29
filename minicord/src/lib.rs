//! `minicord` is a small Discord API wrapper built for Oxibridge.
//!
//! It is designed to be as light as possible, using the same dependencies as Oxibridge itself.
//!
//! As such, its goal is not to be feature-complete. It implements only the features required for Oxibridge.

use secure_string::SecureString;

pub mod types;

pub struct Client {
    token: SecureString,
}

impl Client {
    /// Creates a new [`Client`] instance with a plain-text token.
    fn new(token: &str) -> Self {
        Self {
            token: SecureString::from(token),
        }
    }
}
