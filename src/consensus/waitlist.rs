use crate::{BlockHash, VertexHash, WireVertex};
use lru::LruCache;
use std::{
    collections::{HashMap, HashSet},
    iter::once,
    num::NonZeroUsize,
    result,
    sync::Arc,
};
use tracing::warn;
use tracing_mutex::stdsync::TracingRwLock;

/// Error type for avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error acquiring read lock on the waitlist")]
    WaitlistReadLock,
    #[error("error acquiring write lock on the waitlist")]
    WaitlistWriteLock,
}

// TODO: Mutexes likely not necessary around vertices, since entire DAG is guarded by mutex

/// Result type for avalanche errors
pub type Result<T> = result::Result<T, Error>;

/// A waitlist is a dependency graph of ancestor vertices which must be processed before future
/// child vertices may be processed.
pub struct WaitList {
    /// A collection of vertices waiting on a given block
    by_block: TracingRwLock<LruCache<BlockHash, HashMap<VertexHash, Arc<WireVertex>>>>,
    /// A collection of vertices waiting on a given parent vertex
    by_parent: TracingRwLock<LruCache<VertexHash, HashMap<VertexHash, Arc<WireVertex>>>>,
    /// List of every vertex waiting in the set
    manifest: TracingRwLock<HashSet<VertexHash>>,
}

impl WaitList {
    /// Create a new waitlist
    pub fn new(cap: NonZeroUsize) -> WaitList {
        WaitList {
            by_block: TracingRwLock::new(LruCache::new(cap)),
            by_parent: TracingRwLock::new(LruCache::new(cap)),
            manifest: TracingRwLock::new(HashSet::new()),
        }
    }

    /// Inserts a new vertex into the waitlist.
    pub fn insert(
        &mut self,
        wire_vertex: Arc<WireVertex>,
        missing_parents: Option<Vec<VertexHash>>,
        missing_block: Option<BlockHash>,
    ) -> Result<()> {
        let vhash = wire_vertex.hash();
        // Add vertex hash to the list of waiting vertexes
        if !self
            .manifest
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .insert(vhash)
        {
            // Already in the list
            Ok(())
        } else {
            // Add this vertex to its partens' wait queues
            if let Some(parents) = missing_parents {
                let mut pcache = self
                    .by_parent
                    .write()
                    .map_err(|_| Error::WaitlistWriteLock)?;
                for parent_hash in parents {
                    if let Some(parent_queue) = pcache.get_mut(&parent_hash) {
                        // Add this vertex as a descendent in the parent's queue
                        parent_queue.insert(vhash, wire_vertex.clone());
                    } else {
                        // Insert a new entry
                        if let Some(evicted) =
                            pcache.put(parent_hash, once((vhash, wire_vertex.clone())).collect())
                        {
                            warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                        }
                    }
                }
            }
            // Add this vertex to its block's wait queue
            if let Some(bhash) = missing_block {
                let mut bcache = self
                    .by_block
                    .write()
                    .map_err(|_| Error::WaitlistWriteLock)?;
                if let Some(block_queue) = bcache.get_mut(&bhash) {
                    // Add this vertex as a descendent in the block's queue
                    block_queue.insert(vhash, wire_vertex.clone());
                } else {
                    // Insert a new entry
                    if let Some(evicted) =
                        bcache.put(bhash, once((vhash, wire_vertex.clone())).collect())
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
        Ok(self
            .manifest
            .read()
            .map_err(|_| Error::WaitlistReadLock)?
            .contains(vhash))
    }

    /// Get any vertices waiting on the specified block
    pub fn get_by_block(&mut self, bhash: &BlockHash) -> Result<Option<Vec<Arc<WireVertex>>>> {
        Ok(self
            .by_block
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .get(bhash)
            .map(|hm| hm.values().cloned().collect()))
    }

    /// Get any vertices waiting on the specified parent vertex
    pub fn get_by_vertex(&mut self, vhash: &VertexHash) -> Result<Option<Vec<Arc<WireVertex>>>> {
        Ok(self
            .by_parent
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .get(vhash)
            .map(|hm| hm.values().cloned().collect()))
    }

    /// Remove a vertex from the waitlist which was inserted into the DAG.
    pub fn remove_inserted(&mut self, wire_vertex: Arc<WireVertex>) -> Result<()> {
        let vhash = wire_vertex.hash();
        // Remove from the contents
        self.manifest
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .remove(&vhash);
        // Remove from each of parents' queues, if any
        for parent in &wire_vertex.parents {
            self.by_parent
                .write()
                .map_err(|_| Error::WaitlistWriteLock)?
                .get_mut(parent)
                .and_then(|queue| queue.remove(&vhash));
        }
        // Remove from the block's queue, if any
        self.by_block
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .get_mut(&wire_vertex.bhash)
            .and_then(|queue| queue.remove(&vhash));
        Ok(())
    }
}
