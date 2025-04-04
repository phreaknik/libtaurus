pub mod database;
pub mod raffle;

use super::hash::Hash;
use chrono::{DateTime, Utc};
use heed::{BytesDecode, BytesEncode};
use libp2p::PeerId;
pub use raffle::ValidatorQuorum;
use serde_derive::{Deserialize, Serialize};

/// Path to the validator database, from within the consensus data directory
pub const DATABASE_DIR: &str = "validators_db/";

/// Any node may submit a mined ValidatorTicket to be entered into the validator raffle
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorTicket {
    pub peer: PeerId,
    pub heads: Vec<Hash>,
    pub height: u64,
    pub difficulty: u64,
    pub time: DateTime<Utc>,
    pub nonce: u64,
}

impl ValidatorTicket {
    /// Compute the frontier hash
    pub fn hash(&self) -> Hash {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&serde_cbor::to_vec(self).unwrap());
        hasher.finalize().into()
    }
}

impl Default for ValidatorTicket {
    fn default() -> Self {
        ValidatorTicket {
            peer: PeerId::random(),
            heads: Vec::new(),
            height: 0,
            difficulty: 0,
            time: Utc::now(),
            nonce: 0,
        }
    }
}

impl<'a> BytesEncode<'a> for ValidatorTicket {
    type EItem = ValidatorTicket;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(serde_cbor::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for ValidatorTicket {
    type DItem = ValidatorTicket;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(serde_cbor::from_slice(bytes)?)
    }
}
