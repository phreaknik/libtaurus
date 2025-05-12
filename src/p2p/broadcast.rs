use crate::{
    consensus::Vertex,
    vertex,
    wire::{proto, WireFormat},
};
use libp2p::{
    gossipsub::{self, MessageAcceptance, MessageId, Sha256Topic, TopicHash},
    PeerId,
};
use std::{result, sync::Arc};
use strum_macros::{AsRefStr, EnumIter};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("broadcast has no data")]
    EmptyBroadcast,
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
}

/// Broadcasts that can be sent to/from the gossipsub network
#[derive(Clone, Debug)]
pub struct Broadcast {
    pub id: MessageId,
    pub src: PeerId,
    pub topic: TopicHash,
    pub data: BroadcastData,
}

impl Broadcast {
    /// Generate a validation report to accept this message and propagate it to other peers.
    pub fn accept(&self) -> BroadcastValidationReport {
        BroadcastValidationReport {
            msg_id: self.id.clone(),
            propagation_source: self.src,
            acceptance: MessageAcceptance::Accept,
        }
    }

    /// Generate a validation report to ignore this message and cease propagation, without
    /// penalty to the peer that sent it.
    pub fn ignore(&self) -> BroadcastValidationReport {
        BroadcastValidationReport {
            msg_id: self.id.clone(),
            propagation_source: self.src,
            acceptance: MessageAcceptance::Ignore,
        }
    }

    /// Generate a validation report to reject this message, cease propagation, and penalize the
    /// peer who sent it. Repeated penalization will eventually leading to that peer being banned.
    pub fn reject(&self) -> BroadcastValidationReport {
        BroadcastValidationReport {
            msg_id: self.id.clone(),
            propagation_source: self.src,
            acceptance: MessageAcceptance::Reject,
        }
    }
}

/// Validation report to send back to the p2p client, informing if this message should be accepted
/// and propagated to peers, ignored, or rejected and penalize the peer.
#[derive(Debug)]
pub struct BroadcastValidationReport {
    pub msg_id: MessageId,
    pub propagation_source: PeerId,
    pub acceptance: MessageAcceptance,
}

#[derive(Clone, Debug, PartialEq, Eq, EnumIter, AsRefStr)]
pub enum BroadcastData {
    Vertex(Arc<Vertex>),
}

impl From<Arc<Vertex>> for BroadcastData {
    fn from(vx: Arc<Vertex>) -> Self {
        BroadcastData::Vertex(vx)
    }
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
        Ok(BroadcastData::Vertex(Arc::new(Vertex::from_protobuf(
            broadcast.vertex.as_ref().ok_or(Error::EmptyBroadcast)?,
            check,
        )?)))
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

#[cfg(test)]
mod test {
    use super::*;
    use libp2p::multihash::Multihash;
    use std::assert_matches::assert_matches;

    #[test]
    fn message_accept() {
        let m = Broadcast {
            id: MessageId::new(b"hello"),
            src: PeerId::from_multihash(Multihash::default()).unwrap(),
            data: BroadcastData::Vertex(Arc::new(Vertex::default())),
            topic: Sha256Topic::new("test/topic").hash(),
        };
        let response = m.accept();
        assert_matches!(response.acceptance, MessageAcceptance::Accept);
    }

    #[test]
    fn message_ignore() {
        let m = Broadcast {
            id: MessageId::new(b"hello"),
            src: PeerId::from_multihash(Multihash::default()).unwrap(),
            data: BroadcastData::Vertex(Arc::new(Vertex::default())),
            topic: Sha256Topic::new("test/topic").hash(),
        };
        let response = m.ignore();
        assert_matches!(response.acceptance, MessageAcceptance::Ignore);
    }

    #[test]
    fn message_reject() {
        let m = Broadcast {
            id: MessageId::new(b"hello"),
            src: PeerId::from_multihash(Multihash::default()).unwrap(),
            data: BroadcastData::Vertex(Arc::new(Vertex::default())),
            topic: Sha256Topic::new("test/topic").hash(),
        };
        let response = m.reject();
        assert_matches!(response.acceptance, MessageAcceptance::Reject);
    }
}
