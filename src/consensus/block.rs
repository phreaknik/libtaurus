use crate::{
    p2p, params,
    randomx::{self, RandomXVMInstance},
};
use chrono::{DateTime, Utc};
use heed::{BytesDecode, BytesEncode};
use libp2p::{multihash::Multihash, PeerId};
use num::{BigUint, FromPrimitive};
use serde_derive::{Deserialize, Serialize};
use std::{fmt, hash::Hash, result};

/// Error type for block errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error("invalid difficulty")]
    InvalidDifficulty,
    #[error("invalid proof-of-work")]
    InvalidPoW,
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Multihash(#[from] libp2p::multihash::Error),
    #[error(transparent)]
    RandomX(#[from] randomx::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Result type for block errors
pub type Result<T> = result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Block {
    pub version: u32,
    pub difficulty: u64,
    pub miner: PeerId,
    pub parents: Vec<BlockHash>,
    pub inputs: Vec<BlockHash>, // TODO: define real UTXOs
    pub time: DateTime<Utc>,
    pub nonce: u64,
}

impl Block {
    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<BlockHash> {
        Ok(blake3::hash(&rmp_serde::to_vec(self)?).into())
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
        if BigUint::from_bytes_be(&randomx.calculate_hash(&rmp_serde::to_vec(self)?)?)
            < self.mining_target()?
        {
            Ok(())
        } else {
            Err(Error::InvalidPoW)
        }
    }

    /// Print the parents into a formatted string
    pub fn format_parents(&self) -> String {
        let mut s = format!("[{}", self.parents[0]);
        for p in &self.parents[1..] {
            s += &format!(", {p}");
        }
        s += "]";
        s
    }
}

impl std::fmt::Debug for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "block: {}",
            serde_json::to_string_pretty(&PrettyBlock::from(self)).unwrap()
        )
    }
}

/// This implementation of ['Default'] is nonsense and should never be used. It is only implemented
/// to satisfy a trait boundary, but the actual contents are not used.
impl Default for Block {
    fn default() -> Self {
        Block {
            version: 0,
            difficulty: 0,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            parents: Vec::new(),
            inputs: Vec::new(),
            time: Utc::now(),
            nonce: 0,
        }
    }
}

impl TryFrom<p2p::avalanche_rpc::proto::Block> for Block {
    type Error = Error;

    fn try_from(block: p2p::avalanche_rpc::proto::Block) -> result::Result<Self, Self::Error> {
        Ok(Block {
            version: block.version,
            difficulty: block.difficulty,
            miner: PeerId::from_bytes(&block.miner)?,
            parents: block
                .parents
                .iter()
                .map(|bytes| rmp_serde::from_slice(bytes))
                .try_collect()?,
            inputs: block
                .inputs
                .iter()
                .map(|bytes| rmp_serde::from_slice(bytes))
                .try_collect()?,
            time: rmp_serde::from_slice(&block.time)?,
            nonce: block.nonce,
        })
    }
}

impl TryInto<p2p::avalanche_rpc::proto::Block> for Block {
    type Error = Error;

    fn try_into(self) -> result::Result<p2p::avalanche_rpc::proto::Block, Self::Error> {
        Ok(p2p::avalanche_rpc::proto::Block {
            version: self.version,
            difficulty: self.difficulty,
            miner: self.miner.to_bytes(),
            parents: self
                .parents
                .iter()
                .map(|p| rmp_serde::to_vec(p))
                .try_collect()?,
            inputs: self
                .inputs
                .iter()
                .map(|i| rmp_serde::to_vec(i))
                .try_collect()?,
            time: rmp_serde::to_vec(&self.time)?,
            nonce: self.nonce,
        })
    }
}

/// Wrapper struct around blake3::Hash to facilitate serde implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct BlockHash([u8; blake3::OUT_LEN]);

impl BlockHash {
    /// Format the BlockHash as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Format the BlockHash as a short hex string, better for displaying BlockHashes in large
    /// collections of data
    pub fn to_short_hex(&self) -> String {
        format!("{}..", hex::encode(&self.0[..4]))
    }
}

impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_short_hex())
    }
}

impl From<blake3::Hash> for BlockHash {
    fn from(hash: blake3::Hash) -> Self {
        BlockHash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &BlockHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl Into<blake3::Hash> for BlockHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl<'a> BytesEncode<'a> for BlockHash {
    type EItem = BlockHash;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for BlockHash {
    type DItem = BlockHash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

impl Default for BlockHash {
    fn default() -> Self {
        BlockHash([0; blake3::OUT_LEN])
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrettyBlock {
    version: u32,
    difficulty: u64,
    miner: PeerId,
    parents: Vec<String>,
    inputs: Vec<String>,
    time: DateTime<Utc>,
    nonce: u64,
}

impl From<&Block> for PrettyBlock {
    fn from(block: &Block) -> Self {
        PrettyBlock {
            version: block.version,
            parents: block
                .parents
                .iter()
                .map(|p| p.0.iter().map(|b| format!("{b:02x}")).collect())
                .collect(),
            inputs: block
                .inputs
                .iter()
                .map(|p| p.0.iter().map(|b| format!("{b:02x}")).collect())
                .collect(),
            difficulty: block.difficulty,
            miner: block.miner,
            time: block.time,
            nonce: block.nonce,
        }
    }
}
