mod generated;

use crate::{consensus::block, hash::Hash};
pub use generated::proto;
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::{
    fmt::{Debug, Display},
    io, result,
};

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

pub trait WireFormat<'a, P>: Sized
where
    P: MessageWrite + MessageRead<'a>,
{
    type Error: Debug + From<std::io::Error> + Display;

    /// Serialize into protobuf format
    fn to_protobuf(&self, check: bool) -> result::Result<P, Self::Error>;

    /// Deserialize from protobuf format
    fn from_protobuf(block: &P, check: bool) -> result::Result<Self, Self::Error>;

    /// Serialize data into a format suitable for wire transmission, optionally checking validity
    /// of the data before serialization.
    fn to_wire(&self, check: bool) -> result::Result<Vec<u8>, Self::Error> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf(check).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert Block to protobuf: {e}"),
            )
        })?;
        protobuf
            .write_message(&mut writer)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "unable to serialize Block"))?;
        Ok(bytes)
    }

    /// Deserialize data from wire format, optionally checking validity of the data before
    /// deserialization.
    fn from_wire(bytes: &'a [u8], check: bool) -> result::Result<Self, Self::Error> {
        let protobuf = <P as MessageRead>::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unable to parse block from bytes: {e}"),
                )
            })?;
        Self::from_protobuf(&protobuf, check)
    }

    /// Compute hash of the serialized data
    fn hash(&self) -> Hash {
        blake3::hash(&self.to_wire(false).expect("Error encoding to wire format")).into()
    }
}
