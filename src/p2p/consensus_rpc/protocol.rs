use super::{Config, Request, Response};
use crate::wire::{proto, WireFormat};
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::request_response;
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::io;

pub const PROTOCOL_NAME: &str = "/cordelia/consensus_rpc/0.1.0";

#[derive(Clone, Debug, Default)]
pub struct ConsensusRpcProtocol {
    _config: Config,
}

impl ConsensusRpcProtocol {
    pub fn new(_config: Config) -> Self {
        ConsensusRpcProtocol { _config }
    }
}

impl AsRef<str> for ConsensusRpcProtocol {
    fn as_ref(&self) -> &str {
        PROTOCOL_NAME
    }
}

#[derive(Clone, Default)]
pub struct ConsensusRpcCodec;

#[async_trait]
impl request_response::Codec for ConsensusRpcCodec {
    type Protocol = ConsensusRpcProtocol;
    type Request = super::Request;
    type Response = super::Response;

    async fn read_request<T>(
        &mut self,
        _: &ConsensusRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut bytes = Vec::new();
        io.read_to_end(&mut bytes).await?;
        let mut protobuf = BytesReader::from_bytes(&bytes);
        match proto::Request::from_reader(&mut protobuf, &bytes) {
            Err(e) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to read request message: {e}"),
            )),
            Ok(protobuf) => Request::from_protobuf(&protobuf, true).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("parsed request message is missing data: {e}"),
                )
            }),
        }
    }

    async fn read_response<T>(
        &mut self,
        _: &ConsensusRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut bytes = Vec::new();
        io.read_to_end(&mut bytes).await?;
        let mut protobuf = BytesReader::from_bytes(&bytes);
        match proto::Response::from_reader(&mut protobuf, &bytes) {
            Err(e) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to read response message: {e}"),
            )),
            Ok(protobuf) => Response::from_protobuf(&protobuf, true).map_err(|e| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("parsed response message is missing data: {e}"),
                )
            }),
        }
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &ConsensusRpcProtocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = data.to_protobuf(true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert Request to protobuf: {e}"),
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to serialize request message: {e}"),
            )
        })?;
        io.write_all(bytes.as_slice()).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &ConsensusRpcProtocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = data.to_protobuf(true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to convert Response to protobuf: {e}"),
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to serialize response message: {e}"),
            )
        })?;
        io.write_all(bytes.as_slice()).await?;
        io.close().await
    }
}

#[derive(Debug)]
pub enum UpgradeError {}
