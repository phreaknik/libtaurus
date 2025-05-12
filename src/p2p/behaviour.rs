use super::{broadcast::BroadcastData, request};
use libp2p::{
    allow_block_list,
    gossipsub::{self, MessageAuthenticity, Sha256Topic},
    identify,
    identity::Keypair,
    kad::{self, store::MemoryStore},
    mdns,
    request_response::{self, ProtocolSupport},
    swarm::NetworkBehaviour,
    upnp, PeerId,
};
use std::{iter::once, result, str, time::Duration};
use strum::IntoEnumIterator;
use tracing::debug;

pub const PROTOCOL_NAME: &[u8; 13] = b"/taurus/0.1.0";

/// Kademlia query timeout in seconds
const DEFAULT_KAD_QUERY_TIMOUT: Duration = Duration::from_secs(60);

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Subscription(#[from] gossipsub::SubscriptionError),
}

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

    /// MDNS protocol configuration
    mdns_cfg: mdns::Config,

    /// Request/Response configuration
    request_cfg: request_response::Config,
}

impl Config {
    pub fn new(keys: Keypair) -> Self {
        let mut kad_cfg = kad::Config::default();
        kad_cfg.set_query_timeout(DEFAULT_KAD_QUERY_TIMOUT);
        let pubkey = keys.public();
        let gossipsub_cfg = gossipsub::ConfigBuilder::default()
            .validate_messages() // We must accept every message before its allowed to propagate
            .build()
            .expect("Failed to build gossipsub config");
        Config {
            keys,
            identify_cfg: identify::Config::new(
                str::from_utf8(PROTOCOL_NAME).unwrap().to_string(),
                pubkey,
            ),
            kad_cfg,
            gossipsub_cfg,
            mdns_cfg: mdns::Config::default(),
            request_cfg: request_response::Config::default(),
        }
    }
}

/// Aggregate behaviour for all subprotocols used by the taurus behaviour
#[derive(NetworkBehaviour)]
pub(super) struct Behaviour {
    pub identify: identify::Behaviour,
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub gossipsub: gossipsub::Behaviour,
    pub block_lists: allow_block_list::Behaviour<allow_block_list::BlockedPeers>,
    pub upnp: upnp::tokio::Behaviour,
    pub mdns: mdns::tokio::Behaviour,
    pub requests: request_response::Behaviour<request::Codec>,
}

impl<'a> Behaviour {
    pub fn new(config: Config) -> result::Result<Self, Error> {
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
        Ok(Behaviour {
            identify: identify::Behaviour::new(config.identify_cfg),
            kademlia: kad::Behaviour::with_config(
                local_peer_id,
                MemoryStore::new(local_peer_id),
                config.kad_cfg,
            ),
            gossipsub,
            block_lists: allow_block_list::Behaviour::default(),
            upnp: upnp::tokio::Behaviour::default(),
            mdns: mdns::tokio::Behaviour::new(config.mdns_cfg, local_peer_id)?,
            requests: request_response::Behaviour::new(
                once((request::Protocol::default(), ProtocolSupport::Full)),
                config.request_cfg,
            ),
        })
    }
}
