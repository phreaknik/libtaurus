use super::{api, dag, namespace, pollster, transaction, Vertex, VertexHash};
use crate::WireFormat;
use api::ConsensusApi;
use chrono::DateTime;
use libp2p::PeerId;
use namespace::NamespaceId;
use std::{collections::HashSet, path::PathBuf, result, sync::Arc};
use tokio::{
    select,
    sync::{
        broadcast,
        mpsc::{self, UnboundedReceiver},
        oneshot,
    },
};
use tracing::{debug, error, info, warn};
use transaction::TxRoot;

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const CONSENSUS_EVENT_CHAN_CAPACITY: usize = 32;

/// Events produced by the consensus task
#[derive(Debug, Clone)]
pub enum Event {
    /// The following vertices make up the latest frontier, after a recent graph update. These
    /// vertices are sorted according to the order they were first observed, so that they may be
    /// used as parents in a new vertex which extends the graph.
    NewFrontier(Vec<Arc<Vertex>>),

    /// The task has been stopped
    Stopped,
}

/// Actions that can be performed by the consensus task
#[derive(Debug)]
pub enum Action {
    GetAcceptedFrontier {
        result_ch: oneshot::Sender<Vec<Arc<Vertex>>>,
    },
    GetPreference {
        vhash: VertexHash,
        result_ch: oneshot::Sender<Option<(bool, bool)>>,
    },
    GetVertex {
        vhash: VertexHash,
        result_ch: oneshot::Sender<Option<Arc<Vertex>>>,
    },
    RecordPeerPreference {
        vhash: VertexHash,
        preference: bool,
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
}
type Result<T> = result::Result<T, Error>;

/// Configuration details for the consensus task.
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

/// Run the consensus task, spawning the task as a new thread. Returns an [`broadcast::Sender`],
/// which can be subscribed to, to receive consensus events from the task.
pub fn start(config: Config) -> ConsensusApi {
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, event_receiver) = broadcast::channel(CONSENSUS_EVENT_CHAN_CAPACITY);
    let task = Task::new(config, action_receiver, event_sender.clone())
        .expect("Failed to start consensus task");
    let handle = tokio::spawn(task.task_fn());
    tokio::spawn(async move {
        if let Err(e) = handle.await {
            error!("Consensus stopped with error: {e}");
        }
        event_sender.send(Event::Stopped).unwrap();
    });

    ConsensusApi::new(action_sender, event_receiver)
}

/// Runtime state for the consensus task
pub struct Task {
    _config: Config,
    actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    dag: dag::DAG,
    validators: Vec<PeerId>,
}

impl Task {
    /// Construct a new task
    fn new(
        config: Config,
        actions_in: UnboundedReceiver<Action>,
        events_out: broadcast::Sender<Event>,
    ) -> Result<Task> {
        info!("Starting consensus");

        // Construct the DAG, initialized with the genesis vertex
        let dag = dag::DAG::new(config.dag.clone(), &[config.genesis.to_vertex()])?;

        // Instantiate the task
        Ok(Task {
            _config: config.clone(),
            actions_in,
            events_out,
            dag,
            validators: Vec::new(),
        })
    }

    /// Run the consensus processing loop
    async fn task_fn(mut self) {
        // Wait until the events channel has listeners, before initializing the DAG
        while self.events_out.receiver_count() == 0 {}

        // Emit event for initial frontier
        self.events_out
            .send(Event::NewFrontier(self.dag.get_frontier()))
            .expect("Failed to send initial frontier event");

        // Handle consensus events
        let mut internal_events = self.events_out.subscribe();
        loop {
            select! {
                // Handle requested actions
                Some(action) = self.actions_in.recv() => {
                    match action{
                        Action::GetAcceptedFrontier{result_ch} => {
                            if let Err(_e) = result_ch.send(self.dag.get_frontier()) {
                                debug!("failed to respond to GetAcceptedFrontier");
                            }
                        },
                        Action::GetPreference{vhash, result_ch} => {
                            if let Err(_e) = result_ch.send(self.dag.query(&vhash).ok()) {
                                debug!("failed to respond to GetVertex");
                            }
                        },
                        Action::GetVertex{vhash, result_ch} => {
                            if let Err(_e) = result_ch.send(self.dag.get_vertex(&vhash)) {
                                debug!("failed to respond to GetVertex");
                            }
                        },
                        Action::RecordPeerPreference{vhash, preference} => {
                            let _ = self.dag.record_query_result(&vhash, preference);
                        },
                        Action::SubmitVertex{vertex, result_ch} => {
                            let resp = self.dag.try_insert(&vertex).map_err(Error::from);
                            match &resp {
                                Ok(waiting) => {
                                    info!("inserted {}", vertex.hash().to_hex());

                                    // Retry any pending vertices which were waiting on this one
                                    if let Ok(successful) = self.dag.retry_pending(waiting) {
                                        for vhash in successful {
                                            info!("inserted pending {}", vhash.to_hex());
                                        }
                                    }

                                    // Get the latest frontier
                                    // TODO: this frontier may not actually be "new"
                                    let frontier = self.dag.get_frontier();
                                    self.events_out.send(Event::NewFrontier(frontier)).unwrap();
                                },
                                Err(e) => info!("vertex {} not inserted: {e}", vertex.hash().to_hex()),
                            };
                            if let Err(_) = result_ch.send(resp) {
                                warn!("unable to send response after action");
                            }
                        }
                    }
                },

                // Handle internally generated events
                event = internal_events.recv() => {
                    match event {
                        Err(e) => return error!("Stopping due to consensus_event channel error: {e}"),
                        Ok(_) => {},
                    }
                }
            }
        }
    }
}
