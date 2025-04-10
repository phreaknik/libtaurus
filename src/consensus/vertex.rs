use super::block;
use crate::{p2p, Block, BlockHash};
use heed::{BytesDecode, BytesEncode};
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::{collections::HashMap, result, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

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

    /// A vertex is strongly preferred if it and its entire ancestry are preferred over all
    /// conflicting vertices.
    pub strongly_preferred: bool,

    /// Chit indicates if this vertex received quorum when we queried the network
    pub chit: usize,

    /// Confidence counts how many dependent votes have been received without changing preference
    pub confidence: usize,
}

impl Vertex {
    /// Create a new vertex representing a block's position in the DAG. If this vertex has no
    /// existing preferred conflicts, and its parents are strongly preferred, then it too will be
    /// marked as strongly preferred.
    pub fn new(
        block: Arc<Block>,
        parents: Vec<VertexHash>,
        undecided_vertices: &HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,
        has_conflicts: bool,
    ) -> Result<Vertex> {
        let undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>> = parents
            .iter()
            .filter_map(|&k| undecided_vertices.get(&k).map(|v| (k, v.clone())))
            .collect();
        // Strongly preferred if no conflicts and all undecided parents are also strongly preferred.
        let strongly_preferred = !has_conflicts
            && undecided_parents
                .values()
                .map(|p| {
                    p.read()
                        .map_err(|_| Error::VertexReadLock)
                        .map(|p| p.strongly_preferred)
                })
                .try_fold(true, |all, strongly_preferred| {
                    strongly_preferred.map(|sp| sp && all)
                })?;
        Ok(Vertex {
            version: VERSION,
            block,
            undecided_parents,
            parents,
            known_children: HashMap::new(),
            strongly_preferred,
            chit: 0,
            confidence: 0,
        })
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<VertexHash> {
        self.slim()?.hash()
    }

    /// Convert into a ['SlimVertex']
    pub fn slim(&self) -> Result<SlimVertex> {
        Ok(SlimVertex {
            version: self.version,
            parents: self.parents.clone(),
            block_hash: self.block.hash()?,
        })
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

    /// Convert into a ['SlimVertex']
    pub fn slim(&self) -> Result<SlimVertex> {
        Ok(SlimVertex {
            version: self.version,
            parents: self.parents.clone(),
            block_hash: self.block.hash()?,
        })
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<VertexHash> {
        self.slim()?.hash()
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
