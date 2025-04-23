mod generated;

use crate::{consensus::block, hash::Hash};
pub use generated::proto;
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::{
    fmt::{Debug, Display},
    result,
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
    type Error: Debug + From<quick_protobuf::Error> + Display;

    /// Serialize into protobuf format
    fn to_protobuf(&self, check: bool) -> result::Result<P, Self::Error>;

    /// Deserialize from protobuf format
    fn from_protobuf(data: &P, check: bool) -> result::Result<Self, Self::Error>;

    /// Serialize data into a format suitable for wire transmission, optionally checking validity
    /// of the data before serialization.
    fn to_wire(&self, check: bool) -> result::Result<Vec<u8>, Self::Error> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf(check)?;
        protobuf.write_message(&mut writer)?;
        Ok(bytes)
    }

    /// Deserialize data from wire format, optionally checking validity of the data before
    /// deserialization.
    fn from_wire(bytes: &'a [u8], check: bool) -> result::Result<Self, Self::Error> {
        let protobuf =
            <P as MessageRead>::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)?;
        Self::from_protobuf(&protobuf, check)
    }

    /// Compute hash of the serialized data
    fn hash(&self) -> Hash {
        blake3::hash(&self.to_wire(false).expect("Error encoding to wire format")).into()
    }
}

/// Tests that encoded data matches decoded after encoding and decoding each. Returns the
/// encode and decode errors, if any.
pub fn test_wire_format<'a, P, D>(
    decoded: D,
    encoded: &'a [u8],
) -> (
    Option<<D as WireFormat<'a, P>>::Error>,
    Option<<D as WireFormat<'a, P>>::Error>,
)
where
    P: MessageWrite + MessageRead<'a>,
    D: WireFormat<'a, P> + Debug + Eq,
{
    let encode_err = match decoded.to_wire(true) {
        Ok(bytes) => {
            assert_eq!(bytes, encoded);
            None
        }
        Err(e) => Some(e),
    };
    let decode_err = match <D as WireFormat<'a, P>>::from_wire(&encoded, true) {
        Ok(block) => {
            assert_eq!(block, decoded);
            None
        }
        Err(e) => Some(e),
    };
    (encode_err, decode_err)
}
