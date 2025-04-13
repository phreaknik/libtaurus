use crate::{BlockHash, VertexHash, WireVertex};
use lru::LruCache;
use std::{num::NonZeroUsize, result, sync::Arc};
use tracing::warn;
use tracing_mutex::stdsync::TracingRwLock;

/// Error type for avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
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
    by_block: TracingRwLock<LruCache<BlockHash, Vec<Arc<WireVertex>>>>,
    /// A collection of vertices waiting on a given parent vertex
    by_parent: TracingRwLock<LruCache<VertexHash, Vec<Arc<WireVertex>>>>,
}

impl WaitList {
    /// Create a new waitlist
    pub fn new(cap: NonZeroUsize) -> WaitList {
        WaitList {
            by_block: TracingRwLock::new(LruCache::new(cap)),
            by_parent: TracingRwLock::new(LruCache::new(cap)),
        }
    }

    /// Inserts a new vertex into the waitlist.
    pub fn insert(
        &mut self,
        wire_vertex: Arc<WireVertex>,
        missing_parents: Option<Vec<VertexHash>>,
        missing_block: Option<BlockHash>,
    ) -> Result<()> {
        // TODO: need to prevent duplication
        if let Some(parents) = missing_parents {
            let mut pcache = self
                .by_parent
                .write()
                .map_err(|_| Error::WaitlistWriteLock)?;
            for &parent_hash in &parents {
                if let Some(parent_queue) = pcache.get_mut(&parent_hash) {
                    // Add this vertex as a descendent in the parent's queue
                    parent_queue.push(wire_vertex.clone());
                } else {
                    // Insert a new entry
                    if let Some(evicted) = pcache.put(parent_hash, vec![wire_vertex.clone()]) {
                        warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                    }
                }
            }
        }
        if let Some(bhash) = missing_block {
            let mut bcache = self
                .by_block
                .write()
                .map_err(|_| Error::WaitlistWriteLock)?;
            if let Some(block_queue) = bcache.get_mut(&bhash) {
                // Add this vertex as a descendent in the block's queue
                block_queue.push(wire_vertex.clone());
            } else {
                // Insert a new entry
                if let Some(evicted) = bcache.put(bhash, vec![wire_vertex.clone()]) {
                    warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                }
            }
        }
        Ok(())
    }

    /// Get any vertices waiting on the specified block
    pub fn get_by_block(&mut self, bhash: &BlockHash) -> Result<Option<Vec<Arc<WireVertex>>>> {
        Ok(self
            .by_block
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .pop(bhash))
    }

    /// Get any vertices waiting on the specified parent vertex
    pub fn get_by_vertex(&mut self, vhash: &VertexHash) -> Result<Option<Vec<Arc<WireVertex>>>> {
        Ok(self
            .by_parent
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .pop(vhash))
    }
}
