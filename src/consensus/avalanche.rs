use super::{
    block,
    database::ConsensusDb,
    transaction::{self, Txo, TxoHash},
    vertex::Vertex,
    voter_pool::{self, Scorecard, VoterPool},
    waitlist::{self, WaitList},
    Block, BlockHash, Event,
};
use crate::{
    p2p::{self, consensus_rpc},
    params::{AVALANCHE_ACCEPTANCE_THRESHOLD, AVALANCHE_QUERY_COUNT, QUERY_TIMEOUT_SEC},
    randomx::RandomXVMInstance,
    wire::{self, WireFormat},
    VertexHash,
};
use cached::{Cached, TimedCache};
use libp2p::PeerId;
use std::{
    assert_matches::debug_assert_matches,
    cmp,
    collections::{HashMap, HashSet},
    iter::once,
    num::NonZeroUsize,
    ops::Deref,
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
    #[error("block has already been decided")]
    AlreadyDecidedBlock(VertexHash),
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error("vertex has corrupt block hash")]
    CorruptBlockHash,
    #[error(transparent)]
    Database(#[from] super::database::Error),
    #[error("vertex has previously been inserted")]
    DuplicateInsertion,
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error("missing data necessary to process vertex")]
    MissingData(Vec<VertexHash>, Option<BlockHash>),
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
    #[error("error acquiring read lock on a vertex")]
    VertexReadLock,
    #[error("error acquiring write lock on a vertex")]
    VertexWriteLock,
    #[error(transparent)]
    VoterPool(#[from] voter_pool::Error),
    #[error("waiting for missing vertices or blocks")]
    Waiting,
    #[error(transparent)]
    WaitList(#[from] waitlist::Error),
    #[error(transparent)]
    Wire(#[from] wire::Error),
}

/// Result type for avalanche errors
type Result<T> = result::Result<T, Error>;

/// Configuration details for the consensus process.
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to the consensus data directory
    pub data_dir: PathBuf,

    /// Genesis block to serve as root vertex in the DAG
    pub genesis: Arc<Vertex>,

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
    undecided_vertices: TimedCache<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,

    /// List of transaction outputs being considered in the undecided portion of the DAG
    undecided_txos: HashMap<TxoHash, DagTxo>,

    /// Waitlist for vertices that cannot be inserted yet
    waitlist: WaitList,

    /// The active edge of the DAG, i.e. the preferred vertices which don't yet have children
    frontier: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,

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
        let rw_vertex = Arc::new(TracingRwLock::new(UndecidedVertex::genesis(
            config.genesis.clone(),
        )));
        dag.frontier.insert(genesis_hash, rw_vertex);
        // Return the dag
        dag
    }

    pub fn genesis_hash(&self) -> VertexHash {
        self.config.genesis.hash()
    }

    /// Try to submit this block as a vertex at the frontier of the DAG
    pub fn try_insert_block(&mut self, block: Arc<Block>, broadcast: bool) -> Result<()> {
        let vertex = Arc::new(Vertex::new_full(block));
        self.try_insert_vertices(once(vertex), None, broadcast)
    }

    /// Try to insert a list of vertices, and attempt to also insert any known waiting children
    pub fn try_insert_vertices<V>(
        &mut self,
        vertices: V,
        sender: Option<PeerId>,
        broadcast: bool,
    ) -> Result<()>
    where
        V: IntoIterator<Item = Arc<Vertex>>,
    {
        // Helper to insert the vertex to the dag or the waitlist. If insertion is successfull,
        // returns any vertices which were waiting on the newly inserted data.
        let mut insert = |vertex: Arc<Vertex>| {
            let res = self.try_insert_vertex(vertex.deref().clone(), sender, broadcast);
            match res {
                Ok(preferred) => {
                    let vhash = vertex.hash();
                    info!(
                        "Inserted {}preferred vertex {}",
                        if !preferred { "non-" } else { "" },
                        vhash.to_hex()
                    );
                    let mut waiting = self.waitlist.get_by_vertex(&vhash)?.unwrap_or(Vec::new());
                    waiting.append(
                        &mut self
                            .waitlist
                            .get_by_block(&vertex.bhash)?
                            .unwrap_or(Vec::new()),
                    );
                    Ok(Some(waiting))
                }
                // TODO: figure out how to report missing data for entire insertion list
                Err(Error::MissingData(missing_parents, missing_block)) => {
                    self.waitlist
                        .insert(vertex, missing_parents, missing_block)?;
                    Ok(None)
                }
                Err(Error::Waiting) => Ok(None),
                Err(e) => {
                    // TODO: should also remove this and children from the waitlist.
                    // TODO: should test this ^
                    Err(e)
                }
            }
        };

        // Sanity check (in debug builds) each vertex before proceding. This is not necessary in
        // release, because each vertex should have been sanity checked when it came off the wire.
        let mut insert_list: HashMap<_, _> = vertices
            .into_iter()
            .inspect(|v| debug_assert_matches!(v.sanity_checks(), Ok(())))
            .map(|v| (v.hash(), v))
            .collect();
        let original_list: HashSet<_> = insert_list.keys().copied().collect();

        // Loop until each vertex has been inserted to the dag or waitlist
        let mut successful = HashSet::new(); // Track every successful insertion
        let mut to_retry: Vec<Arc<Vertex>> = Vec::new(); // Vertices that should be retried after each loop iteration
        let mut progressing = true; // Flag to indicate if the loop is still progressing
        while progressing {
            // Reset the progression flag
            progressing = false;

            // Any vertices from the previous loop that need to be retried, should be added to the
            // insertion list now.
            for retry in to_retry.drain(..) {
                if !successful.contains(&retry.hash()) {
                    insert_list.insert(retry.hash(), retry);
                }
            }

            // Attempt to insert each vertex in the insertion list. If successful, add its
            // dependents to the retry list, to be tried on the next iteration.
            for (vhash, v) in insert_list.drain() {
                if let Some(mut dependents) = insert(v.clone())? {
                    // Record the successful insertion
                    successful.insert(vhash);
                    to_retry.append(&mut dependents);
                    progressing = true;
                } else {
                    // Insertion failed. Retry this vertex again later.
                    to_retry.push(v);
                }
            }
        }

        // Error if any of the original vertices are still waiting
        if !successful.is_superset(&original_list) {
            // Some of the original items weren't successful, and are thus waiting in the waitlist
            Err(Error::Waiting)
        } else {
            Ok(())
        }
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
                    .lookup_vertex_for_block(bhash)?
                    .ok_or(Error::NotFound)?;
                Ok(self.get_vertex(&vhash)?.0.block.as_ref().unwrap().clone())
            }
        }
    }

    /// Look up the vertex corresponding to a decided block in the DAG. Note, if the block has
    /// not been decided, this function will return None, even if there exist undecided vertices
    /// for this block.
    pub fn lookup_vertex_for_block(&mut self, bhash: &BlockHash) -> Result<Option<VertexHash>> {
        Ok(self.database.lookup_vertex_for_block(bhash)?.or_else(|| {
            if bhash == &self.config.genesis.bhash {
                Some(self.genesis_hash())
            } else {
                None
            }
        }))
    }

    /// Look up a vertex from the DAG, and indicate if this vertex is strongly preferred.
    // TODO: should return Result<Option<_>>
    pub fn get_vertex(&mut self, vhash: &VertexHash) -> Result<(Arc<Vertex>, bool)> {
        match self.undecided_vertices.cache_get(vhash) {
            Some(rw_vertex) => {
                let vertex = rw_vertex.read().map_err(|_| Error::VertexReadLock)?;
                Ok((vertex.inner.clone(), vertex.strongly_preferred))
            }
            None => Ok((
                self.database
                    .read_vertex(vhash)?
                    .or_else(|| {
                        if vhash == &self.genesis_hash() {
                            Some(self.config.genesis.clone())
                        } else {
                            None
                        }
                    })
                    .ok_or(Error::NotFound)?,
                true,
            )),
        }
    }

    /// Handle any avalanche requests or responses
    pub fn handle_avalanche_message(&mut self, message: consensus_rpc::Event) -> Result<()> {
        // TODO: this method should not be part of DAG
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

    /// Method to request a copy of the current DAG frontier
    pub fn get_frontier(&self) -> Result<Vec<Arc<Vertex>>> {
        self.frontier
            .values()
            .map(|rwlock| {
                rwlock
                    .read()
                    .map_err(|_| Error::VertexReadLock)
                    .map(|undecided| undecided.inner.clone())
            })
            .try_collect()
    }

    /// Method to request the hashes of the current frontier
    pub fn get_frontier_hashes(&self) -> Vec<VertexHash> {
        self.frontier.keys().copied().collect()
    }

    /// Override the consensus, and force a decision for the given vertex. Operation will fail if
    /// the vertex is not in the undecided set.
    pub fn force_decision(&mut self, vhash: VertexHash, preferred: bool) -> Result<()> {
        // TODO: remember "bad vertices" so they don't reapper and retry decision.
        match self.undecided_vertices.cache_get(&vhash) {
            Some(_) => self.finalize_vertices(vec![vhash]),
            None => Err(Error::NotFound),
        }
    }

    /// Register a block as available to be inserted into the DAG. Returns true if this is a new
    /// registration; i.e. the block had not been seen before.
    // TODO: test this function
    pub fn register_block(&mut self, block: Arc<Block>) -> Result<bool> {
        // Check the block has sufficient proof-of-work
        block.verify_pow(&self.randomx_vm)?;
        let bhash = block.hash();
        if let Some(vhash) = self.lookup_vertex_for_block(&bhash)? {
            Err(Error::AlreadyDecidedBlock(vhash))
        } else {
            // Add the block to the list of undecided blocks
            let new_block = self
                .undecided_blocks
                .cache_set(bhash, block.clone())
                .is_none();
            // Register this miner as a potential Avalanche voter
            self.voter_pool.register_from_block(block);
            // Attempt to insert any vertices waiting on that block
            if let Some(vertices) = self.waitlist.get_by_block(&bhash)? {
                self.try_insert_vertices(vertices, None, false)?;
            }
            Ok(new_block)
        }
    }

    /// Attempt to insert the vertex into the DAG. If successful, will return true if the vertex is
    /// currently strongly preferred.
    fn try_insert_vertex(
        &mut self,
        vertex: Vertex,
        sender: Option<PeerId>,
        broadcast: bool,
    ) -> Result<bool> {
        let vhash = vertex.hash();
        debug!("Vertex submitted: {vhash} = {vertex}");

        // If available, register block before anything else
        if let Some(block) = &vertex.block {
            // Check for blockhash mismatch
            if vertex.bhash != block.hash() {
                return Err(Error::CorruptBlockHash);
            }

            // Register the block as available for insertion.
            self.register_block(block.clone())?;
        } else if let Some(vhash) = self.lookup_vertex_for_block(&vertex.bhash)? {
            // Check if block has already been decided by another vertex
            return Err(Error::AlreadyDecidedBlock(vhash));
        }

        // Create an [`UndecidedVertex`], checking if this vertex can be inserted without violating
        // any DAG rules
        let (rw_vertex, conflict_free) = self.create_insertable_vertex(vertex)?;
        let undecided = rw_vertex.read().map_err(|_| Error::VertexWriteLock)?;
        let block = undecided.inner.block.as_ref().unwrap();

        // Add it to the collection of undecided vertices
        self.undecided_vertices.cache_set(vhash, rw_vertex.clone());

        // If this vertex is conflict free, mark it as the preferred spender of its tx inputs
        if conflict_free {
            for txo_hash in &block.inputs {
                self.undecided_txos
                    .get_mut(txo_hash)
                    .expect("Attempted to spend non-existent UTXO")
                    .preferred_spender = Some(rw_vertex.clone());
            }
        };

        // Add its outputs to the list of undecided TXOs
        for &txo in &block.outputs {
            assert!(
                self.undecided_txos
                    .insert(txo.hash(), DagTxo::new(txo))
                    .is_none(),
                "Registered duplicate txo: {}",
                txo.hash()
            );
        }

        // TODO: decision to broadcast should be done outside of the DAG. Maybe DAG shouldn't emit
        // any p2p requests?
        // Broadcast to peers if this vertex didn't already come from our peers
        if broadcast {
            self.p2p_action_ch
                .send(p2p::Action::Broadcast(p2p::BroadcastData::Vertex(
                    // TODO: this really wont work
                    Vertex::default(),
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
        for parent in undecided.undecided_parents.values() {
            parent
                .write()
                .map_err(|_| Error::VertexWriteLock)?
                .known_children
                .insert(vhash, rw_vertex.clone());
        }

        // Remove this vertex from the waitlist
        self.waitlist.remove_inserted(&undecided.inner)?;

        if undecided.strongly_preferred {
            // Remove its parents from the frontier, as they are no longer the youngest
            for parent in &undecided.inner.parents {
                self.frontier.remove(parent);
            }

            // Add it to the frontier
            self.frontier.insert(vhash, rw_vertex.clone());

            // Notify subscribers (if any) of new frontier
            let _ = self
                .events_ch
                .send(Event::NewFrontier(self.frontier.keys().cloned().collect()));
        }
        Ok(undecided.strongly_preferred)
    }

    /// Check that the given vertex can be inserted without violating any rules, and return an
    /// [`UndecidedVertex`] which can be inserted into the [`DAG`]. Additionally returns a boolean
    /// value indicating whether or not the vertex is conflict free.
    fn create_insertable_vertex(
        &mut self,
        mut vertex: Vertex,
    ) -> Result<(Arc<TracingRwLock<UndecidedVertex>>, bool)> {
        // Check if we already have this vertex
        if self.get_vertex(&vertex.hash()).is_ok() {
            return Err(Error::DuplicateInsertion);
        }

        // Gather parents
        let mut height = 0;
        let mut missing_parents = Vec::new();
        let mut undecided_parents = HashMap::new();
        let mut decided_parents = HashMap::new();
        let mut waiting = false;
        for &parent_hash in &vertex.parents {
            if let Some(parent) = self.undecided_vertices.cache_get(&parent_hash) {
                undecided_parents.insert(parent_hash, parent.clone());
                height = cmp::max(
                    height,
                    parent.read().map_err(|_| Error::VertexReadLock)?.height,
                )
            } else if self
                .waitlist
                .contains(&parent_hash)
                .expect("waitlist read error")
            {
                waiting = true;
            } else if let Some(parent) = self.database.read_vertex(&parent_hash)? {
                decided_parents.insert(parent_hash, parent);
            } else if parent_hash == self.genesis_hash() {
                decided_parents.insert(parent_hash, self.config.genesis.clone());
            } else {
                missing_parents.push(parent_hash);
            }
        }

        // Fill the vertex block if we can
        if vertex.block.is_none() {
            vertex.block = self.undecided_blocks.cache_get(&vertex.bhash).cloned();
        }

        // Check for missing data
        if !missing_parents.is_empty() || vertex.block.is_none() {
            return Err(Error::MissingData(
                missing_parents,
                match vertex.block {
                    Some(_) => None,
                    None => Some(vertex.bhash),
                },
            ));
        }

        // If any parent was in the waitlist, this vertex is also waiting
        if waiting {
            return Err(Error::Waiting);
        }

        // Gather info from parent blocks to check transition rules
        undecided_parents
            .values()
            .map(|locked| {
                locked
                    .read()
                    .map_err(|_| Error::VertexReadLock)
                    .map(|v| v.inner.bhash)
            })
            .chain(decided_parents.values().map(|v| Ok(v.bhash)))
            .map(|bhash_res| bhash_res.map(|bhash| self.get_block(&bhash)).flatten())
            .try_for_each(|b_res| {
                b_res
                    .map(|parent_block| {
                        // Check that block is a legal extension of the given parent
                        vertex
                            .block
                            .as_ref()
                            .unwrap()
                            .extends_parent(&parent_block)
                            .map_err(Error::from)
                    })
                    .flatten()
            })?;

        // Check if the vertex is conflict free
        let conflict_free = self.check_conflicts(&vertex)?;

        // TODO: check vertex height
        // TODO: Check ledger rules

        // Determine if this vertex is strongly preferred
        let strongly_preferred = undecided_parents
            .values()
            .map(|p| p.read().map_err(|_| Error::VertexReadLock))
            .try_fold(conflict_free, |all_pref, vertex| {
                vertex.map(|v| (all_pref && v.strongly_preferred))
            })?;

        // Build an [`UndecidedVertex`] which can be inserted into the DAG
        Ok((
            Arc::new(TracingRwLock::new(UndecidedVertex {
                inner: Arc::new(vertex),
                height,
                undecided_parents,
                known_children: HashMap::new(),
                strongly_preferred,
                chit: 0,
                confidence: 0,
            })),
            conflict_free,
        ))
    }

    /// Check that the vertex does not conflict with other vertices. Returns true if no conflicts.
    /// Also reurns a reference to the block, to avoid additional lookups elsewhere.
    fn check_conflicts(&mut self, vertex: &Vertex) -> Result<bool> {
        // Make sure transaction inputs are in the txo set. If they are not in the txo set at all,
        // then reject this vertex entirely. If they are in the txo set, but spent by another
        // vertex, it is still possible to insert this vertex, but it will be in conflict and
        // according to Avalanche consensus rules, it will not be accepted unless its confidence
        // exceeds that of its conflicts.

        let block = vertex
            .block
            .as_ref()
            .cloned()
            .expect("cannot check conflicts for slim vertex");

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
            Ok(conflict_free)
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

    /// Recompute the confidences of given vertex and all undecided ancestors. Returns the hashes of
    /// ancestors for which consensus has reached a decision.
    fn recompute_confidences(
        &mut self,
        rw_vertex: Arc<TracingRwLock<UndecidedVertex>>,
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
            .inner
            .block
            .as_ref()
            .unwrap()
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
            for txo_hash in &conflict.inner.block.as_ref().unwrap().inputs {
                self.undecided_txos
                    .get_mut(txo_hash)
                    .unwrap()
                    .preferred_spender = match vertex
                    .inner
                    .block
                    .as_ref()
                    .unwrap()
                    .inputs
                    .contains(txo_hash)
                {
                    true => Some(rw_vertex.clone()),
                    false => None,
                }
            }
            // Remove this vertex from the frontier, if needed
            self.frontier.remove(&conflict.hash());
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
            accepted.push(vertex.hash());
        }
        accepted
    }

    /// Reset the convidence of this vertex and any children
    fn reset_confidence(&mut self, rw_vertex: Arc<TracingRwLock<UndecidedVertex>>) -> Result<()> {
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
                    .inner
                    .block
                    .as_ref()
                    .unwrap()
                    .clone();
                self.events_ch.send(Event::StalledBlock(block))?;
                self.reset_confidence(child.clone())?;
            }
        }
        Ok(())
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
                match self.register_block(Arc::new(block)) {
                    Err(Error::Block(block::Error::InvalidPoW)) => {
                        // Block the peer for sending us an un-worked block
                        warn!("Blocking peer for sending block with insufficient Prrof-of-Work");
                        self.p2p_action_ch.send(p2p::Action::BlockPeer(from_peer))?
                    }
                    Err(e) => debug!("Unable to insert requested block {bhash}: {e}"),
                    _ => {}
                }
            }
            consensus_rpc::Response::Vertex(vertex) => {
                let vhash = vertex.hash();
                debug!("Received vertex response {vhash}={vertex}");
                if let Err(e) = self.try_insert_vertices(once(vertex), Some(from_peer), false) {
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
        // TODO: accept generic intoIter instead of vec
        // TODO: need to finalize negative decisions as well as positive.
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
                            .remove(&vertex.hash());
                    }
                    // Write the block to the database
                    txn = Some(self.database.write_vertex(
                        txn,
                        vertex.height,
                        vertex.inner.clone(),
                    )?);
                    // Remove this block's inputs from the set of undecided_txos
                    for txo_hash in &vertex.inner.block.as_ref().unwrap().inputs {
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
pub(super) struct DagTxo {
    /// The underlying transaction output
    txo: Txo,

    /// Link to the preferred vertex which spends this UTXO, if any
    preferred_spender: Option<Arc<TracingRwLock<UndecidedVertex>>>,
}

impl DagTxo {
    fn new(txo: Txo) -> DagTxo {
        DagTxo {
            txo,
            preferred_spender: None,
        }
    }
}

/// UndecidedVertex is a wrapper around [`Vertex`] which provides dynamic links and metadata useful
/// during the decision process for accepting new vertices.
#[derive(Debug, Clone)]
struct UndecidedVertex {
    pub inner: Arc<Vertex>,

    /// Height within the DAG
    pub height: u64,

    /// Every parent vertex which hasn't been decided yet
    pub undecided_parents: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,

    /// If this vertex is accepted into the DAG, it will be built upon and acquire children. This
    /// map will accumulate children we learn about, for simplified traversal of the dag when
    /// performing Avalanche operations, such as computing confidence or deciding vertex
    /// acceptance.
    pub known_children: HashMap<VertexHash, Arc<TracingRwLock<UndecidedVertex>>>,

    /// A vertex is strongly preferred if it and its entire ancestry are preferred over all
    /// conflicting vertices.
    pub strongly_preferred: bool,

    /// Chit indicates if this vertex received quorum when we queried the network
    pub chit: usize,

    /// Confidence counts how many dependent votes have been received without changing preference
    pub confidence: usize,
}

impl UndecidedVertex {
    /// Create a genesis undecided-vertex
    pub fn genesis(vertex: Arc<Vertex>) -> UndecidedVertex {
        UndecidedVertex {
            inner: vertex,
            height: 0,
            undecided_parents: HashMap::new(),
            known_children: HashMap::new(),
            strongly_preferred: true,
            chit: 1,
            confidence: usize::MAX,
        }
    }

    /// Compute the hash of the vertex
    pub fn hash(&self) -> VertexHash {
        self.inner.hash()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Block, Vertex, VertexHash};
    use std::sync::Arc;

    #[test]
    fn genesis() {
        let block = Arc::new(Block::default());
        let vertex = Arc::new(Vertex::new_full(block));
        let genesis = UndecidedVertex::genesis(vertex.clone());
        assert_eq!(genesis.inner, vertex);
        assert_eq!(genesis.height, 0);
        assert!(genesis.undecided_parents.is_empty());
        assert!(genesis.known_children.is_empty());
        assert_eq!(genesis.strongly_preferred, true);
        assert_eq!(genesis.chit, 1);
        assert_eq!(genesis.confidence, usize::MAX);
    }

    #[test]
    fn hash() {
        let block = Arc::new(Block::default());
        let vertex = Arc::new(Vertex::new_full(block));
        let genesis = UndecidedVertex::genesis(vertex.clone());
        assert_eq!(
            genesis.hash(),
            VertexHash::with_bytes([
                130, 134, 225, 49, 43, 221, 164, 12, 38, 15, 125, 41, 246, 42, 39, 21, 251, 152,
                53, 232, 109, 18, 23, 57, 78, 72, 63, 203, 39, 150, 0, 46
            ])
        );
    }
}
