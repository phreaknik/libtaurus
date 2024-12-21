use super::{
    api,
    behaviour::{self, Behaviour, BehaviourEvent},
    broadcast::{Broadcast, BroadcastData, BroadcastValidationReport},
};
use crate::{
    p2p::{Request, Response},
    WireFormat,
};
pub use api::P2pApi;
use futures::StreamExt;
use libp2p::{
    gossipsub::{self, Sha256Topic},
    identify,
    identity::Keypair,
    kad,
    multiaddr::Protocol,
    noise,
    request_response::{self, InboundRequestId, OutboundRequestId, ResponseChannel},
    swarm::SwarmEvent,
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use std::{collections::HashMap, net::Ipv4Addr, path::PathBuf, result, time::Duration};
use tokio::{
    select,
    sync::{self, broadcast, mpsc, oneshot},
};
use tracing::{debug, error, info, trace, warn};

/// Event channel capacity. Old events will be dropped if channel exceeds capacity. See
/// [`tokio::sync::broadcast`] for more information.
const P2P_EVENT_CHAN_CAPACITY: usize = 128;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Broadcast(#[from] super::broadcast::Error),
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
#[derive(Debug, Clone, strum::Display)]
pub enum Event {
    /// New message from the gossip sub network
    GossipsubMessage(Broadcast),

    /// A new peer has been added to the routing table
    NewPeer(PeerId),

    /// New request from the request_response protocol
    RequestMessage {
        request_id: InboundRequestId,
        request: Request,
    },
}

/// Actions that can be performed by the p2p client
#[derive(Debug)]
pub enum Action {
    BlockPeer(PeerId),
    Broadcast(BroadcastData),
    GetLocalPeerId(oneshot::Sender<PeerId>),
    ReportMessageValidity(BroadcastValidationReport),
    Request {
        request: Request,
        opt_peer: Option<PeerId>,
        resp_ch: mpsc::UnboundedSender<Response>,
    },
    Respond {
        request_id: InboundRequestId,
        response: Response,
    },
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
pub fn start(config: Config) -> Result<P2pApi> {
    // Spawn the task
    let (action_sender, action_receiver) = mpsc::unbounded_channel();
    let (event_sender, _event_receiver) = sync::broadcast::channel(P2P_EVENT_CHAN_CAPACITY);
    let task = Task::new(config, action_receiver, event_sender.clone());
    tokio::spawn(task.task_fn());

    // Return the communication channels
    Ok(P2pApi::new(action_sender, event_sender))
}

pub struct Task {
    config: Config,
    peer_id: Option<PeerId>,
    actions_in: mpsc::UnboundedReceiver<Action>,
    events_out: broadcast::Sender<Event>,
    swarm: Swarm<behaviour::Behaviour>,
    pending_responses: HashMap<InboundRequestId, ResponseChannel<Response>>,
    pending_requests: HashMap<OutboundRequestId, mpsc::UnboundedSender<Response>>,
}

impl Task {
    /// Construct a new P2P task
    pub fn new(
        config: Config,
        actions_in: mpsc::UnboundedReceiver<Action>,
        events_out: broadcast::Sender<Event>,
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
            peer_id: None,
            actions_in,
            events_out,
            swarm,
            pending_requests: HashMap::new(),
            pending_responses: HashMap::new(),
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

        // Wait until the events channel has listeners
        while self.events_out.receiver_count() == 0 {}

        // Listen for inbound connections
        self.start_listener(false); // Start TCP listener
        self.start_listener(true); // Start UDP QuicV1 listener

        // Bootstrap into the P2P network
        if let Err(e) = self.join_dht() {
            warn!("Failed to join DHT: {e}");
        }

        // Main event loop
        self.peer_id = Some(PeerId::from(self.config.identity_key.public()));
        loop {
            select! {
                // Handle swarm events
                event = self.swarm.select_next_some() => match event {
                    SwarmEvent::NewListenAddr { mut address, .. } => {
                        address.push(Protocol::P2p(self.peer_id.unwrap().into()));
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
                        let opt_event = match event {
                                BehaviourEvent::Gossipsub(evt) => self.handle_gossipsub(evt),
                                BehaviourEvent::Identify(evt) => self.handle_identify(evt),
                                BehaviourEvent::Requests(evt) => self.handle_request_response(evt),
                                _=> None,
                        };

                        // Emit any generated task event to subscribers
                        if let Some(out_event) = opt_event {
                            // TODO: need to handle case of p2p channel becoming full
                            self.events_out.send(out_event).unwrap();
                            // TODO: clean shutdown on channel closure
                        }
                    },
                    e @ _ => {
                        trace!("unhandled p2p event: {e:#?}");
                    }
                },

                // Handle requested actions
                opt_action = self.actions_in.recv() =>{
                    if let Some(action) = opt_action {
                        self.handle_inbound_action(action);
                    } else {
                            // If we do not receive requests from the consensus module, we cannot
                            // participate in the P2P network. Shut down the client.
                            error!("Request channel closed");
                            break;
                    }
                },
            }
        }
    }

    /// Handler for events emitted by the RequestResponse protocol
    fn handle_gossipsub(&mut self, event: gossipsub::Event) -> Option<Event> {
        match event {
            gossipsub::Event::Message {
                propagation_source,
                message_id,
                message,
            } => BroadcastData::from_wire(&message.data, true)
                .ok()
                .and_then(|data| {
                    Some(Event::GossipsubMessage(Broadcast {
                        id: message_id,
                        src: propagation_source,
                        topic: message.topic,
                        data,
                    }))
                }),
            _ => None,
        }
    }

    /// Handler for events emitted by the RequestResponse protocol
    fn handle_request_response(
        &mut self,
        event: request_response::Event<Request, Response>,
    ) -> Option<Event> {
        match event {
            request_response::Event::Message { message, .. } => match message {
                request_response::Message::Request {
                    request_id,
                    request,
                    channel,
                } => {
                    self.pending_responses.insert(request_id, channel);
                    Some(Event::RequestMessage {
                        request_id,
                        request,
                    })
                }
                request_response::Message::Response {
                    request_id,
                    response,
                } => {
                    self.pending_requests
                        .remove(&request_id)
                        .and_then(|ch| ch.send(response.clone()).ok());
                    None
                }
            },
            _ => None,
        }
    }

    /// Handler for events emitted by the Identify protocol
    fn handle_identify(&mut self, event: identify::Event) -> Option<Event> {
        match event {
            identify::Event::Received { peer_id, info } => {
                // TODO: filter by protocol name/version
                for addr in &info.listen_addrs {
                    self.swarm
                        .behaviour_mut()
                        .kademlia
                        .add_address(&peer_id, addr.clone());
                }
                Some(Event::NewPeer(peer_id))
            }
            _ => None,
        }
    }

    /// Handler for inbound P2P actions
    fn handle_inbound_action(&mut self, action: Action) {
        match action {
            Action::BlockPeer(peer) => {
                self.swarm.behaviour_mut().block_lists.block_peer(peer);
            }
            Action::Broadcast(message) => {
                if let Err(e) = message
                    .to_wire(true)
                    .map_err(Error::from)
                    .and_then(|bytes| {
                        self.swarm
                            .behaviour_mut()
                            .gossipsub
                            .publish(Sha256Topic::from(&message), bytes)
                            .map_err(Error::from)
                    })
                {
                    error!("Failed to publish p2p message: {e}");
                }
            }
            Action::GetLocalPeerId(resp_ch) => resp_ch.send(self.peer_id.unwrap()).unwrap(),
            Action::ReportMessageValidity(BroadcastValidationReport {
                msg_id,
                propagation_source,
                acceptance,
            }) => {
                if let Err(e) = self
                    .swarm
                    .behaviour_mut()
                    .gossipsub
                    .report_message_validation_result(&msg_id, &propagation_source, acceptance)
                {
                    warn!("Error updating gossipsub message status: {e}");
                }
            }
            Action::Request {
                request,
                opt_peer,
                resp_ch,
            } => {
                // TODO: make peer selection optional
                let rid = self
                    .swarm
                    .behaviour_mut()
                    .requests
                    .send_request(&opt_peer.unwrap(), request);
                self.pending_requests.insert(rid, resp_ch);
            }
            Action::Respond {
                request_id,
                response,
            } => {
                self.pending_responses.remove(&request_id).and_then(|ch| {
                    self.swarm
                        .behaviour_mut()
                        .requests
                        .send_response(ch, response)
                        .ok()
                });
            }
        }
    }
}
