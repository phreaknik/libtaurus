use super::block;
use crate::{
    p2p::{self, avalanche_rpc::proto},
    Block, BlockHash,
};
use cached::{Cached, TimedCache};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, io, result, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// Current revision of the vertex structure
pub const VERSION: u32 = 32;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("block hash does not match self.bhash")]
    BadBlockHash,
    #[error("bad vertex version")]
    BadVertexVersion,
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error("vertex does not specify any parents")]
    EmptyParents,
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("missing block")]
    MissingBlock,
    #[error("error decoding from protobuf")]
    ProtoDecode(String),
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
}

/// Result type for vertex errors
pub type Result<T> = result::Result<T, Error>;

/// Type alias for vertex hashes
pub type VertexHash = crate::hash::Hash;

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
    /// Create a genesis vertex from a given block
    pub fn genesis(wire_vertex: WireVertex) -> Vertex {
        Vertex {
            version: 0,
            block: Arc::new(wire_vertex.block.unwrap()),
            parents: Vec::new(),
            undecided_parents: HashMap::new(),
            known_children: HashMap::new(),
            strongly_preferred: true,
            chit: 1,
            confidence: 0,
        }
    }

    /// Create a new vertex representing a block's position in the DAG. If this vertex has no
    /// existing preferred conflicts, and its parents are strongly preferred, then it too will be
    /// marked as strongly preferred.
    pub fn new(
        wire_vertex: &WireVertex,
        undecided_blocks: &mut TimedCache<BlockHash, Arc<Block>>,
        undecided_vertices: &mut TimedCache<VertexHash, Arc<TracingRwLock<Vertex>>>,
        conflict_free: bool,
    ) -> Result<Vertex> {
        let undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>> = wire_vertex
            .parents
            .iter()
            .filter_map(|&k| undecided_vertices.cache_get(&k).map(|v| (k, v.clone())))
            .collect();
        // Strongly preferred if no conflicts and all undecided parents are also strongly preferred.
        let strongly_preferred = conflict_free
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
        let block = undecided_blocks
            .cache_get(&wire_vertex.bhash)
            .cloned()
            .ok_or(Error::MissingBlock)?;
        Ok(Vertex {
            version: VERSION,
            block,
            undecided_parents,
            parents: wire_vertex.parents.clone(),
            known_children: HashMap::new(),
            strongly_preferred,
            chit: 0,
            confidence: 0,
        })
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<VertexHash> {
        Ok(self.to_wire()?.slim().1.hash())
    }

    /// Convert into a ['WireVertex']
    pub fn to_wire(&self) -> Result<WireVertex> {
        Ok(WireVertex {
            version: self.version,
            bhash: self.block.hash(),
            parents: self.parents.clone(),
            block: Some((*self.block).clone()),
        })
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct WireVertex {
    pub version: u32,
    pub bhash: BlockHash,
    pub parents: Vec<VertexHash>,
    pub block: Option<Block>,
}

impl WireVertex {
    /// Construct a new vertex for a block at the given frontire
    /// If its expected our peers already know about the block (e.g. in case of transmitting a new
    /// vertex for an existing block), set `full` to false.
    pub fn new<P>(block: Block, parents: P, full: bool) -> Result<WireVertex>
    where
        P: Iterator<Item = Arc<TracingRwLock<Vertex>>>,
    {
        Ok(WireVertex {
            version: VERSION,
            bhash: block.hash(),
            parents: parents
                .map(|rw_vertex| {
                    rw_vertex
                        .read()
                        .map_err(|_| Error::VertexReadLock)
                        .and_then(|v| v.hash())
                })
                .try_collect()?,
            block: if full { Some(block) } else { None },
        })
    }

    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version != VERSION {
            Err(Error::BadVertexVersion)
        } else if self.parents.len() <= 0 {
            Err(Error::EmptyParents)
        } else if let Some(block) = &self.block {
            if block.hash() != self.bhash {
                Err(Error::BadBlockHash)
            } else {
                Ok(block.sanity_checks()?)
            }
        } else {
            Ok(())
        }
    }

    /// Compute the hash of the vertex
    /// This performs a copy, which could be expensive.
    pub fn hash(&self) -> VertexHash {
        blake3::hash(
            &rmp_serde::to_vec(&self.clone().slim().1).expect("Serde encode failure in hash"),
        )
        .into()
    }

    /// Slim this vertex by removing the block
    pub fn slim(mut self) -> (Option<Block>, WireVertex) {
        if let Some(block) = self.block {
            self.bhash = block.hash();
            self.block = None;
            (Some(block), self)
        } else {
            (None, self)
        }
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(vertex: p2p::avalanche_rpc::proto::Vertex) -> Result<WireVertex> {
        let (block, bhash) = if let Some(b) = &vertex.block {
            let block = Block::from_protobuf(b)?;
            let bhash = block.hash();
            (Some(block), bhash)
        } else {
            (
                None,
                BlockHash::from_protobuf(&vertex.block_hash.expect("missing block_hash"))?,
            )
        };
        let vertex = WireVertex {
            version: vertex.version,
            parents: vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(&p))
                .try_collect()?,
            block,
            bhash,
        };
        vertex.sanity_checks()?;
        Ok(vertex)
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Vertex> {
        self.sanity_checks()?;
        let (block, block_hash) = if let Some(b) = &self.block {
            (Some(b.to_protobuf()?), None)
        } else {
            (None, Some(self.bhash.to_protobuf()?))
        };
        Ok(p2p::avalanche_rpc::proto::Vertex {
            version: self.version,
            parents: self.parents.iter().map(|p| p.to_protobuf()).try_collect()?,
            block_hash,
            block,
        })
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &Vec<u8>) -> Result<WireVertex> {
        let protobuf = proto::Vertex::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unable to parse vertex from bytes: {e}"),
                )
            })?;
        WireVertex::from_protobuf(protobuf)
    }

    /// Serialize into bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to convert broadcast data to protobuf",
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to write broadcast data message",
            )
        })?;
        Ok(bytes)
    }
}

impl PartialEq for WireVertex {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.parents == other.parents && self.bhash == other.bhash
    }
}

impl Eq for WireVertex {}

impl PartialOrd for WireVertex {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.parents.contains(&other.hash()) {
            Some(std::cmp::Ordering::Greater)
        } else if other.parents.contains(&self.hash()) {
            Some(std::cmp::Ordering::Less)
        } else {
            None
        }
    }
}

impl Ord for WireVertex {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.partial_cmp(other) {
            None => std::cmp::Ordering::Equal,
            Some(order) => order,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrettyVertex {
    version: u32,
    bhash: String,
    parents: Vec<String>,
}

impl From<&WireVertex> for PrettyVertex {
    fn from(wire_vertex: &WireVertex) -> Self {
        PrettyVertex {
            version: wire_vertex.version,
            bhash: wire_vertex.bhash.to_hex(),
            parents: wire_vertex.parents.iter().map(|txo| txo.to_hex()).collect(),
        }
    }
}

impl std::fmt::Display for WireVertex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string_pretty(&PrettyVertex::from(self)).unwrap()
        )
    }
}

impl std::fmt::Debug for WireVertex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(&self, f)
    }
}
