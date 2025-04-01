mod api;
mod behaviour;
pub mod message;
pub mod peer_db;

pub use api::Api;
pub use behaviour::Behaviour;
use core::result;
use futures::future::{select, Either};
use futures::{pin_mut, StreamExt};
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
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinSet;
use tracing::{error, info};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
const EVENT_CHAN_CAPACITY: usize = 32;

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    Pubsub(gossipsub::Message),
}

/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Api(#[from] api::Error),
    #[error(transparent)]
    Dial(#[from] libp2p::swarm::DialError),
    #[error(transparent)]
    Multiaddr(#[from] libp2p::multiaddr::Error),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Transport(#[from] libp2p::TransportError<io::Error>),
    #[error(transparent)]
    RkvStore(#[from] rkv::StoreError),
    #[error(transparent)]
    PeerDB(#[from] peer_db::Error),
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
#[derive(Debug, Clone)]
pub struct Config {
    /// Path to directory containing the peer database
    pub data_dir: PathBuf,
    /// Bootstrap nodes to join P2P network
    pub boot_nodes: Vec<Multiaddr>,
}

impl Config {
    fn peer_db_path(&self) -> PathBuf {
        self.data_dir.join("peer_db/")
    }
}

/// Run the cordelia p2p networking client, spawning the client as a new thread in the provided
/// [`JoinSet`]. Returns an API handle that can be used to interface with the running task.
pub fn run(config: Config, joinset: &mut JoinSet<()>) -> api::Api {
    info!("Starting p2p client...");

    let local_key = get_keypair(&config.data_dir);
    let local_peer_id = PeerId::from(local_key.public());
    info!("peer_id = {local_peer_id}");

    // Build the swarm
    // TODO: pick a different transport. development_transport() has features we likely don't want,
    // e.g. noise encryption
    let mut swarm = SwarmBuilder::with_tokio_executor(
        libp2p::tokio_development_transport(local_key.clone()).unwrap(),
        Behaviour::new(behaviour::Config::new(
            local_key,
            config.peer_db_path(),
            config.boot_nodes.clone(),
        ))
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

    // Set up channls to service the API
    let (request_ch, mut inbound_request) = mpsc::unbounded_channel();
    let (event_emitter, _) = broadcast::channel(EVENT_CHAN_CAPACITY);

    // Create an API instance to interact with the P2P client task
    let api_handle = api::Api::new(request_ch, event_emitter.clone());

    // Spawn a task to run the event loop
    joinset.spawn(async move {
        // Main event loop
        loop {
            let swarm_out = swarm.select_next_some();
            let request_in = inbound_request.recv();
            pin_mut!(swarm_out, request_in);
            match select(swarm_out, request_in).await {
                // Handle swarm events
                Either::Left((swarm_event, _)) => match swarm_event {
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
                        event_emitter.send(event).expect("channel closed");
                    }
                    _e @ _ => {} // Ignore other events
                },

                // Handle API requests
                Either::Right((request, _)) => match request {
                    Some(api::Request::Broadcast(message)) => {
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
    });
    api_handle
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
