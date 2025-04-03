use super::hash::Hash;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_cbor;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Header {
    pub version: u32,
    pub height: u64,
    pub parents: Vec<Hash>,
    pub difficulty: u64,
    pub nonce: u64,
    pub time: DateTime<Utc>,
}

impl Header {
    /// Compute the block hash
    pub fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
    }
}

impl From<Block> for Header {
    fn from(b: Block) -> Self {
        b.header
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Block {
    pub header: Header,
}

impl Block {
    /// Wraps ['Header::hash']
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }
}
