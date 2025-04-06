pub mod avalanche;
mod block;
mod database;

use crate::p2p::avalanche_rpc;
use crate::randomx::RandomXVMInstance;
use crate::{p2p, randomx};
pub use avalanche::*;
pub use block::*;
use chrono::{DateTime, Utc};
use libp2p::multihash::Multihash;
use libp2p::PeerId;
use lru::LruCache;
use randomx_rs::RandomXFlag;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::result;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::{select, sync::broadcast};
use tracing::{debug, error, info, warn};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Maximum depth (in terms of block height) of the pending block queue
const PENDING_BLOCK_QUEUE_DEPTH: usize = 10;

/// Maximum number of pending blocks we can store at a given height in the queue
const PENDING_BLOCK_QUEUE_WIDTH: usize = 10;

/// Events produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// The DAG frontier has updated
    NewFrontier(Frontier),
}

/// Actions that can be performed by the consensus process
#[derive(Clone, Debug)]
pub enum Action {
    SubmitBlock(Block),
}

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Avalanche(#[from] avalanche::Error),
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error(transparent)]
    SerdeCbor(#[from] serde_cbor::error::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    RandomX(#[from] randomx::Error),
    #[error("new block channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error("p2p action channel error")]
    P2pActionCh(#[from] tokio::sync::mpsc::error::SendError<p2p::Action>),
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error("collection is empty")]
    EmptyCollection,
    #[error("frontier object is empty")]
    EmptyFrontier,
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
    /// Avalanche configuration
    pub avalanche: avalanche::Config,
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
            version: 0,
            parents: Vec::new(),
            height: 0,
            difficulty: self.difficulty,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            time: self.time,
            nonce: 0,
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
    _config: Config,
    peer_id: Option<PeerId>,
    randomx_vm: RandomXVMInstance,
    dag: avalanche::DAG,
    actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    p2p_action_ch: UnboundedSender<p2p::Action>,
    p2p_event_ch: broadcast::Receiver<p2p::Event>,
    pending_blocks: LruCache<u64, LruCache<Hash, Block>>,
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

        // Create a randomx VM instance for verifying proofs of work
        let randomx_vm =
            RandomXVMInstance::new(b"cordelia-randomx", RandomXFlag::get_recommended_flags())?;

        // Instantiate the runtime
        Ok(Runtime {
            _config: config.clone(),
            dag: DAG::new(config.avalanche, events_out.clone()),
            peer_id: None,
            randomx_vm,
            actions_in,
            events_out,
            p2p_action_ch,
            p2p_event_ch,
            pending_blocks: LruCache::new(NonZeroUsize::new(PENDING_BLOCK_QUEUE_DEPTH).unwrap()),
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

        // Wait until the events channel has listeners, before initializing the DAG
        while self.events_out.receiver_count() == 0 {}
        self.dag.init().expect("failed to initialize the DAG");

        // Handle consensus events
        loop {
            select! {
                event = self.p2p_event_ch.recv() => {
                    match event {
                        Err(e) => {
                                error!("Stopping due to p2p_action channel error: {e}");
                                return;
                    },
                        Ok(p2p::Event::Pubsub(msg)) => {
                            let ignore = msg.ignore();
                            let validation = self.handle_p2p_pubsub(msg).unwrap_or_else(|e| {
                                warn!("Error handling p2p message: {e}");
                                ignore
                            });
                            if let Err(e) = self.p2p_action_ch.send(p2p::Action::ReportMessageValidity(validation)) {
                                error!("Stopping due to p2p_action channel error: {e}");
                                return;
                            }
                        },
                        Ok(p2p::Event::Avalanche(event)) => self.handle_avalanche_event(event),
                    }
                },
                Some(action) = self.actions_in.recv() => {
                    match action {
                        Action::SubmitBlock(block) => {
                            // Immediately forward the block on to our peers
                            if let Err(e) = self.p2p_action_ch.send(p2p::Action::Broadcast(p2p::MessageData::Block(block.clone()))) {
                                error!("Stopping due to p2p_action channel error: {e}");
                                return;
                            }

                            // Validate the block and insert it into the DAG
                            let hash = block.hash().unwrap();
                            if let Err(e) = self.try_insert_block(block, None) {
                                error!("Failed to insert mined block {hash}: {e:?}")
                            }
                        }
                    }
                },
            }
        }
    }

    /// Try to insert a block
    fn try_insert_block(&mut self, block: Block, sender: Option<PeerId>) -> Result<()> {
        let hash = block.hash()?;
        let height = block.height;
        block
            .verify_pow(&self.randomx_vm)
            .map_err(Error::from)
            .and_then(|_| self.dag.try_insert(block).map_err(Error::from))
            .or_else(|e| {
                if let Error::Avalanche(avalanche::Error::MissingParent(parent)) = e {
                    debug!("Block {hash} is mising parent {parent}");
                    // Add to the pending block queue to try again later
                    // TODO

                    // Look for parent block in P2P network
                    if let Some(peer) = sender {
                        self.p2p_action_ch.send(p2p::Action::AvalancheRequest(
                            peer,
                            avalanche_rpc::Request::GetBlock(height - 1, parent.into()),
                        ))?;
                    }
                } else {
                    warn!("Unable to insert block {hash}: {e:?}");
                }
                Err(e)
            })
    }

    /// Handle a received avalanche request message from one of our peers
    fn handle_avalanche_event(&mut self, event: avalanche_rpc::Event) {
        match event {
            // Handle inbound avalanche requests from other peers
            avalanche_rpc::Event::Requested(request_id, request) => match request {
                avalanche_rpc::Request::GetBlock(height, hash) => {
                    // Generate a response with the requested block, if we have it
                    let resp = match self.dag.get_block(height, &hash.into()) {
                        Ok(block) => avalanche_rpc::Response::Block(block),
                        Err(_) => avalanche_rpc::Response::NotFound,
                    };
                    // Send the response
                    if let Err(e) = self
                        .p2p_action_ch
                        .send(p2p::Action::AvalancheResponse(request_id, resp))
                    {
                        error!("Stopping due to p2p_action channel error: {e}");
                        return;
                    }
                }
            },

            // Handle responses to avalanche requests we sent out
            avalanche_rpc::Event::Responded(peer, response) => match response {
                // TODO: if the peer didn't have the requested data, what do we do?
                // Do we ban the peer for not having data that they should?
                // Do we try to find the requested data on the DHT instead?
                avalanche_rpc::Response::NotFound => todo!(),
                avalanche_rpc::Response::Block(block) => {
                    if let Ok(hash) = block.hash() {
                        if let Err(e) = self.try_insert_block(block, Some(peer)) {
                            debug!("unable to insert requested block {hash}: {e}");
                        }
                    }
                }
            },
        }
    }

    /// Handle a message from the peer to peer network, and generate a validation report back to the
    /// p2p client.
    fn handle_p2p_pubsub(&mut self, msg: p2p::Message) -> Result<p2p::MessageValidationReport> {
        let accept = msg.accept();
        let ignore = msg.ignore();
        let reject = msg.reject();
        match msg.data {
            p2p::MessageData::Block(block) => {
                match self.try_insert_block(block, Some(msg.msg_source)) {
                    Err(Error::Avalanche(avalanche::Error::MissingParent(_))) => Ok(ignore),
                    Err(_) => Ok(reject),
                    Ok(_) => Ok(accept),
                }
            }
        }
    }
}
