use super::{api, dag, namespace, pollster, transaction, Vertex, VertexHash};
use crate::{p2p::P2pApi, WireFormat};
use api::ConsensusApi;
use chrono::DateTime;
use indexmap::IndexSet;
use itertools::Itertools;
use libp2p::PeerId;
use namespace::NamespaceId;
use rand::Rng;
use std::{collections::HashSet, iter, path::PathBuf, result, sync::Arc};
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

pub const DFLT_QUERY_COUNT: usize = 5;
pub const DFLT_QUORUM_COUNT: usize = 3;
// TODO: pick real values here

/// Events produced by the consensus task
#[derive(Debug, Clone)]
pub enum Event {
    /// The following vertices make up the latest frontier, after a recent graph update. These
    /// vertices are sorted according to the order they were first observed, so that they may be
    /// used as parents in a new vertex which extends the graph.
    NewFrontier(Vec<Arc<Vertex>>),
}

/// Actions that can be performed by the consensus task
#[derive(Debug)]
pub enum Action {
    AddValidator(PeerId),
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
    #[error("config specifies impossible quorum size")]
    BadCfgQuorumCount,
    #[error(transparent)]
    DAG(#[from] dag::Error),
    #[error("consensus event channel error")]
    EventsOutCh(#[from] tokio::sync::broadcast::error::SendError<Event>),
    #[error("not enough validators (need >= {0}, have {1})")]
    NeedValidators(usize, usize),
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

    /// Number of peers to query in each round
    pub query_count: usize,

    /// Number of peers to satisfy a quorum for a round of queries
    pub quorum_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            genesis: GenesisConfig::default(),
            datadir: PathBuf::default(),
            dag: dag::Config::default(),
            query_count: DFLT_QUERY_COUNT,
            quorum_size: DFLT_QUORUM_COUNT,
        }
    }
}

impl Config {
    /// Check the configuration parameters are legal
    pub fn check(&self) -> Result<()> {
        if self.query_count < self.quorum_size {
            Err(Error::BadCfgQuorumCount)
        } else {
            Ok(())
        }
    }
}

/// Genesis configuration
#[derive(Debug, Default, Clone)]
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
pub fn start(config: Config, p2p_api: P2pApi) -> Result<ConsensusApi> {
    let (task, api) = Task::new(config, p2p_api)?;
    tokio::spawn(task.task_fn());
    Ok(api)
}

/// Runtime state for the consensus task
#[derive(Debug)]
pub struct Task {
    config: Config,
    consensus_api: ConsensusApi,
    p2p_api: P2pApi,
    actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    dag: dag::DAG,
    validators: IndexSet<PeerId>,
}

impl Task {
    /// Construct a new task
    fn new(config: Config, p2p_api: P2pApi) -> Result<(Task, ConsensusApi)> {
        // Check the config
        config.check()?;

        // Set up the communication channels
        let (action_sender, actions_in) = mpsc::unbounded_channel();
        let (events_out, _event_receiver) = broadcast::channel(CONSENSUS_EVENT_CHAN_CAPACITY);

        // Construct the DAG, initialized with the genesis vertex
        let dag = dag::DAG::new(config.dag.clone(), &[config.genesis.to_vertex()])?;

        // Construct an API object
        let api = ConsensusApi::new(action_sender, events_out.clone());

        // Instantiate the task
        Ok((
            Task {
                config: config.clone(),
                consensus_api: api.clone(),
                p2p_api,
                actions_in,
                events_out,
                dag,
                validators: IndexSet::new(),
            },
            api,
        ))
    }

    /// Run the consensus processing loop
    async fn task_fn(mut self) {
        info!("Starting consensus");

        // Wait until the events channel has listeners
        while self.events_out.receiver_count() == 0 {}

        // Emit event for initial frontier
        self.events_out
            .send(Event::NewFrontier(self.dag.get_frontier()))
            .expect("Failed to send initial frontier event");

        // Handle consensus events
        // TODO: consider spawning this handler loop in a number of parallel workers, to allow
        // simultaneous handling of read-only events. tracingMutex to get read/write locks
        let mut internal_events = self.events_out.subscribe();
        loop {
            select! {
                // Handle requested actions
                Some(action) = self.actions_in.recv() => {
                    match action{
                        Action::AddValidator(validator) => {self.validators.insert(validator);},
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
                                    info!("inserted {} at height {}", vertex.hash().to_hex(), vertex.height);

                                    // Retry any pending vertices which were waiting on this one
                                    if let Ok(successful) = self.dag.retry_pending(waiting) {
                                        for vhash in successful {
                                            info!("inserted pending {}", vhash.to_hex());
                                        }
                                    }

                                    // Get the latest frontie and kick off a pollster instance to query peer preferences for each frontier vertex
                                    let frontier = self.dag.get_frontier();
                                    for vx in &frontier {
                                        if let Err(e) = self.get_validators_for_query().map(|validators| {
                                            // Kick off a new pollster instance to gather peer
                                            // prefereences for the given vertex
                                            pollster::start(vx.hash(),
                                                self.p2p_api.clone(),
                                                self.consensus_api.clone(),
                                                validators,
                                                self.config.quorum_size
                                            );
                                        }) {
                                            warn!("unable to select validators: {e}");
                                        }
                                    }

                                    // Emit new frontier event
                                    // TODO: this frontier may not actually be "new"
                                    println!(":::: {} consensus events already queued", self.events_out.len());
                                    self.events_out.send(Event::NewFrontier(frontier)).unwrap();
                                },
                                Err(e) => info!("vertex {} (height = {}) not inserted: {e}", vertex.hash().to_hex(), vertex.height),
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

    /// Get a set of validators to query preferences on a new [`Vertex`]
    fn get_validators_for_query(&self) -> Result<HashSet<PeerId>> {
        if self.validators.len() < self.config.query_count {
            Err(Error::NeedValidators(
                self.config.query_count,
                self.validators.len(),
            ))
        } else {
            let mut rng = rand::thread_rng();
            Ok(
                iter::repeat_with(|| rng.gen_range(0..self.validators.len()))
                    .unique()
                    .take(self.config.query_count)
                    .map(|i| *self.validators.get_index(i).unwrap())
                    .collect(),
            )
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Config, Task};
    use crate::{consensus, dag, p2p};
    use libp2p::PeerId;
    use std::{assert_matches::assert_matches, str::FromStr};
    use tokio::sync::{self, mpsc};

    #[test]
    fn new_task() {
        let (dummy_action_ch, _) = mpsc::unbounded_channel();
        let (dummy_event_ch, _) = sync::broadcast::channel(1);
        let p2p_api = p2p::api::P2pApi::new(dummy_action_ch, dummy_event_ch);

        // Default case
        assert_matches!(
            Task::new(
                Config {
                    ..Config::default()
                },
                p2p_api.clone()
            ),
            Ok((_, _))
        );

        // Invalid task configs
        assert_matches!(
            Task::new(
                Config {
                    query_count: 10,
                    quorum_size: 11,
                    ..Config::default()
                },
                p2p_api.clone()
            ),
            Err(consensus::task::Error::BadCfgQuorumCount)
        );

        // Invalid dag configs
        assert_matches!(
            Task::new(
                Config {
                    dag: dag::Config {
                        thresh_safe_early_commit: 5,
                        thresh_accepted: 4,
                        ..dag::Config::default()
                    },
                    ..Config::default()
                },
                p2p_api.clone()
            ),
            Err(consensus::task::Error::DAG(dag::Error::BadCfgThreshold))
        );
    }

    #[test]
    fn get_validators_for_query() {
        let (dummy_action_ch, _) = mpsc::unbounded_channel();
        let (dummy_event_ch, _) = sync::broadcast::channel(1);
        let p2p_api = p2p::api::P2pApi::new(dummy_action_ch, dummy_event_ch);
        let (mut task, _) = Task::new(
            Config {
                query_count: 2,
                quorum_size: 1,
                ..Config::default()
            },
            p2p_api,
        )
        .unwrap();

        assert_matches!(
            task.get_validators_for_query(),
            Err(consensus::task::Error::NeedValidators(2, 0)),
        );
        task.validators.insert(
            PeerId::from_str("12D3KooWNNNsXcxFeHuM4FNCG8pBKCXMgQ6UH35S7dseLiwFkukF").unwrap(),
        );
        assert_matches!(
            task.get_validators_for_query(),
            Err(consensus::task::Error::NeedValidators(2, 1)),
        );
        task.validators.insert(
            PeerId::from_str("12D3KooWCXboa8yR8vGyhgfWzv3peUUJ8qz5jRGXiPiZ8EeWPVmE").unwrap(),
        );
        assert_matches!(task.get_validators_for_query(), Ok(_));
        task.validators.insert(
            PeerId::from_str("12D3KooWJieUiwMjNbn2m2yu1KhzfaU6trWAL951d5XMBWqZ13EE").unwrap(),
        );
        assert_matches!(task.get_validators_for_query(), Ok(_));
    }
}
