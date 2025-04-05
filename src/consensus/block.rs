use super::{Error, Result};
use crate::{params, randomx::RandomXVMInstance};
use blake3::Hash;
use chrono::{DateTime, Utc};
use libp2p::{multihash::Multihash, PeerId};
use num::{BigUint, FromPrimitive};
use serde_derive::{Deserialize, Serialize};

/// A compact representation of the vertex, suitable for serialization and transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    pub version: u32,
    pub parents: Vec<SerdeHash>,
    pub height: u64,
    pub difficulty: u64,
    pub miner: PeerId,
    pub time: DateTime<Utc>,
    pub nonce: u64,
}

impl Block {
    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<Hash> {
        Ok(blake3::hash(&serde_cbor::to_vec(self)?))
    }

    /// Compute the mining target from the given difficulty
    pub fn mining_target(&self) -> Result<BigUint> {
        if self.difficulty < params::MIN_DIFFICULTY {
            Err(Error::InvalidDifficulty)
        } else {
            Ok(
                BigUint::from_u64(2).unwrap().pow(256)
                    / BigUint::from_u64(self.difficulty).unwrap(),
            )
        }
    }

    /// Check if the block has valid proof-of-work
    pub fn verify_pow(&self, randomx: &RandomXVMInstance) -> Result<()> {
        if BigUint::from_bytes_be(&randomx.calculate_hash(&serde_cbor::to_vec(self)?)?)
            < self.mining_target()?
        {
            Ok(())
        } else {
            Err(Error::InvalidPoW)
        }
    }
}

/// This implementation of ['Default'] is nonsense and should never be used. It is only implemented
/// to satisfy a trait boundary, but the actual contents are not used.
impl Default for Block {
    fn default() -> Self {
        Block {
            version: 0,
            parents: Vec::new(),
            height: 0,
            difficulty: 0,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            time: Utc::now(),
            nonce: 0,
        }
    }
}

/// Wrapper struct around blake3::Hash to facilitate serde implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerdeHash([u8; blake3::OUT_LEN]);

impl From<blake3::Hash> for SerdeHash {
    fn from(hash: blake3::Hash) -> Self {
        SerdeHash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &SerdeHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}
