use heed::BytesDecode;
use heed::BytesEncode;
use serde_derive::{Deserialize, Serialize};
use std::fmt;
use std::result;

use crate::p2p;

/// Error type for transaction errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    MsgPackDecode(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    MsgPackEncode(#[from] rmp_serde::encode::Error),
}

/// Result type for vertex errors
pub type Result<T> = result::Result<T, Error>;

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
    pub fn hash(&self) -> Result<TxoHash> {
        Ok(blake3::hash(&rmp_serde::to_vec(self)?).into())
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

/// Hash of a transaction output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct TxoHash(pub [u8; blake3::OUT_LEN]);

impl TxoHash {
    /// Format the TxoHash as a hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Format the TxoHash as a short hex string, better for displaying TxoHashes in large
    /// collections of data
    pub fn to_short_hex(&self) -> String {
        format!("{}..", hex::encode(&self.0[..4]))
    }

    /// Serialize into protobuf format
    pub fn to_protobuf(&self) -> Result<p2p::avalanche_rpc::proto::Hash> {
        Ok(p2p::avalanche_rpc::proto::Hash {
            hash: rmp_serde::to_vec(&self)?,
        })
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(proto: &p2p::avalanche_rpc::proto::Hash) -> Result<TxoHash> {
        Ok(TxoHash(rmp_serde::from_slice(&proto.hash)?))
    }
}

impl fmt::Display for TxoHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_short_hex())
    }
}

impl From<blake3::Hash> for TxoHash {
    fn from(hash: blake3::Hash) -> Self {
        TxoHash(*hash.as_bytes())
    }
}

impl Into<blake3::Hash> for &TxoHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl Into<blake3::Hash> for TxoHash {
    fn into(self) -> blake3::Hash {
        blake3::Hash::from(self.0)
    }
}

impl<'a> BytesEncode<'a> for TxoHash {
    type EItem = TxoHash;

    fn bytes_encode(
        item: &'a Self::EItem,
    ) -> std::result::Result<std::borrow::Cow<'a, [u8]>, Box<dyn std::error::Error>> {
        Ok(rmp_serde::to_vec(item)?.into())
    }
}

impl<'a> BytesDecode<'a> for TxoHash {
    type DItem = TxoHash;

    fn bytes_decode(
        bytes: &'a [u8],
    ) -> std::result::Result<Self::DItem, Box<dyn std::error::Error>> {
        Ok(rmp_serde::from_slice(bytes)?)
    }
}

impl Default for TxoHash {
    fn default() -> Self {
        TxoHash([0; blake3::OUT_LEN])
    }
}
