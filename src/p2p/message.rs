use super::{Error, Result};
use crate::{
    consensus::Vertex,
    wire::{
        proto::{self},
        WireFormat,
    },
};
use libp2p::{
    gossipsub::{self, MessageAcceptance, MessageId, Sha256Topic, TopicHash},
    PeerId,
};
use std::result;
use strum_macros::{AsRefStr, EnumIter};

/// Messages that can be sent to/from the gossipsub network
#[derive(Clone, Debug)]
pub struct Message {
    pub msg_id: MessageId,
    pub msg_source: PeerId,
    pub data: BroadcastData,
}

impl Message {
    /// Generate a validation report to accept this message and propagate it to other peers.
    pub fn accept(&self) -> MessageValidationReport {
        MessageValidationReport {
            msg_id: self.msg_id.clone(),
            msg_source: self.msg_source,
            acceptance: MessageAcceptance::Accept,
        }
    }

    /// Generate a validation report to ignore this message and cease propagation, without
    /// penalty to the peer that sent it.
    pub fn ignore(&self) -> MessageValidationReport {
        MessageValidationReport {
            msg_id: self.msg_id.clone(),
            msg_source: self.msg_source,
            acceptance: MessageAcceptance::Ignore,
        }
    }

    /// Generate a validation report to reject this message, cease propagation, and penalize the
    /// peer who sent it. Repeated penalization will eventually leading to that peer being banned.
    pub fn reject(&self) -> MessageValidationReport {
        MessageValidationReport {
            msg_id: self.msg_id.clone(),
            msg_source: self.msg_source,
            acceptance: MessageAcceptance::Reject,
        }
    }
}

impl TryFrom<gossipsub::Event> for Message {
    type Error = Error;

    fn try_from(event: gossipsub::Event) -> Result<Self> {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => Ok(Message {
                msg_source: propagation_source,
                msg_id: message_id,
                data: BroadcastData::from_wire(&message.data, true)?,
            }),
            _ => Err(Error::NotAMessage),
        }
    }
}

/// Validation report to send back to the p2p client, informing if this message should be accepted
/// and propagated to peers, ignored, or rejected and penalize the peer.
#[derive(Debug)]
pub struct MessageValidationReport {
    pub msg_id: MessageId,
    pub msg_source: PeerId,
    pub acceptance: MessageAcceptance,
}

#[derive(Clone, Debug, EnumIter, AsRefStr)]
pub enum BroadcastData {
    Vertex(Vertex),
}

impl<'a> WireFormat<'a, proto::Broadcast> for BroadcastData {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> result::Result<proto::Broadcast, Error> {
        match self {
            BroadcastData::Vertex(v) => Ok(proto::Broadcast {
                vertex: Some(v.to_protobuf(check)?),
            }),
        }
    }

    fn from_protobuf(broadcast: &proto::Broadcast, check: bool) -> result::Result<Self, Error> {
        Ok(BroadcastData::Vertex(Vertex::from_protobuf(
            broadcast.vertex.as_ref().ok_or(Error::EmptyBroadcast)?,
            check,
        )?))
    }
}

impl<H: gossipsub::Hasher> From<&BroadcastData> for gossipsub::Topic<H> {
    fn from(m: &BroadcastData) -> Self {
        gossipsub::Topic::new(m.as_ref())
    }
}

impl From<BroadcastData> for TopicHash {
    fn from(m: BroadcastData) -> Self {
        Sha256Topic::from(&m).hash()
    }
}
