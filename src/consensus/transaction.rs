use crate::wire::{proto, WireFormat};
use serde_derive::{Deserialize, Serialize};
use std::{io, result};

/// Error type for transaction errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    TryFromSlice(#[from] std::array::TryFromSliceError),
}

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

impl Txo {}

impl<'a> WireFormat<'a, proto::Txo> for Txo {
    type Error = Error;

    fn to_protobuf(&self, _check: bool) -> Result<proto::Txo> {
        Ok(proto::Txo { value: self.value })
    }

    fn from_protobuf(txo: &proto::Txo, _check: bool) -> Result<Txo> {
        Ok(Txo { value: txo.value })
    }
}
