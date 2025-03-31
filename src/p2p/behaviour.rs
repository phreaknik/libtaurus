use crate::p2p::message::Message;
use crate::p2p::peer_db::{self, PeerDB, PeerInfo};
use crate::p2p::Event;
use libp2p::core::Endpoint;
use libp2p::gossipsub::{self, MessageAuthenticity, MessageId};
use libp2p::identity::Keypair;
use libp2p::kad::store::MemoryStore;
use libp2p::kad::{Kademlia, KademliaConfig, NoKnownPeers};
use libp2p::swarm::{
    ConnectionDenied, ConnectionId, FromSwarm, NetworkBehaviour, PollParameters, THandler,
    THandlerInEvent, THandlerOutEvent, ToSwarm,
};
use libp2p::{identify, Multiaddr, PeerId};
use std::borrow::BorrowMut;
use std::path::PathBuf;
use std::str;
use std::task::{Context, Poll};
use std::time::Duration;
use strum::IntoEnumIterator;
use tracing::{error, info, trace, warn};

pub const PROTOCOL_NAME: &[u8; 15] = b"/cordelia/0.1.0";

/// Kademlia query timeout in seconds
const DEFAULT_KAD_QUERY_TIMOUT: Duration = Duration::from_secs(60);

/// Aggregate behaviour for all subprotocols used by the cordelia behaviour
#[derive(NetworkBehaviour)]
pub struct InnerBehaviour {
    identify: identify::Behaviour,
    kademlia: Kademlia<MemoryStore>,
    gossipsub: gossipsub::Behaviour,
}

impl InnerBehaviour {
    fn new(config: Config) -> crate::p2p::Result<Self> {
        let local_peer_id = PeerId::from_public_key(&config.keys.public());
        let mut gossipsub = gossipsub::Behaviour::new(
            MessageAuthenticity::Signed(config.keys),
            config.gossipsub_cfg,
        )
        .unwrap();
        for t in Message::iter() {
            info!("subscribed to topic {}", t.topic().hash());
            gossipsub.subscribe(&t.topic())?;
        }
        Ok(InnerBehaviour {
            identify: identify::Behaviour::new(config.identify_cfg),
            kademlia: Kademlia::with_config(
                local_peer_id,
                MemoryStore::new(local_peer_id),
                config.kad_cfg,
            ),
            gossipsub,
        })
    }
}

/// Network behavior that implements the cordelia p2p protocol
pub struct Behaviour {
    /// Sub protocols of cordelia-p2p
    inner: InnerBehaviour,
    /// Peer database
    peer_db: PeerDB,
}

impl Behaviour {
    /// Create a new instance of the cordelia-p2p ['Behaviour'].
    pub fn new(config: Config) -> crate::p2p::Result<Self> {
        let mut b = Behaviour {
            peer_db: PeerDB::create_or_open(&config.peer_db_path)?,
            inner: InnerBehaviour::new(config.clone())?,
        };
        b.add_boot_nodes(&config);
        match b.inner.kademlia.bootstrap() {
            Err(NoKnownPeers {}) => {
                warn!("No peers. Unable go join P2P network.");
                Ok(b)
            }
            Ok(_) => Ok(b),
        }
    }

    /// Handle peer identification events
    pub fn handle_identify_event(&mut self, peer_id: PeerId, info: identify::Info) {
        // Save the peer to PeerDB
        match self.peer_db.read_peer_info(&peer_id) {
            Ok(_) => {}
            Err(peer_db::Error::EntryNotFound) => match self.peer_db.write_peer_info(&PeerInfo {
                peer_id,
                protocol_version: info.protocol_version,
                agent_version: info.agent_version,
                addresses: info.listen_addrs.clone(),
            }) {
                Ok(_) => {}
                Err(e) => {
                    warn!("Failed to add new peer: {e}");
                }
            },
            Err(e) => {
                error!("Error accessing peer_db: {e}");
            }
        }
        // Add the peer to the kademlia routing table
        for address in info.listen_addrs {
            self.inner.kademlia.add_address(&peer_id, address);
        }
    }

    /// Publish message to gossipsub network
    pub fn publish(&mut self, message: Message) -> crate::p2p::Result<MessageId> {
        self.inner
            .gossipsub
            .publish(message.topic(), message.data())
            .map_err(crate::p2p::Error::from)
    }

    /// Add bootnodes to dial and bootstrap into the P2P network.
    /// The list of bootnodes will include both explicitely provided bootnodes in the config, as
    /// well as previously reachable peers saved in the peer database.
    pub fn add_boot_nodes(&mut self, config: &Config) {
        config
            .boot_nodes
            .clone()
            .into_iter()
            .map(|addr| {
                let peer_id =
                    PeerId::try_from_multiaddr(&addr).expect("Static peer has invalid multiaddr");
                (peer_id, vec![addr])
            })
            .chain(
                // Chain the peer addresses of all saved peers
                self.peer_db
                    .list_peers()
                    .unwrap()
                    .into_iter()
                    .map(|peer_id| (peer_id, self.peer_db.read_peer_info(&peer_id).ok()))
                    .inspect(|(peer_id, info)| {
                        if info.is_none() {
                            warn!("Listed peer doesn't exist in database: {peer_id}");
                        }
                    })
                    .filter_map(|(peer, info)| match info {
                        // Skip any peers we couldn't read from the db
                        Some(info) => Some((peer, info)),
                        None => None,
                    })
                    .map(|(peer, info)| (peer, info.addresses)),
            )
            .for_each(|(peer, addrs)| {
                for address in addrs {
                    let _ = self.inner.borrow_mut().kademlia.add_address(&peer, address);
                }
            });
    }

    /// Perform any handling logic for a peer which we tried, but failed to reach
    pub fn handle_unreachable_peer(&mut self, peer_id: &PeerId) {
        let _ = self.peer_db.delete_peer_info(peer_id);
    }
}

impl NetworkBehaviour for Behaviour {
    type ConnectionHandler = <InnerBehaviour as NetworkBehaviour>::ConnectionHandler;
    type OutEvent = Event;

    fn poll(
        &mut self,
        cx: &mut Context<'_>,
        params: &mut impl PollParameters,
    ) -> Poll<SwarmAction> {
        // Handle any events from the subprotocols
        match self.inner.poll(cx, params) {
            Poll::Ready(ToSwarm::GenerateEvent(InnerBehaviourEvent::Identify(
                identify::Event::Received { peer_id, info },
            ))) => {
                self.handle_identify_event(peer_id, info);
                Poll::Pending
            }
            // Forward pubsub events out
            Poll::Ready(ToSwarm::GenerateEvent(InnerBehaviourEvent::Gossipsub(
                message @ gossipsub::Event::Message { .. },
            ))) => Poll::Ready(ToSwarm::GenerateEvent(Event::Pubsub(message))),
            Poll::Ready(ToSwarm::GenerateEvent(event)) => {
                trace!("internal event: {event:?}");
                Poll::Pending
            } // Trap all sub events
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

type SwarmAction = ToSwarm<<Behaviour as NetworkBehaviour>::OutEvent, THandlerInEvent<Behaviour>>;

/// Configuration for the [`cordelia::Behaviour`](Behaviour).
#[derive(Clone)]
pub struct Config {
    /// Keypair used for signing messages
    keys: Keypair,
    /// Identify protocol configuration
    identify_cfg: identify::Config,
    /// Kademlia protocol configuration
    kad_cfg: KademliaConfig,
    /// Gossipsub protocol configuration
    gossipsub_cfg: gossipsub::Config,
    /// Path to the peer database
    peer_db_path: PathBuf,
    /// Bootstrap nodes to join the P2P network
    boot_nodes: Vec<Multiaddr>,
}

impl Config {
    pub fn new(keys: Keypair, peer_db_path: PathBuf, boot_nodes: Vec<Multiaddr>) -> Self {
        let mut kad_cfg = KademliaConfig::default();
        kad_cfg.set_query_timeout(DEFAULT_KAD_QUERY_TIMOUT);
        let pubkey = keys.public();
        Config {
            keys,
            identify_cfg: identify::Config::new(
                str::from_utf8(PROTOCOL_NAME).unwrap().to_string(),
                pubkey,
            ),
            kad_cfg,
            gossipsub_cfg: gossipsub::Config::default(),
            peer_db_path,
            boot_nodes,
        }
    }
}
