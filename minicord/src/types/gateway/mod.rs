mod codes;
pub mod events;
mod opcodes;

pub use codes::CloseEventCode;
pub use opcodes::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use events::Event;

/// Gateway event payloads have a common structure, but the contents of the associated data ([`Self::data`]) varies between the different events.
///
/// [`Self::sequence`] and [`Self::event`] are [`None`] when [`Self::opcode`] is not [`Opcode::Dispatch`].
#[derive(Debug, Deserialize, Serialize)]
pub struct GatewayEvent {
    #[serde(rename = "op")]
    /// [`Opcode`] which indicates the payload type
    pub opcode: Opcode,

    #[serde(rename = "d")]
    /// Event data
    pub data: Option<Value>,

    #[serde(rename = "s")]
    /// Sequence number of event used for [resuming sessions](https://discord.com/developers/docs/topics/opcodes-and-status-codes#gateway-gateway-opcodes) and [heartbeating](https://discord.com/developers/docs/events/gateway#sending-heartbeats)
    pub sequence: Option<usize>,

    #[serde(rename = "t")]
    /// Event name
    pub event: Option<Event>,
}
