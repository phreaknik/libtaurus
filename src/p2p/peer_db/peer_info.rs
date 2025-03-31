use core::fmt;

use libp2p::{Multiaddr, PeerId};
use serde::{Deserialize, Serialize};

/// Collection of data for a given peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Unique identifier derived from peer's public key
    pub peer_id: PeerId,
    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    pub agent_version: String,
    /// Address observed for the peer
    pub addresses: Vec<Multiaddr>,
}

impl fmt::Display for PeerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json = serde_json::to_string_pretty(self).unwrap();
        write!(f, "{json}")
    }
}
