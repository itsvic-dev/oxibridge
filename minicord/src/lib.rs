//! `minicord` is a small Discord API wrapper built for Oxibridge.
//!
//! It is designed to be as light as possible, using the same dependencies as Oxibridge itself.
//!
//! As such, its goal is not to be feature-complete. It implements only the features required for Oxibridge.

use std::error::Error;

use reqwest::Method;
use secure_string::SecureString;
use serde::{Serialize, de::DeserializeOwned};

use crate::types::http::APIResponse;

pub mod types;

static BASE_URL: &str = "https://discord.com/api/v10";

pub struct Client {
    token: SecureString,
    http: reqwest::Client,
}

impl Client {
    /// Creates a new [`Client`] instance with a plain-text token.
    #[must_use]
    pub fn new(token: &str) -> Self {
        Self {
            token: SecureString::from(token),
            http: reqwest::Client::new(),
        }
    }

    /// # Errors
    /// - [`reqwest::Error`]
    pub async fn get<T: DeserializeOwned>(
        &self,
        endpoint: String,
    ) -> Result<APIResponse<T>, Box<dyn Error>> {
        let response = self
            .http
            .request(Method::GET, format!("{BASE_URL}/{endpoint}"))
            .header("Authorization", format!("Bot {}", self.token.unsecure()))
            .send()
            .await?;

        Ok(response.json().await?)
    }

    /// # Errors
    /// - [`reqwest::Error`]
    pub async fn post<T: Serialize, U: DeserializeOwned>(
        &self,
        endpoint: String,
        body: T,
    ) -> Result<APIResponse<U>, Box<dyn Error>> {
        let response = self
            .http
            .request(Method::GET, format!("{BASE_URL}/{endpoint}"))
            .header("Authorization", format!("Bot {}", self.token.unsecure()))
            .json(&body)
            .send()
            .await?;

        Ok(response.json().await?)
    }
}
