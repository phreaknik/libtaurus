use super::block;
use crate::params::AVALANCHE_ACCEPTANCE_THRESHOLD;
use crate::{p2p, Block, BlockHash};
use heed::{BytesDecode, BytesEncode};
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::{collections::HashMap, result, sync::Arc};
use tracing_mutex::stdsync::{TracingRwLock, TracingRwLockGuard};

/// Current revision of the vertex structure
pub const VERSION: u32 = 32;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad hash")]
    BadHash,
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error("error decoding from protobuf")]
    ProtoDecode(String),
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
}

/// Result type for vertex errors
pub type Result<T> = result::Result<T, Error>;

/// A vertex in the avalanche DAG. A vertex is essentially a block with parent & child links to
/// assist DAG operations.
#[derive(Debug, Clone)]
pub struct Vertex {
    /// Version number for this vertex format
    pub version: u32,

    /// Block of transactions referenced by this vertex
    pub block: Arc<Block>,

    /// Parent vertices in the DAG pointed to by this vertex
    pub parents: Vec<VertexHash>,

    /// Every parent vertex which hasn't been decided yet
    pub undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will accumulate children we learn about, for simplified traversal of the dag when
    /// performing Avalanche operations, such as computing confidence or deciding vertex
    /// acceptance.
    pub known_children: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// Is this vertex currently preferred?
    pub preferred: bool,

    /// Chit indicates if this vertex received quorum when we queried the network
    pub chit: usize,

    /// Confidence counts how many dependent votes have been received without changing preference
    pub confidence: usize,
}

impl Vertex {
    /// Create a new vertex representing a block's position in the DAG
    pub fn new(block: Arc<Block>, parents: Vec<VertexHash>) -> Vertex {
        Vertex {
            version: VERSION,
            parents,
            block,
            undecided_parents: HashMap::new(),
            known_children: HashMap::new(),
            preferred: false,
            chit: 0,
            confidence: 0,
        }
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<VertexHash> {
        SlimVertex::try_from(self)?.hash()
    }

    /// Increase the confidences of this vertex and all undecided ancestors. Returns the hashes of
    /// ancestors which can now be accepted
    pub fn increase_confidence(&mut self) -> Vec<VertexHash> {
        self.confidence += 1;
        // Increase confidence of parents and collect any ancestors which can be accepted
        let mut accepted = self
            .undecided_parents
            .values_mut()
            .map(|parent| {
                // TODO paralellize this walk
                parent.write().unwrap().increase_confidence()
            })
            .reduce(|mut acc, mut new| {
                acc.append(&mut new);
                acc
            })
            .unwrap();
        if self.undecided_parents.len() == 0 && self.confidence >= AVALANCHE_ACCEPTANCE_THRESHOLD {
            // This vertex has reached the acceptance threshold.
            accepted.push(self.hash().unwrap());
        }
        accepted
    }

    /// Reset the convidence of this vertex and any children
    pub fn reset_confidence(&mut self) {
        self.confidence = 0;
        // TODO: According to avalanche, children of a non-virtuous transaction should be retried
        // with new parents closer to genesis. Not doing so results in a liveness failure.
        for child in self.known_children.values() {
            // TODO paralellize this walk
            child.write().unwrap().reset_confidence();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct VertexHash([u8; blake3::OUT_LEN]);

impl VertexHash {
    /// Format the VertexHash as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Format the VertexHash as a short hex string, better for displaying VertexHashes in large
    /// collections of data
    pub fn to_short_hex(&self) -> String {
        format!("{}..", hex::encode(&self.0[..4]))
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Hash> {
        Ok(p2p::avalanche_rpc::proto::Hash {
            hash: rmp_serde::to_vec(&self)?,
        })
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(proto: &p2p::avalanche_rpc::proto::Hash) -> Result<VertexHash> {
        Ok(VertexHash(rmp_serde::from_slice(&proto.hash)?))
    }
}

impl fmt::Display for VertexHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_short_hex())
    }
}

impl From<blake3::Hash> for VertexHash {
    fn from(hash: blake3::Hash) -> Self {
        VertexHash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &VertexHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl Into<blake3::Hash> for VertexHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl<'a> BytesEncode<'a> for VertexHash {
    type EItem = VertexHash;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for VertexHash {
    type DItem = VertexHash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

impl Default for VertexHash {
    fn default() -> Self {
        VertexHash([0; blake3::OUT_LEN])
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WireVertex {
    pub version: u32,
    pub parents: Vec<VertexHash>,
    pub block: Block,
}

impl WireVertex {
    /// Construct a new vertex for a block at the given frontire
    pub fn new<F>(block: Block, frontier: F) -> Result<WireVertex>
    where
        F: Iterator<Item = Arc<TracingRwLock<Vertex>>>,
    {
        Ok(WireVertex {
            version: VERSION,
            parents: frontier
                .map(|rw_vertex| {
                    rw_vertex
                        .read()
                        .map_err(|_| Error::VertexReadLock)
                        .and_then(|v| v.hash())
                })
                .try_collect()?,
            block,
        })
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<VertexHash> {
        SlimVertex::try_from(self)?.hash()
    }
}

/// A reduced representation of a vertex, suitable for encoding for wire transmision.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SlimVertex {
    pub version: u32,
    pub parents: Vec<VertexHash>,
    pub block_hash: BlockHash,
}

impl SlimVertex {
    /// Construct a new vertex for a block at the given frontire
    pub fn new<F>(block_hash: BlockHash, frontier: F) -> Result<SlimVertex>
    where
        F: Iterator<Item = Arc<TracingRwLock<Vertex>>>,
    {
        Ok(SlimVertex {
            version: VERSION,
            parents: frontier
                .map(|rw_vertex| {
                    rw_vertex
                        .read()
                        .map_err(|_| Error::VertexReadLock)
                        .and_then(|v| v.hash())
                })
                .try_collect()?,
            block_hash,
        })
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<VertexHash> {
        Ok(blake3::hash(&rmp_serde::to_vec(self)?).into())
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(vertex: p2p::avalanche_rpc::proto::Vertex) -> Result<SlimVertex> {
        Ok(SlimVertex {
            version: vertex.version,
            parents: vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(&p))
                .try_collect()?,
            block_hash: BlockHash::from_protobuf(
                &vertex
                    .block_hash
                    .ok_or(Error::ProtoDecode("missing block_hash".to_string()))?,
            )?,
        })
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Vertex> {
        Ok(p2p::avalanche_rpc::proto::Vertex {
            version: self.version,
            parents: self.parents.iter().map(|p| p.to_protobuf()).try_collect()?,
            block_hash: Some(self.block_hash.to_protobuf()?),
        })
    }

    /// Inflate the SlimVertex into a ['WireVertex']
    pub fn to_wire(&self, block: Block) -> Result<WireVertex> {
        if self.block_hash != block.hash()? {
            Err(Error::BadHash)
        } else {
            Ok(WireVertex {
                version: self.version,
                parents: self.parents.clone(),
                block,
            })
        }
    }
}

impl TryFrom<WireVertex> for SlimVertex {
    type Error = Error;

    fn try_from(wv: WireVertex) -> result::Result<Self, Self::Error> {
        SlimVertex::try_from(&wv)
    }
}

impl TryFrom<&WireVertex> for SlimVertex {
    type Error = Error;

    fn try_from(wv: &WireVertex) -> result::Result<Self, Self::Error> {
        Ok(SlimVertex {
            version: wv.version,
            parents: wv.parents.clone(),
            block_hash: wv.block.hash()?,
        })
    }
}

impl TryFrom<TracingRwLockGuard<'_, std::sync::RwLockWriteGuard<'_, Vertex>>> for SlimVertex {
    type Error = Error;

    fn try_from(
        vertex: TracingRwLockGuard<'_, std::sync::RwLockWriteGuard<'_, Vertex>>,
    ) -> result::Result<Self, Self::Error> {
        SlimVertex::try_from(&vertex)
    }
}

impl TryFrom<&TracingRwLockGuard<'_, std::sync::RwLockWriteGuard<'_, Vertex>>> for SlimVertex {
    type Error = Error;

    fn try_from(
        vertex: &TracingRwLockGuard<'_, std::sync::RwLockWriteGuard<'_, Vertex>>,
    ) -> result::Result<Self, Self::Error> {
        Ok(SlimVertex {
            version: vertex.version,
            parents: vertex.parents.clone(),
            block_hash: vertex.block.hash()?,
        })
    }
}

impl TryFrom<TracingRwLockGuard<'_, std::sync::RwLockReadGuard<'_, Vertex>>> for SlimVertex {
    type Error = Error;

    fn try_from(
        vertex: TracingRwLockGuard<'_, std::sync::RwLockReadGuard<'_, Vertex>>,
    ) -> result::Result<Self, Self::Error> {
        SlimVertex::try_from(&vertex)
    }
}

impl TryFrom<&TracingRwLockGuard<'_, std::sync::RwLockReadGuard<'_, Vertex>>> for SlimVertex {
    type Error = Error;

    fn try_from(
        vertex: &TracingRwLockGuard<'_, std::sync::RwLockReadGuard<'_, Vertex>>,
    ) -> result::Result<Self, Self::Error> {
        Ok(SlimVertex {
            version: vertex.version,
            parents: vertex.parents.clone(),
            block_hash: vertex.block.hash()?,
        })
    }
}

impl TryFrom<Vertex> for SlimVertex {
    type Error = Error;

    fn try_from(vertex: Vertex) -> result::Result<Self, Self::Error> {
        SlimVertex::try_from(&vertex)
    }
}

impl TryFrom<&Vertex> for SlimVertex {
    type Error = Error;

    fn try_from(vertex: &Vertex) -> result::Result<Self, Self::Error> {
        Ok(SlimVertex {
            version: vertex.version,
            parents: vertex.parents.clone(),
            block_hash: vertex.block.hash()?,
        })
    }
}

impl<'a> BytesEncode<'a> for SlimVertex {
    type EItem = SlimVertex;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for SlimVertex {
    type DItem = SlimVertex;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}
