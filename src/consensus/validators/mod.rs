pub mod database;
pub mod raffle;

use super::{hash::Hash, Error, Result};
use crate::{params, randomx::RandomXVMInstance, Header};
use chrono::{DateTime, Utc};
use heed::{BytesDecode, BytesEncode};
use libp2p::PeerId;
use num::{BigUint, FromPrimitive};
pub use raffle::ValidatorQuorum;
use serde_derive::{Deserialize, Serialize};

/// Any node may submit a mined ValidatorTicket to be entered into the validator raffle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorTicket {
    pub peer: PeerId,
    pub heads: Vec<Hash>,
    pub height: u64,
    pub difficulty: u64,
    pub time: DateTime<Utc>,
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
                time: Utc::now(),
                nonce: 0,
            })
        }
    }

    /// Update the timestamp to the current UTC time
    pub fn update_timestamp(&mut self) -> &mut Self {
        self.time = Utc::now();
        self
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
            time: Utc::now(),
            nonce: 0,
        }
    }
}

impl<'a> BytesEncode<'a> for ValidatorTicket {
    type EItem = ValidatorTicket;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for ValidatorTicket {
    type DItem = ValidatorTicket;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}
