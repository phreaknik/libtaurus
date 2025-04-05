use super::{vertex::Vertex, Error, Result};
use crate::randomx::RandomXVMInstance;
use blake3::Hash;
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
