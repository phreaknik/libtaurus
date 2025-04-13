use super::{
    block,
    database::ConsensusDb,
    transaction::{self, Txo, TxoHash},
    vertex::{self, WireVertex},
    voter_pool::{self, Scorecard, VoterPool},
    Block, BlockHash, Event, Vertex,
};
use crate::{
    p2p::{self, avalanche_rpc},
    params::{AVALANCHE_ACCEPTANCE_THRESHOLD, AVALANCHE_QUERY_COUNT, QUERY_TIMEOUT_SEC},
    randomx::RandomXVMInstance,
    VertexHash,
};
use cached::{Cached, TimedCache};
use libp2p::PeerId;
use lru::LruCache;
use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, result, sync::Arc};
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
    #[error("vertex has corrupt block hash")]
    CorruptBlockHash,
    #[error(transparent)]
    Database(#[from] super::database::Error),
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("missing block")]
    MissingBlock,
    #[error("missing parents")]
    MissingParents,
    #[error("missing the previously mined block from this miner")]
    MissingPrevMined,
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
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
    #[error(transparent)]
    VoterPool(#[from] voter_pool::Error),
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
    pub genesis: WireVertex,

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

    /// Voter pool to select from for Avalanche queries
    voter_pool: VoterPool,

    /// Database for block storage
    database: ConsensusDb,

    /// Collection of scorecards tracking the progress of pending queries
    scorecards: TimedCache<VertexHash, Scorecard>,

    /// RandomX VM instance for verifying proof-of-work
    randomx_vm: RandomXVMInstance,

    /// Channel to make requests of the P2P network
    p2p_action_ch: UnboundedSender<p2p::Action>,

    /// Handle to send events on the consensus event channel
    events_ch: broadcast::Sender<Event>,
}

impl Dag {
    /// Create a new DAG
    pub fn new(
        config: Config,
        randomx_vm: RandomXVMInstance,
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
            voter_pool: VoterPool::new(),
            database: ConsensusDb::open(&config.data_dir.join(DATABASE_DIR), true)
                .expect("Failed to open consensus database"),
            scorecards: TimedCache::with_lifespan(QUERY_TIMEOUT_SEC),
            randomx_vm,
            p2p_action_ch,
            events_ch,
        };
        dag.scorecards.set_refresh(false);
        dag.database
            .write_vertex(None, config.genesis)
            .expect("Failed to write genesis vertex");
        // Return the dag
        dag
    }

    /// Submit a block as available to be inserted into the DAG
    fn submit_block(&mut self, block: Block) -> Result<()> {
        block.verify_pow(&self.randomx_vm)?;
        self.undecided_blocks.insert(block.hash(), Arc::new(block));
        Ok(())
    }

    /// Try to submit this block as a vertex at the frontier of the DAG
    pub fn try_insert_block(&mut self, block: Block) -> Result<bool> {
        self.try_insert_vertex(
            Arc::new(WireVertex::new(
                block,
                self.frontier.values().cloned(),
                true,
            )?),
            None,
        )
    }

    /// Try to create a vertex for the given block. Return a copy of the vertex and a bool
    /// indicating if the vertex is currently preferred.
    pub fn try_insert_vertex(
        &mut self,
        wire_vertex: Arc<WireVertex>,
        sender: Option<PeerId>,
    ) -> Result<bool> {
        // TODO: need to check difficulty, block validity, etc

        debug!("Vertex submitted:\n{wire_vertex:?}");

        // If available, check proof-of-work before doing anything
        if let Some(block) = &wire_vertex.block {
            block.verify_pow(&self.randomx_vm)?;
        }

        // Check if we already have this vertex
        let vhash = wire_vertex.hash();
        if let Ok((_vertex, preferred)) = self.get_vertex(&vhash) {
            return Ok(preferred);
        }

        // Make sure the block referenced in this vertex can be spent
        // TODO: be sure to check this _at the parent state. Retry if fail.
        let conflict_free = self.check_conflicts(&wire_vertex)?;

        // Make sure this vertex isn't missing any parents
        match self.request_if_missing_parents(&wire_vertex, sender) {
            Err(e @ Error::MissingParents) => {
                self.waitlist.insert(wire_vertex.clone())?;
                Err(e)
            }
            Err(e) => Err(e),
            Ok(filled) => Ok(filled),
        }?;

        // Save the new block, or fill with known block if empty.
        let rw_vertex = match self.build_vertex(wire_vertex.clone(), sender, conflict_free) {
            Err(e @ Error::MissingPrevMined | e @ Error::MissingBlock) => {
                self.waitlist.insert(wire_vertex.clone())?;
                Err(e)
            }
            Err(e) => Err(e),
            Ok(filled) => Ok(filled),
        }?;

        // Add it to the collection of undecided vertices
        self.undecided_vertices.insert(vhash, rw_vertex.clone());

        // If this vertex is conflict free, mark it as the preferred spender of its tx inputs
        if conflict_free {
            // TODO: too many block.clones here
            for txo_hash in &wire_vertex.block.clone().unwrap().inputs {
                if let Some(txo) = self.undecided_txos.get_mut(txo_hash) {
                    txo.preferred_spender = Some(rw_vertex.clone());
                } else {
                    error!("Attempt to spend unknown txo");
                    return Err(Error::UnknownTransactionInputs);
                }
            }
        };

        // Add its outputs to the list of undecided TXOs
        for &txo in &wire_vertex.block.clone().unwrap().outputs {
            let txo_hash = txo.hash();
            if self
                .undecided_txos
                .insert(txo_hash, DagTxo::new(txo))
                .is_some()
            {
                error!("Registered duplicate txo: {txo_hash}");
            }
        }

        // TODO: How do we verify if this vertex ties back to genesis... Do we need to?

        // Broadcast to peers if this vertex didn't already come from our peers
        if sender.is_none() {
            self.p2p_action_ch
                .send(p2p::Action::Broadcast(p2p::MessageData::Vertex(
                    wire_vertex.as_ref().clone(),
                )))?;
        }

        // Query peers for their preference, according to the avalanche consensus protocol
        match self.query_peer_preferences(vhash) {
            Err(Error::VoterPool(voter_pool::Error::NotEnoughVoters)) => {
                warn!("Not enough voters to query vertex preference");
                Ok(())
            }
            other @ _ => other,
        }?;

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
        // TODO: Waitlist rn only waits on missing parent. Also need to retry waitlist if new block
        // TODO: Do this async?
        self.remove_and_retry_waitlist(vhash);

        // Return whether or not this vertex is strongly_preferred
        Ok(strongly_preferred)
    }

    /// Build a full vertex from the given ['WireVertex']. If a sender is provided, will attempt to
    /// request missing data from the sender.
    fn build_vertex(
        &mut self,
        wire_vertex: Arc<WireVertex>,
        sender: Option<PeerId>,
        conflict_free: bool,
    ) -> Result<Arc<TracingRwLock<Vertex>>> {
        if let Some(vhash) = self.database.lookup_vertex_for_block(&wire_vertex.bhash)? {
            Err(Error::AlreadyDecided(vhash))
        } else if self.undecided_blocks.contains_key(&wire_vertex.bhash) {
            Ok(Arc::new(TracingRwLock::new(Vertex::new(
                wire_vertex.as_ref(),
                &self.undecided_blocks,
                &self.undecided_vertices,
                conflict_free,
            )?)))
        } else if let Some(block) = &wire_vertex.block {
            if block.hash() == wire_vertex.bhash {
                if let Some(prev_mined) = block.prev_mined {
                    match self.get_block(&prev_mined) {
                        Err(Error::NotFound) => {
                            info!("Missing previous block from miner: {}", prev_mined);
                            if let Some(peer) = sender {
                                self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                                    peer,
                                    avalanche_rpc::Request::GetBlock(prev_mined),
                                ))?;
                                self.request_if_missing_parents(&wire_vertex, sender)?;
                            }
                            Err(Error::MissingPrevMined)
                        }
                        Err(e) => Err(e),
                        Ok(_) => Ok(()),
                    }?;
                }
            } else {
                return Err(Error::CorruptBlockHash);
            }
            let arc_block = Arc::new(block.clone());
            // Save this to new block
            self.undecided_blocks
                .insert(wire_vertex.bhash, arc_block.clone());
            // Register this miner as a potential Avalanche voter
            self.voter_pool.register_from_block(arc_block);
            Ok(Arc::new(TracingRwLock::new(Vertex::new(
                wire_vertex.as_ref(),
                &self.undecided_blocks,
                &self.undecided_vertices,
                conflict_free,
            )?)))
        } else {
            info!(
                "Missing block {} necessary to insert vertex {}",
                wire_vertex.bhash,
                wire_vertex.hash()
            );
            if let Some(peer) = sender {
                self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                    peer,
                    avalanche_rpc::Request::GetBlock(wire_vertex.bhash),
                ))?;
                self.request_if_missing_parents(&wire_vertex, sender)?;
            }
            Err(Error::MissingBlock)
        }
    }

    /// Check for missing parents and request them from peers
    fn request_if_missing_parents(
        &mut self,
        wire_vertex: &WireVertex,
        sender: Option<PeerId>,
    ) -> Result<()> {
        // Lookup any missing parents
        let missing_parents: Vec<String> = wire_vertex
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
            info!(
                "Missing parents necessary to insert vertex {}: {missing_str}",
                wire_vertex.hash()
            );
            Err(Error::MissingParents)
        } else {
            Ok(())
        }
    }

    /// Check that the vertex uniquely spends UTXOs. Returns true if no conflicts.
    fn check_conflicts(&mut self, wire_vertex: &WireVertex) -> Result<bool> {
        // Make sure transaction inputs are in the txo set. If they are not in the txo set at all,
        // then reject this vertex entirely. If they are in the txo set, but spent by another
        // vertex, it is still possible to insert this vertex, but it will be in conflict and
        // according to Avalanche consensus rules, it will not be accepted unless its confidence
        // exceeds that of its conflicts.
        if let Some(block) = &wire_vertex.block {
            if !block
                .inputs
                .iter()
                .all(|txo_hash| self.undecided_txos.contains_key(txo_hash))
            {
                warn!("Received block spends unknown transaction outputs.");
                Err(Error::UnknownTransactionInputs)
            } else if block.inputs.iter().any(|txo_hash| {
                self.undecided_txos
                    .get(txo_hash)
                    .expect("undecided vertex should already have validated inputs")
                    .preferred_spender
                    .is_some()
            }) {
                debug!("Received vertex has conflicts");
                Ok(false)
            } else {
                Ok(true)
            }
        } else {
            Err(Error::MissingBlock)
        }
    }

    /// Begin querying our peers for their preference for the specified vertex
    fn query_peer_preferences(&mut self, vhash: VertexHash) -> Result<()> {
        // TODO: this query could be initiated multiple times, if a block goes through waitlist
        // several times. Fix this. It should only happen once.
        let voters = self.voter_pool.select(AVALANCHE_QUERY_COUNT)?;
        for voter in &voters {
            self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                voter.read().unwrap().id(),
                avalanche_rpc::Request::GetPreference(vhash),
            ))?;
        }
        self.scorecards
            .cache_set(vhash, Scorecard::new_with_voters(voters));
        Ok(())
    }

    /// Look up a block from the DAG
    pub fn get_block(&mut self, bhash: &BlockHash) -> Result<Arc<Block>> {
        match self.undecided_blocks.get(bhash) {
            Some(block) => Ok(block.clone()),
            None => {
                let vhash = self
                    .database
                    .lookup_vertex_for_block(bhash)?
                    .ok_or(Error::NotFound)?;
                Ok(Arc::new(
                    self.database
                        .read_vertex(&vhash)?
                        .ok_or(Error::NotFound)?
                        .block
                        .unwrap(),
                ))
            }
        }
    }

    /// Look up a vertex from the DAG, and indicate if this vertex is strongly preferred.
    pub fn get_vertex(&mut self, vhash: &VertexHash) -> Result<(WireVertex, bool)> {
        match self.undecided_vertices.get(vhash) {
            Some(rw_vertex) => {
                let vertex = rw_vertex.read().map_err(|_| Error::VertexReadLock)?;
                Ok((vertex.to_wire()?, vertex.strongly_preferred))
            }
            None => Ok((
                self.database.read_vertex(vhash)?.ok_or(Error::NotFound)?,
                true,
            )),
        }
    }

    /// Remove the specified entry from the waitlist, and retry inserting any of is descendents
    fn remove_and_retry_waitlist(&mut self, vhash: VertexHash) {
        if let Ok(Some(descendents)) = self.waitlist.remove(&vhash) {
            for descendent in descendents {
                let bhash = descendent.bhash;
                let vhash = descendent.hash();
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
        trace!("Handling request: {request}");
        match request {
            avalanche_rpc::Request::GetBlock(bhash) => match self.get_block(&bhash) {
                Ok(block) => {
                    trace!("Sending block response {bhash}={block:?}");
                    Ok(Some(avalanche_rpc::Response::Block((*block).clone())))
                }
                Err(Error::NotFound) => {
                    debug!("Unable to find requested block: {bhash}");
                    Ok(None)
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
                    Ok(None)
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
            avalanche_rpc::Response::Block(block) => {
                let bhash = block.hash();
                debug!("received block response {bhash}={block:?}");
                match self.submit_block(block) {
                    Err(Error::Block(block::Error::InvalidPoW)) => {
                        // TODO: ban this peer for sending us a block with invalid POW
                        todo!()
                    }
                    Err(e) => debug!("unable to insert requested block {bhash}: {e}"),
                    _ => {}
                }
            }
            avalanche_rpc::Response::Vertex(wire_vertex) => {
                let vhash = wire_vertex.hash();
                debug!("received vertex response {vhash}={wire_vertex:?}");
                if let Err(e) = self.try_insert_vertex(Arc::new(wire_vertex), Some(from_peer)) {
                    debug!("unable to insert requested vertex {vhash}: {e}");
                }
            }
            avalanche_rpc::Response::Preference(vhash, preferred) => {
                if let Some(scorecard) = self.scorecards.cache_get_mut(&vhash) {
                    // Process this peer's vote
                    match scorecard.register_vote(&from_peer, preferred)? {
                        // Vote completed, vertex is preferred amongst peers
                        Some(true) => {
                            // Award a chit and cancel the vote.
                            let mut accepted_vertices = Vec::new();
                            if let Some(v) = self.undecided_vertices.get(&vhash).map(|v| v.clone())
                            {
                                let mut vertex = v.write().map_err(|_| Error::VertexWriteLock)?;
                                vertex.chit = 1;
                                accepted_vertices
                                    .append(&mut self.recompute_confidences(v.clone()));
                            }
                            self.finalize_vertices(accepted_vertices)?;
                        }
                        // Vote completed, vertex is NOT preferred amongst peers
                        Some(false) => {
                            if let Some(v) = self.undecided_vertices.get(&vhash) {
                                self.reset_confidence(v.clone());
                            }
                        }
                        // Vote is still open
                        None => {}
                    }
                }

                // Flush the TimedCache to drop any expired scorecards. This process will trigger
                // any peers which did not respond in time to get penalized in the scorecard drop
                // trait implementation.
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
                self.database.write_vertex(None, vertex.to_wire()?).ok();
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
struct WaitList(TracingRwLock<LruCache<VertexHash, Vec<Arc<WireVertex>>>>);

impl WaitList {
    /// Create a new waitlist
    fn new(cap: NonZeroUsize) -> WaitList {
        WaitList(TracingRwLock::new(LruCache::new(cap)))
    }

    /// Inserts a new vertex into the waitlist.
    fn insert(&mut self, wire_vertex: Arc<WireVertex>) -> Result<()> {
        // TODO: Too many clones in this function! use `Arc<WireVertex>`
        let mut cache = self.0.write().map_err(|_| Error::WaitlistWriteLock)?;
        for &parent_hash in &wire_vertex.parents {
            if let Some(parent_queue) = cache.get_mut(&parent_hash) {
                // Add this vertex as a descendent in the parent's queue
                parent_queue.push(wire_vertex.clone());
            } else {
                // Insert a new entry
                if let Some(evicted) = cache.put(parent_hash, vec![wire_vertex.clone()]) {
                    warn!("Waitlist unexpectedly evicted vertices: {evicted:?}")
                }
            }
        }
        Ok(())
    }

    /// Remove an entry from the list, and return the vertices which were waiting on it
    fn remove(&mut self, vhash: &VertexHash) -> Result<Option<Vec<Arc<WireVertex>>>> {
        Ok(self
            .0
            .write()
            .map_err(|_| Error::WaitlistWriteLock)?
            .pop(vhash))
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
