use super::{Action, BroadcastValidationReport, Event};
use crate::Vertex;
use std::{result, sync::Arc};
use tokio::sync::{broadcast, mpsc, oneshot};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ActionSend(#[from] mpsc::error::SendError<Action>),
    #[error(transparent)]
    ResponseRecv(#[from] oneshot::error::RecvError),
}
type Result<T> = result::Result<T, Error>;

const DEFAULT_TIMEOUT: u64 = 60;

/// API wrapper to communicate with the P2P process
pub struct P2pApi {
    timeout: u64,
    p2p_action_ch: mpsc::UnboundedSender<Action>,
    p2p_event_ch: broadcast::Receiver<Event>,
}

impl P2pApi {
    /// Construct a new instance of the [`P2pApi`]
    pub fn new(
        p2p_action_ch: mpsc::UnboundedSender<Action>,
        p2p_event_ch: broadcast::Receiver<Event>,
    ) -> P2pApi {
        P2pApi {
            timeout: DEFAULT_TIMEOUT,
            p2p_action_ch,
            p2p_event_ch,
        }
    }

    /// Get a subscription handler for events
    pub fn subscribe_events(&self) -> broadcast::Receiver<Event> {
        self.p2p_event_ch.resubscribe()
    }

    /// Indicate the validity of a message received from p2p
    pub fn report_message_validity(&self, validation: BroadcastValidationReport) -> Result<()> {
        self.p2p_action_ch
            .send(Action::ReportMessageValidity(validation))?;
        Ok(())
    }

    /// Publish a new [`Vertex`] to the GossipSub network
    pub fn submit_vertex(&self, vx: &Arc<Vertex>) -> Result<()> {
        Ok(self
            .p2p_action_ch
            .send(Action::Broadcast(vx.clone().into()))?)
    }
}

impl Clone for P2pApi {
    fn clone(&self) -> Self {
        P2pApi {
            timeout: self.timeout,
            p2p_action_ch: self.p2p_action_ch.clone(),
            p2p_event_ch: self.p2p_event_ch.resubscribe(),
        }
    }
}

// TODO: need tests
