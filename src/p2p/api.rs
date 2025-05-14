use super::{broadcast::BroadcastValidationReport, Action, Event, Request, Response};
use crate::Vertex;
use libp2p::{request_response::InboundRequestId, PeerId};
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
        self.p2p_action_ch
            .send(Action::Broadcast(vx.clone().into()))?;
        Ok(())
    }

    /// Request data from our peer. Optionally, provide a peer to request from.
    pub fn request(
        &self,
        request: Request,
        resp_ch: mpsc::UnboundedSender<Response>,
        opt_peer: Option<PeerId>,
    ) -> Result<()> {
        self.p2p_action_ch.send(Action::Request {
            request,
            opt_peer,
            resp_ch,
        })?;
        Ok(())
    }

    pub fn respond(&self, request_id: InboundRequestId, response: Response) -> Result<()> {
        self.p2p_action_ch.send(Action::Respond {
            request_id,
            response,
        })?;
        Ok(())
    }

    /// Block the specified peer
    pub fn block_peer(&self, peer: PeerId) -> Result<()> {
        self.p2p_action_ch.send(Action::BlockPeer(peer))?;
        Ok(())
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
