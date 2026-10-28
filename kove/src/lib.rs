//! A client for the [Kove](https://kove.social) REST API.

mod error;
pub mod model;
mod rest;

pub use error::Error;
pub use model::Id;
pub use rest::{Client, DEFAULT_BASE_URL};
