use super::block;
use crate::{
    wire::{proto, WireFormat},
    Block, BlockHash,
};
use cached::{Cached, TimedCache};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::{cmp, collections::HashMap, io, ops::Deref, result, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// Current revision of the vertex structure
pub const VERSION: u32 = 1;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("block hash does not match vertex")]
    BadBlockHash,
    #[error("block height does not extend parents")]
    BadBlockHeight,
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
    #[error("one or more parent is not in the undecided set")]
    ParentsAlreadyDecided,
    #[error("error decoding from protobuf")]
    ProtoDecode(String),
    #[error("vertex parents are redundant with block parents")]
    RedundantParents,
    #[error("some vertex parents are repeated")]
    RepeatedParents,
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
}

/// Result type for vertex errors
pub type Result<T> = result::Result<T, Error>;

/// Type alias for vertex hashes
pub type VertexHash = crate::hash::Hash;

/// A vertex descibes a block's adapted position within the DAG. Importantly, its adapted position
/// may differ from its mined position, due to dynamic parent reselection, in the case of
/// non-virtuous parents of the mined block..
#[derive(Clone, PartialEq, Eq)]
pub struct Vertex {
    /// Revision number of the vertex structure
    pub version: u32,

    /// Hash of the block this vertex represents
    pub bhash: BlockHash,

    /// Pointers to adaptively reselected parents, if any
    pub parents: Vec<VertexHash>,

    /// Optional full block
    pub block: Option<Arc<Block>>,
}

impl Vertex {
    /// Construct a new vertex for a block at the given frontire
    /// If its expected our peers already know about the block (e.g. in case of transmitting a new
    /// vertex for an existing block), set `full` to false.
    pub fn new<P>(block: Arc<Block>, parents: P, full: bool) -> Result<Vertex>
    where
        P: Iterator<Item = Arc<TracingRwLock<UndecidedVertex>>> + Clone,
    {
        Ok(Vertex {
            version: VERSION,
            bhash: block.hash(),
            parents: parents
                .clone()
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
        self.parents.clear();
        self.block = Some(block);
        self
    }

    /// Set the parent sof this vertex
    pub fn with_parents(mut self, parents: Vec<VertexHash>) -> Vertex {
        self.parents = parents;
        self
    }

    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version != VERSION {
            Err(Error::BadVertexVersion(self.version))
        } else if self.parents.len() <= 0 && self.block.is_none() {
            Err(Error::EmptyParents)
        } else if !self.parents.iter().all_unique() {
            Err(Error::RepeatedParents)
        } else if let Some(block) = &self.block {
            if block.hash() != self.bhash {
                Err(Error::BadBlockHash)
            } else if block.parents == self.parents {
                Err(Error::RedundantParents)
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

impl Default for Vertex {
    fn default() -> Self {
        Vertex {
            version: VERSION,
            bhash: BlockHash::default(),
            parents: Vec::new(),
            block: None,
        }
    }
}

impl From<UndecidedVertex> for Vertex {
    fn from(vertex: UndecidedVertex) -> Self {
        vertex.inner.deref().clone()
    }
}

impl<'a> WireFormat<'a, proto::Vertex> for Vertex {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Vertex> {
        // Optionally perform sanity checks
        if check {
            match self.sanity_checks() {
                // Ignore redundant peers, as they will be removed when encoding
                Ok(()) | Err(Error::RedundantParents) => Ok(()),
                other @ _ => other,
            }?;
        }
        // Only encode the blockhash if the full block is not present
        let (block, block_hash) = if let Some(b) = &self.block {
            (Some(b.to_protobuf(check)?), None)
        } else {
            (None, Some(self.bhash.to_protobuf(check)?))
        };
        // Only encode the vertex parents if they differ from the block parents
        let parents =
            if self.block.is_none() || self.block.as_ref().unwrap().parents != self.parents {
                self.parents
                    .iter()
                    .map(|p| p.to_protobuf(check))
                    .try_collect()?
            } else {
                Vec::new()
            };
        Ok(proto::Vertex {
            version: self.version,
            parents,
            block_hash,
            block,
        })
    }

    fn from_protobuf(vertex: &proto::Vertex, check: bool) -> Result<Vertex> {
        // If no parents are provided, fill with block's parents
        let parents = if vertex.parents.len() == 0 {
            if let Some(b) = &vertex.block {
                Ok(b.parents
                    .iter()
                    .map(|p| VertexHash::from_protobuf(&p, check))
                    .try_collect()?)
            } else {
                Err(Error::EmptyParents)
            }
        } else {
            Ok(vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(&p, check))
                .try_collect()?)
        }?;
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
            parents,
            block,
            bhash,
        };
        // Optionally perform sanity checks
        if check {
            vertex.sanity_checks()?;
        }
        Ok(vertex)
    }
}

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
    fn from(vertex: &Vertex) -> Self {
        PrettyVertex {
            version: vertex.version,
            bhash: vertex.bhash.to_hex(),
            parents: vertex.parents.iter().map(|txo| txo.to_hex()).collect(),
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

/// UndecidedVertex is a wrapper around [`Vertex`] which provides dynamic links and metadata useful
/// during the decision process for accepting new vertices.
#[derive(Debug, Clone)]
pub struct UndecidedVertex {
    pub inner: Arc<Vertex>,

    /// Height within the DAG, of the block as it was mined
    pub mined_height: u64,

    /// Height within the DAG, of the vertex after dynamic parent reselection
    pub adapted_height: u64,

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
    pub fn genesis(vertex: Arc<Vertex>) -> UndecidedVertex {
        UndecidedVertex {
            inner: vertex,
            mined_height: 0,
            adapted_height: 0,
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
        genesis_hash: VertexHash,
        vertex: Arc<Vertex>,
        undecided_blocks: &mut TimedCache<BlockHash, Arc<Block>>,
        undecided_vertices: &mut TimedCache<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,
        conflict_free: bool,
    ) -> Result<UndecidedVertex> {
        let undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>> = vertex
            .parents
            .iter()
            .filter(|&p| p != &genesis_hash) //TODO: 99.999% of the time, this is wasted cycles
            .map(|&k| {
                undecided_vertices
                    .cache_get(&k)
                    .map(|v| (k, v.clone()))
                    .ok_or(Error::ParentsAlreadyDecided)
            })
            .try_collect()?;
        // Determine height of this vertex and whether or not it is strongly preferred.
        // Strongly preferred if no conflicts and all undecided parents are also strongly preferred.
        let (adapted_height, strongly_preferred) = undecided_parents
            .values()
            .map(|p| p.read().map_err(|_| Error::VertexReadLock))
            .try_fold((1, conflict_free), |(max_ah, all_pref), vertex| {
                vertex.map(|v| {
                    (
                        cmp::max(max_ah, v.adapted_height + 1),
                        all_pref && v.strongly_preferred,
                    )
                })
            })?;
        let block = vertex.block.as_ref().ok_or(Error::MissingBlock)?;
        let mined_height = if vertex.parents != block.parents {
            block
                .parents
                .iter()
                .map(|&k| {
                    undecided_vertices
                        .cache_get(&k)
                        .ok_or(Error::ParentsAlreadyDecided)
                        .map(|v| {
                            v.read()
                                .map_err(|_| Error::VertexReadLock)
                                .map(|v| v.mined_height)
                        })
                })
                .try_fold(1, |max_mh, mined_height| {
                    mined_height.flatten().map(|mh| cmp::max(max_mh, mh + 1))
                })?
        } else {
            adapted_height
        };
        let inner = match vertex.block {
            Some(_) => vertex,
            None => Arc::new(
                vertex.deref().clone().with_block(
                    undecided_blocks
                        .cache_get(&vertex.bhash)
                        .cloned()
                        .ok_or(Error::MissingBlock)?,
                ),
            ),
        };
        Ok(UndecidedVertex {
            inner,
            mined_height,
            adapted_height,
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

#[cfg(test)]
pub mod tests {
    use crate::hash::tests::generate_test_hashes;

    use super::*;

    pub struct TestCase<'a> {
        pub name: &'a str,
        pub decoded: Vertex,
        pub encoded: Vec<u8>,
        pub expect_encode_err: bool,
        pub expect_decode_err: bool,
    }

    pub fn generate_test_vertices<'a>() -> impl Iterator<Item = TestCase<'a>> {
        let dummy_block = Arc::new(Block::default().with_parents(vec![VertexHash::default()]));
        [
            TestCase {
                name: "slim vertex no parents",
                decoded: Vertex::default(),
                encoded: vec![
                    0, 1, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                expect_encode_err: true, // Missing parents
                expect_decode_err: true, // Missing parents
            },
            TestCase {
                name: "slim vertex with one parent",
                decoded: Vertex::default().with_parents(vec![VertexHash::default()]),
                encoded: vec![
                    0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                ],
                expect_encode_err: false,
                expect_decode_err: false,
            },
            TestCase {
                name: "slim vertex with multiple parents",
                decoded: Vertex::default()
                    .with_parents(generate_test_hashes().map(|tc| tc.decoded).collect()),
                encoded: vec![
                    0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34,
                    2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 1, 10, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 10, 34, 2, 32, 127, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 34, 2, 32, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 18, 34,
                    2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0,
                ],
                expect_encode_err: false,
                expect_decode_err: false,
            },
            TestCase {
                name: "slim vertex with repeated parents",
                decoded: Vertex::default()
                    .with_parents(vec![BlockHash::default(), BlockHash::default()]),
                encoded: vec![
                    0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34,
                    2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0,
                ],
                expect_encode_err: true, // Repeated parents
                expect_decode_err: true, // Repeated parents
            },
            TestCase {
                name: "full vertex with one parent",
                decoded: Vertex::default()
                    .with_block(dummy_block.clone())
                    .with_parents(vec![generate_test_hashes()
                        .map(|tc| tc.decoded)
                        .nth(1)
                        .unwrap()]),
                encoded: vec![
                    0, 1, 10, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0,
                    26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45, 48, 49, 45, 48,
                    49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
                ],
                expect_encode_err: false,
                expect_decode_err: false,
            },
            TestCase {
                name: "full vertex with multiple parents",
                decoded: Vertex::default()
                    .with_block(dummy_block.clone())
                    .with_parents(generate_test_hashes().map(|tc| tc.decoded).collect()),
                encoded: vec![
                    0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34,
                    2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 1, 10, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 10, 34, 2, 32, 127, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 34, 2, 32, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 26, 72,
                    0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57,
                    55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48,
                    58, 48, 48,
                ],
                expect_encode_err: false,
                expect_decode_err: false,
            },
            TestCase {
                name: "full vertex with redundant parents",
                decoded: Vertex::default()
                    .with_block(dummy_block.clone())
                    .with_parents(dummy_block.parents.clone()),
                encoded: vec![
                    0, 1, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50,
                    25, 49, 57, 55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48,
                    43, 48, 48, 58, 48, 48,
                ],
                expect_encode_err: false, // No error. Encode should silently remove redundant data
                expect_decode_err: true,  // Should not decode block with redundant parent data
            },
            TestCase {
                name: "full vertex with repeated parents",
                decoded: Vertex::default()
                    .with_block(dummy_block.clone())
                    .with_parents(vec![BlockHash::default(), BlockHash::default()]),
                encoded: vec![
                    0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 26, 72,
                    0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57,
                    55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48,
                    58, 48, 48,
                ],

                expect_encode_err: true, // Repeated parents
                expect_decode_err: true, // Repeated parents
            },
            TestCase {
                name: "full vertex with bad block hash",
                decoded: {
                    let mut v = Vertex::default().with_block(dummy_block);
                    v.bhash = BlockHash::default();
                    v
                },
                encoded: vec![
                    0, 1, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0,
                    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50,
                    25, 49, 57, 55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48,
                    43, 48, 48, 58, 48, 48,
                ],
                expect_encode_err: true, // Block hash references a different block
                expect_decode_err: true, // Block hash references a different block
            },
        ]
        .into_iter()
    }

    // Attempt to serialize and deserialize each vertex test case
    #[test]
    fn encoding_vertex() {
        for case in generate_test_vertices() {
            // Encode
            let encode_res = case.decoded.to_wire(true);
            if case.expect_encode_err {
                if encode_res.is_ok() {
                    panic!("Encode({}) unexpectedly succeeded", case.name);
                }
            } else {
                if encode_res.is_err() {
                    println!("Unexpected error: {}", encode_res.unwrap_err());
                    panic!("Encode({}) unexpectedly threw an error", case.name);
                }
                let encoded = encode_res.unwrap();
                if encoded != case.encoded {
                    println!("Expected: {:?}", case.encoded);
                    println!("Have: {:?}", encoded);
                    panic!("Encode({}) result is different than expected", case.name);
                }
            }

            // Decode
            let decode_res = Vertex::from_wire(&case.encoded, true);
            if case.expect_decode_err {
                if decode_res.is_ok() {
                    panic!("Decode({}) unexpectedly succeeded", case.name);
                }
            } else {
                if decode_res.is_err() {
                    println!("Unexpected error: {}", decode_res.unwrap_err());
                    panic!("Decode({}) unexpectedly threw an error", case.name);
                }
                let decoded = decode_res.unwrap();
                if decoded != case.decoded {
                    println!("Expected: {:?}", case.decoded);
                    println!("Have: {:?}", decoded);
                    panic!("Decode({}) result is different than expected", case.name);
                }
            }
        }
    }
}
