use super::block;
use crate::{
    wire::{proto, WireFormat},
    Block, BlockHash,
};
use cached::{Cached, TimedCache};
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, io, ops::Deref, result, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// Current revision of the vertex structure
pub const VERSION: u32 = 0;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("block hash does not match vertex")]
    BadBlockHash,
    #[error("bad vertex version")]
    BadVertexVersion(u32),
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

/// A vertex descibes a block's position within the DAG.
#[derive(Clone, Default)]
pub struct Vertex {
    pub version: u32,
    pub bhash: BlockHash,
    pub parents: Vec<VertexHash>,
    pub block: Option<Arc<Block>>,
}

impl Vertex {
    /// Construct a new vertex for a block at the given frontire
    /// If its expected our peers already know about the block (e.g. in case of transmitting a new
    /// vertex for an existing block), set `full` to false.
    pub fn new<P>(block: Arc<Block>, parents: P, full: bool) -> Result<Vertex>
    where
        P: Iterator<Item = Arc<TracingRwLock<UndecidedVertex>>>,
    {
        Ok(Vertex {
            version: VERSION,
            bhash: block.hash(),
            parents: parents
                .map(|rw_vertex| {
                    rw_vertex
                        .read()
                        .map_err(|_| Error::VertexReadLock)
                        .map(|v| v.hash())
                })
                .try_collect()?,
            block: if full { Some(block) } else { None },
        })
    }

    /// Update a vertex to contain the given block
    pub fn with_block(mut self, block: Arc<Block>) -> Vertex {
        self.bhash = block.hash();
        self.block = Some(block);
        self
    }

    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version != VERSION {
            Err(Error::BadVertexVersion(self.version))
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

    /// Slim this vertex by removing the block
    pub fn slim(mut self) -> (Option<Arc<Block>>, Vertex) {
        if let Some(block) = self.block {
            self.bhash = block.hash();
            self.block = None;
            (Some(block), self)
        } else {
            (None, self)
        }
    }
}

impl From<UndecidedVertex> for Vertex {
    fn from(value: UndecidedVertex) -> Self {
        Vertex {
            version: VERSION,
            bhash: value.inner.hash(),
            parents: value.inner.parents.clone(),
            block: value.inner.block.clone(),
        }
    }
}

impl<'a> WireFormat<'a, proto::Vertex> for Vertex {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Vertex> {
        if check {
            self.sanity_checks()?;
        }
        let (block, block_hash) = if let Some(b) = &self.block {
            (Some(b.to_protobuf(check)?), None)
        } else {
            (None, Some(self.bhash.to_protobuf(check)?))
        };
        Ok(proto::Vertex {
            version: self.version,
            parents: self
                .parents
                .iter()
                .map(|p| p.to_protobuf(check))
                .try_collect()?,
            block_hash,
            block,
        })
    }

    fn from_protobuf(vertex: &proto::Vertex, check: bool) -> Result<Vertex> {
        let (block, bhash) = if let Some(b) = &vertex.block {
            let block = Block::from_protobuf(b, check)?;
            let bhash = block.hash();
            (Some(Arc::new(block)), bhash)
        } else {
            (
                None,
                BlockHash::from_protobuf(
                    &vertex.block_hash.as_ref().expect("missing block_hash"),
                    check,
                )?,
            )
        };
        let vertex = Vertex {
            version: vertex.version,
            parents: vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(&p, check))
                .try_collect()?,
            block,
            bhash,
        };
        if check {
            vertex.sanity_checks()?;
        }
        Ok(vertex)
    }
}

impl PartialEq for Vertex {
    fn eq(&self, other: &Self) -> bool {
        self.version == other.version && self.parents == other.parents && self.bhash == other.bhash
    }
}

impl Eq for Vertex {}

impl PartialOrd for Vertex {
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

impl Ord for Vertex {
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

impl From<&Vertex> for PrettyVertex {
    fn from(wire_vertex: &Vertex) -> Self {
        PrettyVertex {
            version: wire_vertex.version,
            bhash: wire_vertex.bhash.to_hex(),
            parents: wire_vertex.parents.iter().map(|txo| txo.to_hex()).collect(),
        }
    }
}

impl std::fmt::Display for Vertex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string_pretty(&PrettyVertex::from(self)).unwrap()
        )
    }
}

impl std::fmt::Debug for Vertex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(&self, f)
    }
}

/// UndecidedVertex is a wrapper around ['Vertex'] which provides dynamic links and metadata useful
/// during the decision process for accepting new vertices.
#[derive(Debug, Clone)]
pub struct UndecidedVertex {
    pub inner: Arc<Vertex>,

    /// Every parent vertex which hasn't been decided yet
    pub undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will accumulate children we learn about, for simplified traversal of the dag when
    /// performing Avalanche operations, such as computing confidence or deciding vertex
    /// acceptance.
    pub known_children: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,

    /// A vertex is strongly preferred if it and its entire ancestry are preferred over all
    /// conflicting vertices.
    pub strongly_preferred: bool,

    /// Chit indicates if this vertex received quorum when we queried the network
    pub chit: usize,

    /// Confidence counts how many dependent votes have been received without changing preference
    pub confidence: usize,
}

impl UndecidedVertex {
    /// Create a genesis vertex from a given block
    pub fn genesis(wire_vertex: Arc<Vertex>) -> UndecidedVertex {
        UndecidedVertex {
            inner: wire_vertex,
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
        wire_vertex: Arc<Vertex>,
        undecided_blocks: &mut TimedCache<BlockHash, Arc<Block>>,
        undecided_vertices: &mut TimedCache<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,
        conflict_free: bool,
    ) -> Result<UndecidedVertex> {
        let undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>> =
            wire_vertex
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
        let inner = match wire_vertex.block {
            Some(_) => wire_vertex,
            None => Arc::new(
                wire_vertex.deref().clone().with_block(
                    undecided_blocks
                        .cache_get(&wire_vertex.bhash)
                        .cloned()
                        .ok_or(Error::MissingBlock)?,
                ),
            ),
        };
        Ok(UndecidedVertex {
            inner,
            undecided_parents,
            known_children: HashMap::new(),
            strongly_preferred,
            chit: 0,
            confidence: 0,
        })
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> VertexHash {
        self.inner.hash()
    }
}
