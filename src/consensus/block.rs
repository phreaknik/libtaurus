use super::transaction::{self, Txo, TxoHash};
use crate::{
    p2p::consensus_rpc::proto,
    params::{self, FUTURE_BLOCK_LIMIT_SECS, MIN_DIFFICULTY},
    randomx::{self, RandomXVMInstance},
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use heed::{BytesDecode, BytesEncode};
use libp2p::{multihash::Multihash, PeerId};
use num::{BigUint, FromPrimitive};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use serde_derive::{Deserialize, Serialize};
use std::{io, result};

/// Current version of the block structure
pub const VERSION: u32 = 0;

/// Error type for block errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("bad block version")]
    BadBlockVersion,
    #[error(transparent)]
    Chrono(#[from] chrono::ParseError),
    #[error("block has timestamp in the future")]
    FutureTime,
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error("invalid difficulty")]
    InvalidDifficulty,
    #[error("invalid proof-of-work")]
    InvalidPoW,
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Libp2pParse(#[from] libp2p::identity::ParseError),
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

/// Type alias for block hashes
pub type BlockHash = crate::hash::Hash;

#[derive(Clone, Serialize, Deserialize)]
pub struct Block {
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
    pub time: DateTime<Utc>,

    /// Nonce used in proof-of-work
    pub nonce: u64,
}

impl Block {
    /// Create a new empty block
    pub fn new(miner: PeerId, prev_mined: Option<BlockHash>) -> Block {
        Block {
            version: VERSION,
            difficulty: MIN_DIFFICULTY,
            miner,
            prev_mined,
            inputs: Vec::new(),
            outputs: Vec::new(),
            time: Utc::now(),
            nonce: 0,
        }
    }

    /// Make sure the block passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        if self.version != VERSION {
            Err(Error::BadBlockVersion)
        } else if self.difficulty < MIN_DIFFICULTY {
            Err(Error::InvalidDifficulty)
        } else if self.time - Utc::now() > Duration::seconds(FUTURE_BLOCK_LIMIT_SECS) {
            Err(Error::FutureTime)
        } else {
            Ok(())
        }
    }

    /// Compute the hash of the block
    pub fn hash(&self) -> BlockHash {
        blake3::hash(&self.to_bytes().expect("Serde encode failure in hash")).into()
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
        if BigUint::from_bytes_be(&randomx.calculate_hash(&self.to_bytes()?)?)
            < self.mining_target()?
        {
            Ok(())
        } else {
            Err(Error::InvalidPoW)
        }
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(block: &proto::Block, check: bool) -> Result<Block> {
        let block = Block {
            version: block.version,
            difficulty: block.difficulty,
            miner: PeerId::from_bytes(&block.miner)?,
            prev_mined: match &block.prev_mined {
                Some(hash) => Some(BlockHash::from_protobuf(hash)?),
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
            time: DateTime::parse_from_rfc3339(&String::from_utf8(block.time.clone())?)?.into(),
            nonce: block.nonce,
        };
        if check {
            block.sanity_checks()?;
        }
        Ok(block)
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self, check: bool) -> Result<proto::Block> {
        if check {
            self.sanity_checks()?;
        }
        Ok(proto::Block {
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
            time: self
                .time
                .to_rfc3339_opts(SecondsFormat::Secs, false)
                .into_bytes(),
            nonce: self.nonce,
        })
    }

    /// Deserialize from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Block> {
        let protobuf = proto::Block::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unable to parse block from bytes: {e}"),
                )
            })?;
        Block::from_protobuf(&protobuf, true)
    }

    /// Serialize into bytes
    // TODO: implement WireFormat instead
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf(true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert Block to protobuf: {e}"),
            )
        })?;
        protobuf
            .write_message(&mut writer)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "unable to serialize Block"))?;
        Ok(bytes)
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

/// This implementation of ['Default'] is nonsense and should never be used. It is only implemented
/// to satisfy a trait boundary, but the actual contents are not used.
impl Default for Block {
    fn default() -> Self {
        Block {
            version: VERSION,
            difficulty: MIN_DIFFICULTY,
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
        Ok(item.to_bytes()?.into())
    }
}

impl<'a> BytesDecode<'a> for Block {
    type DItem = Block;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(Block::from_bytes(bytes)?)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PrettyBlock {
    version: u32,
    difficulty: u64,
    miner: PeerId,
    prev_mined: Vec<String>,
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
            prev_mined: block
                .prev_mined
                .map(|bhash| vec![bhash.to_hex()])
                .unwrap_or(Vec::new()),
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
