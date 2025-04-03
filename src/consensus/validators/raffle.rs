use super::database::ValidatorDatabase;
use crate::consensus::{hash::Hash, Config, Error, Result};
use crate::randomx::RandomXVMInstance;
use crate::{params, Header};
use libp2p::PeerId;
use num::{BigUint, FromPrimitive};
use serde::{Deserialize, Serialize};
use serde_cbor;
use std::path::PathBuf;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{mpsc, watch};
use tracing::{error, info};

/// Path to the consensus database, from within the consensus data directory
pub const DATABASE_DIR: &str = "validators_db/";

/// Starts a raffle task, which periodically selects a new random set of validators from the set of
/// raffle tickets which have been entered.
pub fn start(
    config: &Config,
) -> (
    UnboundedSender<ValidatorTicket>,
    watch::Receiver<ValidatorSet>,
) {
    // Spawn the task
    let (ticket_sender, ticket_receiver) = mpsc::unbounded_channel();
    let (validator_set_sender, validator_set_receiver) = watch::channel(ValidatorSet(Vec::new()));
    tokio::spawn(task_fn(
        config.data_dir.join(DATABASE_DIR),
        ticket_receiver,
        validator_set_sender,
    ));

    // Return the communication channels
    (ticket_sender, validator_set_receiver)
}

/// The task function which runs the raffle
async fn task_fn(
    db_path: PathBuf,
    mut tickets_in: UnboundedReceiver<ValidatorTicket>,
    _validator_set_ch: watch::Sender<ValidatorSet>,
) {
    // Open the validator database
    let _database =
        ValidatorDatabase::open(&db_path, true).expect("Failed to open validator database");

    // Event loop
    loop {
        select! {
            item = tickets_in.recv() => {
                match item {
                    None => {
                        error!("Ticket channel closed. Stopping raffle.");
                        return;
                    },
                    Some(ticket) => {
                        info!("Received ticket {} from peer {}", ticket.hash(), ticket.peer);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSet(Vec<PeerId>);

/// Any node may submit a mined ValidatorTicket to be entered into the validator raffle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorTicket {
    pub peer: PeerId,
    pub heads: Vec<Hash>,
    pub height: u64,
    pub difficulty: u64,
    pub nonce: u64,
}

impl ValidatorTicket {
    /// Construct a new ticket to be mined
    pub fn new(peer: PeerId, heads: Vec<Header>) -> Result<ValidatorTicket> {
        if heads.iter().count() == 0 {
            Err(Error::EmptyCollection)
        } else {
            Ok(ValidatorTicket {
                peer,
                heads: heads.iter().map(|h| h.hash()).collect(),
                height: heads[0].height,
                difficulty: heads.into_iter().map(|h| h.difficulty).max().unwrap(),
                nonce: 0,
            })
        }
    }

    /// Compute the frontier hash
    pub fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
    }

    /// Compute the mining target from the given difficulty
    pub fn mining_target(&self) -> Result<BigUint> {
        if self.difficulty < params::MIN_DIFFICULTY {
            Err(Error::InvalidDifficulty)
        } else {
            Ok(
                BigUint::from_u64(2).unwrap().pow(256)
                    / BigUint::from_u64(self.difficulty).unwrap(),
            )
        }
    }

    /// Check if the block has valid proof-of-work
    pub fn verify_pow(&self, randomx: &RandomXVMInstance) -> Result<()> {
        if BigUint::from_bytes_be(&randomx.calculate_hash(&serde_cbor::to_vec(self)?)?)
            < self.mining_target()?
        {
            Ok(())
        } else {
            Err(Error::InvalidPoW)
        }
    }
}

impl Default for ValidatorTicket {
    fn default() -> Self {
        ValidatorTicket {
            peer: PeerId::random(),
            heads: Vec::new(),
            height: 0,
            difficulty: 0,
            nonce: 0,
        }
    }
}
