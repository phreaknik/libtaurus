mod message;
mod protocol;

use self::protocol::{AvalancheRpcCodec, AvalancheRpcProtocol};
use libp2p::core::Endpoint;
use libp2p::request_response::{ProtocolSupport, RequestId, ResponseChannel};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p::{request_response, Multiaddr, PeerId};
pub use message::{Request, Response};
use std::collections::HashMap;
use std::iter;
use std::task::{Context, Poll};
use thiserror;
use tracing::{trace, warn};

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    Requested(RequestId, Request),
    Responded(PeerId, Response),
}

/// Error type for peer RPC errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// [`NetworkBehaviour`] to implement the peer RPC message routing
pub struct Behaviour {
    /// Inner behaviour for sending requests and receiving the response.
    inner: request_response::Behaviour<AvalancheRpcCodec>,
    /// Configuration parameters
    _config: Config,
    /// Map of response channels for pending requests
    pending: HashMap<RequestId, ResponseChannel<Response>>,
}

impl<'a> Behaviour {
    pub fn new(_config: Config) -> Self {
        let inner_protocols = iter::once((
            AvalancheRpcProtocol::new(_config.clone()),
            ProtocolSupport::Full,
        ));
        let inner_config = request_response::Config::default();
        Behaviour {
            inner: request_response::Behaviour::new(
                AvalancheRpcCodec,
                inner_protocols,
                inner_config,
            ),
            _config,
            pending: HashMap::new(),
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
                trace!("Received avalanche_rpc request from {peer_id}: {request:?}");
                // Save the response channel, so we can forward the response later
                self.pending.insert(request_id, channel);
                Poll::Ready(ToSwarm::GenerateEvent(Event::Requested(
                    request_id, request,
                )))
            }
            request_response::Message::Response { response, .. } => {
                trace!("Received avalanche_rpc response from {peer_id}: {response:?}");
                Poll::Ready(ToSwarm::GenerateEvent(Event::Responded(peer_id, response)))
            }
        }
    }

    pub fn send_request(&mut self, peer: &PeerId, request: Request) {
        self.inner.send_request(peer, request);
    }

    pub fn send_response(&mut self, request_id: RequestId, response: Response) {
        if let Some(ch) = self.pending.remove(&request_id) {
            if let Err(_) = self.inner.send_response(ch, response) {
                warn!("error responding to avalanche request");
            }
        }
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <request_response::Behaviour<AvalancheRpcCodec> as NetworkBehaviour>::ConnectionHandler;
    type OutEvent = Event;

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<SwarmAction> {
        match self.inner.poll(cx, params) {
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

    fn on_swarm_event(&mut self, event: FromSwarm<Self::ConnectionHandler>) {
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
    ToSwarm<<Behaviour as NetworkBehaviour>::OutEvent, THandlerInEvent<Behaviour>>;

/// Configuration for the ['avalanche_rpc::Behaviour'](Behaviour)
#[derive(Debug, Clone)]
pub struct Config {}
