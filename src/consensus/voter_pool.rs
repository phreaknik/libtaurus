use crate::{
    params::{AVALANCHE_QUORUM, VOTER_EXPERATION_HRS, VOTER_REGISTRATION_WINDOW_HRS},
    Block, VertexHash,
};
use chrono::{Duration, Utc};
use libp2p::PeerId;
use rand::{
    seq::{IteratorRandom, SliceRandom},
    thread_rng,
};
use std::{collections::HashMap, result, sync::Arc};
use tokio::sync::broadcast;
use tracing::{debug, trace};
use tracing_mutex::stdsync::TracingRwLock;

use super::Event;

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
            all_voters: HashMap::new(),
            sieve: Vec::new(),
        }
    }

    /// Remove any miners which have become unresponsive for some time
    pub fn remove_offline_miners(&mut self) {
        let offline = self
            .all_voters
            .iter()
            .filter(|(_peer, rw_record)| rw_record.read().unwrap().is_offline())
            .map(|(&id, _record)| id)
            .collect::<Vec<_>>();
        for peer in &offline {
            self.all_voters.remove(peer);
        }
    }

    /// Register the voter for a new block, or update the voters record if he already exists
    pub fn register_from_block(&mut self, block: Arc<Block>) -> bool {
        if let Some(voter) = self.all_voters.get_mut(&block.miner) {
            let mut v = voter.write().unwrap();
            if block.prev_mined == Some(v.last_block.hash()) {
                // Only register if the miner linked the correct last block
                v.record_block(block);
            }
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
        self.remove_offline_miners();
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

    /// Is this voter unresponsive?
    pub fn is_offline(&self) -> bool {
        Utc::now() - self.last_block.time > Duration::hours(VOTER_EXPERATION_HRS)
            && self.query_timeouts > 0
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
    /// Hash of the vertex being queried
    vhash: VertexHash,
    /// Events channel to notify of query events
    events_ch: broadcast::Sender<Event>,
    /// Peers that still need to vote
    pending: HashMap<PeerId, Arc<TracingRwLock<VoterRecord>>>,
    /// Total score from peers which have voted
    score: usize,
}

impl Scorecard {
    /// Create a new scoreard for a running vote among the given voters, querying the preference of
    /// the specified vertex
    pub fn new_with_voters<P>(
        vhash: VertexHash,
        voters: P,
        events_ch: broadcast::Sender<Event>,
    ) -> Scorecard
    where
        P: IntoIterator<Item = Arc<TracingRwLock<VoterRecord>>>,
    {
        Scorecard {
            vhash,
            events_ch,
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
    /// Register the vote from a voter.
    ///
    /// Votes is only counted if this voter is one of the allowed voters and has not voted already.
    /// Return ['Some'] if a decision has been reached (even if we're still awaiting some
    /// votes). If the voters reched quorum for the corresponding vertex, then the result will
    /// be `Some(true)`. If the vote completed without reaching quorum, the result will be
    /// `Some(false)`. If the vote has not completed yet, the result will be `None`.
    pub fn register_vote(&mut self, peer: &PeerId, preference: bool) -> Result<Option<bool>> {
        // Make sure a peer's vote only gets counted once
        if let Some(voter) = self.pending.remove(peer) {
            self.score += preference as usize;
            voter.write().unwrap().count_vote();
        }
        if self.score >= AVALANCHE_QUORUM {
            Ok(Some(true))
        } else if self.score + self.pending.len() < AVALANCHE_QUORUM {
            Ok(Some(false))
        } else {
            Ok(None)
        }
    }
}

impl Drop for Scorecard {
    fn drop(&mut self) {
        // Notify system that query has completed
        let _ = self.events_ch.send(Event::QueryCompleted(self.vhash));
        // Record a timeout for each peer which did not respond in time
        for (_id, record) in &self.pending {
            record.write().unwrap().count_timeout();
        }
    }
}
