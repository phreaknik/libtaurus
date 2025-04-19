use crate::{
    p2p::{self, consensus_rpc::proto},
    wire::WireFormat,
};
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
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

impl Txo {
    /// Serialize into protobuf format
    pub fn to_protobuf(&self, _check: bool) -> Result<p2p::consensus_rpc::proto::Txo> {
        Ok(p2p::consensus_rpc::proto::Txo { value: self.value })
    }

    /// Deserialize from protobuf format
    pub fn from_protobuf(txo: &p2p::consensus_rpc::proto::Txo, _check: bool) -> Result<Txo> {
        Ok(Txo { value: txo.value })
    }
}

impl WireFormat for Txo {
    type Error = Error;
    fn to_wire(&self, check: bool) -> result::Result<Vec<u8>, Self::Error> {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = self.to_protobuf(check).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert Block to protobuf: {e}"),
            )
        })?;
        protobuf
            .write_message(&mut writer)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "unable to serialize Block"))?;
        Ok(bytes)
    }

    /// Deserialize from bytes
    fn from_wire(bytes: &[u8], check: bool) -> result::Result<Self, Self::Error> {
        let protobuf = proto::Txo::from_reader(&mut BytesReader::from_bytes(bytes), &bytes)
            .map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unable to parse block from bytes: {e}"),
                )
            })?;
        Txo::from_protobuf(&protobuf, check)
    }
}
