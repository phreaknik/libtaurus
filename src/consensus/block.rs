use super::Result;
use super::{hash::Hash, Error};
use crate::params;
use crate::randomx::RandomXVMInstance;
use chrono::{DateTime, Utc};
use num::{BigUint, FromPrimitive};
use serde::{Deserialize, Serialize};
use serde_cbor;
use std::convert::TryFrom;

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frontier {
    pub heads: Vec<Header>,
    pub nonce: u64,
}

impl Frontier {
    /// Compute the frontier hash
    pub fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
    }

    /// Return the DAG height of the frontier
    pub fn height(&self) -> Result<u64> {
        if self.heads.len() <= 0 {
            // Make sure the frontier isn't empty
            Err(Error::EmptyFrontier)
        } else if !self
            // Make sure each head is for the same height
            .heads
            .iter()
            .map(|h| h.height)
            .all(|height| height == self.heads[0].height)
        {
            Err(Error::InvalidFrontier)
        } else {
            // Return the head height
            Ok(self.heads[0].height)
        }
    }

    /// Compute the difficulty of the frontier, as a function of all its heads
    pub fn difficulty(&self) -> Result<u64> {
        if self.heads.len() <= 0 {
            Err(Error::EmptyFrontier)
        } else {
            Ok(self.heads.iter().map(|h| h.difficulty).max().unwrap())
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlimFrontier {
    pub heads: Vec<Hash>,
    pub height: u64,
    pub difficulty: u64,
    pub nonce: u64,
}

impl SlimFrontier {
    /// Compute the frontier hash
    pub fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
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

impl TryFrom<Frontier> for SlimFrontier {
    type Error = Error;
    fn try_from(value: Frontier) -> std::result::Result<Self, Self::Error> {
        Ok(SlimFrontier {
            heads: value.heads.iter().map(|h| h.hash()).collect(),
            height: value.height()?,
            difficulty: value.difficulty()?,
            nonce: value.nonce,
        })
    }
}
