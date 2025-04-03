use super::Result;
use super::{hash::Hash, Error};
use crate::randomx::RandomXVMInstance;
use crate::{params, Header};
use libp2p::PeerId;
use num::{BigUint, FromPrimitive};
use serde::{Deserialize, Serialize};
use serde_cbor;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorTicket {
    pub peer: PeerId,
    pub heads: Vec<Hash>,
    pub height: u64,
    pub difficulty: u64,
    pub nonce: u64,
}

impl ValidatorTicket {
    /// Construct a new ticket to be mined
    pub fn new(peer: PeerId, heads: Vec<Header>) -> Result<ValidatorTicket> {
        if heads.iter().count() == 0 {
            Err(Error::EmptyCollection)
        } else {
            Ok(ValidatorTicket {
                peer,
                heads: heads.iter().map(|h| h.hash()).collect(),
                height: heads[0].height,
                difficulty: heads.into_iter().map(|h| h.difficulty).max().unwrap(),
                nonce: 0,
            })
        }
    }

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

impl Default for ValidatorTicket {
    fn default() -> Self {
        ValidatorTicket {
            peer: PeerId::random(),
            heads: Vec::new(),
            height: 0,
            difficulty: 0,
            nonce: 0,
        }
    }
}
