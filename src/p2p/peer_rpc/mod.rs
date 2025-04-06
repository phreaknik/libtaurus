mod generated;
mod protocol;

use self::proto::rpc_messages::{self, Request};
use self::protocol::{PeerRpcCodec, PeerRpcProtocol};
use libp2p::core::Endpoint;
use libp2p::request_response::{Message, ProtocolSupport};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p::{request_response, Multiaddr, PeerId};
use std::iter;
use std::task::{Context, Poll};
use thiserror;
use tracing::trace;

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {}

/// Error type for peer RPC errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// [`NetworkBehaviour`] to implement the peer RPC message routing
pub struct Behaviour {
    /// Inner behaviour for sending requests and receiving the response.
    inner: request_response::Behaviour<PeerRpcCodec>,
    /// Configuration parameters
    _config: Config,
}

impl<'a> Behaviour {
    pub fn new(_config: Config) -> Self {
        let inner_protocols =
            iter::once((PeerRpcProtocol::new(_config.clone()), ProtocolSupport::Full));
        let inner_config = request_response::Config::default();
        Behaviour {
            inner: request_response::Behaviour::new(PeerRpcCodec, inner_protocols, inner_config),
            _config,
        }
    }

    /// Handle messages passed up from the request_response behaviour
    fn handle_message(
        &mut self,
        peer_id: PeerId,
        message: Message<rpc_messages::Request, rpc_messages::Response>,
    ) -> Poll<SwarmAction> {
        // Process any response to our request, and remove the request from the
        // ongoing_outbound queue
        match message {
            request_response::Message::Request { request, .. } => {
                trace!("Received peer_rpc request from {peer_id}: {request:?}");
                //match self
                //    .inner
                //    .send_response(channel, rpc_messages::Response { .. }.into())
                //{
                //    Err(_) => Poll::Ready(SwarmAction::GenerateEvent(Event::ResponseFailed)),
                //    Ok(_) => Poll::Ready(ToSwarm::GenerateEvent(Event::ProcessedPeerRequest)),
                //}
                Poll::Pending
            }
            request_response::Message::Response { response, .. } => {
                trace!("Received peer_rpc response from {peer_id}: {response:?}");
                //Poll::Ready(ToSwarm::GenerateEvent(Event::Response((
                //    peer_id,
                //    response
                //        .proto()
                //        .peers
                //        .clone()
                //        .into_iter()
                //        .filter_map(|p| p.try_into().ok())
                //        .collect(),
                //))))
                Poll::Pending
            }
        }
    }

    pub fn send_request(&mut self, peer: &PeerId, request: Request) {
        self.inner.send_request(peer, request);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler =
        <request_response::Behaviour<PeerRpcCodec> as NetworkBehaviour>::ConnectionHandler;
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

/// Configuration for the ['peer_rpc::Behaviour'](Behaviour)
#[derive(Debug, Clone)]
pub struct Config {}

#[allow(clippy::derive_partial_eq_without_eq)]
pub mod proto {
    include!("generated/mod.rs");
}
