use super::{Action, Event};
use crate::{consensus, Vertex, VertexHash, WireFormat};
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
    Consensus(#[from] consensus::Error),
    #[error(transparent)]
    TimerElapsed(#[from] tokio::time::error::Elapsed),
    #[error(transparent)]
    ResponseRecv(#[from] oneshot::error::RecvError),
}
type Result<T> = result::Result<T, Error>;

const DEFAULT_TIMEOUT: u64 = 60;

/// API wrapper to communicate with the consensus process
pub struct ConsensusApi {
    timeout: u64,
    consensus_action_ch: mpsc::UnboundedSender<Action>,
    consensus_event_ch: broadcast::Receiver<Event>,
}

impl ConsensusApi {
    /// Construct a new instance of the [`ConsensusApi`]
    pub fn new(
        consensus_action_ch: mpsc::UnboundedSender<Action>,
        consensus_event_ch: broadcast::Receiver<Event>,
    ) -> ConsensusApi {
        ConsensusApi {
            timeout: DEFAULT_TIMEOUT,
            consensus_action_ch,
            consensus_event_ch,
        }
    }

    /// Get a subscription handler for events
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.consensus_event_ch.resubscribe()
    }

    /// Get the accepted frontier of the DAG
    pub async fn get_frontier(&self) -> Result<Vec<Arc<Vertex>>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.consensus_action_ch
            .send(Action::GetAcceptedFrontier { result_ch: resp_tx })?;
        Ok(timeout(Duration::from_secs(self.timeout), resp_rx).await??)
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

    /// Try to insert the given [`Vertex`] into the [`DAG`]
    pub async fn submit_vertex(&self, vx: &Arc<Vertex>) -> Result<HashSet<VertexHash>> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.consensus_action_ch.send(Action::SubmitVertex {
            vertex: vx.clone(),
            result_ch: resp_tx,
        })?;
        Ok(timeout(Duration::from_secs(self.timeout), resp_rx).await???)
    }
}

impl Clone for ConsensusApi {
    fn clone(&self) -> Self {
        ConsensusApi {
            timeout: self.timeout,
            consensus_action_ch: self.consensus_action_ch.clone(),
            consensus_event_ch: self.consensus_event_ch.resubscribe(),
        }
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
