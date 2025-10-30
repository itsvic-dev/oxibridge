mod codes;
mod opcodes;

pub use codes::GatewayCloseEventCode;
pub use opcodes::GatewayOpcode;
use serde::{Deserialize, Serialize};

/// Gateway event payloads have a common structure, but the contents of the associated data ([`Self::data`]) varies between the different events.
///
/// [`Self::sequence`] and [`Self::event`] are [`None`] when [`Self::opcode`] is not [`GatewayOpcode::Dispatch`].
#[derive(Debug, Deserialize, Serialize)]
pub struct GatewayEvent<T> {
    #[serde(rename = "op")]
    /// [`GatewayOpcode`] which indicates the payload type
    opcode: GatewayOpcode,

    #[serde(rename = "d")]
    /// Event data
    data: Option<T>,

    #[serde(rename = "s")]
    /// Sequence number of event used for [resuming sessions](https://discord.com/developers/docs/topics/opcodes-and-status-codes#gateway-gateway-opcodes) and [heartbeating](https://discord.com/developers/docs/events/gateway#sending-heartbeats)
    sequence: Option<usize>,

    #[serde(rename = "t")]
    /// Event name
    event: Option<String>,
}
