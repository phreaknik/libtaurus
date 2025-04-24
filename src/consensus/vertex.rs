use super::block;
use crate::{
    wire::{proto, WireFormat},
    Block, BlockHash,
};
use itertools::Itertools;
use serde_derive::{Deserialize, Serialize};
use std::{result, sync::Arc};

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
    Protobuf(#[from] quick_protobuf::Error),
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
pub type Result<T> = result::Result<T, Error>;

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
    pub parents: Option<Vec<VertexHash>>,

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
            parents: None,
            block: Some(block),
        }
    }

    /// Construct a new vertex for a given block, with just a reference to a block
    pub fn new_slim(bhash: BlockHash, parents: Vec<VertexHash>) -> Vertex {
        Vertex {
            version: VERSION,
            bhash,
            parents: Some(parents),
            block: None,
        }
    }

    /// Construct the genesis vertex, with the given the genesis block
    pub fn genesis(block: Block) -> Vertex {
        Vertex {
            version: 1,
            parents: None,
            bhash: block.hash(),
            block: Some(Arc::new(block)),
        }
    }

    /// Update a vertex to contain the given block
    pub fn with_block(mut self, block: Arc<Block>) -> Vertex {
        self.bhash = block.hash();
        self.parents = None;
        self.block = Some(block);
        self
    }

    /// Set new parents for this vertex, clearing the block
    pub fn with_new_parents(mut self, parents: Vec<VertexHash>) -> Vertex {
        self.parents = Some(parents);
        self.block = None;
        self
    }

    /// Get the parents for the given vertex
    pub fn parents(&self) -> &Vec<VertexHash> {
        if let Some(b) = &self.block {
            assert!(self.parents.is_none(), "slim vertex must not have parents");
            &b.parents
        } else {
            &self
                .parents
                .as_ref()
                .expect("slim vertex must have parents!")
        }
    }

    /// Slim this vertex by removing the block
    pub fn slim(self) -> (Option<Arc<Block>>, Vertex) {
        let block = self.block.clone();
        let parents = self.parents().clone();
        let slim = self.with_new_parents(parents);
        (block, slim)
    }

    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version > VERSION {
            Err(Error::BadVertexVersion(self.version))
        } else if let Some(block) = &self.block {
            if block.hash() != self.bhash {
                Err(Error::BadBlockHash)
            } else if self.parents.is_some() {
                Err(Error::RedundantParents)
            } else {
                Ok(block.sanity_checks()?)
            }
        } else if self.parents.is_none() {
            Err(Error::EmptyParents)
        } else if !self.parents.as_ref().unwrap().iter().all_unique() {
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
        let (block, block_hash) = if let Some(b) = &self.block {
            (Some(b.to_protobuf(check)?), None)
        } else {
            (None, Some(self.bhash.to_protobuf(check)?))
        };
        let parents = self
            .parents
            .as_ref()
            .unwrap_or(&Vec::new())
            .iter()
            .map(|p| p.to_protobuf(check))
            .try_collect()?;
        Ok(proto::Vertex {
            version: self.version,
            parents,
            block_hash,
            block,
        })
    }

    fn from_protobuf(vertex: &proto::Vertex, check: bool) -> Result<Vertex> {
        let pars: Vec<_> = vertex
            .parents
            .iter()
            .map(|p| VertexHash::from_protobuf(&p, check))
            .try_collect()?;
        let (block, bhash) = if let Some(b) = &vertex.block {
            if vertex.block_hash.is_some() {
                Err(Error::RedundantBlockHash)
            } else {
                let block = Block::from_protobuf(b, check)?;
                let bhash = block.hash();
                Ok((Some(Arc::new(block)), bhash))
            }
        } else {
            Ok((
                None,
                BlockHash::from_protobuf(
                    &vertex.block_hash.as_ref().expect("missing block_hash"),
                    check,
                )?,
            ))
        }?;
        let vertex = Vertex {
            version: vertex.version,
            parents: if pars.is_empty() { None } else { Some(pars) },
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
        if self.parents().contains(&other.hash()) {
            Some(std::cmp::Ordering::Greater)
        } else if other.parents().contains(&self.hash()) {
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
            parents: vertex.parents().iter().map(|txo| txo.to_hex()).collect(),
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
