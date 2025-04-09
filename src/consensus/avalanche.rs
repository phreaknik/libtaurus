use super::{database::BlocksDatabase, Block, BlockHash, Event};
use crate::{
    p2p::{self, avalanche_rpc},
    params::{self, QUORUM_MINER_AGE, QUORUM_SIZE},
};
use blake3::Hash;
use chrono::Utc;
use libp2p::PeerId;
use lru::LruCache;
//TODO: use RC instead of ARC. Don't need thread safety
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, result, sync::Arc};
use tokio::sync::{broadcast, mpsc::UnboundedSender};
use tracing::{debug, error, info, trace, warn};
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
    #[error("p2p action channel error")]
    P2pActionCh(#[from] tokio::sync::mpsc::error::SendError<p2p::Action>),
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
    vertices: HashMap<BlockHash, Arc<TracingRwLock<Vertex>>>,

    /// Conflict sets for each transaction input
    conflicts: HashMap<BlockHash, Vec<BlockHash>>,

    /// Waitlist for vertices that cannot be inserted yet
    waitlist: WaitList,

    /// The active edge of the DAG, i.e. the preferred vertices which don't yet have children
    frontier: HashMap<BlockHash, Block>,

    /// Database for block storage
    database: BlocksDatabase,

    /// Hash of the genesis block
    genesis_hash: BlockHash,

    /// Channel to make requests of the P2P network
    p2p_action_ch: UnboundedSender<p2p::Action>,

    /// Handle to send events on the consensus event channel
    events_ch: broadcast::Sender<Event>,
}

impl DAG {
    /// Create a new DAG
    pub fn new(
        config: Config,
        p2p_action_ch: UnboundedSender<p2p::Action>,
        events_ch: broadcast::Sender<Event>,
    ) -> DAG {
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
            p2p_action_ch,
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
        self.try_insert_block(self.config.genesis.clone(), None)?;
        Ok(())
    }

    /// Try to insert a block as a new vertex in the DAG. Returns true if the block is preferred
    /// over all possible conflicts, according to the Avalanche consensus protocol.
    pub fn try_insert_block(&mut self, block: Block, sender: Option<PeerId>) -> Result<bool> {
        // See if this is a new block
        let hash = block.hash()?;
        if self.vertices.contains_key(&hash) {
            return self.is_preferred(&hash);
        }

        // Make sure this block has parents to attach to the DAG
        if block.parents.len() == 0 && hash != self.genesis_hash {
            // This block has no parents to attach to the DAG
            return Err(Error::MissingData);
        }

        // Query peers for their preference, according to the avalanche consensus protocol
        for peer in self
            .database
            .select_random_miners(QUORUM_SIZE, QUORUM_MINER_AGE)?
        {
            self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                peer,
                avalanche_rpc::Request::GetPreference(hash.into()),
            ))?;
        }

        // Create a new vertex from this block, and link to its parents if we have them
        let vertex = Arc::new(TracingRwLock::new(Vertex::new(block.clone())));
        match self.link_parents(vertex.clone()) {
            Err(Error::MissingParents(e)) => {
                debug!("Block {hash} is missing parents");
                self.waitlist.insert(vertex.clone())?;
                // Initiate lookup for missing parents
                // TODO: Lookup should fail over to another mechanism if this peer fails
                if let Some(peer) = sender {
                    for parent in block.parents {
                        self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                            peer,
                            avalanche_rpc::Request::GetBlock(parent.into()),
                        ))?;
                    }
                }
                Err(Error::MissingParents(e))
            }
            Err(e) => Err(e),
            Ok(_) => Ok(self.try_insert_vertex(vertex)?),
        }
    }

    /// Try to insert a vertex into the DAG. Returns true if the vertex is considered strongly
    /// preffered, according to Avalanche consensus.
    fn try_insert_vertex(&mut self, arc_vertex: Arc<TracingRwLock<Vertex>>) -> Result<bool> {
        // TODO: validate block (has parents, valid height, valid difficulty, etc...)

        // Determine vertex preference
        let preference = self.is_vertex_preferred(arc_vertex.clone())?;

        let mut vertex = arc_vertex.write().map_err(|_| Error::VertexWriteLock)?;
        let hash = vertex.block.hash()?;
        vertex.preferred = preference;

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
            if let Some(conflict_set) = self.conflicts.get_mut(&input) {
                conflict_set.push(hash);
            } else {
                // Create a new conflict set for this transaction input
                self.conflicts.insert(hash, vec![hash]);
            }
        }

        // Add it to the collection of vertices
        self.vertices.insert(hash, arc_vertex.clone());

        if vertex.preferred {
            // Add it to the frontier
            self.frontier.insert(hash, vertex.block.clone());

            // Remove its parents from the frontier, as they are no longer the youngest
            for parent in vertex.block.parents.iter() {
                self.frontier.remove(&parent);
            }

            // Announce the new frontier
            self.events_ch.send(Event::NewFrontier(Frontier(
                self.frontier.values().cloned().collect(),
            )))?;

            info!("Accepted block {}", hash.to_hex());
        } else {
            info!("Received non-preferred block {}", hash.to_hex());
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
                    .get(&hash)
                    .ok_or(Error::MissingParents(vec![hash.into()]))
                    .map(|val| val.clone())
            })
            .try_collect()?)
    }

    /// Remove the specified entry from the waitlist, and retry inserting any of is descendents
    fn remove_and_retry_waitlist(&mut self, hash: BlockHash) {
        if let Some(descendents) = self.waitlist.remove(&hash) {
            for descendent in descendents {
                let _ = self.try_insert_vertex(descendent);
            }
        }
    }

    /// Handle any avalanche requests or responses
    pub fn handle_avalanche_message(&mut self, message: avalanche_rpc::Event) -> Result<()> {
        match message {
            avalanche_rpc::Event::Requested(peer, request_id, request) => {
                // Handle the request and respond to the requester
                self.handle_avalanche_request(peer, request)
                    .and_then(|response| {
                        self.p2p_action_ch
                            .send(p2p::Action::AvalancheResponse(request_id, response))
                            .map_err(Error::from)
                    })
            }
            avalanche_rpc::Event::Responded(peer, response) => {
                self.handle_avalanche_response(peer, response)
            }
        }
    }

    /// Handle a received avalanche request message from one of our peers
    fn handle_avalanche_request(
        &mut self,
        from_peer: PeerId,
        request: avalanche_rpc::Request,
    ) -> Result<avalanche_rpc::Response> {
        debug!("Handling request: {request}");
        match request {
            avalanche_rpc::Request::GetBlock(hash) => match self.get_block(&hash) {
                Ok(block) => {
                    trace!("Sending block response {hash}={block:?}");
                    Ok(avalanche_rpc::Response::Block(block))
                }
                Err(Error::NotFound) => {
                    debug!("Unable to find requested block: {hash}");
                    Ok(avalanche_rpc::Response::Error(
                        avalanche_rpc::proto::mod_Response::Error::NOT_FOUND,
                    ))
                }
                Err(e) => {
                    error!("Unexpected error while looking for block {hash}: {e}");
                    Err(e.into())
                }
            },
            avalanche_rpc::Request::GetPreference(hash) => {
                self.is_strongly_preferred(&hash)
                    .map(|preference| avalanche_rpc::Response::Preference(preference))
                    .map_err(|error| {
                        debug!("Unable to determine preference for {hash}: {error}");
                        if let Error::NotFound = error {
                            match self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                                from_peer,
                                avalanche_rpc::Request::GetBlock(hash.into()),
                            )) {
                                Ok(_) => error, // Bubble up the NotFound error
                                Err(e) => e.into(),
                            }
                        } else {
                            error
                        }
                    })
            }
        }
    }

    /// Handle a response message corresponding to an avalanche request
    fn handle_avalanche_response(
        &mut self,
        from_peer: PeerId,
        response: avalanche_rpc::Response,
    ) -> Result<()> {
        Ok(match response {
            // TODO: if the peer didn't have the requested data, what do we do?
            // Do we ban the peer for not having data that they should?
            // Do we try to find the requested data on the DHT instead?
            avalanche_rpc::Response::Error(_) => todo!(),
            avalanche_rpc::Response::Block(block) => {
                if let Ok(hash) = block.hash() {
                    debug!("received block response {hash}={block:?}");
                    // TODO: need to check POW here
                    if let Err(e) = self.try_insert_block(block, Some(from_peer)) {
                        debug!("unable to insert requested block {hash}: {e}");
                    }
                }
            }
            avalanche_rpc::Response::Preference(_) => todo!(),
        })
    }

    /// Look up a block from the DAG
    pub fn get_block(&mut self, hash: &BlockHash) -> Result<Block> {
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
    fn is_strongly_preferred(&mut self, hash: &BlockHash) -> Result<bool> {
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
        .all(|parent| {
            self.is_preferred(&parent).unwrap_or_else(|e| {
                // This condition should never occur, because the block should not even be present
                // in the DAG or database without known parents
                warn!("Failed to determine preference for parent {parent}: {e}",);
                false
            })
        }))
    }

    /// Determine if the vertex corresponding to the given hash is preferred, as described in the
    /// Avalanche consensus paper.
    pub fn is_preferred(&mut self, hash: &BlockHash) -> Result<bool> {
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
            match self.conflicts.get(&input) {
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
// TODO: Frontier should be a vector of Arc<Rw<Vertex>>
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
            difficulty: self.next_difficulty(),
            miner,
            parents: self.0.iter().map(|v| v.hash().unwrap().into()).collect(),
            inputs: Vec::new(),
            time: Utc::now(),
            nonce: 0,
        }
    }
}

/// A waitlist is a dependency graph of ancestor blocks which must be processed before future child
/// blocks may be processed.
///
/// The list is constructed as a map of <K, V>, where K is the hash of a block, and V is a list
/// child blocks which depend on that block.
struct WaitList(LruCache<BlockHash, Vec<Arc<TracingRwLock<Vertex>>>>);

impl WaitList {
    /// Create a new waitlist
    fn new(cap: NonZeroUsize) -> WaitList {
        WaitList(LruCache::new(cap))
    }

    /// Inserts a new vertex into the waitlist.
    fn insert(&mut self, vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        for parent_hash in &vertex
            .read()
            .map_err(|_| Error::VertexReadLock)?
            .block
            .parents
        {
            if let Some(parent_queue) = self.0.get_mut(&parent_hash) {
                // Add this vertex as a descendent in the parent's queue
                parent_queue.push(vertex.clone());
            } else {
                // Insert a new entry
                let evicted = self.0.put(*parent_hash, vec![vertex.clone()]);

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
    fn remove(&mut self, hash: &BlockHash) -> Option<Vec<Arc<TracingRwLock<Vertex>>>> {
        self.0.pop(hash)
    }
}
