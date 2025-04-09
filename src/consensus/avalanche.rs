use super::{database::BlocksDatabase, Block, Event};
use crate::params;
use blake3::Hash;
use chrono::Utc;
use libp2p::PeerId;
use lru::LruCache;
//TODO: use RC instead of ARC. Don't need thread safety
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, result, sync::Arc};
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use tracing_mutex::stdsync::TracingRwLock;

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "blocks_db/";

/// Error type for avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] super::block::Error),
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Database(#[from] super::database::Error),
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("missing data")]
    MissingData,
    #[error("missing parent")]
    MissingParents(Vec<Hash>),
    #[error("new block channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error("data not found")]
    NotFound,
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
    #[error("error acquiring write lock on a vertex")]
    VertexWriteLock,
}

/// Result type for avalanche errors
pub type Result<T> = result::Result<T, Error>;

/// Configuration details for the consensus process.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the consensus data directory
    pub data_dir: PathBuf,

    /// Genesis block to serve as root vertex in the DAG
    pub genesis: Block,

    /// Maximum number of vertices which can wait in the list
    pub waitlist_cap: NonZeroUsize,
}

/// Implementation of Avalanche DAG, using blocks as vertices
pub struct DAG {
    /// Configuration details
    config: Config,

    /// Complete list of active vertices
    vertices: HashMap<Hash, Arc<TracingRwLock<Vertex>>>,

    /// Conflict sets for each transaction input
    conflicts: HashMap<Hash, Vec<Hash>>,

    /// Waitlist for vertices that cannot be inserted yet
    waitlist: WaitList,

    /// The active edge of the DAG, i.e. the preferred vertices which don't yet have children
    frontier: HashMap<Hash, Block>,

    /// Database for block storage
    database: BlocksDatabase,

    /// Hash of the genesis block
    genesis_hash: Hash,

    /// Handle to send events on the consensus event channel
    events_ch: broadcast::Sender<Event>,
}

impl DAG {
    /// Create a new DAG
    pub fn new(config: Config, events_ch: broadcast::Sender<Event>) -> DAG {
        let genesis_hash = config.genesis.hash().unwrap();

        // Create DAG instance, initialized with genesis block in the frontier
        let dag = DAG {
            vertices: HashMap::new(),
            conflicts: HashMap::new(),
            waitlist: WaitList::new(config.waitlist_cap),
            frontier: [(genesis_hash, config.genesis.clone())]
                .iter()
                .cloned()
                .collect(),
            database: BlocksDatabase::open(&config.data_dir.join(DATABASE_DIR), true)
                .expect("Failed to open blocks database"),
            genesis_hash,
            events_ch,
            config,
        };

        // Return the dag
        dag
    }

    /// Initialize the DAG on startup
    pub fn init(&mut self) -> Result<bool> {
        // TODO: load blocks from database
        // For now just init an empty DAG with the genesis block
        self.try_insert(self.config.genesis.clone())
    }

    /// Try to insert a block as a new vertex in the DAG. Returns true if the vertex is considered
    /// strongly preffered, according to Avalanche consensus.
    pub fn try_insert(&mut self, block: Block) -> Result<bool> {
        let hash = block.hash()?;

        // Make sure this block has parents to attach to the DAG
        if block.parents.len() == 0 && hash != self.genesis_hash {
            // This block has no parents to attach to the DAG
            return Err(Error::MissingData);
        }

        // Create a new vertex from this block, and link to its parents if we have them
        let vertex = Arc::new(TracingRwLock::new(Vertex::new(block.clone())));
        match self.link_parents(vertex.clone()) {
            Err(Error::MissingParents(e)) => {
                debug!("Block {hash} is missing parents");
                self.waitlist.insert(vertex.clone())?;
                Err(Error::MissingParents(e))
            }
            Err(e) => Err(e),
            Ok(_) => self.try_insert_vertex(vertex),
        }
    }

    /// Try to insert a vertex into the DAG. Returns true if the vertex is considered strongly
    /// preffered, according to Avalanche consensus.
    fn try_insert_vertex(&mut self, arc_vertex: Arc<TracingRwLock<Vertex>>) -> Result<bool> {
        // TODO: validate block (has parents, valid height, valid difficulty, etc...)

        let mut vertex = arc_vertex.write().map_err(|_| Error::VertexWriteLock)?;
        let hash = vertex.block.hash()?;

        // Update each parent
        for parent in &vertex.parents {
            parent
                .write()
                .map_err(|_| Error::VertexWriteLock)?
                .children
                .push(arc_vertex.clone());
        }

        // Update the conflict set for each transaction input
        for input in vertex.block.inputs.iter() {
            // Make sure conflict sets exist for each tx input
            if let Some(conflict_set) = self.conflicts.get_mut(&input.into()) {
                conflict_set.push(hash);
            } else {
                // Create a new conflict set for this transaction input
                self.conflicts.insert(hash, vec![hash]);
            }
        }

        // Add it to the collection of vertices
        self.vertices.insert(hash, arc_vertex.clone());

        // Set the vertex preference
        vertex.preferred = self.is_vertex_preferred(arc_vertex.clone())?;

        if vertex.preferred {
            // Add it to the frontier
            self.frontier.insert(hash, vertex.block.clone());

            // Remove its parents from the frontier, as they are no longer the youngest
            for parent in vertex.block.parents.iter() {
                self.frontier.remove(&parent.into());
            }

            // Announce the new frontier
            self.events_ch.send(Event::NewFrontier(Frontier(
                self.frontier.values().cloned().collect(),
            )))?;

            info!("Accepted block: {hash}");
        } else {
            info!("Received non-preferred block: {hash}");
        }

        // Retry vertices in the waitlist
        self.remove_and_retry_waitlist(hash);

        // Return true if this vertex is strongly preferred
        Ok(vertex.preferred
            && vertex
                .parents
                .iter()
                .all(|v| v.read().unwrap().preferred == true))
    }

    /// Fill out parent links if the parents are present in the DAG
    fn link_parents(&mut self, arc_vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        let mut vertex = arc_vertex.write().map_err(|_| Error::VertexWriteLock)?;
        Ok(vertex.parents = vertex
            .block
            .parents
            .iter()
            .map(|hash| {
                self.vertices
                    .get(&hash.into())
                    .ok_or(Error::MissingParents(vec![hash.into()]))
                    .map(|val| val.clone())
            })
            .try_collect()?)
    }

    /// Remove the specified entry from the waitlist, and retry inserting any of is descendents
    fn remove_and_retry_waitlist(&mut self, hash: Hash) {
        if let Some(descendents) = self.waitlist.remove(&hash) {
            for descendent in descendents {
                let _ = self.try_insert_vertex(descendent);
            }
        }
    }

    /// Look up a block from the DAG
    pub fn get_block(&mut self, hash: &Hash) -> Result<Block> {
        match self.vertices.get(hash) {
            // Look for the vertex in the in-memory DAG
            Some(vertex) => Ok(vertex
                .read()
                .map_err(|_| Error::VertexReadLock)?
                .block
                .clone()),
            // If its not in the in-memory DAG, check the database.
            None => self
                .database
                .read_block(hash.clone())?
                .ok_or(Error::NotFound),
        }
    }

    /// Determine if the specified vertex is strongly preferred, as described in the Avalanche
    /// consensus paper.
    pub fn is_strongly_preferred(&mut self, hash: &Hash) -> Result<bool> {
        Ok(match self.vertices.get(hash) {
            // Look for the vertex in the in-memory DAG
            Some(vertex) => Ok(vertex
                .read()
                .map_err(|_| Error::VertexReadLock)?
                .block
                .clone()),
            // If its not in the in-memory DAG, check the database.
            None => self
                .database
                .read_block(hash.clone())?
                .ok_or(Error::NotFound),
        }?
        .parents
        .iter()
        .all(|p| {
            self.is_preferred(&p.into()).unwrap_or_else(|e| {
                // This condition should never occur, because the block should not even be present
                // in the DAG or database without known parents
                warn!(
                    "Failed to determine preference for parent {}: {e}",
                    Into::<blake3::Hash>::into(p)
                );
                false
            })
        }))
    }

    /// Determine if the vertex corresponding to the given hash is preferred, as described in the
    /// Avalanche consensus paper.
    pub fn is_preferred(&mut self, hash: &Hash) -> Result<bool> {
        Ok(match self.vertices.get(hash) {
            // Look for the vertex in the in-memory DAG
            Some(vertex) => self.is_vertex_preferred(vertex.clone())?,
            // If its not in the in-memory DAG, check the database. If it exists, then the block is
            // preferred.
            None => self.database.read_block(hash.clone())?.is_some(),
        })
    }

    /// Determine if the given vertex is preferred, as described in the Avalanche consensus paper.
    fn is_vertex_preferred(&self, arc_vertex: Arc<TracingRwLock<Vertex>>) -> Result<bool> {
        let vertex = arc_vertex.read().map_err(|_| Error::VertexReadLock)?;
        let hash = vertex.block.hash()?;
        Ok(vertex.block.inputs.iter().all(|input| {
            match self.conflicts.get(&input.into()) {
                Some(cs) => !cs.into_iter().any(|h| {
                    // If a conflict set exists, and any other vertex in the conflict set is
                    // preferred, this vertex cannot be preferred
                    h != &hash && self.vertices.get(&h).unwrap().read().unwrap().preferred == true
                }),
                None => true,
            }
        }))
    }
}

/// A vertex in the avalanche DAG. A vertex is essentially a block with parent & child links to
/// assist DAG operations.
#[derive(Debug, Clone)]
pub struct Vertex {
    /// Block represented by this vertex
    block: Block,

    /// Every vertex after the genesis vertex will have parents.
    parents: Vec<Arc<TracingRwLock<Vertex>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will be empty when the vertex is first mined, and omitted from the marshalled output
    /// when the vertex is marshalled for storage or transmission.
    children: Vec<Arc<TracingRwLock<Vertex>>>,

    /// Chit score this vertex received when it was queried by the network
    chit: usize,

    /// Is this vertex currently preferred?
    preferred: bool,
}

impl Vertex {
    pub fn new(block: Block) -> Vertex {
        Vertex {
            parents: Vec::new(),
            children: Vec::new(),
            block: block.clone(),
            chit: 0,
            preferred: false,
        }
    }
}

impl Into<Block> for Vertex {
    fn into(self) -> Block {
        self.block
    }
}

/// The frontier describes the highest vertices in the DAG
#[derive(Debug, Clone)]
pub struct Frontier(pub Vec<Block>);

impl Frontier {
    /// Compute the difficulty of the next vertex to build on this frontier
    pub fn next_difficulty(&self) -> u64 {
        self.0.iter().map(|h| h.difficulty).min().unwrap()
    }

    /// Compute a candidate block to mine atop the given frontier
    pub fn to_candidate(&self, miner: PeerId) -> Block {
        Block {
            version: params::PROTOCOL_VERSION,
            height: self.height() + 1,
            difficulty: self.next_difficulty(),
            miner,
            parents: self.0.iter().map(|v| v.hash().unwrap().into()).collect(),
            inputs: Vec::new(),
            time: Utc::now(),
            nonce: 0,
        }
    }

    /// This vertex's height in the DAG
    pub fn height(&self) -> u64 {
        self.0.iter().map(|b| b.height).max().unwrap()
    }
}

/// A waitlist is a dependency graph of ancestor blocks which must be processed before future child
/// blocks may be processed.
///
/// The list is constructed as a map of <K, V>, where K is the hash of a block, and V is a list
/// child blocks which depend on that block.
struct WaitList(LruCache<Hash, Vec<Arc<TracingRwLock<Vertex>>>>);

impl WaitList {
    /// Create a new waitlist
    fn new(cap: NonZeroUsize) -> WaitList {
        WaitList(LruCache::new(cap))
    }

    /// Inserts a new vertex into the waitlist.
    fn insert(&mut self, vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        for parent_hash in vertex
            .read()
            .map_err(|_| Error::VertexReadLock)?
            .block
            .parents
            .iter()
            .map(|h| h.into())
        {
            if let Some(parent_queue) = self.0.get_mut(&parent_hash) {
                // Add this vertex as a descendent in the parent's queue
                parent_queue.push(vertex.clone());
            } else {
                // Insert a new entry
                let evicted = self.0.put(parent_hash, vec![vertex.clone()]);

                // If an existing entry was evicted, demote all the descendents in its queue
                if let Some(queue) = evicted {
                    for child in queue {
                        self.0.demote(
                            &child
                                .read()
                                .map_err(|_| Error::VertexReadLock)?
                                .block
                                .hash()?,
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Remove an entry from the list, and return the vertices which were waiting on it
    fn remove(&mut self, hash: &Hash) -> Option<Vec<Arc<TracingRwLock<Vertex>>>> {
        self.0.pop(hash)
    }
}
