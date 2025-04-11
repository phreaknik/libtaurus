use super::{
    block,
    database::ConsensusDb,
    transaction::{self, Txo, TxoHash},
    vertex, Block, BlockHash, Event, SlimVertex, Vertex,
};
use crate::{
    p2p::{self, avalanche_rpc},
    params::{
        AVALANCHE_ACCEPTANCE_THRESHOLD, AVALANCHE_QUERY_COUNT, AVALANCHE_QUORUM,
        MAX_QUERY_MINER_AGE, QUERY_TIMEOUT_SEC,
    },
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
    #[error("block has already been decide")]
    AlreadyDecided(VertexHash),
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
    #[error("new block channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error("data not found")]
    NotFound,
    #[error("p2p action channel error")]
    P2pActionCh(#[from] tokio::sync::mpsc::error::SendError<p2p::Action>),
    #[error(transparent)]
    Transaction(#[from] transaction::Error),
    #[error("block spends unknown UTXOs")]
    UnknownTransactionInputs,
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
    #[error("error acquiring write lock on a vertex")]
    VertexWriteLock,
    #[error("error acquiring write lock on the waitlist")]
    WaitlistWriteLock,
}

// TODO: Mutexes likely not necessary around vertices, since entire DAG is guarded by mutex

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

/// Implementation of Avalanche DAG, for determining the preference of new vertices. Finalized
/// vertices are moved to a database. The Dag structure will only contain vertices which are not
/// yet decided.
pub struct Dag {
    /// Complete list of undecided blocks
    undecided_blocks: HashMap<BlockHash, Arc<Block>>,

    /// Complete list of undecided vertices
    undecided_vertices: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// List of transaction outputs being considered in the undecided portion of the DAG
    undecided_txos: HashMap<TxoHash, DagTxo>,

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

impl Dag {
    /// Create a new DAG
    pub fn new(
        config: Config,
        p2p_action_ch: UnboundedSender<p2p::Action>,
        events_ch: broadcast::Sender<Event>,
    ) -> Dag {
        // Create DAG instance, initialized with genesis block in the frontier
        let mut dag = Dag {
            undecided_blocks: HashMap::new(),
            undecided_vertices: HashMap::new(),
            undecided_txos: HashMap::new(),
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
            .write_block(None, config.genesis)
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
        } else if block
            .inputs
            .iter()
            .any(|txo_hash| !self.undecided_txos.contains_key(txo_hash))
        {
            // Block spends at least one UTXO, which does not exist in the active txo set
            Err(Error::UnknownTransactionInputs)
        } else {
            // Add its outputs to the list of undecided TXOs
            for &txo in &block.outputs {
                let txo_hash = txo.hash()?;
                if self
                    .undecided_txos
                    .insert(txo_hash, DagTxo::new(txo))
                    .is_some()
                {
                    error!("Registered duplicate txo: {txo_hash}");
                }
            }
            // Add it to the list of undecided blocks
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

    /// Try to create a vertex for the given block. Return a copy of the vertex and a bool
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

        // Check for missing data (missing parents, block, etc), and if any, request it from peers
        match self.request_if_missing_data(slim_vertex.clone(), sender) {
            Err(Error::MissingData) => {
                // Insert this vertex into the waitlist to be retried later
                self.waitlist.insert(slim_vertex.clone())?;
                Err(Error::MissingData)
            }
            Err(e) => Err(e),
            Ok(()) => Ok(()),
        }?;

        // Create a new undecided vertex object and insert it into the DAG
        let (rw_vertex, has_conflicts, block) = match self
            .create_undecided_vertex(slim_vertex.clone())
        {
            Err(Error::AlreadyDecided(vhash)) => {
                let bhash = &slim_vertex.block_hash;
                error!("Attempted to create a new undecided vertex for an already decided block");
                error!("Block {bhash} alredy decided in vertex {vhash}");
                Err(Error::AlreadyDecided(vhash))
            }
            Err(e) => Err(e),
            Ok(rw_vertex) => Ok(rw_vertex),
        }?;

        // If this vertex is conflict free, mark it as the preferred spender of its tx inputs
        if !has_conflicts {
            for txo_hash in &block.inputs {
                if let Some(txo) = self.undecided_txos.get_mut(txo_hash) {
                    txo.preferred_spender = Some(rw_vertex.clone());
                } else {
                    error!("Attempt to spend unknown txo");
                    return Err(Error::UnknownTransactionInputs);
                }
            }
        }

        // Query peers for their preference, according to the avalanche consensus protocol
        self.query_peer_preferences(vhash)?;

        // Add it to the collection of undecided vertices
        self.undecided_vertices.insert(vhash, rw_vertex.clone());

        let strongly_preferred = {
            // Add vertex as known child to each of its parents
            let vertex = rw_vertex.read().map_err(|_| Error::VertexWriteLock)?;
            for parent in vertex.undecided_parents.values() {
                parent
                    .write()
                    .map_err(|_| Error::VertexWriteLock)?
                    .known_children
                    .insert(vhash, rw_vertex.clone());
            }

            if vertex.strongly_preferred {
                // Remove its parents from the frontier, as they are no longer the youngest
                for parent in &vertex.parents {
                    self.frontier.remove(parent);
                }

                // Add it to the frontier
                self.frontier.insert(vhash, rw_vertex.clone());

                // Notify subscribers of new frontier
                self.events_ch
                    .send(Event::NewFrontier(self.frontier.keys().cloned().collect()))?;

                info!("Inserted preferred vertex {}", vhash.to_hex());
            } else {
                info!("Inserted non-preferred vertex {}", vhash.to_hex());
            }
            vertex.strongly_preferred
        };

        // Retry vertices in the waitlist
        // TODO: Waitlest rn only waits on missing parent. Also need to retry waitlist if new block
        // TODO: Do this async?
        self.remove_and_retry_waitlist(vhash);

        // Return whether or not this vertex is strongly_preferred
        Ok(strongly_preferred)
    }

    /// Begin querying our peers for their preference for the specified vertex
    fn query_peer_preferences(&mut self, vhash: VertexHash) -> Result<()> {
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
        Ok(())
    }

    /// Check if we have the necessary data to insert the given vertex, and request our peers for
    /// any data we are missing.
    fn request_if_missing_data(
        &mut self,
        slim_vertex: Arc<SlimVertex>,
        sender: Option<PeerId>,
    ) -> Result<()> {
        let vhash = slim_vertex.hash()?;
        // Look up block if missing
        let opt_block = self.undecided_blocks.get(&slim_vertex.block_hash);
        if opt_block.is_none() {
            if let Some(peer) = sender {
                self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                    peer,
                    avalanche_rpc::Request::GetBlock(slim_vertex.block_hash),
                ))?;
            }
            info!(
                "Missing block {} necessary to insert vertex {vhash}",
                slim_vertex.block_hash
            );
        }
        // Lookup any missing parents
        let missing_parents: Vec<String> = slim_vertex
            .parents
            .iter()
            .filter(|vhash| {
                // Filter out any vertex already in the undecided DAG
                self.undecided_vertices.get(vhash).is_none()
            })
            .filter(|vhash| {
                // Filter out any vertex already finalized in the database
                self.database
                    .read_vertex(vhash)
                    .expect("database read error")
                    .is_none()
            })
            .map(|&parent| {
                // Request missing parent from peers
                if let Some(peer) = sender {
                    self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                        peer,
                        avalanche_rpc::Request::GetVertex(parent),
                    ))?;
                }
                // Build a print statement
                Ok::<String, Error>(parent.to_short_hex())
            })
            .try_collect()?;
        if missing_parents.len() > 0 {
            let mut missing_str = format!("[{}", missing_parents[0]);
            for missing in missing_parents[1..].into_iter() {
                missing_str += &format!(", {}", missing);
            }
            missing_str += "]";
            info!("Missing parents necessary to insert vertex {vhash}: {missing_str}");
        }
        // Make sure transaction inputs are in the txo set. If they are not in the txo set at all,
        // then reject this vertex entirely. If they are in the txo set, but spent by another
        // vertex, it is still possible to insert this vertex, but it will be in conflict and
        // according to Avalanche consensus rules, it will not be accepted unless its confidence
        // exceeds that of its conflicts.
        if let Some(block) = opt_block {
            if !block
                .inputs
                .iter()
                .all(|txo_hash| self.undecided_txos.contains_key(txo_hash))
            {
                warn!("Received block spends unknown transaction outputs.");
                return Err(Error::UnknownTransactionInputs);
            }
        };
        if opt_block.is_none() || missing_parents.len() > 0 {
            Err(Error::MissingData)
        } else {
            Ok(())
        }
    }

    /// Build a ['Vertex'] from a ['SlimVertex'] and link it to the other vertices in the DAG.
    /// Returns the newly created vertex and a boolean indicating if this vertex has any conflicts
    /// with other vertices.
    fn create_undecided_vertex(
        &mut self,
        slim_vertex: Arc<SlimVertex>,
    ) -> Result<(Arc<TracingRwLock<Vertex>>, bool, Arc<Block>)> {
        // Only create undecided vertices from undecided blocks. If a block is not known or has
        // already been decided, there is no need to create a vertex object.
        if let Some(vhash) = self
            .database
            .lookup_vertex_for_block(&slim_vertex.block_hash)?
        {
            Err(Error::AlreadyDecided(vhash))
        } else {
            let block = self
                .undecided_blocks
                .get(&slim_vertex.block_hash)
                .ok_or(Error::NotFound)?;
            let has_conflicts = block.inputs.iter().any(|txo_hash| {
                self.undecided_txos
                    .get(txo_hash)
                    .expect("undecided vertex should already have validated inputs")
                    .preferred_spender
                    .is_some()
            });
            Ok((
                Arc::new(TracingRwLock::new(Vertex::new(
                    block.clone(),
                    slim_vertex.parents.clone(),
                    &self.undecided_vertices,
                    has_conflicts,
                )?)),
                has_conflicts,
                block.clone(),
            ))
        }
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

    /// Look up a vertex from the DAG, and indicate if this vertex is strongly preferred.
    pub fn get_vertex(&mut self, vhash: &VertexHash) -> Result<(Arc<SlimVertex>, bool)> {
        match self.undecided_vertices.get(vhash) {
            Some(rw_vertex) => {
                let vertex = rw_vertex.read().map_err(|_| Error::VertexReadLock)?;
                Ok((Arc::new(vertex.slim()?), vertex.strongly_preferred))
            }
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
                let bhash = descendent.block_hash;
                let vhash = descendent.hash().unwrap();
                if let Err(Error::UnknownTransactionInputs) =
                    self.try_insert_vertex(descendent, None)
                {
                    // If the TXOs spent by this vertex have been decidedly spent by another
                    // vertex, they will have been removed from the undecided txo set, and this
                    // block will no longer be spendable.
                    self.undecided_blocks.remove(&bhash);
                    self.undecided_vertices.remove(&vhash);
                    // TODO: should also proc on timer, to prevent accumulation of orphaned
                    // blocks/vertices
                }
            }
        }
    }

    /// Recompute the confidences of given vertex and all undecided ancestors. Returns the hashes of
    /// ancestors which can now be accepted.
    pub fn recompute_confidences(
        &mut self,
        rw_vertex: Arc<TracingRwLock<Vertex>>,
    ) -> Vec<VertexHash> {
        let mut vertex = rw_vertex.write().unwrap();
        // Recursively compute confidence as sum of chits in progeny
        vertex.confidence = vertex.chit
            + vertex
                .known_children
                .values()
                .map(|v| v.read().unwrap().confidence)
                .sum::<usize>();
        // Check if this vertex's confidence has surpassed that of any vertices in its conflict set
        let overtaken = vertex
            .block
            .inputs
            .iter()
            .filter_map(|txo_hash| {
                self.undecided_txos
                    .get(&txo_hash)
                    .unwrap()
                    .preferred_spender
                    .clone()
            })
            .filter(|pref| pref.read().unwrap().confidence < vertex.confidence)
            .collect::<Vec<_>>();
        for pref in overtaken {
            // Reset the conflicting vertex's confidence
            self.reset_confidence(rw_vertex.clone());
            // Reset the preferred spender of each UTXO spent by the overtaken vertex
            let conflict = pref.read().unwrap();
            for txo_hash in &conflict.block.inputs {
                self.undecided_txos
                    .get_mut(txo_hash)
                    .unwrap()
                    .preferred_spender = match vertex.block.inputs.contains(txo_hash) {
                    true => Some(rw_vertex.clone()),
                    false => None,
                }
            }
            // Remove this vertex from the frontier, if needed
            self.frontier.remove(&conflict.hash().unwrap());
        }
        // Increase confidence of parents and collect any ancestors which can be accepted
        let mut accepted = vertex
            .undecided_parents
            .values()
            .map(|parent| self.recompute_confidences(parent.clone()))
            .reduce(|mut acc, mut new| {
                acc.append(&mut new);
                acc
            })
            .unwrap();
        // Check if this vertex has reached the acceptance threshold
        if vertex.undecided_parents.len() == 0
            && vertex.confidence >= AVALANCHE_ACCEPTANCE_THRESHOLD
        {
            accepted.push(vertex.hash().unwrap());
        }
        accepted
    }

    /// Reset the convidence of this vertex and any children
    pub fn reset_confidence(&mut self, rw_vertex: Arc<TracingRwLock<Vertex>>) {
        let mut vertex = rw_vertex.write().unwrap();
        vertex.strongly_preferred = false;
        vertex.confidence = 0;
        vertex.chit = 0;
        // TODO: According to avalanche, children of a non-virtuous transaction should be retried
        // with new parents closer to genesis. Not doing so results in a liveness failure.
        for child in vertex.known_children.values() {
            // TODO paralellize this walk
            self.reset_confidence(child.clone());
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
                self.get_vertex(&vhash)
                    .map(|(_sv, pref)| Some(avalanche_rpc::Response::Preference(vhash, pref)))
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
            avalanche_rpc::Response::Preference(vhash, preferred) => {
                if let Some(scorecard) = self.scorecards.cache_get_mut(&vhash) {
                    // Make sure a peer's vote only gets counted once
                    if scorecard.pending.remove(&from_peer) {
                        scorecard.score += preferred as usize;
                    }
                    if scorecard.score >= AVALANCHE_QUORUM {
                        // Quorum has been reached. Award a chit and cancel the vote.
                        let mut accepted_vertices = Vec::new();
                        if let Some(v) = self.undecided_vertices.get(&vhash).map(|v| v.clone()) {
                            let mut vertex = v.write().map_err(|_| Error::VertexWriteLock)?;
                            vertex.chit = 1;
                            accepted_vertices.append(&mut self.recompute_confidences(v.clone()));
                        }
                        self.scorecards.cache_remove(&vhash);
                        self.finalize_vertices(accepted_vertices)?;
                    } else if scorecard.score + scorecard.pending.len() < AVALANCHE_QUORUM {
                        // Quorum is not possible. Reset confidence and cancel the vote.
                        if let Some(v) = self.undecided_vertices.get(&vhash) {
                            self.reset_confidence(v.clone());
                        }
                        self.scorecards.cache_remove(&vhash);
                    }
                }
                self.scorecards.flush();
            }
        })
    }

    /// Finalize the specified blocks
    fn finalize_vertices(&mut self, hashes: Vec<VertexHash>) -> Result<()> {
        for rw_vertex in hashes
            .iter()
            .filter_map(|hash| self.undecided_vertices.get(hash))
        {
            let vertex = rw_vertex.read().map_err(|_| Error::VertexWriteLock)?;
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
                self.database.write_vertex(None, vertex.slim()?).ok();
                // Remove this block's inputs from the set of undecided_txos
                for txo_hash in &vertex.block.inputs {
                    self.undecided_txos.remove(txo_hash);
                }
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

/// Wrapper around Utxo for use in undecided DAG
#[derive(Debug, Clone)]
pub struct DagTxo {
    /// The underlying transaction output
    txo: Txo,

    /// Link to the preferred vertex which spends this UTXO, if any
    preferred_spender: Option<Arc<TracingRwLock<Vertex>>>,
}

impl DagTxo {
    fn new(txo: Txo) -> DagTxo {
        DagTxo {
            txo,
            preferred_spender: None,
        }
    }
}
