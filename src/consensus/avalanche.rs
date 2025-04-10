use super::{block, database::ConsensusDb, vertex, Block, BlockHash, Event, SlimVertex, Vertex};
use crate::{
    p2p::{self, avalanche_rpc},
    params::{AVALANCHE_QUERY_COUNT, AVALANCHE_QUORUM, MAX_QUERY_MINER_AGE, QUERY_TIMEOUT_SEC},
    VertexHash,
};
use cached::{Cached, TimedCache};
use libp2p::PeerId;
use lru::LruCache;
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroUsize,
    path::PathBuf,
    result,
    sync::Arc,
};
use tokio::sync::{broadcast, mpsc::UnboundedSender};
use tracing::{debug, error, info, trace, warn};
use tracing_mutex::stdsync::TracingRwLock;
//TODO: use RC instead of ARC. Don't need thread safety

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "blocks_db/";

/// Error type for avalanche errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] block::Error),
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
    MissingParents(Vec<VertexHash>),
    #[error("new block channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error("data not found")]
    NotFound,
    #[error("p2p action channel error")]
    P2pActionCh(#[from] tokio::sync::mpsc::error::SendError<p2p::Action>),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
    #[error("error acquiring write lock on a vertex")]
    VertexWriteLock,
    #[error("error acquiring write lock on the waitlist")]
    WaitlistWriteLock,
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
    /// Complete list of undecided blocks
    undecided_blocks: HashMap<BlockHash, Arc<Block>>,

    /// Complete list of undecided vertices
    vertices: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// Conflict sets for each transaction input
    conflicts: HashMap<BlockHash, Vec<Arc<TracingRwLock<Vertex>>>>,

    /// Waitlist for vertices that cannot be inserted yet
    waitlist: WaitList,

    /// The active edge of the DAG, i.e. the preferred vertices which don't yet have children
    frontier: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// Database for block storage
    database: ConsensusDb,

    /// Collection of scorecards tracking the progress of pending queries
    scorecards: TimedCache<VertexHash, Scorecard>,

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
        // Create DAG instance, initialized with genesis block in the frontier
        let mut dag = DAG {
            undecided_blocks: HashMap::new(),
            vertices: HashMap::new(),
            conflicts: HashMap::new(),
            waitlist: WaitList::new(config.waitlist_cap),
            frontier: HashMap::new(),
            database: ConsensusDb::open(&config.data_dir.join(DATABASE_DIR), true)
                .expect("Failed to open consensus database"),
            scorecards: TimedCache::with_lifespan(QUERY_TIMEOUT_SEC),
            p2p_action_ch,
            events_ch,
        };
        dag.scorecards.set_refresh(false);
        dag.database
            .write_block(None, &config.genesis)
            .expect("Failed to write genesis block");

        // Return the dag
        dag
    }

    /// Register a block as available to be inserted into the DAG. This must occur before a vertex
    /// referencing this block can be inserted.
    pub fn submit_block(&mut self, block: Block, build_vertex: bool) -> Result<()> {
        // TODO: need to check pow, difficulty, block validity, etc
        let bhash = block.hash()?;
        if let Ok(Some(_)) = self.database.read_block(&bhash) {
            // First see if the block is already in the database of decided blocks
            Ok(())
        } else {
            // Otherwise add it to the list of undecided blocks
            info!("Registered new block {}", bhash.to_hex());
            self.undecided_blocks.insert(bhash, Arc::new(block.clone()));
            if build_vertex {
                let slim_vertex =
                    Arc::new(SlimVertex::new(bhash, self.frontier.values().cloned())?);
                self.try_insert_vertex(slim_vertex.clone(), None)?;
                self.p2p_action_ch
                    .send(p2p::Action::Broadcast(p2p::MessageData::Vertex(
                        slim_vertex.to_wire(block)?,
                    )))?;
            }
            Ok(())
        }
    }

    /// Try to create a vertex for the given block. Return a copy of the vertex ans a bool
    /// indicating if the vertex is currently preferred.
    pub fn try_insert_vertex(
        &mut self,
        slim_vertex: Arc<SlimVertex>,
        sender: Option<PeerId>,
    ) -> Result<bool> {
        // First see if we already have this vertex
        let vhash = slim_vertex.hash()?;
        if let Ok((_vertex, preferred)) = self.get_vertex(&vhash) {
            return Ok(preferred);
        }

        // TODO: How do we verify if this vertex ties back to genesis... Do we need to?

        // Query peers for their preference, according to the avalanche consensus protocol
        // TODO: this query could be initiated multiple times, if a block goes through waitlist
        // several times. Fix this. It should only happen once.
        let voters = self
            .database
            .select_random_miners(AVALANCHE_QUERY_COUNT, MAX_QUERY_MINER_AGE)?
            .into_iter()
            .map(|peer| {
                self.p2p_action_ch
                    .send(p2p::Action::AvalancheRequest(
                        peer,
                        avalanche_rpc::Request::GetPreference(vhash),
                    ))
                    .map(|_| peer)
            })
            .try_collect()?;
        self.scorecards
            .cache_set(vhash, Scorecard::new_with_voters(voters));

        // Create a new undecided vertex object and insert it into the DAG
        let rw_vertex = match self.create_undecided_vertex(slim_vertex.clone()) {
            Err(Error::NotFound) | Err(Error::MissingParents(_)) => {
                debug!("Vertex {vhash} is missing block or parents");
                self.waitlist.insert(slim_vertex.clone())?;
                if let Some(peer) = sender {
                    // Initiate lookup for missing data
                    // TODO: Lookup should fail over to another mechanism if this peer fails
                    self.request_missing_data(slim_vertex, peer)?;
                }
                Err(Error::MissingData)
            }
            Err(e) => Err(e),
            Ok(rw_vertex) => Ok(rw_vertex),
        }?;

        // Determine vertex preference
        {
            let preference = self.is_vertex_preferred(&rw_vertex)?;
            let mut vertex = rw_vertex.write().map_err(|_| Error::VertexWriteLock)?;
            vertex.preferred = preference;

            // Update each parent
            for parent in vertex.undecided_parents.values_mut() {
                parent
                    .write()
                    .map_err(|_| Error::VertexWriteLock)?
                    .known_children
                    .insert(vhash, rw_vertex.clone());
            }

            // Update the conflict set for each transaction input
            for &input in vertex.block.inputs.iter() {
                // Make sure conflict sets exist for each tx input
                if let Some(conflict_set) = self.conflicts.get_mut(&input) {
                    conflict_set.push(rw_vertex.clone());
                } else {
                    // Create a new conflict set for this transaction input
                    self.conflicts.insert(input, vec![rw_vertex.clone()]);
                }
            }

            // Add it to the collection of vertices
            self.vertices.insert(vhash, rw_vertex.clone());

            if vertex.preferred {
                // Add it to the frontier
                self.frontier.insert(vhash, rw_vertex.clone());

                // Remove its parents from the frontier, as they are no longer the youngest
                for parent in &vertex.parents {
                    self.frontier.remove(parent);
                }

                // Notify subscribers of new frontier
                self.events_ch
                    .send(Event::NewFrontier(self.frontier.keys().cloned().collect()))?;

                info!("Appended preferred vertex {}", vhash.to_hex());
            } else {
                info!("Received non-preferred vertex {}", vhash.to_hex());
            }
        }

        // Retry vertices in the waitlist
        self.remove_and_retry_waitlist(vhash);

        // See if this vertex is strongly preferred, and return the result
        self.is_strongly_preferred(&vhash)
    }

    fn request_missing_data(&mut self, slim_vertex: Arc<SlimVertex>, peer: PeerId) -> Result<()> {
        // Look up block if missing
        if let Err(Error::NotFound) = self.get_block(&slim_vertex.block_hash) {
            self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                peer,
                avalanche_rpc::Request::GetBlock(slim_vertex.block_hash),
            ))?;
        }
        // Lookup any missing parents
        for &parent in slim_vertex
            .parents
            .iter()
            .filter(|vhash| {
                // Filter out any vertex already in the undecided DAG
                self.vertices.get(vhash).is_none()
            })
            .filter(|vhash| {
                // Filter out any vertex already finalized in the database
                self.database
                    .read_vertex(vhash)
                    .expect("database read error")
                    .is_none()
            })
        {
            self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                peer,
                avalanche_rpc::Request::GetVertex(parent),
            ))?;
        }
        Ok(())
    }

    /// Build a ['Vertex'] from a ['SlimVertex'] and link it to the other vertices in the DAG
    fn create_undecided_vertex(
        &mut self,
        slim_vertex: Arc<SlimVertex>,
    ) -> Result<Arc<TracingRwLock<Vertex>>> {
        // Only create undecided vertices from undecided blocks. If a block is not known or has
        // already been finalized, there is no need to create a vertex object.
        let block = self
            .undecided_blocks
            .get(&slim_vertex.block_hash)
            .ok_or(Error::NotFound)?;
        let rw_vertex = Arc::new(TracingRwLock::new(Vertex::new(
            block.clone(),
            slim_vertex.parents.clone(),
        )));
        self.fill_undecided_parents(rw_vertex.clone())?;
        Ok(rw_vertex)
    }

    /// Fill out parent links if the parents are present in the DAG
    fn fill_undecided_parents(&mut self, rw_vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        let mut vertex = rw_vertex.write().map_err(|_| Error::VertexWriteLock)?;
        vertex.undecided_parents = vertex
            .parents
            .iter()
            .filter(|&vhash| {
                // Filter out any vertex already finalized in the database
                self.database
                    .read_vertex(vhash)
                    .expect("database read error")
                    .is_none()
            })
            .map(|&vhash| {
                self.vertices
                    .get(&vhash)
                    .ok_or(Error::MissingParents(vec![vhash]))
                    .map(|vertex| (vhash, vertex.clone()))
            })
            .try_collect()?;
        Ok(())
    }

    /// Look up a block from the DAG
    pub fn get_block(&mut self, bhash: &BlockHash) -> Result<Arc<Block>> {
        match self.undecided_blocks.get(bhash) {
            Some(block) => Ok(block.clone()),
            None => Ok(Arc::new(
                self.database.read_block(bhash)?.ok_or(Error::NotFound)?,
            )),
        }
    }

    /// Look up a block from the DAG
    pub fn get_vertex(&mut self, vhash: &VertexHash) -> Result<(Arc<SlimVertex>, bool)> {
        match self.vertices.get(vhash) {
            Some(vertex) => Ok((
                Arc::new(
                    vertex
                        .read()
                        .map_err(|_| Error::VertexReadLock)?
                        .try_into()?,
                ),
                self.is_vertex_preferred(vertex)?,
            )),
            None => Ok((
                Arc::new(self.database.read_vertex(vhash)?.ok_or(Error::NotFound)?),
                true,
            )),
        }
    }

    /// Remove the specified entry from the waitlist, and retry inserting any of is descendents
    fn remove_and_retry_waitlist(&mut self, vhash: VertexHash) {
        if let Ok(Some(descendents)) = self.waitlist.remove(&vhash) {
            for descendent in descendents {
                let _ = self.try_insert_vertex(descendent, None);
            }
        }
    }

    /// Handle any avalanche requests or responses
    pub fn handle_avalanche_message(&mut self, message: avalanche_rpc::Event) -> Result<()> {
        match message {
            avalanche_rpc::Event::Requested(peer, request_id, request) => {
                // Handle the request and respond to the requester
                self.handle_avalanche_request(peer, request)
                    .and_then(|opt_response| match opt_response {
                        Some(response) => self
                            .p2p_action_ch
                            .send(p2p::Action::AvalancheResponse(request_id, response))
                            .map_err(Error::from),
                        None => Ok(()),
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
    ) -> Result<Option<avalanche_rpc::Response>> {
        debug!("Handling request: {request}");
        match request {
            avalanche_rpc::Request::GetBlock(bhash) => match self.get_block(&bhash) {
                Ok(block) => {
                    trace!("Sending block response {bhash}={block:?}");
                    Ok(Some(avalanche_rpc::Response::Block((*block).clone())))
                }
                Err(Error::NotFound) => {
                    debug!("Unable to find requested block: {bhash}");
                    Ok(Some(avalanche_rpc::Response::Error(
                        avalanche_rpc::proto::mod_Response::Error::NOT_FOUND,
                    )))
                }
                Err(e) => {
                    error!("Unexpected error while looking for block {bhash}: {e}");
                    Err(e.into())
                }
            },
            avalanche_rpc::Request::GetVertex(vhash) => match self.get_vertex(&vhash) {
                Ok((vertex, _preferred)) => {
                    trace!("Sending vertex response {vhash}={vertex:?}");
                    Ok(Some(avalanche_rpc::Response::Vertex(vertex)))
                }
                Err(Error::NotFound) => {
                    debug!("Unable to find requested vertex: {vhash}");
                    Ok(Some(avalanche_rpc::Response::Error(
                        avalanche_rpc::proto::mod_Response::Error::NOT_FOUND,
                    )))
                }
                Err(e) => {
                    error!("Unexpected error while looking for vertex {vhash}: {e}");
                    Err(e.into())
                }
            },
            avalanche_rpc::Request::GetPreference(vhash) => {
                self.is_strongly_preferred(&vhash)
                    .map(|preferred| Some(avalanche_rpc::Response::Preference(vhash, preferred)))
                    .map_err(|error| {
                        debug!("Unable to determine preference for {vhash}: {error}");
                        if let Error::NotFound = error {
                            match self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                                from_peer,
                                avalanche_rpc::Request::GetVertex(vhash),
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
                if let Ok(bhash) = block.hash() {
                    debug!("received block response {bhash}={block:?}");
                    // TODO: need to check POW here
                    if let Err(e) = self.submit_block(block, false) {
                        debug!("unable to insert requested block {bhash}: {e}");
                    }
                }
            }
            avalanche_rpc::Response::Vertex(slim_vertex) => {
                if let Ok(vhash) = slim_vertex.hash() {
                    debug!("received vertex response {vhash}={slim_vertex:?}");
                    // TODO: need to check POW here
                    if let Err(e) = self.try_insert_vertex(slim_vertex, Some(from_peer)) {
                        debug!("unable to insert requested vertex {vhash}: {e}");
                    }
                }
            }
            avalanche_rpc::Response::Preference(hash, preferred) => {
                if let Some(scorecard) = self.scorecards.cache_get_mut(&hash) {
                    // Make sure a peer's vote only gets counted once
                    if scorecard.pending.remove(&from_peer) {
                        scorecard.score += preferred as usize;
                    }
                    if scorecard.score >= AVALANCHE_QUORUM {
                        // Quorum has been reached. Award a chit and cancel the vote.
                        let mut accepted_vertices = Vec::new();
                        if let Some(v) = self.vertices.get(&hash) {
                            let mut vertex = v.write().map_err(|_| Error::VertexWriteLock).unwrap();
                            vertex.chit = 1;
                            accepted_vertices.append(&mut vertex.increase_confidence());
                        }
                        self.scorecards.cache_remove(&hash);
                        self.finalize_vertices(accepted_vertices)?;
                    } else if scorecard.score + scorecard.pending.len() < AVALANCHE_QUORUM {
                        // Quorum is not possible. Reset confidence and cancel the vote.
                        if let Some(v) = self.vertices.get(&hash) {
                            let mut vertex = v.write().map_err(|_| Error::VertexWriteLock).unwrap();
                            vertex.reset_confidence();
                        }
                        self.scorecards.cache_remove(&hash);
                    }
                }
                self.scorecards.flush();
            }
        })
    }

    /// Determine if the specified vertex is strongly preferred, as described in the Avalanche
    /// consensus paper.
    fn is_strongly_preferred(&mut self, vhash: &VertexHash) -> Result<bool> {
        Ok(match self.vertices.get(vhash) {
            // Look for the vertex in the in-memory DAG
            Some(vertex) => vertex
                .read()
                .map_err(|_| Error::VertexReadLock)?
                .parents
                .clone(),
            // If its not in the in-memory DAG, check the database.
            None => self
                .database
                .read_vertex(&vhash.clone())?
                .ok_or(Error::NotFound)?
                .parents
                .clone(),
        }
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
    pub fn is_preferred(&mut self, vhash: &VertexHash) -> Result<bool> {
        Ok(match self.vertices.get(vhash) {
            // Look for the vertex in the in-memory DAG
            Some(vertex) => self.is_vertex_preferred(vertex)?,
            // If its not in the in-memory DAG, check the database. If it exists, then the block is
            // preferred.
            None => self.database.read_vertex(vhash)?.is_some(),
        })
    }

    /// Determine if the given vertex is preferred, as described in the Avalanche consensus paper.
    fn is_vertex_preferred(&self, rw_vertex: &Arc<TracingRwLock<Vertex>>) -> Result<bool> {
        let vertex = rw_vertex.read().map_err(|_| Error::VertexReadLock)?;
        let vhash = vertex.hash()?;
        Ok(vertex.block.inputs.iter().all(|input| {
            match self.conflicts.get(&input) {
                Some(cs) => !cs.into_iter().any(|v| {
                    // If a conflict set exists, and any other vertex in the conflict set is
                    // preferred, this vertex cannot be preferred
                    let cs_entry_hash = v.read().unwrap().hash().unwrap(); // TODO: this would be
                                                                           // much more efficient
                                                                           // if conflict set was
                                                                           // HashMap<vhash, v>
                                                                           // instead of Vec<v>
                    cs_entry_hash != vhash
                        && self
                            .vertices
                            .get(&cs_entry_hash)
                            .unwrap()
                            .read()
                            .unwrap()
                            .preferred
                            == true
                }),
                None => true,
            }
        }))
    }

    /// Finalize the specified blocks
    fn finalize_vertices(&mut self, hashes: Vec<VertexHash>) -> Result<()> {
        for rw_vertex in hashes.iter().filter_map(|hash| self.vertices.get(hash)) {
            let vertex = rw_vertex.write().map_err(|_| Error::VertexWriteLock)?;
            // Only finalize the block if it has no undecided parents
            if vertex.undecided_parents.len() == 0 {
                // Remove this block from its children's undecided_parents lists
                for child in vertex.known_children.values() {
                    child
                        .write()
                        .unwrap()
                        .undecided_parents
                        .remove(&vertex.hash()?);
                }
                // Write the block to the database
                // TODO: Figure out how to batch these database writes
                self.database.write_vertex(&vertex.try_into()?).ok();
            }
        }
        Ok(())
    }
}

/// A waitlist is a dependency graph of ancestor vertices which must be processed before future
/// child vertices may be processed.
///
/// The list is constructed as a map of <K, V>, where K is the hash of a vertex, and V is a list
/// child vertices which depend on that vertex.
struct WaitList(TracingRwLock<LruCache<VertexHash, Vec<Arc<SlimVertex>>>>);

impl WaitList {
    /// Create a new waitlist
    fn new(cap: NonZeroUsize) -> WaitList {
        WaitList(TracingRwLock::new(LruCache::new(cap)))
    }

    /// Inserts a new vertex into the waitlist.
    fn insert(&mut self, slim_vertex: Arc<SlimVertex>) -> Result<()> {
        let mut cache = self.0.write().map_err(|_| Error::WaitlistWriteLock)?;
        for &parent_hash in slim_vertex.clone().parents.iter() {
            if let Some(parent_queue) = cache.get_mut(&parent_hash) {
                // Add this vertex as a descendent in the parent's queue
                parent_queue.push(slim_vertex.clone());
            } else {
                // Insert a new entry
                if let Some(evicted) = cache.put(parent_hash, vec![slim_vertex.clone()]) {
                    warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                }
            }
        }
        Ok(())
    }

    /// Remove an entry from the list, and return the vertices which were waiting on it
    fn remove(&mut self, vhash: &VertexHash) -> Result<Option<Vec<Arc<SlimVertex>>>> {
        Ok(self
            .0
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .pop(vhash))
    }
}

/// A scorecard to count votes received from peers during a round of queries
struct Scorecard {
    pending: HashSet<PeerId>, // Peers that still need to vote
    score: usize,             // Total score from peers which have voted
}

impl Scorecard {
    fn new_with_voters(voters: HashSet<PeerId>) -> Scorecard {
        Scorecard {
            pending: voters,
            score: 0,
        }
    }
}
