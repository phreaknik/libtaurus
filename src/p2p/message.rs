use super::Error;
use crate::consensus::Block;
use libp2p::{
    gossipsub::{self, MessageAcceptance, MessageId, Sha256Topic, TopicHash},
    PeerId,
};
use serde::{Deserialize, Serialize};
use strum_macros::{AsRefStr, EnumIter};

/// Messages that can be sent to/from the gossipsub network
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub msg_id: MessageId,
    pub msg_source: PeerId,
    pub data: MessageData,
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

    fn try_from(event: gossipsub::Event) -> Result<Self, Self::Error> {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => Ok(Message {
                msg_source: propagation_source,
                msg_id: message_id,
                data: rmp_serde::from_slice(&message.data)?,
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

#[derive(Clone, Debug, Serialize, Deserialize, EnumIter, AsRefStr)]
pub enum MessageData {
    Block(Block),
}

impl<H: gossipsub::Hasher> From<&MessageData> for gossipsub::Topic<H> {
    fn from(m: &MessageData) -> Self {
        gossipsub::Topic::new(m.as_ref())
    }
}

impl From<MessageData> for TopicHash {
    fn from(m: MessageData) -> Self {
        Sha256Topic::from(&m).hash()
    }
}
