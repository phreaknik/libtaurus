use crate::p2p;
use serde_derive::{Deserialize, Serialize};
use std::result;

/// Error type for transaction errors
#[derive(thiserror::Error, Debug)]
pub enum Error {}

/// Result type for vertex errors
pub type Result<T> = result::Result<T, Error>;

/// Type alias for txo hashes
pub type TxoHash = crate::hash::Hash;

#[derive(Clone, Serialize, Deserialize)]
pub struct Transaction {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct TxHash(pub [u8; blake3::OUT_LEN]);

/// Transaction output object
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Txo {
    value: u64,
}

impl Txo {
    /// Compute the hash of the transaction output
    pub fn hash(&self) -> TxoHash {
        blake3::hash(&rmp_serde::to_vec(self).expect("Serde encode failure in hash")).into()
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(txo: &p2p::avalanche_rpc::proto::Txo) -> Result<Txo> {
        Ok(Txo { value: txo.value })
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Txo> {
        Ok(p2p::avalanche_rpc::proto::Txo { value: self.value })
    }
}
