use std::{collections::HashSet, result};

use libp2p::PeerId;
use rand::{
    seq::{IteratorRandom, SliceRandom},
    thread_rng,
};

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
    all_voters: HashSet<PeerId>,
    /// Queue which drains one at a time, to ensure every miner gets queried within N blocks
    sieve: Vec<PeerId>,
}

impl VoterPool {
    /// Create new instance of a voter pool
    pub fn new() -> VoterPool {
        VoterPool {
            // TODO: how do miners get removed from the pool? multiple failed queries?
            all_voters: HashSet::new(),
            sieve: Vec::new(),
        }
    }

    /// Register a new voter with the pool. Returns true if the voter is new to the pool.
    pub fn register(&mut self, voter: PeerId) -> bool {
        self.all_voters.insert(voter)
    }

    /// Select a number of voters. Resets the sieve if its empty.
    pub fn select(&mut self, count: usize) -> Result<Vec<PeerId>> {
        if count == 0 {
            Ok(Vec::new())
        } else if self.all_voters.len() < count {
            Err(Error::NotEnoughVoters)
        } else {
            let mut selected = vec![self.sieve.pop().unwrap()];
            if count > 1 {
                selected.extend(
                    self.all_voters
                        .iter()
                        .choose_multiple(&mut thread_rng(), count - 1),
                )
            }
            if self.sieve.is_empty() {
                // Reset the sieve
                self.sieve.extend(self.all_voters.iter());
                self.sieve.as_mut_slice().shuffle(&mut thread_rng());
            }
            Ok(selected)
        }
    }
}
