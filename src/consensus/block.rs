use super::Result;
use super::{hash::Hash, Error};
use chrono::{DateTime, Utc};
use num::{BigUint, FromPrimitive};
use rand;
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
    pub fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
    }

    /// Compute the mining target from the given difficulty
    pub fn mining_target(&self) -> Result<BigUint> {
        if self.difficulty == 0 {
            Err(Error::InvalidDifficulty)
        } else {
            Ok(
                BigUint::from_u64(2).unwrap().pow(256)
                    / BigUint::from_u64(self.difficulty).unwrap(),
            )
        }
    }

    /// Check if the block has valid proof-of-work
    pub fn verify_pow(&self) -> Result<()> {
        if BigUint::from_bytes_be(self.hash().as_bytes()) < self.mining_target()? {
            Ok(())
        } else {
            Err(Error::InvalidPoW)
        }
    }

    /// Find a nonce which satisfies the difficulty target
    pub fn mine(mut self) -> Result<Header> {
        let target = self.mining_target()?;
        let mut hasher = blake3::Hasher::new();
        self.nonce = rand::random();
        loop {
            hasher.update(&serde_cbor::to_vec(&self).unwrap());
            if BigUint::from_bytes_be(hasher.finalize().as_bytes()) < target {
                return Ok(self);
            }
            self.nonce += 1;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Block {
    header: Header,
}

impl Block {
    /// Wraps ['Header::hash']
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }
}
