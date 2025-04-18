use crate::p2p;
use crate::wire::WireFormat;
use heed::{BytesDecode, BytesEncode};
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::result;

/// Error type for vertex errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad hash")]
    BadHash,
    #[error("error decoding from protobuf")]
    ProtoDecode(String),
    #[error(transparent)]
    TryFromSlice(#[from] std::array::TryFromSliceError),
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
            hash: self.0.to_vec(),
        })
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(proto: &p2p::consensus_rpc::proto::Hash) -> Result<Hash> {
        Ok(Hash(proto.hash[..].try_into()?))
    }
}

impl WireFormat for Hash {
    type Error = Error;

    fn to_wire(&self) -> result::Result<Vec<u8>, Self::Error> {
        Ok(self.0.to_vec())
    }

    fn from_wire(bytes: &[u8]) -> result::Result<Self, Self::Error> {
        Ok(Hash(bytes.try_into()?))
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
        Ok(item.to_wire()?.into())
    }
}

impl<'a> BytesDecode<'a> for Hash {
    type DItem = Hash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(Hash::from_wire(bytes.try_into()?)?)
    }
}

impl Default for Hash {
    fn default() -> Self {
        Hash([0; blake3::OUT_LEN])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestCase<'a> {
        input: Hash,
        long_hex: &'a str,
        short_hex: &'a str,
    }
    const TESTS: &[TestCase] = &[
        TestCase {
            input: Hash([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ]),
            long_hex: "0000000000000000000000000000000000000000000000000000000000000000",
            short_hex: "00000000..",
        },
        TestCase {
            input: Hash([
                128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
            ]),
            long_hex: "8000000000000000000000000000000000000000000000000000000000000000",
            short_hex: "80000000..",
        },
        TestCase {
            input: Hash([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1,
            ]),
            long_hex: "0000000000000000000000000000000000000000000000000000000000000001",
            short_hex: "00000000..",
        },
        TestCase {
            input: Hash([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            long_hex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            short_hex: "ffffffff..",
        },
        TestCase {
            input: Hash([
                127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            long_hex: "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            short_hex: "7fffffff..",
        },
        TestCase {
            input: Hash([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254,
            ]),
            long_hex: "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
            short_hex: "ffffffff..",
        },
    ];

    #[test]
    fn to_hex() -> Result<()> {
        for test in TESTS {
            assert_eq!(test.input.to_hex(), test.long_hex);
        }
        Ok(())
    }

    #[test]
    fn to_short_hex() -> Result<()> {
        for test in TESTS {
            assert_eq!(test.input.to_short_hex(), test.short_hex);
        }
        Ok(())
    }
}
