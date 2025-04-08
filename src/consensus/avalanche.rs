use super::{database::BlocksDatabase, Block, Event};
use crate::params;
use blake3::Hash;
use chrono::Utc;
use libp2p::PeerId;
use lru::LruCache;
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, result, sync::Arc};
use tokio::sync::broadcast;
use tracing::{debug, info};
use tracing_mutex::stdsync::TracingRwLock;

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "blocks_db/";

/// Error type for avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] super::block::Error),
    #[error(transparent)]
    Cbor(#[from] serde_cbor::error::Error),
    #[error(transparent)]
    Database(#[from] super::database::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("new block channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error("missing parent")]
    MissingParents(Vec<Hash>),
    #[error("data not found")]
    NotFound,
    #[error("missing data")]
    MissingData,
    #[error("error acquiring read lock")]
    ReadLock,
    #[error("stale block")]
    StaleBlock,
    #[error("error acquiring write lock")]
    WriteLock,
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
    pub fn init(&mut self) -> Result<()> {
        // TODO: load blocks from database
        // For now just init an empty DAG with the genesis block
        self.try_insert(self.config.genesis.clone())
    }

    /// Try to insert a block as a new vertex in the DAG
    pub fn try_insert(&mut self, block: Block) -> Result<()> {
        let hash = block.hash()?;

        // Make sure this block has parents to attach to the DAG
        if block.parents.len() == 0 && hash != self.genesis_hash {
            // This block has no parents to attach to the DAG
            return Err(Error::MissingData);
        }

        // Create a new vertex from this block, and link to its parents if we have them
        let mut vertex = Vertex::new(block.clone());
        match self.link_parents(&mut vertex) {
            Err(Error::MissingParents(e)) => {
                debug!("Block {hash} is missing parents");
                self.waitlist.insert(Arc::new(TracingRwLock::new(vertex)))?;
                Err(Error::MissingParents(e))
            }
            Ok(_) => self.try_insert_vertex(Arc::new(TracingRwLock::new(vertex))),
            error @ Err(_) => error,
        }
    }

    /// Try to insert a vertex into the DAG
    fn try_insert_vertex(&mut self, arc_vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        // TODO: validate block (has parents, valid height, valid difficulty, etc...)

        let mut vertex = arc_vertex.write().map_err(|_| Error::WriteLock)?;
        let hash = vertex.block.hash()?;

        // Write it to the database
        let wtxn = self
            .database
            .write_block(None, vertex.block.clone(), false)?;

        // Update each parent
        for parent in vertex.parents.iter_mut() {
            parent
                .write()
                .map_err(|_| Error::WriteLock)?
                .children
                .push(arc_vertex.clone());
        }

        // Add it to the collection of vertices
        self.vertices.insert(hash, arc_vertex.clone());

        // TODO: This is naive longest-chain-rule consensus.A better form of consensus should
        // replace this.
        // Add it to the frontier
        println!("len(frontier)={}", self.frontier.len());
        if vertex.block.height < self.frontier.values().map(|b| b.height).min().unwrap_or(0) {
            return Err(Error::StaleBlock);
        }
        self.frontier.insert(hash, vertex.block.clone());

        // Remove its parents from the frontier, as they are no longer the youngest
        for parent in vertex.block.parents.iter() {
            self.frontier.remove(&parent.into());
        }

        // Announce the new frontier
        self.events_ch.send(Event::NewFrontier(Frontier(
            self.frontier.values().cloned().collect(),
        )))?;

        info!("Inserted block: {hash}");

        // Commit the block to the database only now that it has passed all validation
        wtxn.commit().map_err(Error::from)?;

        // Retry vertices in the waitlist
        Ok(self.remove_and_retry_waitlist(hash))
    }

    /// Fill out parent links if the parents are present in the DAG
    fn link_parents(&mut self, vertex: &mut Vertex) -> Result<()> {
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
    pub fn get_block(&mut self, _height: u64, hash: &Hash) -> Result<Block> {
        match self.vertices.get(hash) {
            Some(vertex) => Ok(vertex.read().map_err(|_| Error::ReadLock)?.block.clone()),
            None => self
                .database
                .read_block(hash.clone())?
                .ok_or(Error::NotFound),
        }
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
}

impl Vertex {
    pub fn new(block: Block) -> Vertex {
        Vertex {
            parents: Vec::new(),
            children: Vec::new(),
            block: block.clone(),
            chit: 0,
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
            parents: self.0.iter().map(|v| v.hash().unwrap().into()).collect(),
            height: self.height() + 1,
            difficulty: self.next_difficulty(),
            miner,
            time: Utc::now(),
            nonce: 0,
        }
    }

    /// This vertex's height in the DAG
    pub fn height(&self) -> u64 {
        self.0[0].height
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
    fn insert(&mut self, arc_vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        let vertex = arc_vertex.read().map_err(|_| Error::ReadLock)?;
        for parent_hash in vertex.block.parents.iter().map(|h| h.into()) {
            if let Some(parent_queue) = self.0.get_mut(&parent_hash) {
                // Add this vertex as a descendent in the parent's queue
                parent_queue.push(arc_vertex.clone());
            } else {
                // Insert a new entry
                let evicted = self.0.put(parent_hash, vec![arc_vertex.clone()]);

                // If an existing entry was evicted, demote all the descendents in its queue
                if let Some(queue) = evicted {
                    for child in queue {
                        self.0
                            .demote(&child.read().map_err(|_| Error::ReadLock)?.block.hash()?);
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
