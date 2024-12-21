use crate::{
    util::Randomizer,
    wire::{proto, WireFormat},
};
use rand::Fill;
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::result;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    TryFromSlice(#[from] std::array::TryFromSliceError),
}
type Result<T> = result::Result<T, Error>;

pub const HASH_LEN: usize = blake3::OUT_LEN;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash, PartialOrd, Ord)]
pub struct Hash([u8; HASH_LEN]);

impl Hash {
    /// Instantiate a hash object from the given bytes
    pub const fn with_bytes(bytes: [u8; HASH_LEN]) -> Hash {
        Hash(bytes)
    }

    /// Yield a reference to the inner bytes
    pub fn as_slice(&self) -> &[u8; 32] {
        &self.0
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

impl Default for Hash {
    fn default() -> Self {
        Hash([0; HASH_LEN])
    }
}

impl Randomizer for Hash {
    fn random() -> Self {
        let mut bytes = [0; HASH_LEN];
        bytes.try_fill(&mut rand::thread_rng()).unwrap();
        Hash(bytes)
    }
}
