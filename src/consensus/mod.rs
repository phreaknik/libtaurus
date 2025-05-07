pub mod api;
pub mod dag;
pub mod namespace;
pub mod transaction;
pub mod vertex;

use crate::p2p::api::P2pApi;
use crate::{p2p, WireFormat};
use api::ConsensusApi;
use chrono::DateTime;
use namespace::NamespaceId;
use std::collections::HashSet;
use std::path::PathBuf;
use std::result;
use std::sync::Arc;
use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::{select, sync::broadcast, sync::oneshot};
use tracing::{debug, error, info, warn};
use transaction::TxRoot;
pub use vertex::{Vertex, VertexHash};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Events produced by the consensus process
#[derive(Debug, Clone)]
pub enum Event {
    /// The following vertices should be re-inserted. This usually means a missing parent has been
    /// found and it may now be possible to process these vertices.
    RetryInsert(HashSet<VertexHash>),

    /// The following vertices make up the latest frontier, after a recent graph update. These
    /// vertices are sorted according to the order they were first observed, so that they may be
    /// used as parents in a new vertex which extends the graph.
    NewFrontier(Vec<Arc<Vertex>>),

    /// The process has been stopped
    Stopped,
}

/// Actions that can be performed by the consensus process
#[derive(Debug)]
pub enum Action {
    GetAcceptedFrontier {
        result_ch: oneshot::Sender<Vec<Arc<Vertex>>>,
    },
    SubmitVertex {
        vertex: Arc<Vertex>,
        result_ch: oneshot::Sender<Result<HashSet<VertexHash>>>,
    },
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    DAG(#[from] dag::Error),
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
    pub datadir: PathBuf,

    /// DAG configuration
    pub dag: dag::Config,
}

/// Genesis configuration
#[derive(Debug, Clone)]
pub struct GenesisConfig {}

impl GenesisConfig {
    /// Create a genesis block
    pub fn to_vertex(&self) -> Arc<Vertex> {
        Arc::new(Vertex {
            version: 1,
            height: 0,
            parents: Vec::new(),
            namespace_id: NamespaceId::default(),
            root: TxRoot::default(),
            timestamp: DateTime::parse_from_rfc2822("Wed, 18 Feb 2015 23:16:09 GMT")
                .unwrap()
                .to_utc(),
        })
    }
}

/// Run the consensus process, spawning the task as a new thread. Returns an [`broadcast::Sender`],
/// which can be subscribed to, to receive consensus events from the task.
pub fn start(config: Config, p2p_api: P2pApi) -> ConsensusApi {
    // Spawn a task to run the process
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, event_receiver) = broadcast::channel(CONSENSUS_EVENT_CHAN_CAPACITY);
    let process = Process::new(config, action_receiver, event_sender.clone(), p2p_api)
        .expect("Failed to start consensus process");
    let handle = tokio::spawn(process.task_fn());
    tokio::spawn(async move {
        if let Err(e) = handle.await {
            error!("Consensus stopped with error: {e}");
        }
        event_sender.send(Event::Stopped).unwrap();
    });

    // Return the communication channels
    ConsensusApi::new(action_sender, event_receiver)
}

/// Runtime state for the consensus process
pub struct Process {
    _config: Config,
    actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    p2p_api: P2pApi,
    dag: dag::DAG,
}

impl Process {
    fn new(
        config: Config,
        actions_in: UnboundedReceiver<Action>,
        events_out: broadcast::Sender<Event>,
        p2p_api: P2pApi,
    ) -> Result<Process> {
        info!("Starting consensus...");

        // Construct the DAG, initialized with the genesis vertex
        let dag = dag::DAG::new(config.dag.clone(), &[config.genesis.to_vertex()])?;

        // Instantiate the process
        Ok(Process {
            _config: config.clone(),
            actions_in,
            events_out,
            p2p_api,
            dag,
        })
    }

    // Run the consensus processing loop
    async fn task_fn(mut self) {
        // Wait until the events channel has listeners, before initializing the DAG
        while self.events_out.receiver_count() == 0 {}

        // Emit event for initial frontier
        self.events_out
            .send(Event::NewFrontier(self.dag.get_frontier()))
            .expect("Failed to send initial frontier event");

        // Handle consensus events
        let mut internal_events = self.events_out.subscribe();
        let mut p2p_events = self.p2p_api.subscribe_events();
        loop {
            select! {
                // Handle P2P events
                event = p2p_events.recv() => {
                    match event {
                        Ok(p2p::Event::Pubsub(msg)) => {
                            let ignore = msg.ignore();
                             if let Err(e) = self.handle_p2p_pubsub(msg)
                                .or_else(|e| {
                                    warn!("Error handling p2p message: {e}");
                                    Ok(ignore)
                                })
                                .and_then(|validation| self.p2p_api.report_message_validity(validation)) {
                                return error!("Stopping due to p2p_action channel error: {e}");
                            }
                        },
                        Err(e) => return error!("Stopping due to p2p_event channel error: {e}"),
                    }
                },
                // Handle requested actions
                Some(action) = self.actions_in.recv() => {
                        match action{
                            Action::GetAcceptedFrontier{result_ch} => {
                                if let Err(_e) = result_ch.send(self.dag.get_frontier()) {
                                    debug!("failed to respond to GetAcceptedFrontier");
                                }
                            },
                            Action::SubmitVertex{vertex, result_ch} => {
                                if let Err(_e) = result_ch.send(self.dag.try_insert(&vertex).map_err(Error::from)) {
                                    debug!("failed to respond to SubmitVertex");
                                } else {
                                    info!("inserted {}", vertex.hash().to_hex());
                                }
                            }
                        }
                },
                // Handle internally generated events
                event = internal_events.recv() => {
                    match event {
                        Ok(Event::RetryInsert(_)) => {todo!()},
                        Err(e) => return error!("Stopping due to consensus_event channel error: {e}"),
                        Ok(_) => {},
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
        let accept = bcast.accept();
        let ignore = bcast.ignore();
        let reject = bcast.reject();
        match bcast.data {
            p2p::BroadcastData::Vertex(vx) => match self.dag.try_insert(&vx) {
                Ok(waiting) => {
                    // Send internal signal to retry any vertices which were waiting on this one
                    self.events_out.send(Event::RetryInsert(waiting))?;
                    // Emit event indicating the latest frontier
                    self.events_out
                        .send(Event::NewFrontier(self.dag.get_frontier()))?;
                    Ok(accept)
                }
                Err(dag::Error::MissingParents(_)) => Ok(accept),
                Err(dag::Error::AlreadyInserted | dag::Error::RejectedAncestor) => Ok(ignore),
                Err(_) => Ok(reject),
            },
        }
    }
}
