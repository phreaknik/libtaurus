use crate::{consensus, p2p::consensus_rpc::proto, Block, BlockHash, VertexHash};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use serde_derive::{Deserialize, Serialize};
use std::{io, result, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

use super::{Error, Result, WireFormat};

/// Current revision of the vertex structure
pub const VERSION: u32 = 0;

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
        P: Iterator<Item = Arc<TracingRwLock<consensus::Vertex>>>,
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
    // TODO: make sure this is used after dynamic parent selection
    pub fn slim(mut self) -> (Option<Arc<Block>>, Vertex) {
        if let Some(block) = self.block {
            self.bhash = block.hash();
            self.block = None;
            (Some(block), self)
        } else {
            (None, self)
        }
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(vertex: &proto::Vertex, check: bool) -> Result<Vertex> {
        let (block, bhash) = if let Some(b) = &vertex.block {
            let block = Block::from_protobuf(b, check)?;
            let bhash = block.hash();
            (Some(Arc::new(block)), bhash)
        } else {
            (
                None,
                BlockHash::from_protobuf(&vertex.block_hash.as_ref().expect("missing block_hash"))?,
            )
        };
        let vertex = Vertex {
            version: vertex.version,
            parents: vertex
                .parents
                .iter()
                .map(|p| VertexHash::from_protobuf(&p))
                .try_collect()?,
            block,
            bhash,
        };
        if check {
            vertex.sanity_checks()?;
        }
        Ok(vertex)
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self, check: bool) -> Result<proto::Vertex> {
        if check {
            self.sanity_checks()?;
        }
        let (block, block_hash) = if let Some(b) = &self.block {
            (Some(b.to_protobuf(check)?), None)
        } else {
            (None, Some(self.bhash.to_protobuf()?))
        };
        Ok(proto::Vertex {
            version: self.version,
            parents: self.parents.iter().map(|p| p.to_protobuf()).try_collect()?,
            block_hash,
            block,
        })
    }
}

impl From<consensus::Vertex> for Vertex {
    fn from(value: consensus::Vertex) -> Self {
        Vertex {
            version: VERSION,
            bhash: value.inner.hash(),
            parents: value.inner.parents.clone(),
            block: value.inner.block.clone(),
        }
    }
}

impl WireFormat for Vertex {
    type Error = Error;

    fn to_wire(&self) -> result::Result<Vec<u8>, Self::Error> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf(true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to consensuss::Vertex to protobuf: {e}"),
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to serialize consensus::Vertex",
            )
        })?;
        Ok(bytes)
    }

    fn from_wire(bytes: &[u8]) -> result::Result<Self, Self::Error> {
        let protobuf = proto::Vertex::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unable to parse vertex from bytes: {e}"),
                )
            })?;
        Vertex::from_protobuf(&protobuf, true)
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
