pub mod avalanche_rpc;
mod behaviour;
mod database;
pub mod message;

pub use behaviour::Behaviour;
use core::result;
pub use database::{PeerDatabase, PeerInfo};
use futures::StreamExt;
use libp2p::gossipsub;
use libp2p::identity::Keypair;
use libp2p::kad;
use libp2p::multiaddr::Protocol;
use libp2p::request_response::RequestId;
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{Multiaddr, PeerId};
pub use message::{Message, MessageData, MessageValidationReport};
use std::io;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use thiserror;
use tokio::select;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{broadcast, oneshot};
use tracing::{debug, error, info};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const P2P_EVENT_CHAN_CAPACITY: usize = 32;

/// Path to the peer database, from within the peer data directory
pub const DATABASE_DIR: &str = "peer_db/";

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    Pubsub(Message),
    Avalanche(avalanche_rpc::Event),
}

/// Actions that can be performed by the p2p client
#[derive(Debug)]
pub enum Action {
    BlockPeer(PeerId),
    Broadcast(MessageData),
    GetLocalPeerId(oneshot::Sender<PeerId>),
    ReportMessageValidity(MessageValidationReport),
    AvalancheRequest(PeerId, avalanche_rpc::Request),
    AvalancheResponse(RequestId, avalanche_rpc::Response),
}

/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Dial(#[from] libp2p::swarm::DialError),
    #[error(transparent)]
    Heed(#[from] heed::Error),
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    Multiaddr(#[from] libp2p::multiaddr::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Transport(#[from] libp2p::TransportError<io::Error>),
    #[error(transparent)]
    Publish(#[from] gossipsub::PublishError),
    #[error(transparent)]
    Subscription(#[from] gossipsub::SubscriptionError),
    #[error(transparent)]
    NoKnownPeers(#[from] kad::NoKnownPeers),
    #[error("data is not a message")]
    NotAMessage,
}

/// Result type for cordelia-p2p
pub type Result<T> = result::Result<T, Error>;

/// Configuration details for ['cordelia-p2p'].
#[derive(Clone)]
pub struct Config {
    /// Path to the p2p data directory
    pub data_dir: PathBuf,
    /// Bootstrap nodes to join P2P network
    pub boot_nodes: Vec<Multiaddr>,
    /// Key used to identify self on p2p network
    pub identity_key: Keypair,
}

/// Run the p2p networking client, spawning the client task as a new thread. Returns an
/// ['UnboundedSender'], which can be used to send actions to the running task. Also returns a
/// ['broadcast::Sender'], which can be subscribed to, to receive P2P events from the task.
pub fn start(config: Config) -> (UnboundedSender<Action>, broadcast::Sender<Event>) {
    // Spawn the task
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(P2P_EVENT_CHAN_CAPACITY);
    tokio::spawn(task_fn(config, action_receiver, event_sender.clone()));

    // Return the communication channels
    (action_sender, event_sender)
}

/// The task function which runs the p2p networking client.
async fn task_fn(
    config: Config,
    mut actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
) {
    info!("Starting p2p client...");
    // Open the peer database
    let peer_db = PeerDatabase::open(&config.data_dir.join(DATABASE_DIR), true)
        .expect("Failed to open peer database");

    // Build the swarm
    // TODO: pick a different transport. development_transport() has features we likely don't want,
    // e.g. noise encryption
    let local_peer_id = PeerId::from(config.identity_key.public());
    let mut swarm = SwarmBuilder::with_tokio_executor(
        libp2p::tokio_development_transport(config.identity_key.clone()).unwrap(),
        Behaviour::new(
            behaviour::Config::new(config.identity_key, config.boot_nodes.clone()),
            peer_db,
        )
        .unwrap(),
        local_peer_id,
    )
    .build();

    // Listen for inbound connections
    let local_addr = Multiaddr::empty()
        .with(Protocol::Ip4(Ipv4Addr::UNSPECIFIED))
        .with(Protocol::Tcp(0));
    swarm
        .listen_on(local_addr.clone())
        .expect("Cannot start listener on {local_addr}");

    // Main event loop
    loop {
        select! {
            // Handle swarm events
            event = swarm.select_next_some() => match event {
                SwarmEvent::NewListenAddr { mut address, .. } => {
                    address.push(Protocol::P2p(local_peer_id.into()));
                    info!("Listening on {address}")
                }
                SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                    info!(
                        "Connected to {peer_id}. Now have {} peers.",
                        swarm.connected_peers().count()
                    );
                }
                SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                    info!(
                        "Disconnected from {peer_id}. Now have {} peers.",
                        swarm.connected_peers().count()
                    );
                    if let Some(c) = cause {
                        debug!("Disconnection reason: {c}");
                    }
                }
                SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                    if let Some(peer_id) = peer_id {
                        swarm.behaviour_mut().handle_unreachable_peer(&peer_id);
                    }
                }
                SwarmEvent::Behaviour(event) => {
                    // emit behaviour event to any subscribers
                    events_out.send(event).expect("Channel closed");
                }
                _e @ _ => {} // Ignore other events
            },

            // Handle requested actions
            action = actions_in.recv() => match action {
                Some(Action::BlockPeer(peer)) => {
                    swarm.behaviour_mut().block_peer(peer);
                },
                Some(Action::Broadcast(message)) => {
                    if let Err(e) = swarm.behaviour_mut().publish(message) {
                        error!("Failed to publish p2p message: {e}");
                    }
                },
                Some(Action::GetLocalPeerId(resp_ch)) => resp_ch.send(local_peer_id).unwrap(),
                Some(Action::ReportMessageValidity(MessageValidationReport{
                    msg_id, msg_source, acceptance,
                })) => {
                    swarm.behaviour_mut().report_message_validation_result(&msg_id, &msg_source, acceptance)
                },
                Some(Action::AvalancheRequest(peer, request)) => {
                    // TODO: also look in DHT in case this peer fails
                    swarm.behaviour_mut().avalanche_request(&peer, request)
                },
                Some(Action::AvalancheResponse(request_id, response)) => {
                    swarm.behaviour_mut().avalanche_response(request_id, response)
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
