mod behaviour;
pub mod message;
pub mod peer_db;

use core::result;
use futures::future::{select, Either};
use futures::{pin_mut, StreamExt};
use libp2p::gossipsub;
use libp2p::identity::Keypair;
use libp2p::kad;
use libp2p::multiaddr::Protocol;
use libp2p::swarm::{SwarmBuilder, SwarmEvent};
use libp2p::{Multiaddr, PeerId};
use message::Message;
use std::fs;
use std::io;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use thiserror;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::{error, info};

pub use self::behaviour::Behaviour;

/// Event produced by [`Behaviour`].
#[derive(Debug)]
pub enum Event {
    Pubsub(gossipsub::Event),
}

/// Error type for cordelia-p2p errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    DialError(#[from] libp2p::swarm::DialError),
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

/// Run the cordelia p2p networking client
/// messages sent to the msg_in will be published to the gossipsub network
/// messages received from the gossipsub network will be forwarded to the msg_out
pub async fn run(
    config: Config,
    mut msg_in: UnboundedReceiver<Message>,
    msg_out: UnboundedSender<Message>,
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

    // Main event loop
    loop {
        let swarm_fut = swarm.select_next_some();
        let msg_in_fut = msg_in.recv();
        pin_mut!(swarm_fut, msg_in_fut);
        match select(swarm_fut, msg_in_fut).await {
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
                SwarmEvent::Behaviour(Event::Pubsub(gossipsub::Event::Message {
                    message, ..
                })) => {
                    msg_out
                        .send(Message::from_gossipsub(message))
                        .expect("channel closed");
                }
                _e @ _ => {} // Ignore other events
            },

            // Publish any pending messages to network
            Either::Right((msg_to_publish, _)) => {
                if let Err(e) = swarm.behaviour_mut().publish(msg_to_publish.unwrap()) {
                    error!("Failed to publish p2p message: {e}");
                }
            }
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
