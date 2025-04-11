use crate::{params::VOTER_REGISTRATION_WINDOW_HRS, Block};
use chrono::{Duration, Utc};
use libp2p::PeerId;
use rand::{
    seq::{IteratorRandom, SliceRandom},
    thread_rng,
};
use std::{collections::HashMap, result, sync::Arc};
use tracing::{debug, trace};
use tracing_mutex::stdsync::TracingRwLock;

/// Error type for VoterPool errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("not enough registered voters")]
    NotEnoughVoters,
}

/// Result type for VoterPool errors
pub type Result<T> = result::Result<T, Error>;

pub struct VoterPool {
    /// Set of all miners expected to participate as voters
    all_voters: HashMap<PeerId, Arc<TracingRwLock<VoterRecord>>>,
    /// Queue which drains one at a time, to ensure every miner gets queried within N blocks
    sieve: Vec<Arc<TracingRwLock<VoterRecord>>>,
}

impl VoterPool {
    /// Create new instance of a voter pool
    pub fn new() -> VoterPool {
        VoterPool {
            // TODO: how do miners get removed from the pool? multiple failed queries?
            all_voters: HashMap::new(),
            sieve: Vec::new(),
        }
    }

    /// Register the voter for a new block, or update the voters record if he already exists
    pub fn register_from_block(&mut self, block: Arc<Block>) -> bool {
        // TODO: should it error if voter already exists, but block does not specify a last_block?
        if let Some(voter) = self.all_voters.get_mut(&block.miner) {
            voter.write().unwrap().record_block(block);
            false
        } else {
            self.all_voters.insert(
                block.miner,
                Arc::new(TracingRwLock::new(VoterRecord::new_from_block(block))),
            );
            true
        }
    }

    /// Select a number of voters. Resets the sieve if its empty.
    pub fn select(&mut self, count: usize) -> Result<Vec<Arc<TracingRwLock<VoterRecord>>>> {
        if count == 0 {
            Ok(Vec::new())
        } else if self.all_voters.len() < count {
            Err(Error::NotEnoughVoters)
        } else {
            let mut selected = vec![self.sieve.pop().unwrap()];
            let active_voters = self
                .all_voters
                .values()
                .filter(|voter| voter.read().unwrap().is_active())
                .cloned();
            if count > 1 {
                selected.extend(
                    active_voters
                        .clone()
                        .choose_multiple(&mut thread_rng(), count - 1),
                )
            }
            if self.sieve.is_empty() {
                // Reset the sieve
                self.sieve.extend(active_voters);
                self.sieve.as_mut_slice().shuffle(&mut thread_rng());
            }
            Ok(selected)
        }
    }
}

/// Information we track about each known miner eligible for voting
// TODO: need process to remove voters who haven't mined a block in a while AND are unresponsive
pub struct VoterRecord {
    /// ID this voter record applies to
    id: PeerId,

    /// Timestamp of the last block this voter mined
    last_block: Arc<Block>,

    /// Number of times this voter has timed out when queried about a vertex
    query_timeouts: u64,
}

impl VoterRecord {
    /// Create a new voter record
    fn new_from_block(block: Arc<Block>) -> VoterRecord {
        VoterRecord {
            id: block.miner,
            last_block: block,
            query_timeouts: 0,
        }
    }

    /// Return the ['PeerID'] associated with this voter record
    pub fn id(&self) -> PeerId {
        self.id
    }

    /// Is this voter active and available to vote?
    pub fn is_active(&self) -> bool {
        Utc::now() - self.last_block.time <= Duration::hours(VOTER_REGISTRATION_WINDOW_HRS)
    }

    /// Mark this peer for successfully mining a block
    pub fn record_block(&mut self, block: Arc<Block>) {
        trace!("{} mined a block", self.id);
        self.last_block = block;
    }

    /// Mark this peer for successfully voting
    pub fn count_vote(&mut self) {
        trace!("{} submitted a vote", self.id);
        self.query_timeouts -= 1;
    }

    /// Mark this peer for failing to respond to a vote in time
    pub fn count_timeout(&mut self) {
        debug!("{} timed out waiting for preference vote", self.id);
        self.query_timeouts += 1;
    }
}

/// A scorecard to count votes received from peers during a round of queries
pub struct Scorecard {
    /// Peers that still need to vote
    pub pending: HashMap<PeerId, Arc<TracingRwLock<VoterRecord>>>,
    /// Total score from peers which have voted
    pub score: usize,
}

impl Scorecard {
    pub fn new_with_voters<P>(voters: P) -> Scorecard
    where
        P: IntoIterator<Item = Arc<TracingRwLock<VoterRecord>>>,
    {
        Scorecard {
            pending: voters
                .into_iter()
                .map(|rw_record| {
                    let id = rw_record.clone().read().unwrap().id();
                    (id, rw_record)
                })
                .collect(),
            score: 0,
        }
    }
}

impl Drop for Scorecard {
    fn drop(&mut self) {
        for (_id, record) in &self.pending {
            record.write().unwrap().count_timeout();
        }
    }
}
