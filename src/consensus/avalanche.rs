use super::{
    block,
    database::ConsensusDb,
    transaction::{self, Txo, TxoHash},
    vertex::{self, WireVertex},
    voter_pool::{self, Scorecard, VoterPool},
    waitlist::{self, WaitList},
    Block, BlockHash, Event, Vertex,
};
use crate::{
    p2p::{self, consensus_rpc},
    params::{AVALANCHE_ACCEPTANCE_THRESHOLD, AVALANCHE_QUERY_COUNT, QUERY_TIMEOUT_SEC},
    randomx::RandomXVMInstance,
    VertexHash,
};
use cached::{Cached, TimedCache};
use libp2p::PeerId;
use std::{
    collections::{HashMap, HashSet},
    iter::once,
    num::NonZeroUsize,
    ops::DerefMut,
    path::PathBuf,
    result,
    sync::Arc,
};
use tokio::sync::{broadcast, mpsc::UnboundedSender};
use tracing::{debug, error, info, trace, warn};
use tracing_mutex::stdsync::TracingRwLock;

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "blocks_db/";

/// Time a block or vertex is allowed to remain undecided without making progress towards a
/// decision.
pub const DECISION_TIMEOUT_SEC: u64 = 60 * 60 * 24;

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
    MissingBlock(BlockHash),
    #[error("missing parents")]
    MissingParents(Vec<VertexHash>),
    #[error("missing the previously mined block from this miner")]
    MissingPrevMined(BlockHash),
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
    #[error(transparent)]
    WaitList(#[from] waitlist::Error),
}

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
    /// Configuration parameters
    config: Config,

    /// Complete list of undecided blocks
    undecided_blocks: TimedCache<BlockHash, Arc<Block>>,

    /// Complete list of undecided vertices
    undecided_vertices: TimedCache<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// List of transaction outputs being considered in the undecided portion of the DAG
    undecided_txos: HashMap<TxoHash, DagTxo>,

    /// Waitlist for vertices that cannot be inserted yet
    waitlist: WaitList,

    /// The active edge of the DAG, i.e. the preferred vertices which don't yet have children
    frontier: HashMap<VertexHash, Arc<TracingRwLock<Vertex>>>,

    /// Voter pool to select from for Avalanche queries
    voter_pool: VoterPool,

    /// List of vertices actively being queried
    pending_queries: HashSet<VertexHash>,

    /// Collection of scorecards tracking the progress of pending queries
    scorecards: TimedCache<VertexHash, Scorecard>,

    /// Database for block storage
    database: ConsensusDb,

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
            undecided_blocks: TimedCache::with_lifespan_and_refresh(DECISION_TIMEOUT_SEC, true),
            undecided_vertices: TimedCache::with_lifespan_and_refresh(DECISION_TIMEOUT_SEC, true),
            undecided_txos: HashMap::new(),
            waitlist: WaitList::new(config.waitlist_cap),
            frontier: HashMap::new(),
            voter_pool: VoterPool::new(),
            pending_queries: HashSet::new(),
            scorecards: TimedCache::with_lifespan(QUERY_TIMEOUT_SEC),
            database: ConsensusDb::open(&config.data_dir.join(DATABASE_DIR), true)
                .expect("Failed to open consensus database"),
            randomx_vm,
            p2p_action_ch,
            events_ch,
            config: config.clone(),
        };
        dag.scorecards.set_refresh(false);
        let genesis_hash = config.genesis.hash();
        let rw_vertex = Arc::new(TracingRwLock::new(Vertex::genesis(config.genesis.clone())));
        dag.database
            .write_vertex(None, config.genesis)
            .unwrap()
            .commit()
            .unwrap();
        dag.frontier.insert(genesis_hash, rw_vertex);
        // Return the dag
        dag
    }

    pub fn genesis_hash(&self) -> VertexHash {
        self.config.genesis.hash()
    }

    /// Register a block as available to be inserted into the DAG
    fn register_block(&mut self, block: Block) -> Result<()> {
        // Check the block has sufficient proof-of-work
        block.verify_pow(&self.randomx_vm)?;
        let bhash = block.hash();
        if let Some(vhash) = self.database.lookup_vertex_for_block(&bhash)? {
            Err(Error::AlreadyDecided(vhash))
        } else {
            let arc_block = Arc::new(block);
            // Add the block to the list of undecided blocks
            self.undecided_blocks.cache_set(bhash, arc_block.clone());
            // Register this miner as a potential Avalanche voter
            self.voter_pool.register_from_block(arc_block);
            // Attempt to insert any vertices waiting on that block
            if let Some(vertices) = self.waitlist.get_by_block(&bhash)? {
                self.try_insert_vertices(vertices, None, false)?;
            }
            Ok(())
        }
    }

    /// Try to submit this block as a vertex at the frontier of the DAG
    pub fn try_insert_block(&mut self, block: Block, broadcast: bool) -> Result<()> {
        let wire_vertex = Arc::new(WireVertex::new(
            block,
            self.frontier.values().cloned(),
            true,
        )?);
        self.try_insert_vertices(once(wire_vertex), None, broadcast)
    }

    /// Try to insert a list of vertices, and attempt to also insert any known waiting children
    pub fn try_insert_vertices<V>(
        &mut self,
        vertices: V,
        sender: Option<PeerId>,
        broadcast: bool,
    ) -> Result<()>
    where
        V: IntoIterator<Item = Arc<WireVertex>>,
    {
        // Collect and sort vertices to avoid silly insertion sequence errors
        let mut insert_list: Vec<_> = vertices.into_iter().collect();
        insert_list.deref_mut().sort();

        // Try to insert the given vertices
        let mut successful = Vec::new();
        for wire_vertex in insert_list {
            let vhash = wire_vertex.hash();
            let bhash = wire_vertex.bhash;
            match self.try_insert_vertex(wire_vertex.clone(), sender, broadcast) {
                Ok(preferred) => {
                    info!(
                        "Inserted {}preferred vertex {}",
                        if !preferred { "non-" } else { "" },
                        vhash.to_hex()
                    );
                    Ok(successful.push((vhash, bhash)))
                }
                Err(
                    e @ Error::MissingPrevMined(_)
                    | e @ Error::MissingBlock(_)
                    | e @ Error::MissingParents(_),
                ) => {
                    info!("Error inserting {vhash}: {e}");
                    Ok(())
                }
                Err(e) => {
                    info!("Error inserting {vhash}: {e}");
                    Err(e)
                }
            }?;
        }
        // Attempt to insert any dependents of the successful insertions
        while let Some((vhash, bhash)) = successful.pop() {
            let waiting_by_block = self.waitlist.get_by_block(&bhash)?;
            let waiting_by_vertex = self.waitlist.get_by_vertex(&vhash)?;
            let mut try_insert_waiting = |wire_vertex: Arc<WireVertex>| -> Result<()> {
                let vhash = wire_vertex.hash();
                let bhash = wire_vertex.bhash;
                // Exclude sender, because there is no expectation the sender can serve a
                // missing data request for items in the waitlist.
                match self.try_insert_vertex(wire_vertex.clone(), None, false) {
                    Ok(preferred) => {
                        info!(
                            "Inserted {}preferred vertex {}",
                            if !preferred { "non-" } else { "" },
                            vhash.to_hex()
                        );
                        Ok(successful.push((vhash, bhash)))
                    }
                    Err(
                        e @ Error::MissingPrevMined(_)
                        | e @ Error::MissingBlock(_)
                        | e @ Error::MissingParents(_),
                    ) => {
                        info!("Error inserting {vhash}: {e}");
                        Ok(())
                    }
                    Err(e) => {
                        info!("Error inserting {vhash}: {e}");
                        Err(e)
                    }
                }
            };
            // Try to insert any vertices which were waiting on the block just inserted
            if let Some(waiting) = waiting_by_block {
                for wire_vertex in waiting {
                    try_insert_waiting(wire_vertex)?;
                }
            }
            // Try to insert any vertices which were waiting on the vertex just inserted
            if let Some(waiting) = waiting_by_vertex {
                for wire_vertex in waiting {
                    try_insert_waiting(wire_vertex)?;
                }
            }
        }
        Ok(())
    }

    /// Attempt to insert the vertex into the DAG. If successful, will return true if the vertex is
    /// currently strongly preferred.
    fn try_insert_vertex(
        &mut self,
        wire_vertex: Arc<WireVertex>,
        sender: Option<PeerId>,
        broadcast: bool,
    ) -> Result<bool> {
        let vhash = wire_vertex.hash();
        debug!("Vertex submitted: {vhash} = {wire_vertex}");

        // Check if we already have this vertex
        if let Ok((_v, preferred)) = self.get_vertex(&vhash) {
            return Ok(preferred);
        }

        // If available, check proof-of-work before doing anything
        if let Some(block) = &wire_vertex.block {
            if wire_vertex.bhash != block.hash() {
                return Err(Error::CorruptBlockHash);
            }
            self.register_block(block.clone())?;
        }

        // Make sure this vertex isn't missing any necessary data to process this vertex
        match self.request_if_missing_data(&wire_vertex, sender) {
            Err(Error::MissingPrevMined(bhash) | Error::MissingBlock(bhash)) => {
                self.waitlist
                    .insert(wire_vertex.clone(), None, Some(bhash))?;
                Err(Error::MissingBlock(bhash))
            }
            Err(Error::MissingParents(parents)) => {
                self.waitlist
                    .insert(wire_vertex.clone(), Some(parents.clone()), None)?;
                Err(Error::MissingParents(parents))
            }
            Err(e) => Err(e),
            Ok(()) => Ok(()),
        }?;

        // Make sure the block referenced in this vertex can be spent
        let (conflict_free, opt_block) = self.check_conflicts(&wire_vertex)?;

        // Create a mutex protected vertex for inserting into the DAG structure
        let rw_vertex = Arc::new(TracingRwLock::new(Vertex::new(
            wire_vertex.as_ref(),
            &mut self.undecided_blocks,
            &mut self.undecided_vertices,
            conflict_free,
        )?));

        // Add it to the collection of undecided vertices
        self.undecided_vertices.cache_set(vhash, rw_vertex.clone());

        // If this vertex is conflict free, mark it as the preferred spender of its tx inputs
        if conflict_free {
            for txo_hash in opt_block
                .as_ref()
                .map(|b| b.inputs.iter())
                .unwrap_or_else(|| wire_vertex.block.as_ref().unwrap().inputs.iter())
            {
                self.undecided_txos
                    .get_mut(txo_hash)
                    .expect("Attempted to spend non-existent UTXO")
                    .preferred_spender = Some(rw_vertex.clone());
            }
        };

        // Add its outputs to the list of undecided TXOs
        for &txo in opt_block
            .as_ref()
            .map(|b| b.outputs.iter())
            .unwrap_or_else(|| wire_vertex.block.as_ref().unwrap().outputs.iter())
        {
            let txo_hash = txo.hash();
            if self
                .undecided_txos
                .insert(txo_hash, DagTxo::new(txo))
                .is_some()
            {
                error!("Registered duplicate txo: {txo_hash}");
            }
        }

        // Broadcast to peers if this vertex didn't already come from our peers
        if broadcast {
            self.p2p_action_ch
                .send(p2p::Action::Broadcast(p2p::BroadcastData::Vertex(
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

        // Add vertex as known child to each of its parents
        let vertex = rw_vertex.read().map_err(|_| Error::VertexWriteLock)?;
        for parent in vertex.undecided_parents.values() {
            parent
                .write()
                .map_err(|_| Error::VertexWriteLock)?
                .known_children
                .insert(vhash, rw_vertex.clone());
        }

        // Remove this vertex from the waitlist
        self.waitlist.remove_inserted(wire_vertex.clone())?;

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
        }
        Ok(vertex.strongly_preferred)
    }

    /// Check for missing data and request them from peers
    fn request_if_missing_data(
        &mut self,
        wire_vertex: &WireVertex,
        sender: Option<PeerId>,
    ) -> Result<()> {
        // Lookup any missing parents
        let missing_parents: Vec<VertexHash> = wire_vertex
            .parents
            .iter()
            .filter(|vhash| {
                // Filter out any vertex already in the undecided DAG
                self.undecided_vertices.cache_get(vhash).is_none()
            })
            .filter(|vhash| {
                // Filter out any vertex already finalized in the database
                self.database
                    .read_vertex(vhash)
                    .expect("database read error")
                    .is_none()
            })
            .filter(|vhash| {
                // Filter out any vertex already waiting in the waitlist
                !self.waitlist.contains(vhash).expect("waitlist read error")
            })
            .map(|&parent| {
                // Request missing parent from peers
                if let Some(peer) = sender {
                    debug!("Requesting missing parent {parent}");
                    self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                        peer,
                        consensus_rpc::Request::GetVertex(parent),
                    ))?;
                }
                Ok::<_, Error>(parent)
            })
            .try_collect()?;
        if missing_parents.len() > 0 {
            let mut missing_str = format!("[{}", missing_parents[0]);
            for missing in missing_parents[1..].into_iter() {
                missing_str += &format!(", {}", missing);
            }
            missing_str += "]";
            debug!(
                "Missing parents for vertex {}: {missing_str}",
                wire_vertex.hash()
            );
            return Err(Error::MissingParents(missing_parents));
        }
        let vhash = wire_vertex.hash();
        let bhash = wire_vertex.bhash;
        // Make sure we have the block referenced by the given vertex
        let block = match self.get_block(&bhash) {
            Err(Error::NotFound) => {
                info!("Missing block for vertex {vhash}: {bhash}");
                if let Some(peer) = sender {
                    debug!("Requesting block {bhash}");
                    self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                        peer,
                        consensus_rpc::Request::GetBlock(bhash),
                    ))?;
                }
                Err(Error::MissingBlock(bhash))
            }
            other @ _ => other,
        }?;

        // Make sure we have the parent block, if any
        if let Some(prev_hash) = block.prev_mined {
            match self.get_block(&prev_hash) {
                Err(Error::NotFound) => {
                    info!("Missing previously mined block {prev_hash} from miner.");
                    if let Some(peer) = sender {
                        debug!("Requesting block {bhash}");
                        self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                            peer,
                            consensus_rpc::Request::GetBlock(bhash),
                        ))?;
                    }
                    Err(Error::MissingPrevMined(prev_hash))
                }
                other @ _ => other,
            }?;
        }
        Ok(())
    }

    /// Check that the vertex uniquely spends UTXOs. Returns true if no conflicts. Also reurns a
    /// reference to the block, if the block is not included in the WireVertex.
    fn check_conflicts(&mut self, wire_vertex: &WireVertex) -> Result<(bool, Option<Arc<Block>>)> {
        // Make sure transaction inputs are in the txo set. If they are not in the txo set at all,
        // then reject this vertex entirely. If they are in the txo set, but spent by another
        // vertex, it is still possible to insert this vertex, but it will be in conflict and
        // according to Avalanche consensus rules, it will not be accepted unless its confidence
        // exceeds that of its conflicts.

        let bhash = wire_vertex.bhash;
        let block = wire_vertex
            .block
            .as_ref()
            .map(|b| Arc::new(b.clone()))
            .unwrap_or(self.get_block(&bhash)?);

        // Make sure each UTXO is in the UTXO set
        if block
            .inputs
            .iter()
            .any(|txo_hash| !self.undecided_txos.contains_key(txo_hash))
        {
            warn!("Received block spends unknown transaction outputs.");
            Err(Error::UnknownTransactionInputs)
        } else {
            // Make sure none of the UTXOs are spent by a conflicting transaction
            let conflict_free = block.inputs.iter().all(|txo_hash| {
                self.undecided_txos
                    .get(txo_hash)
                    .expect("undecided vertex should already have validated inputs")
                    .preferred_spender
                    .is_none()
            });
            Ok((conflict_free, Some(block)))
        }
    }

    /// Begin querying our peers for their preference for the specified vertex
    fn query_peer_preferences(&mut self, vhash: VertexHash) -> Result<()> {
        if !self.pending_queries.contains(&vhash) {
            let voters = self.voter_pool.select(AVALANCHE_QUERY_COUNT)?;
            for voter in &voters {
                self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                    voter.read().unwrap().id(),
                    consensus_rpc::Request::GetPreference(vhash),
                ))?;
            }
            self.scorecards.cache_set(
                vhash,
                Scorecard::new_with_voters(vhash, voters, self.events_ch.clone()),
            );
            self.pending_queries.insert(vhash);
        }
        Ok(())
    }

    /// Remove completed query from pending queries
    pub fn clear_pending_query(&mut self, vhash: &VertexHash) {
        self.pending_queries.remove(vhash);
    }

    /// Look up a block from the DAG
    pub fn get_block(&mut self, bhash: &BlockHash) -> Result<Arc<Block>> {
        match self.undecided_blocks.cache_get(bhash) {
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
        match self.undecided_vertices.cache_get(vhash) {
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

    /// Recompute the confidences of given vertex and all undecided ancestors. Returns the hashes of
    /// ancestors which can now be accepted.
    pub fn recompute_confidences(
        &mut self,
        rw_vertex: Arc<TracingRwLock<Vertex>>,
    ) -> Vec<VertexHash> {
        // Recursively compute confidence as sum of chits in progeny
        {
            let mut vertex = rw_vertex.write().unwrap();
            vertex.confidence = vertex.chit
                + vertex
                    .known_children
                    .values()
                    .map(|v| v.read().unwrap().confidence)
                    .sum::<usize>();
        }
        let vertex = rw_vertex.read().unwrap();
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
            self.reset_confidence(rw_vertex.clone()).unwrap();
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
    pub fn reset_confidence(&mut self, rw_vertex: Arc<TracingRwLock<Vertex>>) -> Result<()> {
        let orig_confidnce = {
            let mut vertex = rw_vertex.write().map_err(|_| Error::VertexWriteLock)?;
            let orig_confidnce = vertex.confidence;
            vertex.strongly_preferred = false;
            vertex.confidence = 0;
            vertex.chit = 0;
            orig_confidnce
        };
        // Only need to continue resetting confidences if this vertex wasn't already reset
        if orig_confidnce > 0 {
            let vertex = rw_vertex.read().map_err(|_| Error::VertexReadLock)?;
            for child in vertex.known_children.values() {
                let block = child
                    .read()
                    .map_err(|_| Error::VertexReadLock)?
                    .block
                    .clone();
                self.events_ch.send(Event::StalledBlock(block))?;
                self.reset_confidence(child.clone())?;
            }
        }
        Ok(())
    }

    /// Handle any avalanche requests or responses
    pub fn handle_avalanche_message(&mut self, message: consensus_rpc::Event) -> Result<()> {
        match message {
            consensus_rpc::Event::Requested(peer, request_id, request) => {
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
            consensus_rpc::Event::Responded(peer, response) => {
                self.handle_avalanche_response(peer, response)
            }
        }
    }

    /// Handle a received avalanche request message from one of our peers
    fn handle_avalanche_request(
        &mut self,
        from_peer: PeerId,
        request: consensus_rpc::Request,
    ) -> Result<Option<consensus_rpc::Response>> {
        trace!("Handling request: {request}");
        match request {
            consensus_rpc::Request::GetBlock(bhash) => match self.get_block(&bhash) {
                Ok(block) => {
                    debug!("Sending block response {bhash}={block}");
                    Ok(Some(consensus_rpc::Response::Block((*block).clone())))
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
            consensus_rpc::Request::GetVertex(vhash) => match self.get_vertex(&vhash) {
                Ok((vertex, _preferred)) => {
                    debug!("Sending vertex response {vhash}={vertex}");
                    Ok(Some(consensus_rpc::Response::Vertex(vertex)))
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
            consensus_rpc::Request::GetPreference(vhash) => {
                self.get_vertex(&vhash)
                    .map(|(_sv, pref)| Some(consensus_rpc::Response::Preference(vhash, pref)))
                    .map_err(|error| {
                        debug!("Unable to determine preference for {vhash}: {error}");
                        if let Error::NotFound = error {
                            match self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                                from_peer,
                                consensus_rpc::Request::GetVertex(vhash),
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
        response: consensus_rpc::Response,
    ) -> Result<()> {
        Ok(match response {
            consensus_rpc::Response::Block(block) => {
                let bhash = block.hash();
                debug!("Received block response {bhash}={block}");
                match self.register_block(block) {
                    Err(Error::Block(block::Error::InvalidPoW)) => {
                        // Block the peer for sending us an un-worked block
                        warn!("Blocking peer for sending block with insufficient Prrof-of-Work");
                        self.p2p_action_ch.send(p2p::Action::BlockPeer(from_peer))?
                    }
                    Err(e) => debug!("Unable to insert requested block {bhash}: {e}"),
                    _ => {}
                }
            }
            consensus_rpc::Response::Vertex(wire_vertex) => {
                let vhash = wire_vertex.hash();
                debug!("Received vertex response {vhash}={wire_vertex}");
                if let Err(e) =
                    self.try_insert_vertices(once(Arc::new(wire_vertex)), Some(from_peer), false)
                {
                    debug!("Unable to insert requested vertex {vhash}: {e}");
                }
            }
            consensus_rpc::Response::Preference(vhash, preferred) => {
                if let Some(scorecard) = self.scorecards.cache_get_mut(&vhash) {
                    // Process this peer's vote
                    match scorecard.register_vote(&from_peer, preferred)? {
                        // Vote completed, vertex is preferred amongst peers
                        Some(true) => {
                            // Award a chit and cancel the vote.
                            let mut accepted_vertices = Vec::new();
                            if let Some(v) =
                                self.undecided_vertices.cache_get(&vhash).map(|v| v.clone())
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
                            if let Some(v) =
                                self.undecided_vertices.cache_get(&vhash).map(|v| v.clone())
                            {
                                self.reset_confidence(v)?;
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
        for hash in hashes {
            if let Some(rw_vertex) = self.undecided_vertices.cache_get(&hash) {
                let vertex = rw_vertex.read().map_err(|_| Error::VertexWriteLock)?;
                // Only finalize the block if it has no undecided parents
                if vertex.undecided_parents.len() == 0 {
                    // Remove this block from its children's undecided_parents lists
                    let mut txn = None;
                    for child in vertex.known_children.values() {
                        child
                            .write()
                            .unwrap()
                            .undecided_parents
                            .remove(&vertex.hash()?);
                    }
                    // Write the block to the database
                    txn = Some(self.database.write_vertex(txn, vertex.to_wire()?)?);
                    // Remove this block's inputs from the set of undecided_txos
                    for txo_hash in &vertex.block.inputs {
                        self.undecided_txos.remove(txo_hash);
                    }
                    txn.unwrap().commit()?;
                }
            }
        }
        Ok(())
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
