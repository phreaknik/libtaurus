use super::transaction::{self, Txo, TxoHash};
use crate::{
    p2p,
    params::{self, GENESIS_DIFFICULTY},
    randomx::{self, RandomXVMInstance},
};
use chrono::{DateTime, Utc};
use heed::{BytesDecode, BytesEncode};
use libp2p::{multihash::Multihash, PeerId};
use num::{BigUint, FromPrimitive};
use serde_derive::{Deserialize, Serialize};
use std::{fmt, hash::Hash, result};

/// Current version of the block structure
pub const VERSION: u32 = 32;

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
    Transaction(#[from] transaction::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Result type for block errors
pub type Result<T> = result::Result<T, Error>;

#[derive(Clone, Serialize, Deserialize)]
pub struct Block {
    /// TODO: Need block validation rules. Not much though:
    ///    * version must match
    ///    * difficulty must be valid (maybe DAA control average DAG width, not production time?)
    ///        * ^ this is actually very interesting. Feedback loop regulates for network
    ///        efficiency, instead of for block production time as a proxy for efficiency.
    ///    * time must be relatively recent and not future

    /// Block format revision number
    pub version: u32,

    /// Difficulty this block's proof-of-work must satisfy
    pub difficulty: u64,

    /// Which peer mined this block
    pub miner: PeerId,

    /// Block hash of this peer's previously mined block
    pub prev_mined: Option<BlockHash>,

    /// Transaction outputs consumed by this block
    pub inputs: Vec<TxoHash>,

    /// Transaction outputs created by this bock
    pub outputs: Vec<Txo>,

    /// Time when the miner claims to have solved this proof-of-work
    pub time: DateTime<Utc>, // TODO: don't need a timestamp... might be nice in Vertex though

    /// Nonce used in proof-of-work
    pub nonce: u64,
}

impl Block {
    /// Compute the hash of the block
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

    /// Deserialize from protobuf format
    pub fn from_protobuf(block: p2p::avalanche_rpc::proto::Block) -> Result<Block> {
        Ok(Block {
            version: block.version,
            difficulty: block.difficulty,
            miner: PeerId::from_bytes(&block.miner)?,
            prev_mined: match block.prev_mined {
                Some(hash) => Some(BlockHash::from_protobuf(&hash)?),
                None => None,
            },
            inputs: block
                .inputs
                .iter()
                .map(|txo_hash| TxoHash::from_protobuf(txo_hash))
                .try_collect()?,
            outputs: block
                .outputs
                .iter()
                .map(|txo| Txo::from_protobuf(txo))
                .try_collect()?,
            time: rmp_serde::from_slice(&block.time)?,
            nonce: block.nonce,
        })
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Block> {
        Ok(p2p::avalanche_rpc::proto::Block {
            version: self.version,
            difficulty: self.difficulty,
            miner: self.miner.to_bytes(),
            prev_mined: match self.prev_mined {
                Some(hash) => Some(hash.to_protobuf()?),
                None => None,
            },
            inputs: self
                .inputs
                .iter()
                .map(|txo_hash| txo_hash.to_protobuf())
                .try_collect()?,
            outputs: self
                .outputs
                .iter()
                .map(|txo| txo.to_protobuf())
                .try_collect()?,
            time: rmp_serde::to_vec(&self.time)?,
            nonce: self.nonce,
        })
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
            version: VERSION,
            difficulty: GENESIS_DIFFICULTY,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            prev_mined: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            time: Utc::now(),
            nonce: 0,
        }
    }
}

impl<'a> BytesEncode<'a> for Block {
    type EItem = Block;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for Block {
    type DItem = Block;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

// TODO: These hash types should be wrappers around one common hash type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct BlockHash(pub [u8; blake3::OUT_LEN]);

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

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Hash> {
        Ok(p2p::avalanche_rpc::proto::Hash {
            hash: rmp_serde::to_vec(&self)?,
        })
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(proto: &p2p::avalanche_rpc::proto::Hash) -> Result<BlockHash> {
        Ok(BlockHash(rmp_serde::from_slice(&proto.hash)?))
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
    inputs: Vec<String>,
    time: DateTime<Utc>,
    nonce: u64,
}

impl From<&Block> for PrettyBlock {
    fn from(block: &Block) -> Self {
        PrettyBlock {
            version: block.version,
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
