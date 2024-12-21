use super::task::{self, Action, Event};
use crate::{Vertex, VertexHash, WireFormat};
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, result, sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, mpsc, oneshot},
    time::timeout,
};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ActionSend(#[from] mpsc::error::SendError<Action>),
    #[error(transparent)]
    Task(#[from] task::Error),
    #[error(transparent)]
    TimerElapsed(#[from] tokio::time::error::Elapsed),
    #[error(transparent)]
    ResponseRecv(#[from] oneshot::error::RecvError),
}
type Result<T> = result::Result<T, Error>;

const DEFAULT_TIMEOUT: u64 = 60;

/// API wrapper to communicate with the consensus process
#[derive(Debug, Clone)]
pub struct ConsensusApi {
    timeout: u64,
    consensus_action_ch: mpsc::UnboundedSender<Action>,
    consensus_event_sender: broadcast::Sender<Event>,
}

impl ConsensusApi {
    /// Construct a new instance of the [`ConsensusApi`]. `consensus_action_ch` is the channel which
    /// can be used to request an action from the consensus task. `consensus_event_sender` is
    /// the send handle to the consensus event broadcast channel, but is only used as a handle
    /// to create new subscribers on demand.
    pub fn new(
        consensus_action_ch: mpsc::UnboundedSender<Action>,
        consensus_event_sender: broadcast::Sender<Event>,
    ) -> ConsensusApi {
        ConsensusApi {
            timeout: DEFAULT_TIMEOUT,
            consensus_action_ch,
            consensus_event_sender,
        }
    }

    /// Get a subscription handler for events
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.consensus_event_sender.subscribe()
    }

    /// Add a peer to the validator set
    pub fn add_validator(&self, validator: PeerId) -> Result<()> {
        self.consensus_action_ch
            .send(Action::AddValidator(validator))?;
        Ok(())
    }

    /// Look up the specified vertex
    pub async fn get_vertex(&self, vhash: VertexHash) -> Result<Option<Arc<Vertex>>> {
        let (resp_sender, resp_receiver) = oneshot::channel();
        self.consensus_action_ch.send(Action::GetVertex {
            vhash,
            result_ch: resp_sender,
        })?;
        Ok(timeout(Duration::from_secs(self.timeout), resp_receiver).await??)
    }

    /// Get the accepted frontier of the DAG
    pub async fn get_frontier(&self) -> Result<Vec<Arc<Vertex>>> {
        let (resp_sender, resp_receiver) = oneshot::channel();
        self.consensus_action_ch.send(Action::GetAcceptedFrontier {
            result_ch: resp_sender,
        })?;
        Ok(timeout(Duration::from_secs(self.timeout), resp_receiver).await??)
    }

    /// Get metadata or the accepted frontier
    pub async fn get_frontier_meta(&self) -> Result<Vec<VertexMeta>> {
        Ok(self
            .get_frontier()
            .await?
            .iter()
            .map(|vx| vx.as_ref().into())
            .collect())
    }

    /// Get the preference of the specified [`Vertex`]
    pub async fn get_preference(&self, vhash: VertexHash) -> Result<Option<bool>> {
        let (resp_sender, resp_receiver) = oneshot::channel();
        self.consensus_action_ch.send(Action::GetPreference {
            vhash,
            result_ch: resp_sender,
        })?;
        Ok(timeout(Duration::from_secs(self.timeout), resp_receiver)
            .await??
            .map(|(pref, _final)| pref))
    }

    /// Try to insert the given [`Vertex`] into the [`DAG`]
    pub async fn submit_vertex(&self, vx: &Arc<Vertex>) -> Result<HashSet<VertexHash>> {
        let (resp_sender, resp_receiver) = oneshot::channel();
        self.consensus_action_ch.send(Action::SubmitVertex {
            vertex: vx.clone(),
            result_ch: resp_sender,
        })?;
        Ok(timeout(Duration::from_secs(self.timeout), resp_receiver).await???)
    }

    /// Record the peer preference for the specified vertex
    pub fn record_peer_preference(&self, vhash: VertexHash, preference: bool) -> Result<()> {
        self.consensus_action_ch
            .send(Action::RecordPeerPreference { vhash, preference })?;
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct VertexMeta {
    pub hash: VertexHash,
    pub height: u64,
}

impl From<&Vertex> for VertexMeta {
    fn from(vx: &Vertex) -> Self {
        VertexMeta {
            hash: vx.hash(),
            height: vx.height,
        }
    }
}

// TODO: need tests
