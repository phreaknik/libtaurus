use crate::p2p;
use heed::BytesDecode;
use heed::BytesEncode;
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::result;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad hash")]
    BadHash,
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error("error decoding from protobuf")]
    ProtoDecode(String),
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
}

/// Result type for hash errors
pub type Result<T> = result::Result<T, Error>;

pub const HASH_LEN: usize = blake3::OUT_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Hash([u8; HASH_LEN]);

impl Hash {
    /// Format the Hash as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Format the Hash as a short hex string, better for displaying Hashes in large
    /// collections of data
    pub fn to_short_hex(&self) -> String {
        format!("{}..", hex::encode(&self.0[..4]))
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::consensus_rpc::proto::Hash> {
        Ok(p2p::consensus_rpc::proto::Hash {
            hash: rmp_serde::to_vec(&self)?,
        })
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(proto: &p2p::consensus_rpc::proto::Hash) -> Result<Hash> {
        Ok(Hash(rmp_serde::from_slice(&proto.hash)?))
    }

    /// The raw bytes of the `Hash`.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; HASH_LEN] {
        &self.0
    }

    /// Create a `Hash` from its raw bytes representation.
    pub const fn from_bytes(bytes: [u8; HASH_LEN]) -> Self {
        Self(bytes)
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

impl<'a> BytesEncode<'a> for Hash {
    type EItem = Hash;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for Hash {
    type DItem = Hash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

impl Default for Hash {
    fn default() -> Self {
        Hash([0; blake3::OUT_LEN])
    }
}
