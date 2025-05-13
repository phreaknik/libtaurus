use super::{
    api,
    behaviour::{self, Behaviour, BehaviourEvent},
    broadcast::{self, Broadcast, BroadcastData, BroadcastValidationReport},
    fetcher::Fetcher,
};
use crate::WireFormat;
pub use api::P2pApi;
use futures::StreamExt;
use libp2p::{
    gossipsub::{self, Sha256Topic},
    identify::{self},
    identity::Keypair,
    kad,
    multiaddr::Protocol,
    noise,
    swarm::SwarmEvent,
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use std::{net::Ipv4Addr, path::PathBuf, result, time::Duration};
use tokio::{
    select,
    sync::{
        self,
        mpsc::{self, UnboundedReceiver},
        oneshot,
    },
};
use tracing::{debug, error, info, trace, warn};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
const P2P_EVENT_CHAN_CAPACITY: usize = 32;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Broadcast(#[from] broadcast::Error),
    #[error("malformed address")]
    MalformedAddress,
    #[error(transparent)]
    Publish(#[from] gossipsub::PublishError),
    #[error(transparent)]
    Subscription(#[from] gossipsub::SubscriptionError),
    #[error(transparent)]
    NoKnownPeers(#[from] kad::NoKnownPeers),
    #[error("unsupported event")]
    UnsupportedEvent,
}
type Result<T> = result::Result<T, Error>;

/// Event produced by [`Behaviour`].
#[derive(Debug, Clone)]
pub enum Event {
    /// New message from the gossip sub network
    GossipsubMessage(Broadcast),

    /// task has stopped
    Stopped,
}

impl TryFrom<BehaviourEvent> for Event {
    type Error = Error;

    fn try_from(value: BehaviourEvent) -> result::Result<Self, Self::Error> {
        match value {
            BehaviourEvent::Gossipsub(gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            }) => Ok(Event::GossipsubMessage(Broadcast {
                src: propagation_source,
                id: message_id,
                topic: message.topic,
                data: BroadcastData::from_wire(&message.data, true)?,
            })),
            _ => Err(Error::UnsupportedEvent),
        }
    }
}

/// Actions that can be performed by the p2p client
#[derive(Debug)]
pub enum Action {
    BlockPeer(PeerId),
    GetLocalPeerId(oneshot::Sender<PeerId>),
    GetFetcher(oneshot::Sender<Fetcher>),
    Broadcast(BroadcastData),
    ReportMessageValidity(BroadcastValidationReport),
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

    /// Optionally force kademlia mode of operation. Leave as [`None`] to rely on automatic mode
    /// configuration
    pub kad_mode_override: Option<kad::Mode>,
}

/// Run the p2p networking client, spawning the client task as a new thread. Returns an
/// [`UnboundedSender`], which can be used to send actions to the running task. Also returns a
/// [`broadcast::Sender`], which can be subscribed to, to receive P2P events from the task.
pub fn start(config: Config) -> P2pApi {
    // Spawn the task
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, event_receiver) = sync::broadcast::channel(P2P_EVENT_CHAN_CAPACITY);
    let task = Task::new(config, action_receiver, event_sender.clone());
    let handle = tokio::spawn(task.task_fn());
    tokio::spawn(async move {
        if let Err(e) = handle.await {
            error!("Consensus stopped with error: {e}");
        }
        event_sender.send(Event::Stopped).unwrap();
    });

    // Return the communication channels
    P2pApi::new(action_sender, event_receiver)
}

pub struct Task {
    config: Config,
    actions_in: UnboundedReceiver<Action>,
    events_out: sync::broadcast::Sender<Event>,
    swarm: Swarm<behaviour::Behaviour>,
}

impl Task {
    /// Construct a new P2P task
    pub fn new(
        config: Config,
        actions_in: UnboundedReceiver<Action>,
        events_out: sync::broadcast::Sender<Event>,
    ) -> Task {
        // Build the swarm
        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(config.identity_key.clone())
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )
            .unwrap()
            .with_quic()
            .with_behaviour(|key| Behaviour::new(behaviour::Config::new(key.clone())).unwrap())
            .unwrap()
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();
        swarm
            .behaviour_mut()
            .kademlia
            .set_mode(config.kad_mode_override);

        Task {
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

    /// Connect to initial peers and bootstrap into the network
    fn join_dht(&mut self) -> Result<()> {
        for mut addr in self.config.boot_peers.iter().cloned() {
            let peer_id = match addr.pop() {
                Some(Protocol::P2p(peer_id)) => Ok(peer_id),
                _ => Err(Error::MalformedAddress),
            }?;
            self.swarm
                .behaviour_mut()
                .kademlia
                .add_address(&peer_id, addr);
        }
        self.swarm.behaviour_mut().kademlia.bootstrap()?;
        Ok(())
    }

    /// The task function which runs the p2p networking client.
    async fn task_fn(mut self) {
        info!("Starting p2p client");

        // Listen for inbound connections
        self.start_listener(false); // Start TCP listener
        self.start_listener(true); // Start UDP QuicV1 listener

        // Bootstrap into the P2P network
        if let Err(e) = self.join_dht() {
            warn!("Failed to join DHT: {e}");
        }

        // Main event loop
        let local_peer_id = PeerId::from(self.config.identity_key.public());
        loop {
            select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::NewListenAddr { mut address, .. } => {
                        address.push(Protocol::P2p(local_peer_id.into()));
                        info!("Listening on {address}")
                    },
                    SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                        info!(
                            "Connected to {peer_id}. Now have {} peers.",
                            self.swarm.connected_peers().count()
                        );
                    },
                    SwarmEvent::ConnectionClosed { peer_id, cause, .. } => {
                        info!(
                            "Disconnected from {peer_id}. Now have {} peers.",
                            self.swarm.connected_peers().count()
                        );
                        if let Some(c) = cause {
                            debug!("Disconnection reason: {c}");
                        }
                    },
                    SwarmEvent::OutgoingConnectionError { .. } => {
                        // TODO: remove peer from db? from kademlia?
                    },
                    SwarmEvent::Behaviour(event) => {
                        match &event {
                                BehaviourEvent::Identify(identify::Event::Received { peer_id, info }) => {
                                    // TODO: filter by protocol name/version
                                    for addr in &info.listen_addrs {
                                        self.swarm.behaviour_mut().kademlia.add_address(&peer_id, addr.clone());
                                    }
                                },
                                _=> {},
                        }
                        // emit behaviour events to any subscribers
                        if let Ok(evt) = Event::try_from(event) {
                            self.events_out.send(evt).expect("Channel closed");
                            // TODO: clean shutdown on channel closure
                        }
                    },
                    e @ _ => {
                        trace!("unhandled p2p event: {e:#?}");
                    }
                },

                // Handle requested actions
                action = self.actions_in.recv() => match action {
                    Some(Action::BlockPeer(peer)) => {
                        self.swarm.behaviour_mut().block_lists.block_peer(peer);
                    },
                    Some(Action::GetFetcher(resp_ch)) => todo!(),
                    Some(Action::Broadcast(message)) => {
                        if let Err(e) = message.to_wire(true).map_err(Error::from).and_then(|bytes|
                        self.swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(Sha256Topic::from(&message), bytes)
                            .map_err(Error::from)) {
                                error!("Failed to publish p2p message: {e}");
                        }
                    },
                    Some(Action::GetLocalPeerId(resp_ch)) => resp_ch.send(local_peer_id).unwrap(),
                    Some(Action::ReportMessageValidity(BroadcastValidationReport{msg_id, propagation_source, acceptance})) => {
                        if let Err(e) = self.swarm.behaviour_mut().gossipsub.report_message_validation_result(
                            &msg_id,
                            &propagation_source,
                            acceptance,
                        ) {
                            warn!("Error updating gossipsub message status: {e}");
                        }
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
