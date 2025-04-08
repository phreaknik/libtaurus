use super::Result;
use core::fmt;
use heed::{BytesDecode, BytesEncode, Database, Env, EnvOpenOptions};
use libp2p::{identify, Multiaddr, PeerId};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

/// Database of peer info, using the ['heed'] LMDB database wrapper.
#[derive(Clone)]
pub struct PeerDatabase {
    pub env: Env,
    pub db: Database<PeerDbKey, PeerInfo>,
}

impl PeerDatabase {
    /// Open the database at the given path, or optionally create it if nonexistent
    pub fn open(path: &PathBuf, create: bool) -> Result<PeerDatabase> {
        if create {
            fs::create_dir_all(path.as_path())?;
        }
        let env = EnvOpenOptions::new().open(path)?;
        let db = if create {
            env.create_database(None)?
        } else {
            env.open_database(None)?.unwrap()
        };
        Ok(PeerDatabase { env, db })
    }
}

/// Key by which peer info is stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerDbKey(PeerId);

impl PeerDbKey {
    pub fn as_peerid(self) -> PeerId {
        self.0
    }
}

impl<'a> BytesEncode<'a> for PeerDbKey {
    type EItem = PeerDbKey;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for PeerDbKey {
    type DItem = PeerDbKey;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

impl From<PeerId> for PeerDbKey {
    fn from(id: PeerId) -> Self {
        PeerDbKey(id)
    }
}

impl From<&PeerId> for PeerDbKey {
    fn from(id: &PeerId) -> Self {
        PeerDbKey(*id)
    }
}

/// Collection of data for a given peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    /// Application-specific version of the protocol family used by the peer,
    /// e.g. `ipfs/1.0.0` or `polkadot/1.0.0`.
    pub protocol_version: String,
    /// Name and version of the peer, similar to the `User-Agent` header in
    /// the HTTP protocol.
    pub agent_version: String,
    /// Address observed for the peer
    pub addresses: Vec<Multiaddr>,
}

impl<'a> BytesEncode<'a> for PeerInfo {
    type EItem = PeerInfo;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for PeerInfo {
    type DItem = PeerInfo;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

impl fmt::Display for PeerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json = serde_json::to_string_pretty(self).unwrap();
        write!(f, "{json}")
    }
}

impl From<identify::Info> for PeerInfo {
    fn from(info: identify::Info) -> Self {
        let mut addresses = info.listen_addrs.clone();
        addresses.sort();
        addresses.dedup();
        PeerInfo {
            protocol_version: info.protocol_version,
            agent_version: info.agent_version,
            addresses,
        }
    }
}
