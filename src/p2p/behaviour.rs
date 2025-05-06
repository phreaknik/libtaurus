use super::{consensus_rpc, BroadcastData, Event, PeerDatabase, PeerInfo};
use crate::wire::WireFormat;
use libp2p::{
    allow_block_list,
    core::Endpoint,
    gossipsub::{self, MessageAcceptance, MessageAuthenticity, MessageId, Sha256Topic},
    identity::Keypair,
    kad::{self, store::MemoryStore, NoKnownPeers},
    multiaddr::Protocol,
    request_response::InboundRequestId,
    swarm::{
        ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, THandler, THandlerInEvent,
        THandlerOutEvent, ToSwarm,
    },
    upnp, {identify, Multiaddr, PeerId},
};
use std::{
    borrow::BorrowMut,
    str,
    task::{Context, Poll},
    time::Duration,
};
use strum::IntoEnumIterator;
use tracing::{debug, error, trace, warn};

pub const PROTOCOL_NAME: &[u8; 13] = b"/taurus/0.1.0";

/// Kademlia query timeout in seconds
const DEFAULT_KAD_QUERY_TIMOUT: Duration = Duration::from_secs(60);

/// Aggregate behaviour for all subprotocols used by the taurus behaviour
#[derive(NetworkBehaviour)]
pub(super) struct InnerBehaviour {
    identify: identify::Behaviour,
    kademlia: kad::Behaviour<MemoryStore>,
    gossipsub: gossipsub::Behaviour,
    consensus_rpc: consensus_rpc::Behaviour,
    block_lists: allow_block_list::Behaviour<allow_block_list::BlockedPeers>,
    upnp: upnp::tokio::Behaviour,
}

impl<'a> InnerBehaviour {
    fn new(config: Config) -> crate::p2p::Result<Self> {
        let local_peer_id = PeerId::from_public_key(&config.keys.public());
        let mut gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(config.keys),
            config.gossipsub_cfg,
        )
        .unwrap();
        for m in BroadcastData::iter() {
            let topic = Sha256Topic::from(&m);
            debug!("Subscribed to {topic} topic");
            gossipsub.subscribe(&topic)?;
        }
        Ok(InnerBehaviour {
            identify: identify::Behaviour::new(config.identify_cfg),
            kademlia: kad::Behaviour::with_config(
                local_peer_id,
                MemoryStore::new(local_peer_id),
                config.kad_cfg,
            ),
            gossipsub,
            consensus_rpc: consensus_rpc::Behaviour::new(config.consensus_rpc_cfg),
            block_lists: allow_block_list::Behaviour::default(),
            upnp: upnp::tokio::Behaviour::default(),
        })
    }
}

/// Network behavior that implements the taurus p2p protocol
pub(super) struct Behaviour {
    /// Sub protocols of taurus-p2p
    inner: InnerBehaviour,
    /// Peer database
    peer_db: PeerDatabase,
}

impl<'a> Behaviour {
    /// Create a new instance of the taurus-p2p [`Behaviour`].
    pub fn new(config: Config, peer_db: PeerDatabase) -> crate::p2p::Result<Self> {
        let mut b = Behaviour {
            peer_db,
            inner: InnerBehaviour::new(config.clone())?,
        };
        b.add_boot_peers(&config);
        Ok(b)
    }

    /// Publish message to gossipsub network
    pub fn publish(&mut self, message: BroadcastData) -> crate::p2p::Result<MessageId> {
        self.inner
            .gossipsub
            .publish(Sha256Topic::from(&message), message.to_wire(true)?)
            .map_err(crate::p2p::Error::from)
    }

    /// Report back if the gossipped message should be propagated/ignored/rejected
    pub fn report_message_validation_result(
        &mut self,
        msg_id: &MessageId,
        propagation_source: &PeerId,
        acceptance: MessageAcceptance,
    ) {
        if let Err(e) = self.inner.gossipsub.report_message_validation_result(
            msg_id,
            propagation_source,
            acceptance,
        ) {
            warn!("Error updating gossipsub message status: {e}");
        }
    }

    /// Add boot peers to dial and bootstrap into the P2P network. The list of boot peers will
    /// include both explicitely provided boot peers in the config, as well as previously reachable
    /// peers saved in the peer database.
    pub fn add_boot_peers(&mut self, config: &Config) {
        let rtxn = self.peer_db.env.read_txn().unwrap();
        config
            .boot_peers
            .clone()
            .into_iter()
            .filter_map(|mut addr| {
                if let Protocol::P2p(peer_id) = addr.pop().unwrap() {
                    Some((peer_id, vec![addr]))
                } else {
                    warn!("Skipping incomplete boot peer address: {addr}");
                    None
                }
            })
            .chain(
                // Chain the peer addresses of all saved peers
                self.peer_db
                    .db
                    .iter(&rtxn)
                    .expect("Failed to access peer database")
                    .filter_map(|entry| match entry {
                        Ok((peer_db_key, info)) => Some((peer_db_key.as_peerid(), info.addresses)),
                        Err(e) => {
                            error!("Error reading peer entry at boot: {e}");
                            None
                        }
                    }),
            )
            .for_each(|(peer, addrs)| {
                for address in addrs {
                    let _ = self.inner.borrow_mut().kademlia.add_address(&peer, address);
                }
            });
    }

    /// Perform any handling logic for a peer which we tried, but failed to reach
    pub fn handle_unreachable_peer(&mut self, peer_id: &PeerId) {
        let mut wtxn = self.peer_db.env.write_txn().unwrap();
        if let Ok(Some(info)) = self.peer_db.db.get(&wtxn, &peer_id.into()) {
            for address in info.addresses {
                self.inner.kademlia.remove_address(peer_id, &address);
            }
            if let Err(e) = self.peer_db.db.delete(&mut wtxn, &peer_id.into()) {
                warn!("Failed to delete unreachable peer {peer_id}: {e}");
            }
        }
        wtxn.commit().unwrap();
    }

    /// Block and refuse all connections to the specified peer.
    pub fn block_peer(&mut self, peer: PeerId) {
        self.inner.block_lists.block_peer(peer)
    }

    /// Send a request to a peer via the [`consensus_rpc`] protocol
    pub fn avalanche_request(&mut self, peer: &PeerId, request: consensus_rpc::Request) {
        trace!("Sending request: {request}");
        self.inner.consensus_rpc.send_request(peer, request)
    }

    /// Respond to a received avalanche request, via the [`consensus_rpc`] protocol
    pub fn avalanche_response(
        &mut self,
        request_id: InboundRequestId,
        response: consensus_rpc::Response,
    ) {
        trace!("Sending response: {response:?}");
        self.inner.consensus_rpc.send_response(request_id, response)
    }

    /// Handle peer identification events
    fn handle_identify_event(&mut self, peer_id: PeerId, info: identify::Info) {
        // Add the peer to the kademlia routing table
        for address in info.listen_addrs.clone() {
            self.inner.kademlia.add_address(&peer_id, address);
        }
        let mut wtxn = self.peer_db.env.write_txn().unwrap();
        // Save the peer info to the peer database
        self.peer_db
            .db
            .put(&mut wtxn, &peer_id.into(), &PeerInfo::from(info))
            .expect("Failed to write to peer database");
        wtxn.commit().unwrap();
    }

    /// Bootstrap into the P2P network
    pub fn bootstrap(&mut self) {
        if let Err(NoKnownPeers {}) = self.inner.kademlia.bootstrap() {
            warn!("No peers. Unable to join DHT.");
        }
    }
}

impl<'a> NetworkBehaviour for Behaviour {
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type ToSwarm = Event;

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<SwarmAction> {
        // Handle any events from the subprotocols
        match self.inner.poll(cx) {
            //  Handle received identities
            Poll::Ready(ToSwarm::GenerateEvent(InnerBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info },
            ))) => {
                self.handle_identify_event(peer_id, info);
                Poll::Pending
            }
            // Forward pubsub events out
            Poll::Ready(ToSwarm::GenerateEvent(InnerBehaviourEvent::Gossipsub(
                message @ gossipsub::Event::Message { .. },
            ))) => Poll::Ready(ToSwarm::GenerateEvent(Event::Pubsub(
                message.try_into().unwrap(),
            ))),
            // Trap all other generated events
            Poll::Ready(ToSwarm::GenerateEvent(event)) => {
                trace!("internal event: {event:?}");
                Poll::Pending
            }
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

/// Configuration for the [`taurus::Behaviour`](Behaviour).
#[derive(Clone)]
pub(super) struct Config {
    /// Keypair used for signing messages
    keys: Keypair,
    /// Identify protocol configuration
    identify_cfg: identify::Config,
    /// Kademlia protocol configuration
    kad_cfg: kad::Config,
    /// Gossipsub protocol configuration
    gossipsub_cfg: gossipsub::Config,
    /// Peer RPC protocol configuration
    consensus_rpc_cfg: consensus_rpc::Config,
    /// Bootstrap nodes to join the P2P network
    boot_peers: Vec<Multiaddr>,
}

impl Config {
    pub fn new(keys: Keypair, boot_peers: Vec<Multiaddr>) -> Self {
        let mut kad_cfg = kad::Config::default();
        kad_cfg.set_query_timeout(DEFAULT_KAD_QUERY_TIMOUT);
        let pubkey = keys.public();
        let gossipsub_cfg = gossipsub::ConfigBuilder::default()
            .validate_messages() // We mus accept every message before its allowed to propagate
            .build()
            .expect("Failed to build gossipsub config");
        Config {
            keys,
            identify_cfg: identify::Config::new(
                str::from_utf8(PROTOCOL_NAME).unwrap().to_string(),
                pubkey,
            ),
            kad_cfg,
            consensus_rpc_cfg: consensus_rpc::Config {},
            gossipsub_cfg,
            boot_peers,
        }
    }
}
