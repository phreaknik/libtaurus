use super::block;
use crate::{
    wire::{generated::proto, WireFormat},
    Block, BlockHash,
};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::{result, sync::Arc};

/// Current revision of the vertex structure
pub const VERSION: u32 = 0;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("block hash does not match block")]
    BadBlockHash,
    #[error("bad version")]
    BadVersion(u32),
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error("vertex does not specify any parents")]
    EmptyParents,
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error("encoded vertex contains redundant block hash")]
    RedundantBlockHash,
    #[error("vertex parents are redundant with block parents")]
    RedundantParents,
    #[error("some vertex parents are repeated")]
    RepeatedParents,
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
}

/// Result type for vertex errors
type Result<T> = result::Result<T, Error>;

/// Type alias for vertex hashes
pub type VertexHash = crate::hash::Hash;

/// A vertex descibes a block's adapted position within the DAG. Importantly, its adapted position
/// may differ from its mined position, due to dynamic parent reselection, in the case of
/// non-virtuous parents of the mined block..
#[derive(Clone, Default, Debug, PartialEq, Eq)]
pub struct Vertex {
    /// Revision number of the vertex structure
    pub version: u32,

    // TODO: bhash should be optional & handled same as parents
    /// Hash of the block this vertex represents
    pub bhash: BlockHash,

    /// Adaptively reselected parents, if any
    pub parents: Vec<VertexHash>,

    /// Optional full block
    pub block: Option<Arc<Block>>,
}

impl Vertex {
    /// Construct a new vertex for a given block, with a full block
    ///
    /// A full vertex does not specify parents, because the block already has parents.
    pub fn new_full(block: Arc<Block>) -> Vertex {
        Vertex {
            version: VERSION,
            bhash: block.hash(),
            parents: block.parents.clone(),
            block: Some(block),
        }
    }

    /// Construct a new vertex for a given block, with just a reference to a block
    pub fn new_slim(bhash: BlockHash, parents: Vec<VertexHash>) -> Vertex {
        Vertex {
            version: VERSION,
            bhash,
            parents,
            block: None,
        }
    }

    /// Construct the genesis vertex, with the given the genesis block
    pub fn genesis(block: Block) -> Vertex {
        Vertex {
            version: 0,
            parents: Vec::new(),
            bhash: block.hash(),
            block: Some(Arc::new(block)),
        }
    }

    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version > VERSION {
            Err(Error::BadVersion(self.version))
        } else if let Some(block) = &self.block {
            if block.hash() != self.bhash {
                Err(Error::BadBlockHash)
            } else {
                Ok(block.sanity_checks()?)
            }
        } else if self.parents.is_empty() {
            Err(Error::EmptyParents)
        } else if !self.parents.iter().all_unique() {
            Err(Error::RepeatedParents)
        } else {
            Ok(())
        }
    }
}

impl<'a> WireFormat<'a, proto::Vertex> for Vertex {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Vertex> {
        // Optionally perform sanity checks
        if check {
            self.sanity_checks()?;
        }
        // Only encode the blockhash if the full block is not present
        let (block, block_hash, parents) = if let Some(b) = &self.block {
            // Only encode vertex parents if they differ from the block parents
            let parents = if self.parents.len() != b.parents.len()
                || self.parents.iter().any(|p| !b.parents.contains(p))
            {
                self.parents.clone()
            } else {
                Vec::new()
            };
            (Some(b.to_protobuf(check)?), None, parents)
        } else {
            (
                None,
                Some(self.bhash.to_protobuf(check)?),
                self.parents.clone(),
            )
        };
        Ok(proto::Vertex {
            version: self.version,
            parents: parents.iter().map(|p| p.to_protobuf(check)).try_collect()?,
            block_hash,
            block,
        })
    }

    fn from_protobuf(vertex: &proto::Vertex, check: bool) -> Result<Vertex> {
        let (bhash, block) = if let Some(b) = &vertex.block {
            if vertex.block_hash.is_some() {
                Err(Error::RedundantBlockHash)
            } else {
                let block = Block::from_protobuf(b, check)?;
                Ok((block.hash(), Some(Arc::new(block))))
            }
        } else {
            Ok((
                BlockHash::from_protobuf(
                    &vertex.block_hash.as_ref().expect("missing block_hash"),
                    check,
                )?,
                None,
            ))
        }?;
        // Get parents from block, if not explicitely provided in vertex
        let parents = if !vertex.parents.is_empty() {
            let parents: Vec<_> = vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(&p, check))
                .try_collect()?;
            // Vertex parents may only be supplid if they differ from block parents
            if let Some(b) = &block {
                if parents.len() == b.parents.len()
                    && parents.iter().all(|p| b.parents.iter().contains(p))
                {
                    return Err(Error::RedundantParents);
                }
            }
            parents
        } else if let Some(b) = &block {
            b.parents.clone()
        } else {
            Vec::new()
        };
        // Parents must be provided
        if parents.is_empty() {
            return Err(Error::EmptyParents);
        }
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
            parents: vertex.parents.iter().map(|p| p.to_hex()).collect(),
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
