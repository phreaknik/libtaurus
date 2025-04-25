use super::transaction::{self, Txo, TxoHash};
use crate::{
    params::{self, FUTURE_BLOCK_LIMIT_SECS, MIN_DIFFICULTY},
    randomx::{self, RandomXVMInstance},
    util::Randomizer,
    wire::{generated::proto, WireFormat},
    VertexHash,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use heed::{BytesDecode, BytesEncode};
use itertools::Itertools;
use libp2p::{multihash::Multihash, PeerId};
use num::{BigUint, FromPrimitive};
use serde_derive::{Deserialize, Serialize};
use std::result;

/// Current version of the block structure
pub const VERSION: u32 = 0;

/// Error type for block errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad version")]
    BadVersion(u32),
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error("time is older than parent time")]
    DecreasingTime,
    #[error("block has timestamp in the future")]
    FutureTime,
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error("invalid difficulty")]
    InvalidDifficulty,
    #[error("invalid proof-of-work")]
    InvalidPoW,
    #[error(transparent)]
    Libp2pParse(#[from] libp2p::identity::ParseError),
    #[error("block doesn't specify any parents")]
    MissingParents,
    #[error(transparent)]
    Multihash(#[from] libp2p::multihash::Error),
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    RandomX(#[from] randomx::Error),
    #[error("some tx inputs are repeated")]
    RepeatedInputs,
    #[error("some tx outputs are repeated")]
    RepeatedOutputs,
    #[error("some vertex parents are repeated")]
    RepeatedParents,
    #[error(transparent)]
    Transaction(#[from] transaction::Error),
    #[error(transparent)]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Result type for block errors
type Result<T> = result::Result<T, Error>;

/// Type alias for block hashes
pub type BlockHash = crate::hash::Hash;

/// A block represents a collection of transactions which have been mined with proof-of-work, as
/// well as their position within the DAG. This differs slightly from a vertex, as the vertex may
/// be repositioned in the DAG if parents turn out to be non-virtuous. This is referred to as
/// dynamic parent reselection.
#[derive(Clone, Serialize, Deserialize)]
pub struct Block {
    /// Block format revision number
    pub version: u32,

    /// Difficulty this block's proof-of-work must satisfy
    pub difficulty: u64,

    /// Which peer mined this block
    pub miner: PeerId,

    /// Vertex hashes of parent vertices in the DAG.
    pub parents: Vec<VertexHash>,

    /// Transaction outputs consumed by this block
    pub inputs: Vec<TxoHash>,

    /// Transaction outputs created by this bock
    pub outputs: Vec<Txo>,

    /// Time when the miner claims to have solved this proof-of-work
    pub time: DateTime<Utc>,

    /// Nonce used in proof-of-work
    pub nonce: u64,
}

impl Block {
    /// Compute the hash of the block
    pub fn hash(&self) -> BlockHash {
        blake3::hash(&self.to_wire(false).expect("encode failure in hash")).into()
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
        // TODO: validate difficulty against DAA
        if BigUint::from_bytes_be(&randomx.calculate_hash(&self.to_wire(false)?)?)
            < self.mining_target()?
        {
            Ok(())
        } else {
            Err(Error::InvalidPoW)
        }
    }

    /// Set the timestamp
    pub fn with_timestamp(mut self, time: DateTime<Utc>) -> Block {
        self.time = time;
        self
    }

    /// Set the parent vertices
    pub fn with_miner(mut self, miner: PeerId) -> Block {
        self.miner = miner;
        self
    }

    /// Set the parent vertices
    pub fn with_parents(mut self, parents: Vec<VertexHash>) -> Block {
        self.parents = parents;
        self
    }

    /// Make sure the block passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version > VERSION {
            Err(Error::BadVersion(self.version))
        } else if self.difficulty < MIN_DIFFICULTY {
            Err(Error::InvalidDifficulty)
        } else if self.time - Utc::now() > Duration::seconds(FUTURE_BLOCK_LIMIT_SECS) {
            Err(Error::FutureTime)
        } else if self.parents.is_empty() {
            Err(Error::MissingParents)
        } else if !self.parents.iter().all_unique() {
            Err(Error::RepeatedParents)
        } else if !self.inputs.iter().all_unique() {
            Err(Error::RepeatedInputs)
        } else if !self.outputs.iter().all_unique() {
            Err(Error::RepeatedOutputs)
        } else {
            Ok(())
        }
    }

    /// Check if the block is a legal extension of the specified parent
    // TODO: test this
    pub fn extends_parent(&self, parent: &Block) -> Result<()> {
        if self.time <= parent.time {
            Err(Error::DecreasingTime)
        } else {
            Ok(())
        }
        // TODO: check InvalidDifficulty
        // TODO: check block height
        // TODO: check other block constraints
    }
}

impl PartialEq for Block {
    fn eq(&self, other: &Self) -> bool {
        self.hash() == other.hash()
    }
}

impl Eq for Block {}

impl<'a> WireFormat<'a, proto::Block> for Block {
    type Error = Error;

    /// Serialize into protobuf format
    fn to_protobuf(&self, check: bool) -> Result<proto::Block> {
        if check {
            self.sanity_checks()?;
        }
        Ok(proto::Block {
            version: self.version,
            difficulty: self.difficulty,
            miner: self.miner.to_bytes(),
            parents: self
                .parents
                .iter()
                .map(|vhash| vhash.to_protobuf(check))
                .try_collect()?,
            inputs: self
                .inputs
                .iter()
                .map(|txo_hash| txo_hash.to_protobuf(check))
                .try_collect()?,
            outputs: self
                .outputs
                .iter()
                .map(|txo| txo.to_protobuf(check))
                .try_collect()?,
            time: self
                .time
                .to_rfc3339_opts(SecondsFormat::Secs, false)
                .into_bytes(),
            nonce: self.nonce,
        })
    }

    fn from_protobuf(block: &proto::Block, check: bool) -> Result<Block> {
        let block = Block {
            version: block.version,
            difficulty: block.difficulty,
            miner: PeerId::from_bytes(&block.miner)?,
            parents: block
                .parents
                .iter()
                .map(|vhash| VertexHash::from_protobuf(vhash, check))
                .try_collect()?,
            inputs: block
                .inputs
                .iter()
                .map(|txo_hash| TxoHash::from_protobuf(txo_hash, check))
                .try_collect()?,
            outputs: block
                .outputs
                .iter()
                .map(|txo| Txo::from_protobuf(txo, check))
                .try_collect()?,
            time: DateTime::parse_from_rfc3339(&String::from_utf8(block.time.clone())?)?.into(),
            nonce: block.nonce,
        };
        if check {
            block.sanity_checks()?;
        }
        Ok(block)
    }
}

impl std::fmt::Display for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string_pretty(&PrettyBlock::from(self)).unwrap()
        )
    }
}

impl std::fmt::Debug for Block {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <Self as std::fmt::Display>::fmt(&self, f)
    }
}

// TODO: use a builder().with_a().with_b().build() pattern instead of default
impl Default for Block {
    fn default() -> Self {
        Block {
            version: VERSION,
            difficulty: MIN_DIFFICULTY,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            parents: Vec::new(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            time: DateTime::default(),
            nonce: 0,
        }
    }
}

impl Randomizer for Block {
    fn random() -> Self {
        Block {
            version: VERSION,
            difficulty: std::cmp::max(MIN_DIFFICULTY, rand::random()),
            miner: PeerId::random(),
            parents: vec![
                VertexHash::random(),
                VertexHash::random(),
                VertexHash::random(),
                VertexHash::random(),
                VertexHash::random(),
                VertexHash::random(),
                VertexHash::random(),
            ]
            .into_iter()
            .take(std::cmp::max(1, rand::random()))
            .collect(),
            inputs: vec![
                TxoHash::random(),
                TxoHash::random(),
                TxoHash::random(),
                TxoHash::random(),
                TxoHash::random(),
                TxoHash::random(),
                TxoHash::random(),
            ]
            .into_iter()
            .take(std::cmp::max(1, rand::random()))
            .collect(),
            outputs: vec![
                Txo::random(),
                Txo::random(),
                Txo::random(),
                Txo::random(),
                Txo::random(),
                Txo::random(),
                Txo::random(),
            ]
            .into_iter()
            .take(std::cmp::max(1, rand::random()))
            .collect(),
            time: Utc::now(),
            nonce: rand::random(),
        }
    }
}

impl<'a> BytesEncode<'a> for Block {
    type EItem = Block;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(item.to_wire(false)?.into())
    }
}

impl<'a> BytesDecode<'a> for Block {
    type DItem = Block;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(Block::from_wire(bytes, false)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrettyBlock {
    version: u32,
    difficulty: u64,
    miner: PeerId,
    parents: Vec<String>,
    inputs: Vec<String>,
    outputs: Vec<String>,
    time: DateTime<Utc>,
    nonce: u64,
}

impl From<&Block> for PrettyBlock {
    fn from(block: &Block) -> Self {
        PrettyBlock {
            version: block.version,
            difficulty: block.difficulty,
            miner: block.miner,
            parents: block.parents.iter().map(|vhash| vhash.to_hex()).collect(),
            inputs: block.inputs.iter().map(|txo| txo.to_hex()).collect(),
            outputs: block
                .outputs
                .iter()
                .map(|txo| txo.hash().to_hex())
                .collect(),
            time: DateTime::parse_from_rfc3339(
                &block.time.to_rfc3339_opts(SecondsFormat::Secs, false),
            )
            .unwrap()
            .into(),
            nonce: block.nonce,
        }
    }
}
