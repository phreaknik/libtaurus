mod vertex;

use crate::{consensus::block, hash::Hash};
use std::{fmt::Debug, io, result};
pub use vertex::Vertex;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("block hash does not match self.bhash")]
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

pub trait WireFormat: Sized {
    type Error: Debug;

    /// Serialize data into a format suitable for wire transmission
    fn to_wire(&self) -> result::Result<Vec<u8>, Self::Error>;

    /// Deserialize data from wire format
    fn from_wire(bytes: &[u8]) -> result::Result<Self, Self::Error>;

    /// Compute hash of the serialized data
    fn hash(&self) -> Hash {
        blake3::hash(&self.to_wire().expect("Error encoding to wire format")).into()
    }
}
