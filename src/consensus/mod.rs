// TODO: Consider ledger using MiRitH signature algorithm: iacr 2023/1666

pub mod avalanche;
pub mod block;
mod database;
mod transaction;
pub mod vertex;
mod voter_pool;

use crate::randomx::RandomXVMInstance;
use crate::{p2p, randomx};
pub use avalanche::*;
pub use block::*;
use chrono::{DateTime, Utc};
use libp2p::multihash::Multihash;
use libp2p::PeerId;
use randomx_rs::RandomXFlag;
use std::path::PathBuf;
use std::result;
use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::{select, sync::broadcast};
use tracing::{error, info, trace, warn};
use tracing_mutex::stdsync::TracingRwLock;
pub use vertex::*;

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Events produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// The DAG frontier has updated
    NewFrontier(Vec<VertexHash>),
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
    #[error("error acquiring write lock on Avalanche DAG")]
    DagWriteLock,
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error("new block channel error")]
    NewBlockCh(#[from] tokio::sync::mpsc::error::SendError<Block>),
    #[error("p2p action channel error")]
    P2pActionCh(#[from] tokio::sync::mpsc::error::SendError<p2p::Action>),
    #[error(transparent)]
    RandomX(#[from] randomx::Error),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
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
            difficulty: self.difficulty,
            miner: PeerId::from_multihash(Multihash::default()).unwrap(),
            prev_mined: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
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
    dag: TracingRwLock<avalanche::Dag>,
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

        // Create a randomx VM instance for verifying proofs of work
        let randomx_vm =
            RandomXVMInstance::new(b"cordelia-randomx", RandomXFlag::get_recommended_flags())?;

        // Instantiate the runtime
        Ok(Runtime {
            _config: config.clone(),
            dag: TracingRwLock::new(Dag::new(
                config.avalanche,
                randomx_vm,
                p2p_action_ch.clone(),
                events_out.clone(),
            )),
            peer_id: None,
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

        // Wait until the events channel has listeners, before initializing the DAG
        while self.events_out.receiver_count() == 0 {}

        // Handle consensus events
        loop {
            select! {
                event = self.p2p_event_ch.recv() => {
                    match event {
                        Ok(p2p::Event::Pubsub(msg)) => {
                            let ignore = msg.ignore();
                             if let Err(e) = self.handle_p2p_pubsub(msg)
                                .or_else(|e| {
                                    warn!("Error handling p2p message: {e}");
                                    Ok(ignore)
                                })
                                .and_then(|validation| self.p2p_action_ch.send(p2p::Action::ReportMessageValidity(validation)).map_err(Error::from)) {
                                return error!("Stopping due to p2p_action channel error: {e}");
                            }
                        },
                        Ok(p2p::Event::Avalanche(message)) => {
                            match self.dag.write().map_err(|_| Error::DagWriteLock).unwrap().handle_avalanche_message(message) {
                                Ok(()) => trace!("Handled avalanche message"),
                                Err(avalanche::Error::P2pActionCh(e)) => return error!("Stopping due to p2p_action channel error: {e}"),
                                Err(e) => error!("Error while handling Avalanche message: {e}"),
                            }
                        }
                        Err(e) => return error!("Stopping due to p2p_action channel error: {e}"),
                    }
                },
                Some(action) = self.actions_in.recv() => {
                    match action {
                        Action::SubmitBlock(block) => {
                            // Immediately forward the block on to our peers
                            let hash = block.hash();
                            // Insert the vertex into the DAG
                            match self
                                .dag
                                .write()
                                .map_err(|_| Error::DagWriteLock)
                                .and_then(|mut dag| dag.submit_block(block, true).map_err(Error::from))
                            {
                                Ok(_) => {},
                                Err(Error::P2pActionCh(e)) => return error!("Stopping due to p2p_action channel error: {e}"),
                                Err(e) => error!("Failed to submit mined block {hash}: {e}"),
                            }
                        }
                    }
                },
            }
        }
    }

    /// Handle a message from the peer to peer network, and generate a validation report back to the
    /// p2p client.
    fn handle_p2p_pubsub(&mut self, msg: p2p::Message) -> Result<p2p::MessageValidationReport> {
        let accept = msg.accept();
        let ignore = msg.ignore();
        let reject = msg.reject();
        match msg.data {
            p2p::MessageData::Vertex(wire_vertex) => {
                let mut dag = self.dag.write().map_err(|_| Error::DagWriteLock)?;
                let slim = wire_vertex.slim();
                match dag
                    .submit_block(wire_vertex.block, false)
                    .and_then(|_| dag.try_insert_vertex(Arc::new(slim), Some(msg.msg_source)))
                {
                    Err(avalanche::Error::MissingData) => Ok(ignore),
                    Err(_) => Ok(reject),
                    Ok(_) => Ok(accept),
                }
            }
        }
    }
}
