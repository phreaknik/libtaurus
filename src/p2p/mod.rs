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
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{Multiaddr, PeerId};
pub use message::Message;
use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use thiserror;
use tokio::select;
use tokio::sync::broadcast;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
pub const P2P_EVENT_CHAN_CAPACITY: usize = 32;

/// Path to the peer database, from within the peer data directory
pub const PEER_DATABASE_DIR: &str = "peer_db/";

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    Pubsub(gossipsub::Message),
}

/// Actions that can be performed by the p2p client
#[derive(Clone, Debug)]
pub enum Action {
    Broadcast(Message),
}

/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Dial(#[from] libp2p::swarm::DialError),
    #[error(transparent)]
    Cbor(#[from] serde_cbor::Error),
    #[error(transparent)]
    Heed(#[from] heed::Error),
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
}

/// Result type for cordelia-p2p
pub type Result<T> = result::Result<T, Error>;

/// Configuration details for ['cordelia-p2p'].
#[derive(Clone)]
pub struct Config {
    /// Path to directory containing the peer database
    pub data_dir: PathBuf,
    /// Bootstrap nodes to join P2P network
    pub boot_nodes: Vec<Multiaddr>,
}

/// Run the p2p networking client, spawning the client task as a new thread. Returns an
/// ['UnboundedSender'], which can be used to send actions to the running task. Also returns a
/// ['broadcast::Sender'], which can be subscribed to, to receive P2P events from the task.
pub fn start(config: Config) -> (UnboundedSender<Action>, broadcast::Sender<Event>) {
    // Open the peer database
    let peer_db = PeerDatabase::open(&config.data_dir.join(PEER_DATABASE_DIR), true)
        .expect("failed to open peer database");

    // Spawn the task
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _) = broadcast::channel(P2P_EVENT_CHAN_CAPACITY);
    tokio::spawn(task_fn(
        config,
        peer_db,
        action_receiver,
        event_sender.clone(),
    ));

    // Return the communication channels
    (action_sender, event_sender)
}

/// The task function which runs the p2p networking client.
async fn task_fn(
    config: Config,
    peer_db: PeerDatabase,
    mut actions_in: UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
) {
    info!("Starting p2p client...");

    let local_key = get_keypair(&config.data_dir);
    let local_peer_id = PeerId::from(local_key.public());
    info!("peer_id = {local_peer_id}");

    // Build the swarm
    // TODO: pick a different transport. development_transport() has features we likely don't want,
    // e.g. noise encryption
    let mut swarm = SwarmBuilder::with_tokio_executor(
        libp2p::tokio_development_transport(local_key.clone()).unwrap(),
        Behaviour::new(
            behaviour::Config::new(local_key, config.boot_nodes.clone()),
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
        .expect("cannot start listener on {local_addr}");

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
                SwarmEvent::ConnectionClosed { peer_id, .. } => {
                    info!(
                        "Disconnected from {peer_id}. Now have {} peers.",
                        swarm.connected_peers().count()
                    );
                }
                SwarmEvent::OutgoingConnectionError { peer_id, .. } => {
                    if let Some(peer_id) = peer_id {
                        swarm.behaviour_mut().handle_unreachable_peer(&peer_id);
                    }
                }
                SwarmEvent::Behaviour(event) => {
                    // emit behaviour event to any subscribers
                    events_out.send(event).expect("channel closed");
                }
                _e @ _ => {} // Ignore other events
            },

            // Handle API requests
            action = actions_in.recv() => match action {
                Some(Action::Broadcast(message)) => {
                    if let Err(e) = swarm.behaviour_mut().publish(message) {
                        error!("Failed to publish p2p message: {e}");
                    }
                }
                None => {
                    // If we do not receive requests from the consensus module, we cannot
                    // participate in the P2P network. Shut down the client.
                    error!("request channel closed");
                    break;
                }
            },
        }
    }
}

fn get_keypair(data_dir: &PathBuf) -> Keypair {
    let keypath = data_dir.join("private-key");
    let _ = fs::create_dir_all(&data_dir);
    match fs::read(&keypath) {
        Ok(keydata) => {
            Keypair::from_protobuf_encoding(&keydata).expect("Failed to decode keyfile!")
        }
        Err(_) => {
            // Generate a random new key
            let newkey = Keypair::generate_ed25519();
            // Save the key to the file
            fs::write(
                keypath,
                newkey
                    .to_protobuf_encoding()
                    .expect("Failed to encode key to save to keyfile!"),
            )
            .expect("Failed to write to keyfile!");
            newkey
        }
    }
}
