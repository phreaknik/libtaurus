mod avalanche;
pub mod block;
mod database;
pub mod hash;

use self::database::BlocksDatabase;
use crate::randomx::RandomXVMInstance;
use crate::{p2p, randomx};
pub use block::{Block, Frontier, Header};
use chrono::{DateTime, Utc};
pub use hash::Hash;
use libp2p::multihash::Multihash;
use libp2p::PeerId;
use randomx_rs::RandomXFlag;
use std::path::PathBuf;
use std::result;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::{select, sync::broadcast};
use tracing::{error, info, warn};

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "blocks_db/";

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Events produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// The DAG frontier has updated
    NewFrontier(Frontier),
}

/// Actions that can be performed by the consensus process
#[derive(Clone, Debug)]
pub enum Action {
    SubmitMinedBlock(Block),
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
    #[error("validator ticket channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
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
                miner: PeerId::from_multihash(Multihash::default()).unwrap(),
                time: self.time,
                nonce: 0,
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
    // Spawn a task to execute the runtime
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(CONSENSUS_EVENT_CHAN_CAPACITY);
    let runtime = Runtime::new(
        config,
        action_receiver,
        event_sender.clone(),
        p2p_action_ch,
        p2p_event_ch,
    )
    .expect("Failed to start consensus runtime");
    tokio::spawn(runtime.run());

    // Return the communication channels
    (action_sender, event_sender)
}

/// Runtime state for the consensus process
pub struct Runtime {
    config: Config,
    database: BlocksDatabase,
    peer_id: Option<PeerId>,
    randomx_vm: RandomXVMInstance,
    actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    p2p_action_ch: UnboundedSender<p2p::Action>,
    p2p_event_ch: broadcast::Receiver<p2p::Event>,
}

impl Runtime {
    fn new(
        config: Config,
        actions_in: UnboundedReceiver<Action>,
        events_out: broadcast::Sender<Event>,
        p2p_action_ch: UnboundedSender<p2p::Action>,
        p2p_event_ch: broadcast::Receiver<p2p::Event>,
    ) -> Result<Runtime> {
        info!("Starting consensus...");

        // Open the blocks database
        let database = BlocksDatabase::open(&config.data_dir.join(DATABASE_DIR), true)
            .expect("Failed to open blocks database");

        // Create a randomx VM instance for verifying proofs of work
        let randomx_vm =
            RandomXVMInstance::new(b"cordelia-randomx", RandomXFlag::get_recommended_flags())?;

        // Instantiate the runtime
        Ok(Runtime {
            config,
            database,
            peer_id: None,
            randomx_vm,
            actions_in,
            events_out,
            p2p_action_ch,
            p2p_event_ch,
        })
    }

    // Run the consensus processing loop
    async fn run(mut self) {
        // Get peer id from p2p client
        let (resp_sender, resp_ch) = oneshot::channel();
        self.p2p_action_ch
            .send(p2p::Action::GetLocalPeerId(resp_sender))
            .unwrap();
        self.peer_id = Some(resp_ch.await.unwrap());

        // TODO: This just initializes a frontier on genesis... obviously we don't always want to do
        // this. We usually want to load state from a db or something.
        let genesis = self.config.genesis.to_block();
        while let Err(e) = self
            .events_out
            .send(Event::NewFrontier(Frontier(vec![genesis.header.clone()])))
        {
            error!("failed to send consensus event: {e}");
        }

        loop {
            select! {
                event = self.p2p_event_ch.recv() => {
                    match event {
                        Err(e) => {
                                error!("Stopping due to p2p_action channel error: {e}");
                                return;
                    },
                        Ok(p2p::Event::Pubsub(msg)) => {
                            let validation = self.handle_p2p_message(&msg).unwrap_or_else(|e| {
                                warn!("Error handling p2p message: {e}");
                                msg.ignore()
                            });
                            if let Err(e) = self.p2p_action_ch.send(p2p::Action::ReportMessageValidity(validation)) {
                                error!("Stopping due to p2p_action channel error: {e}");
                                return;
                            }
                        },
                    }
                },
                Some(action) = self.actions_in.recv() => {
                    match action {
                        Action::SubmitMinedBlock(block) => {
                            if block.header.verify_pow(&self.randomx_vm).is_ok() {
                                if let Err(e) = self.p2p_action_ch.send(p2p::Action::Broadcast(p2p::MessageData::Block(block.clone()))) {
                                    error!("Stopping due to p2p_action channel error: {e}");
                                    return;
                                }
                                if let Err(e) = self.database.write_block(block, false) {
                                    error!("Failed to write block: {e}");
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
    fn handle_p2p_message(&mut self, msg: &p2p::Message) -> Result<p2p::MessageValidationReport> {
        match &msg.data {
            p2p::MessageData::Block(block) => {
                if block.header.verify_pow(&self.randomx_vm).is_ok() {
                    info!("Received block {}", block.header.hash());
                    self.database.write_block(block.clone(), false)?;
                    Ok(msg.accept())
                } else {
                    Ok(msg.reject())
                }
            }
            _ => Ok(msg.accept()), // Accept all other messages without validation
        }
    }
}
