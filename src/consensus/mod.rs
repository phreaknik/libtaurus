pub mod block;
pub mod database;
pub mod hash;
pub mod validator_set;

use self::database::ConsensusDatabase;
use crate::randomx::RandomXVMInstance;
use crate::{p2p, randomx};
pub use block::{Block, Header};
use chrono::{DateTime, Utc};
use randomx_rs::RandomXFlag;
use std::path::PathBuf;
use std::result;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::{select, sync::broadcast};
use tracing::{error, info};
pub use validator_set::ValidatorTicket;

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Path to the consensus database, from within the consensus data directory
pub const DATABASE_DIR: &str = "consensus_db/";

/// Events produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// New ValidatorTicket to be sent to miners
    NewMiningJob(ValidatorTicket),
}

/// Actions that can be performed by the consensus process
#[derive(Clone, Debug)]
pub enum Action {
    SubmitMinedTicket(ValidatorTicket),
}

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    RandomX(#[from] randomx::Error),
    #[error(transparent)]
    Cbor(#[from] serde_cbor::error::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("collection is empty")]
    EmptyCollection,
    #[error("frontier object is empty")]
    EmptyFrontier,
    #[error("invalid frontier")]
    InvalidFrontier,
    #[error("invalid difficulty")]
    InvalidDifficulty,
    #[error("invalid proof-of-work")]
    InvalidPoW,
}

/// Result type for consensus errors
pub type Result<T> = result::Result<T, Error>;

/// Configuration details for the consensus process.
#[derive(Debug, Clone)]
pub struct Config {
    /// Genesis block details
    pub genesis: GenesisConfig,
    /// Path to the consensus data directory
    pub data_dir: PathBuf,
}

/// Genesis configuration
#[derive(Debug, Clone)]
pub struct GenesisConfig {
    pub difficulty: u64,
    pub time: DateTime<Utc>,
}

impl GenesisConfig {
    /// Create a genesis block
    pub fn to_block(&self) -> Block {
        Block {
            header: Header {
                version: 0,
                height: 0,
                parents: Vec::new(),
                difficulty: self.difficulty,
                nonce: 0,
                time: self.time,
            },
        }
    }
}

/// Run the consensus process, spawning the task as a new thread. Returns an ['broadcast::Sender'],
/// which can be subscribed to, to receive consensus events from the task.
pub fn start(
    config: Config,
    p2p_action_ch: UnboundedSender<p2p::Action>,
    p2p_event_ch: broadcast::Receiver<p2p::Event>,
) -> (UnboundedSender<Action>, broadcast::Sender<Event>) {
    // Spawn the task
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(CONSENSUS_EVENT_CHAN_CAPACITY);
    tokio::spawn(task_fn(
        config,
        action_receiver,
        event_sender.clone(),
        p2p_action_ch,
        p2p_event_ch,
    ));

    // Return the communication channels
    (action_sender, event_sender)
}

/// The task function which runs the consensus process.
async fn task_fn(
    config: Config,
    mut actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    p2p_action_ch: UnboundedSender<p2p::Action>,
    mut p2p_event_ch: broadcast::Receiver<p2p::Event>,
) {
    info!("Starting consensus...");

    // Open the peer database
    let _consensus_db = ConsensusDatabase::open(&config.data_dir.join(DATABASE_DIR), true)
        .expect("failed to open peer database");

    // Get peer id from p2p client
    let (resp_sender, resp_ch) = oneshot::channel();
    p2p_action_ch
        .send(p2p::Action::GetLocalPeerId(resp_sender))
        .unwrap();
    let peer_id = resp_ch.await.unwrap();

    // TODO: This just initializes a frontier on genesis... obviously we don't always want to do
    // this. We usually want to load state from a db or something.
    let genesis = config.genesis.to_block();
    let ticket = ValidatorTicket::new(peer_id, vec![genesis.into()])
        .expect("Failed to create initial ValidatorTicket");
    while let Err(e) = events_out.send(Event::NewMiningJob(ticket.clone())) {
        error!("failed to send consensus event: {e}");
    }

    // Create a randomx VM instance for verifying proofs of work
    let randomx_vm =
        RandomXVMInstance::new(b"cordelia-randomx", RandomXFlag::get_recommended_flags()).unwrap();

    // Handle consensus events
    loop {
        select! {
            event = p2p_event_ch.recv() => {
                match event {
                    Err(e) => {
                            error!("Stopping due to p2p_action channel error: {e}");
                            return;
                },
                    Ok(p2p::Event::Pubsub(msg)) => {
                        let validation = handle_p2p_message(&msg, &randomx_vm);
                        if let Err(e) = p2p_action_ch.send(p2p::Action::ReportMessageValidity(validation)) {
                            error!("Stopping due to p2p_action channel error: {e}");
                            return;
                        }
                    },
                }
            },
            Some(action) = actions_in.recv() => {
                match action {
                    Action::SubmitMinedTicket(ticket) => {
                        if ticket.verify_pow(&randomx_vm).is_ok() {
                            if let Err(e) = p2p_action_ch.send(p2p::Action::Broadcast(p2p::MessageData::Ticket(ticket))) {
                            error!("Stopping due to p2p_action channel error: {e}");
                            return;
                            }
                        }
                    }
                }
            },
        }
    }
}

/// Handle a message from the peer to peer network, and generate a validation report back to the
/// p2p client.
fn handle_p2p_message(
    msg: &p2p::Message,
    randomx: &RandomXVMInstance,
) -> p2p::MessageValidationReport {
    match &msg.data {
        p2p::MessageData::Ticket(ticket) => {
            if ticket.verify_pow(randomx).is_ok() {
                info!("Received ticket from {}", ticket.peer);
                msg.accept()
            } else {
                msg.reject()
            }
        }
        _ => msg.accept(), // Accept all other messages without validation
    }
}
