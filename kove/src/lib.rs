//! A client for the [Kove](https://kove.social) REST API and gateway.

mod error;
mod gateway;
pub mod model;
mod rest;

pub use error::Error;
pub use gateway::{
    Dispatch, Event, Gateway, Intent, MessageDelete, MessageDeleteBulk, ReactionEvent,
    ReactionRemoveAll, Ready,
};
pub use model::Id;
pub use rest::{Client, DEFAULT_BASE_URL};
