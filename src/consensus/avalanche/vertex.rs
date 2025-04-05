use super::{Error, Result};
use crate::{params, randomx::RandomXVMInstance};
use blake3::Hash;
use chrono::{DateTime, Utc};
use libp2p::{multihash::Multihash, PeerId};
use num::{BigUint, FromPrimitive};
use serde_derive::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// A vertex in the avalanche DAG
#[derive(Debug, Clone)]
pub struct Vertex {
    /// Version number for this vertex structure
    pub version: u32,

    /// Every vertex after the genesis vertex will have parents.
    pub parents: Vec<Arc<TracingRwLock<Vertex>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will be empty when the vertex is first mined, and omitted from the marshalled output
    /// when the vertex is marshalled for storage or transmission.
    pub children: Vec<Arc<TracingRwLock<Vertex>>>,

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

impl Vertex {
    /// Compute the hash of the vertex. Note, this only hashes the static components. Dynamic
    /// components (such as children) may be updated without changing the hash.
    pub fn hash(&self) -> Result<Hash> {
        CompactVertex::from(self).hash()
    }

    /// Marshal the vertex into bytes for storage/transmission
    pub fn marshal_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_cbor::to_vec(&CompactVertex::from(self))?)
    }

    /// Unmarshal a byte slice into a complete vertex object
    pub fn unmarshal_bytes<'a>(
        ancestors: &HashMap<Hash, Arc<TracingRwLock<Vertex>>>,
        rxvm: &RandomXVMInstance,
        bytes: &[u8],
    ) -> Result<Vertex> {
        let compact: CompactVertex = serde_cbor::from_slice(bytes)?;
        compact.verify_pow(rxvm)?;
        Ok(Vertex {
            version: compact.version,
            children: Vec::new(),
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
                        .get(&hash.into())
                        .ok_or(Error::MissingParent)
                        .map(|val| val.clone())
                })
                .try_collect()?,
        })
    }
}

/// A compact representation of the vertex, suitable for serialization and transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactVertex {
    pub version: u32,
    pub parents: Vec<SerdeHash>,
    pub height: u64,
    pub difficulty: u64,
    pub miner: PeerId,
    pub time: DateTime<Utc>,
    pub nonce: u64,
}

impl CompactVertex {
    /// Compute the hash of the vertex
    pub fn hash(&self) -> Result<Hash> {
        Ok(blake3::hash(&serde_cbor::to_vec(self)?))
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

/// This implementation of ['Default'] is nonsense and should never be used. It is only implemented
/// to satisfy a trait boundary, but the actual contents are not used.
impl Default for CompactVertex {
    fn default() -> Self {
        CompactVertex {
            version: 0,
            parents: Vec::new(),
            height: 0,
            difficulty: 0,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            time: Utc::now(),
            nonce: 0,
        }
    }
}

impl From<&Vertex> for CompactVertex {
    fn from(vertex: &Vertex) -> Self {
        CompactVertex {
            version: vertex.version,
            parents: vertex
                .parents
                .iter()
                .map(|p| p.read().unwrap().hash().unwrap().into())
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
pub struct SerdeHash([u8; blake3::OUT_LEN]);

impl From<blake3::Hash> for SerdeHash {
    fn from(hash: blake3::Hash) -> Self {
        SerdeHash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &SerdeHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}
