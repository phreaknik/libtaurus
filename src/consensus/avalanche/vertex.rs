use super::{Error, Result};
use crate::{params, randomx::RandomXVMInstance};
use blake3::Hash;
use chrono::{DateTime, Utc};
use libp2p::PeerId;
use num::{BigUint, FromPrimitive};
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// A vertex in the avalanche DAG
#[derive(Debug, Clone)]
pub struct Vertex<'a> {
    /// Version number for this vertex structure
    pub version: u32,

    /// Every vertex after the genesis vertex will have parents.
    pub parents: HashMap<Hash, Arc<TracingRwLock<Vertex<'a>>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will be empty when the vertex is first mined, and omitted from the marshalled output
    /// when the vertex is marshalled for storage or transmission.
    pub children: HashMap<Hash, Arc<TracingRwLock<Vertex<'a>>>>,

    /// The height of this vertex in the DAG
    pub height: u64,

    /// The network difficulty used to mine this block
    pub difficulty: u64,

    /// The [`PeerId`] of this vertex's miner
    pub miner: PeerId,

    /// The time the miner reports having mined the block
    pub time: DateTime<Utc>,

    /// Unique value used to grind a proof-of-work solution
    pub nonce: u64,
}

impl Vertex<'_> {
    /// Marshal the vertex into bytes for storage/transmission
    pub fn marshal_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_cbor::to_vec(&CompactVertex::from(self))?)
    }

    /// Unmarshal a byte slice into a complete vertex object
    pub fn unmarshal_bytes<'a>(
        ancestors: HashMap<Hash, Arc<TracingRwLock<Vertex<'a>>>>,
        rxvm: &RandomXVMInstance,
        bytes: &[u8],
    ) -> Result<Vertex<'a>> {
        let compact: CompactVertex = serde_cbor::from_slice(bytes)?;
        compact.verify_pow(rxvm)?;
        Ok(Vertex {
            version: compact.version,
            children: HashMap::new(),
            height: compact.height,
            difficulty: compact.difficulty,
            miner: compact.miner,
            time: compact.time,
            nonce: compact.nonce,

            // Inflate parent vertices from the provided ancesors
            parents: compact
                .parents
                .iter()
                .map(|hash| {
                    ancestors
                        .get_key_value(&hash.into())
                        .ok_or(Error::MissingParent)
                        .map(|(key, val)| (*key, val.clone()))
                })
                .try_collect()?,
        })
    }
}

/// A compact representation of the vertex, suitable for serialization and transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompactVertex {
    version: u32,
    parents: Vec<SerdeHash>,
    height: u64,
    difficulty: u64,
    miner: PeerId,
    time: DateTime<Utc>,
    nonce: u64,
}

impl CompactVertex {
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

impl<'a> From<&Vertex<'_>> for CompactVertex {
    fn from(vertex: &Vertex) -> Self {
        CompactVertex {
            version: vertex.version,
            parents: vertex
                .parents
                .iter()
                .map(|(hash, _vertex)| hash.into())
                .collect(),
            height: vertex.height,
            difficulty: vertex.difficulty,
            miner: vertex.miner,
            time: vertex.time,
            nonce: vertex.nonce,
        }
    }
}

/// Wrapper struct around blake3::Hash to facilitate serde implementation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SerdeHash([u8; blake3::OUT_LEN]);

impl From<&blake3::Hash> for SerdeHash {
    fn from(hash: &blake3::Hash) -> Self {
        SerdeHash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &SerdeHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}
