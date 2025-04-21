mod message;
mod protocol;

use self::protocol::{ConsensusRpcCodec, ConsensusRpcProtocol};
use crate::consensus::{block, vertex};
use crate::wire;
use cached::{Cached, TimedCache};
use libp2p::core::Endpoint;
use libp2p::request_response::{InboundRequestId, ProtocolSupport, ResponseChannel};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
    THandlerOutEvent, ToSwarm,
};
use libp2p::{request_response, Multiaddr, PeerId};
pub use message::{Request, Response};
use std::iter;
use std::task::{Context, Poll};
use thiserror;
use tracing::{error, trace, warn};

/// How long before a pending request times out
const REQUEST_TIMEOUT_SECS: u64 = 60;

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    Requested(PeerId, InboundRequestId, Request),
    Responded(PeerId, Response),
}

/// Error type for peer RPC errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Block(#[from] block::Error),
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error("request message is missing data")]
    IncompleteRequest,
    #[error("response message is missing data")]
    IncompleteResponse,
    #[error(transparent)]
    Protobuf(#[from] quick_protobuf::Error),
    #[error(transparent)]
    Vertex(#[from] vertex::Error),
    #[error(transparent)]
    Wire(#[from] wire::Error),
}

/// [`NetworkBehaviour`] to implement the peer RPC message routing
pub struct Behaviour {
    /// Inner behaviour for sending requests and receiving the response.
    inner: request_response::Behaviour<ConsensusRpcCodec>,
    /// Configuration parameters
    _config: Config,
    /// Map of response channels for pending requests
    pending: TimedCache<InboundRequestId, ResponseChannel<Response>>,
}

impl<'a> Behaviour {
    pub fn new(_config: Config) -> Self {
        let inner_protocols = iter::once((
            ConsensusRpcProtocol::new(_config.clone()),
            ProtocolSupport::Full,
        ));
        let inner_config = request_response::Config::default();
        Behaviour {
            inner: request_response::Behaviour::new(inner_protocols, inner_config),
            _config,
            pending: TimedCache::with_lifespan(REQUEST_TIMEOUT_SECS),
        }
    }

    /// Handle messages passed up from the request_response behaviour
    fn handle_message(
        &mut self,
        peer_id: PeerId,
        message: request_response::Message<Request, Response>,
    ) -> Poll<SwarmAction> {
        // Process any response to our request, and remove the request from the
        // ongoing_outbound queue
        match message {
            request_response::Message::Request {
                request_id,
                request,
                channel,
            } => {
                trace!("Received consensus_rpc request from {peer_id}: {request:?}");
                // Save the response channel, so we can forward the response later
                self.pending.cache_set(request_id, channel);
                Poll::Ready(ToSwarm::GenerateEvent(Event::Requested(
                    peer_id, request_id, request,
                )))
            }
            request_response::Message::Response { response, .. } => {
                trace!("Received consensus_rpc response from {peer_id}: {response:?}");
                Poll::Ready(ToSwarm::GenerateEvent(Event::Responded(peer_id, response)))
            }
        }
    }

    pub fn send_request(&mut self, peer: &PeerId, request: Request) {
        self.inner.send_request(peer, request);
    }

    pub fn send_response(&mut self, request_id: InboundRequestId, response: Response) {
        if let Some(ch) = self.pending.cache_remove(&request_id) {
            if let Err(_) = self.inner.send_response(ch, response) {
                warn!("error responding to avalanche request");
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <request_response::Behaviour<ConsensusRpcCodec> as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<SwarmAction> {
        match self.inner.poll(cx) {
            Poll::Ready(ToSwarm::GenerateEvent(request_response::Event::Message {
                peer,
                message,
            })) => self.handle_message(peer, message),
            Poll::Ready(ToSwarm::GenerateEvent(_e)) => Poll::Pending, // Trap all sub events
            Poll::Ready(action) => Poll::Ready(action.map_out(|_| unreachable!())),
            Poll::Pending => Poll::Pending,
        }
    }

    fn handle_pending_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<(), ConnectionDenied> {
        self.inner
            .handle_pending_inbound_connection(connection_id, local_addr, remote_addr)
    }

    fn handle_established_inbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        local_addr: &Multiaddr,
        remote_addr: &Multiaddr,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner.handle_established_inbound_connection(
            connection_id,
            peer,
            local_addr,
            remote_addr,
        )
    }

    fn handle_pending_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        maybe_peer: Option<PeerId>,
        addresses: &[Multiaddr],
        effective_role: Endpoint,
    ) -> Result<Vec<Multiaddr>, ConnectionDenied> {
        self.inner.handle_pending_outbound_connection(
            connection_id,
            maybe_peer,
            addresses,
            effective_role,
        )
    }

    fn handle_established_outbound_connection(
        &mut self,
        connection_id: ConnectionId,
        peer: PeerId,
        addr: &Multiaddr,
        role_override: Endpoint,
    ) -> Result<THandler<Self>, ConnectionDenied> {
        self.inner
            .handle_established_outbound_connection(connection_id, peer, addr, role_override)
    }

    fn on_swarm_event(&mut self, event: FromSwarm) {
        self.inner.on_swarm_event(event);
    }

    fn on_connection_handler_event(
        &mut self,
        peer_id: PeerId,
        connection_id: ConnectionId,
        event: THandlerOutEvent<Self>,
    ) {
        self.inner
            .on_connection_handler_event(peer_id, connection_id, event)
    }
}

type SwarmAction<'a> =
    ToSwarm<<Behaviour as NetworkBehaviour>::ToSwarm, THandlerInEvent<Behaviour>>;

/// Configuration for the [`consensus_rpc::Behaviour`](Behaviour)
#[derive(Debug, Clone, Default)]
pub struct Config {}
