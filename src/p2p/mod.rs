pub mod api;
mod behaviour;
mod broadcast;
pub mod consensus_rpc;
mod database;

use api::P2pApi;
use behaviour::Behaviour;
pub use broadcast::{Broadcast, BroadcastData, BroadcastValidationReport};
use core::result;
pub use database::{PeerDatabase, PeerInfo};
use futures::StreamExt;
use libp2p::{
    gossipsub, identity::Keypair, kad, multiaddr::Protocol, noise,
    request_response::InboundRequestId, swarm::SwarmEvent, tcp, yamux, Multiaddr, PeerId, Swarm,
};
use std::{io, net::Ipv4Addr, path::PathBuf, time::Duration};
use tokio::{
    select,
    sync::{
        self,
        mpsc::{self, UnboundedReceiver},
        oneshot,
    },
};
use tracing::{debug, error, info};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
const P2P_EVENT_CHAN_CAPACITY: usize = 32;

/// Path to the peer database, from within the peer data directory
const DATABASE_DIR: &str = "peer_db/";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Dial(#[from] libp2p::swarm::DialError),
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error("broadcast has no data")]
    EmptyBroadcast,
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error("data is not a valid broadcast message")]
    InvalidBroadast,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Multiaddr(#[from] libp2p::multiaddr::Error),
    #[error(transparent)]
    Transport(#[from] libp2p::TransportError<io::Error>),
    #[error(transparent)]
    Publish(#[from] gossipsub::PublishError),
    #[error(transparent)]
    Subscription(#[from] gossipsub::SubscriptionError),
    #[error(transparent)]
    NoKnownPeers(#[from] kad::NoKnownPeers),
    #[error(transparent)]
    Vertex(#[from] crate::consensus::vertex::Error),
    #[error(transparent)]
    Wire(#[from] crate::wire::Error),
}
type Result<T> = result::Result<T, Error>;

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    Pubsub(Broadcast),
    Stopped,
}

/// Actions that can be performed by the p2p client
#[derive(Debug)]
pub enum Action {
    BlockPeer(PeerId),
    Broadcast(BroadcastData),
    GetLocalPeerId(oneshot::Sender<PeerId>),
    ReportMessageValidity(BroadcastValidationReport),
    AvalancheRequest(PeerId, consensus_rpc::Request),
    AvalancheResponse(InboundRequestId, consensus_rpc::Response),
}

/// Configuration details for [`taurus-p2p`].
#[derive(Clone)]
pub struct Config {
    /// Path to the p2p data directory
    pub datadir: PathBuf,

    /// Bootstrap nodes to join P2P network
    pub boot_peers: Vec<Multiaddr>,

    /// Key used to identify self on p2p network
    pub identity_key: Keypair,

    /// Bind address for P2P connections
    pub addr: Ipv4Addr,
    pub port: u16,

    /// If true, increment port number until one is available
    pub search_port: bool,
}

/// Run the p2p networking client, spawning the client task as a new thread. Returns an
/// [`UnboundedSender`], which can be used to send actions to the running task. Also returns a
/// [`broadcast::Sender`], which can be subscribed to, to receive P2P events from the task.
pub fn start(config: Config) -> P2pApi {
    // Spawn the task
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, event_receiver) = sync::broadcast::channel(P2P_EVENT_CHAN_CAPACITY);
    let process = Process::new(config, action_receiver, event_sender.clone());
    let handle = tokio::spawn(process.task_fn());
    tokio::spawn(async move {
        if let Err(e) = handle.await {
            error!("Consensus stopped with error: {e}");
        }
        event_sender.send(Event::Stopped).unwrap();
    });

    // Return the communication channels
    P2pApi::new(action_sender, event_receiver)
}

pub struct Process {
    config: Config,
    actions_in: UnboundedReceiver<Action>,
    events_out: sync::broadcast::Sender<Event>,
    swarm: Swarm<behaviour::Behaviour>,
}

impl Process {
    /// Construct a new P2P process
    pub fn new(
        config: Config,
        actions_in: UnboundedReceiver<Action>,
        events_out: sync::broadcast::Sender<Event>,
    ) -> Process {
        // Open the peer database
        let peer_db = PeerDatabase::open(&config.datadir.join(DATABASE_DIR), true)
            .expect("Failed to open peer database");

        // Build the swarm
        let swarm = libp2p::SwarmBuilder::with_existing_identity(config.identity_key.clone())
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .unwrap()
            .with_quic()
            .with_behaviour(|key| {
                Behaviour::new(
                    behaviour::Config::new(key.clone(), config.boot_peers.clone()),
                    peer_db,
                )
                .unwrap()
            })
            .unwrap()
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        Process {
            config,
            actions_in,
            events_out,
            swarm,
        }
    }

    /// Bind the process to a listening address
    fn start_listener(&mut self, quicv1: bool) {
        let mut port = self.config.port;
        loop {
            let addr = if quicv1 {
                Multiaddr::empty()
                    .with(Protocol::Ip4(self.config.addr))
                    .with(Protocol::Udp(port))
                    .with(Protocol::QuicV1)
            } else {
                Multiaddr::empty()
                    .with(Protocol::Ip4(self.config.addr))
                    .with(Protocol::Tcp(port))
            };
            match self.swarm.listen_on(addr.clone()) {
                Ok(_) => break,
                Err(e) => {
                    if self.config.search_port && port < u16::MAX {
                        port += 1;
                    } else {
                        error!("Cannot start P2P listener on {addr}: {e}");
                        return;
                    }
                }
            }
        }
    }

    /// The task function which runs the p2p networking client.
    async fn task_fn(mut self) {
        info!("Starting p2p client");

        // Listen for inbound connections
        self.start_listener(false); // Start TCP listener
        self.start_listener(true); // Start UDP QuicV1 listener

        // Bootstrap into the P2P network
        self.swarm.behaviour_mut().bootstrap();

        // Main event loop
        let local_peer_id = PeerId::from(self.config.identity_key.public());
        loop {
            select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::NewListenAddr { mut address, .. } => {
                        address.push(Protocol::P2p(local_peer_id.into()));
                        info!("Listening on {address}")
                    }
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        info!(
                            "Connected to {peer_id}. Now have {} peers.",
                            self.swarm.connected_peers().count()
                        );
                    }
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        info!(
                            "Disconnected from {peer_id}. Now have {} peers.",
                            self.swarm.connected_peers().count()
                        );
                        if let Some(c) = cause {
                            debug!("Disconnection reason: {c}");
                        }
                    }
                    SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                        if let Some(peer_id) = peer_id {
                            self.swarm.behaviour_mut().handle_unreachable_peer(&peer_id);
                        }
                    }
                    SwarmEvent::Behaviour(event) => {
                        // emit behaviour event to any subscribers
                        self.events_out.send(event).expect("Channel closed");
                    }
                    e => {
                        debug!("unhandled p2p event: {e:#?}");
                    }
                },

                // Handle requested actions
                action = self.actions_in.recv() => match action {
                    Some(Action::BlockPeer(peer)) => {
                        self.swarm.behaviour_mut().block_peer(peer);
                    },
                    Some(Action::Broadcast(message)) => {
                        if let Err(e) = self.swarm.behaviour_mut().publish(message) {
                            error!("Failed to publish p2p message: {e}");
                        }
                    },
                    Some(Action::GetLocalPeerId(resp_ch)) => resp_ch.send(local_peer_id).unwrap(),
                    Some(Action::ReportMessageValidity(BroadcastValidationReport{
                        id, src, acceptance,
                    })) => {
                        self.swarm.behaviour_mut().report_message_validation_result(&id, &src, acceptance)
                    },
                    Some(Action::AvalancheRequest(peer, request)) => {
                        // TODO: also look in DHT in case this peer fails
                        self.swarm.behaviour_mut().avalanche_request(&peer, request)
                    },
                    Some(Action::AvalancheResponse(request_id, response)) => {
                        self.swarm.behaviour_mut().avalanche_response(request_id, response)
                    },
                    None => {
                        // If we do not receive requests from the consensus module, we cannot
                        // participate in the P2P network. Shut down the client.
                        error!("Request channel closed");
                        break;
                    }
                },
            }
        }
    }
}
