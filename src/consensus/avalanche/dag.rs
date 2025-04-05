use super::{vertex::Vertex, CompactVertex, Error, Result};
use crate::{params, randomx::RandomXVMInstance};
use blake3::Hash;
use chrono::Utc;
use libp2p::PeerId;

use std::{collections::HashMap, sync::Arc};
use tracing_mutex::stdsync::TracingRwLock;

/// Implementation of Avalanche DAG
pub struct DAG {
    vertices: HashMap<Hash, Arc<TracingRwLock<Vertex>>>,
    randomx: RandomXVMInstance,
}

impl DAG {
    /// Unmarshal a vertex and insert it into the DAG
    pub fn push_marshalled(&mut self, bytes: &[u8]) -> Result<()> {
        // Unmarshal the vertex
        let unmarshalled = Arc::new(TracingRwLock::new(Vertex::unmarshal_bytes(
            &self.vertices,
            &mut self.randomx,
            bytes,
        )?));
        let mut vertex = unmarshalled.write().map_err(|_| Error::WriteLock)?;
        let hash = vertex.hash()?;

        // Update each parent
        for parent in vertex.parents.iter_mut() {
            parent
                .write()
                .map_err(|_| Error::WriteLock)?
                .children
                .push(unmarshalled.clone());
        }

        // Add it to the collection of vertices
        self.vertices.insert(hash, unmarshalled.clone());

        Ok(())
    }
}

/// The frontier describes the highest vertexes in the DAG
#[derive(Debug, Clone)]
pub struct Frontier(pub Vec<CompactVertex>);

impl Frontier {
    /// Compute the difficulty of the next vertex to build on this frontier
    pub fn next_difficulty(&self) -> u64 {
        self.0.iter().map(|h| h.difficulty).min().unwrap()
    }

    /// Compute a candidate block to mine atop the given frontier
    pub fn to_candidate(&self, miner: PeerId) -> CompactVertex {
        CompactVertex {
            version: params::PROTOCOL_VERSION,
            parents: self.0.iter().map(|v| v.hash().unwrap().into()).collect(),
            height: self.height() + 1,
            difficulty: self.next_difficulty(),
            miner,
            time: Utc::now(),
            nonce: 0,
        }
    }

    pub fn height(&self) -> u64 {
        self.0[0].height
    }
}
