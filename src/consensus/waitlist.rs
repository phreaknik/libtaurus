use crate::{wire::WireFormat, BlockHash, VertexHash};
use lru::LruCache;
use std::{
    collections::{HashMap, HashSet},
    iter::once,
    num::NonZeroUsize,
    result,
    sync::Arc,
};
use tracing::warn;

use super::Vertex;

/// Error type for avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// Result type for avalanche errors
type Result<T> = result::Result<T, Error>;

/// A waitlist is a dependency graph of ancestor vertices which must be processed before future
/// child vertices may be processed.
pub(super) struct WaitList {
    /// A collection of vertices waiting on a given block
    by_block: LruCache<BlockHash, HashMap<VertexHash, Arc<Vertex>>>,
    /// A collection of vertices waiting on a given parent vertex
    by_parent: LruCache<VertexHash, HashMap<VertexHash, Arc<Vertex>>>,
    /// List of every vertex waiting in the set
    manifest: HashSet<VertexHash>,
}

impl WaitList {
    /// Create a new waitlist
    pub fn new(cap: NonZeroUsize) -> WaitList {
        WaitList {
            by_block: LruCache::new(cap),
            by_parent: LruCache::new(cap),
            manifest: HashSet::new(),
        }
    }

    /// Inserts a new vertex into the waitlist.
    pub fn insert(
        &mut self,
        vertex: Arc<Vertex>,
        missing_parents: Option<Vec<VertexHash>>,
        missing_block: Option<BlockHash>,
    ) -> Result<()> {
        let vhash = vertex.hash();
        // Add vertex hash to the list of waiting vertexes
        if !self.manifest.insert(vhash) {
            // Already in the list
            Ok(())
        } else {
            // Add this vertex to its partens' wait queues
            if let Some(parents) = missing_parents {
                for parent_hash in parents {
                    if let Some(parent_queue) = self.by_parent.get_mut(&parent_hash) {
                        // Add this vertex as a descendent in the parent's queue
                        parent_queue.insert(vhash, vertex.clone());
                    } else {
                        // Insert a new entry
                        if let Some(evicted) = self
                            .by_parent
                            .put(parent_hash, once((vhash, vertex.clone())).collect())
                        {
                            warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                        }
                    }
                }
            }
            // Add this vertex to its block's wait queue
            if let Some(bhash) = missing_block {
                if let Some(block_queue) = self.by_block.get_mut(&bhash) {
                    // Add this vertex as a descendent in the block's queue
                    block_queue.insert(vhash, vertex.clone());
                } else {
                    // Insert a new entry
                    if let Some(evicted) = self
                        .by_block
                        .put(bhash, once((vhash, vertex.clone())).collect())
                    {
                        warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                    }
                }
            }
            Ok(())
        }
    }

    /// Check if the waitlist contains the specified vertex
    pub fn contains(&mut self, vhash: &VertexHash) -> Result<bool> {
        Ok(self.manifest.contains(vhash))
    }

    /// Get any vertices waiting on the specified block
    pub fn get_by_block(&mut self, bhash: &BlockHash) -> Result<Option<Vec<Arc<Vertex>>>> {
        Ok(self
            .by_block
            .get(bhash)
            .map(|hm| hm.values().cloned().collect()))
    }

    /// Get any vertices waiting on the specified parent vertex
    pub fn get_by_vertex(&mut self, vhash: &VertexHash) -> Result<Option<Vec<Arc<Vertex>>>> {
        Ok(self
            .by_parent
            .get(vhash)
            .map(|hm| hm.values().cloned().collect()))
    }

    /// Remove a vertex from the waitlist which was inserted into the DAG.
    pub fn remove_inserted(&mut self, vertex: Arc<Vertex>) -> Result<()> {
        let vhash = vertex.hash();
        // Remove from the contents
        self.manifest.remove(&vhash);
        // Remove from each of parents' queues, if any
        for parent in vertex.parents() {
            self.by_parent
                .get_mut(parent)
                .and_then(|queue| queue.remove(&vhash));
        }
        // Remove from the block's queue, if any
        self.by_block
            .get_mut(&vertex.bhash)
            .and_then(|queue| queue.remove(&vhash));
        Ok(())
    }
}
