use crate::consensus::{Error, Result};
use crate::Hash;
use crate::{params, randomx::RandomXVMInstance};
use chrono::{DateTime, Utc};
use libp2p::multihash::Multihash;
use libp2p::PeerId;
use num::{BigUint, FromPrimitive};
use serde::{Deserialize, Serialize};
use serde_cbor;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Frontier(pub Vec<Header>);

impl Frontier {
    /// Compute the difficulty of the current frontier
    pub fn difficulty(&self) -> u64 {
        self.0.iter().map(|h| h.difficulty).min().unwrap()
    }

    /// Compute a candidate block to mine atop the given frontier
    pub fn to_candidate_block(&self, miner: PeerId) -> Block {
        Block::new(Header {
            version: params::PROTOCOL_VERSION,
            height: self.0[0].height + 1,
            parents: self.0.iter().map(|header| header.hash()).collect(),
            difficulty: self.difficulty(), // TODO: needs to adjust
            miner,
            time: Utc::now(),
            nonce: 0,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Header {
    pub version: u32,
    pub height: u64,
    pub parents: Vec<Hash>,
    pub difficulty: u64,
    pub miner: PeerId,
    pub time: DateTime<Utc>,
    pub nonce: u64,
}

impl Header {
    /// Compute the block hash
    pub fn hash(&self) -> Hash {
        blake3::hash(&serde_cbor::to_vec(self).unwrap()).into()
    }

    /// Update the timestamp to the current UTC time
    pub fn update_timestamp(&mut self) -> &mut Self {
        self.time = Utc::now();
        self
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

impl From<Block> for Header {
    fn from(b: Block) -> Self {
        b.header
    }
}

/// This implementation of ['Default'] is nonsense and should never be used. It is only implemented
/// to satisfy a trait boundary, but the actual contents are not used.
impl Default for Header {
    fn default() -> Self {
        Header {
            version: 0,
            height: 0,
            parents: Vec::new(),
            difficulty: 0,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            time: Utc::now(),
            nonce: 0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Block {
    pub header: Header,
}

impl Block {
    /// Construct a new block with the given header
    pub fn new(header: Header) -> Block {
        Block { header }
    }
    /// Wraps ['Header::hash']
    pub fn hash(&self) -> Hash {
        self.header.hash()
    }
}
