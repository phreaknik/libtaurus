use super::block;
use crate::{
    wire::{self, WireFormat},
    Block, BlockHash,
};
use cached::{Cached, TimedCache};
use std::{collections::HashMap, io, ops::Deref, result, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("block hash does not match self.bhash")]
    BadBlockHash,
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
    pub inner: Arc<wire::Vertex>,

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
    pub fn genesis(wire_vertex: Arc<wire::Vertex>) -> Vertex {
        Vertex {
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
        wire_vertex: Arc<wire::Vertex>,
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
        Ok(Vertex {
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
