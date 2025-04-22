use crate::wire::{proto, WireFormat};
use serde_derive::{Deserialize, Serialize};
use std::result;

/// Error type for transaction errors
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Protobuf(#[from] quick_protobuf::Error),
    #[error(transparent)]
    TryFromSlice(#[from] std::array::TryFromSliceError),
}

/// Result type for vertex errors
pub type Result<T> = result::Result<T, Error>;

/// Type alias for transaction hashes
pub type TxHash = crate::hash::Hash;

#[derive(Clone, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
pub struct Transaction {
    inputs: Vec<Txo>,
}

impl<'a> WireFormat<'a, proto::Transaction> for Transaction {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Transaction> {
        Ok(proto::Transaction {
            inputs: self
                .inputs
                .iter()
                .map(|i| i.to_protobuf(check))
                .try_collect()?,
        })
    }

    fn from_protobuf(transaction: &proto::Transaction, check: bool) -> Result<Transaction> {
        Ok(Transaction {
            inputs: transaction
                .inputs
                .iter()
                .map(|i| Txo::from_protobuf(i, check))
                .try_collect()?,
        })
    }
}

/// Type alias for txo hashes
pub type TxoHash = crate::hash::Hash;

/// Transaction output object
#[derive(Debug, Clone, Default, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct Txo {
    value: u64,
}

impl<'a> WireFormat<'a, proto::Txo> for Txo {
    type Error = Error;

    fn to_protobuf(&self, _check: bool) -> Result<proto::Txo> {
        Ok(proto::Txo { value: self.value })
    }

    fn from_protobuf(txo: &proto::Txo, _check: bool) -> Result<Txo> {
        Ok(Txo { value: txo.value })
    }
}
