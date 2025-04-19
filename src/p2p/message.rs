use super::{Error, Result};
use crate::wire::{
    self,
    proto::{self, Broadcast},
    WireFormat,
};
use libp2p::{
    gossipsub::{self, MessageAcceptance, MessageId, Sha256Topic, TopicHash},
    PeerId,
};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::io;
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
                data: BroadcastData::from_bytes(&message.data)?,
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
    Vertex(wire::Vertex),
}

impl BroadcastData {
    /// Deserialize from bytes
    pub fn from_bytes(bytes: &Vec<u8>) -> Result<BroadcastData> {
        let protobuf = proto::Broadcast::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to read broadcast data message: {e}"),
            )
        })?;
        BroadcastData::from_protobuf(protobuf)
    }

    /// Serialize into bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert BroadcastData to protobuf: {e}"),
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to serialize BroadcastData",
            )
        })?;
        Ok(bytes)
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(data: Broadcast) -> Result<BroadcastData> {
        let v = wire::Vertex::from_protobuf(&data.vertex.unwrap(), true)?;
        v.sanity_checks()?;
        Ok(BroadcastData::Vertex(v))
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<Broadcast> {
        let BroadcastData::Vertex(v) = self;
        v.sanity_checks()?;
        Ok(Broadcast {
            vertex: Some(v.to_protobuf(true)?),
        })
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
