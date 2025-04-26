// TODO: Consider ledger using CRYSTALS-Dilithium signature algorithm:
// crates.io/crystals/crystals-dilithium

// TODO: ALTERNATE: Consider ledger using MiRitH signature algorithm: iacr 2023/1666
// TODO: ALTERNATE: Consider ledger using MQ on My Mind signature algorithm: iacr 2023/1719

pub mod dag;
pub mod event;
pub mod namespace;
pub mod vertex;

use crate::p2p;
use libp2p::PeerId;
use std::path::PathBuf;
use std::result;
use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::{select, sync::broadcast};
use tracing::{error, info, warn};
pub use vertex::{Vertex, VertexHash};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Events produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// The DAG frontier has updated
    NewFrontier(Vec<VertexHash>),
    /// If a vertex has parents which become non-virtuous, it will be impossible for this
    /// vertex to be accepted. According to Avalanche consensus, we should dynamically
    /// select parents to retry submitting
    StalledVertex(Arc<Vertex>),
    /// Indicates the specified vertex has been queried, and the query is complete.
    QueryCompleted(VertexHash),
}

/// Actions that can be performed by the consensus process
#[derive(Clone, Debug)]
pub enum Action {}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error("p2p action channel error")]
    P2pActionCh(#[from] tokio::sync::mpsc::error::SendError<p2p::Action>),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
}
type Result<T> = result::Result<T, Error>;

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
pub struct GenesisConfig {}

impl GenesisConfig {
    /// Make sure the vertex passes all basic sanity checks
    pub fn sanity_checks(&self) -> Result<()> {
        todo!()
    }

    /// Create a genesis block
    pub fn to_vertex(&self) -> Arc<Vertex> {
        todo!()
    }
}

/// Run the consensus process, spawning the task as a new thread. Returns an [`broadcast::Sender`],
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

        // Instantiate the runtime
        Ok(Runtime {
            _config: config.clone(),
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
        let mut internal_events = self.events_out.subscribe();
        loop {
            select! {
                // Handle P2P events
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
                        Err(e) => return error!("Stopping due to p2p_event channel error: {e}"),
                    }
                },
                // Handle requested actions
                Some(_action) = self.actions_in.recv() => {
                    todo!();
                },
                // Handle internally generated events
                event = internal_events.recv() => {
                    match event {
                        Ok(Event::NewFrontier(_)) => {todo!()},
                        Ok(Event::StalledVertex(_)) => {todo!()},
                        Ok(Event::QueryCompleted(_)) => {todo!()},
                        Err(e) => return error!("Stopping due to consensus_event channel error: {e}"),
                    }
                }
            }
        }
    }

    /// Handle a message from the peer to peer network, and generate a validation report back to the
    /// p2p client.
    fn handle_p2p_pubsub(
        &mut self,
        bcast: p2p::Broadcast,
    ) -> Result<p2p::BroadcastValidationReport> {
        let _accept = bcast.accept();
        let _ignore = bcast.ignore();
        let _reject = bcast.reject();
        match bcast.data {
            p2p::BroadcastData::Vertex(_) => {
                todo!()
            }
        }
    }
}
