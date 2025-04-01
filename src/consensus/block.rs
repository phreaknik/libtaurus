use super::hash::Hash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_cbor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    version: u32,
    height: u64,
    parents: Vec<Hash>,
    difficulty: u64,
    nonce: u64,
    time: DateTime<Utc>,
}

impl Header {
    /// Compute the block hash
    fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    header: Header,
}

impl Block {
    /// Compute the block hash
    fn hash(&self) -> Hash {
        self.header.hash()
    }
}
