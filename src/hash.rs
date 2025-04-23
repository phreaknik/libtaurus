use crate::wire::{proto, WireFormat};
use heed::{BytesDecode, BytesEncode};
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::result;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad hash")]
    BadHash,
    #[error(transparent)]
    Protobuf(#[from] quick_protobuf::Error),
    #[error(transparent)]
    TryFromSlice(#[from] std::array::TryFromSliceError),
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
}

/// Result type for hash errors
pub type Result<T> = result::Result<T, Error>;

pub const HASH_LEN: usize = blake3::OUT_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, PartialOrd, Ord)]
pub struct Hash([u8; HASH_LEN]);

impl Hash {
    /// Instantiate a hash object from the given bytes
    pub const fn with_bytes(bytes: [u8; HASH_LEN]) -> Hash {
        Hash(bytes)
    }

    /// Format the Hash as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Format the Hash as a short hex string, better for displaying Hashes in large
    /// collections of data
    pub fn to_short_hex(&self) -> String {
        format!("{}..", hex::encode(&self.0[..4]))
    }
}

impl<'a> WireFormat<'a, proto::Hash> for Hash {
    type Error = Error;

    fn to_protobuf(&self, _check: bool) -> Result<proto::Hash> {
        Ok(proto::Hash {
            hash: self.0.to_vec(),
        })
    }

    fn from_protobuf(proto: &proto::Hash, _check: bool) -> Result<Hash> {
        Ok(Hash(proto.hash[..].try_into()?))
    }
}

impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_short_hex())
    }
}

impl From<blake3::Hash> for Hash {
    fn from(hash: blake3::Hash) -> Self {
        Hash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &Hash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl Into<blake3::Hash> for Hash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

// BytesEncode redundant with WireFormat?
impl<'a> BytesEncode<'a> for Hash {
    type EItem = Hash;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(item.to_wire(false)?.into())
    }
}

impl<'a> BytesDecode<'a> for Hash {
    type DItem = Hash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(Hash::from_wire(bytes.try_into()?, false)?)
    }
}

impl Default for Hash {
    fn default() -> Self {
        Hash([0; blake3::OUT_LEN])
    }
}
