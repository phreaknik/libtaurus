pub mod block;
pub mod hash;

use crate::p2p::{self, Message};
use crate::{Block, Frontier, Header};
use chrono::{DateTime, Utc};
use std::result;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::time::interval;
use tokio::{select, sync::broadcast};
use tracing::{error, info};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Event produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// The DAG has a new ['Frontier']
    NewFrontier(Frontier),
}

/// Actions that can be performed by the consensus process
#[derive(Clone, Debug)]
pub enum Action {}

/// Error type for consensus errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
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
    pub genesis: GenesisConfig,
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
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(CONSENSUS_EVENT_CHAN_CAPACITY);
    tokio::spawn(task_fn(
        config,
        action_receiver,
        event_sender.clone(),
        p2p_action_ch,
        p2p_event_ch,
    ));
    (action_sender, event_sender)
}

/// The task function which runs the consensus process.
async fn task_fn(
    config: Config,
    mut _actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    p2p_action_ch: UnboundedSender<p2p::Action>,
    mut p2p_event_ch: broadcast::Receiver<p2p::Event>,
) {
    info!("Starting consensus...");

    let mut ticker = interval(Duration::from_secs(5));
    let start = Instant::now();

    // TODO: This just initializes a frontier on genesis... obviously we don't always want to do
    // this. We usually want to load state from a db or something.
    let genesis = config.genesis.to_block();
    if let Err(e) = events_out.send(Event::NewFrontier(Frontier {
        heads: vec![genesis.header],
        nonce: 0,
    })) {
        error!("failed to send consensus event: {e}");
        return;
    };

    loop {
        select! {
            event = p2p_event_ch.recv() => {
                    info!("received p2p event {event:?}");
            },
            _ = ticker.tick() => {
                match p2p_action_ch.send(p2p::Action::Broadcast(Message::Hello(format!("I'm online for {:?}", start.elapsed()).into()))) {
                    Ok(_) => info!("message sent!"),
                    Err(e) => error!("error sending message: {e}"),
                }
            },
        }
    }
}
