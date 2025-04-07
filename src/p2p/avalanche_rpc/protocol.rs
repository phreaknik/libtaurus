use super::{proto, Config, Request, Response};
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::request_response;
use libp2p::request_response::ProtocolName;
use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
use std::io;
use thiserror::Error;
use tracing::error;

pub const PROTOCOL_NAME: &[u8] = b"/cordelia/avalanche_rpc/0.1.0";

#[derive(Clone, Debug)]
pub struct AvalancheRpcProtocol {
    _config: Config,
}

impl AvalancheRpcProtocol {
    pub fn new(_config: Config) -> Self {
        AvalancheRpcProtocol { _config }
    }
}

impl ProtocolName for AvalancheRpcProtocol {
    fn protocol_name(&self) -> &[u8] {
        PROTOCOL_NAME
    }
}

#[derive(Clone)]
pub struct AvalancheRpcCodec;

#[async_trait]
impl request_response::Codec for AvalancheRpcCodec {
    type Protocol = AvalancheRpcProtocol;
    type Request = super::Request;
    type Response = super::Response;

    async fn read_request<T>(
        &mut self,
        _: &AvalancheRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut bytes = Vec::new();
        io.read_to_end(&mut bytes).await?;
        let mut protobuf = BytesReader::from_bytes(&bytes);
        match proto::Request::from_reader(&mut protobuf, &bytes) {
            Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to read request message",
            )),
            Ok(protobuf) => Request::from_protobuf(protobuf).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "parsed request message is missing data",
                )
            }),
        }
    }

    async fn read_response<T>(
        &mut self,
        _: &AvalancheRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut bytes = Vec::new();
        io.read_to_end(&mut bytes).await?;
        let mut protobuf = BytesReader::from_bytes(&bytes);
        match proto::Response::from_reader(&mut protobuf, &bytes) {
            Err(_) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to read response message",
            )),
            Ok(protobuf) => Response::from_protobuf(protobuf).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "parsed response message is missing data",
                )
            }),
        }
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &AvalancheRpcProtocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = data.to_protobuf().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to convert request to protobuf",
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to write request message",
            )
        })?;
        io.write_all(bytes.as_slice()).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &AvalancheRpcProtocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        let protobuf = data.to_protobuf().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to convert response to protobuf",
            )
        })?;
        protobuf.write_message(&mut writer).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "unable to write response message",
            )
        })?;
        io.write_all(bytes.as_slice()).await?;
        io.close().await
    }
}

#[derive(Debug, Error)]
pub enum UpgradeError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("Stream closed")]
    StreamClosed,
}

impl From<UpgradeError> for io::Error {
    fn from(e: UpgradeError) -> Self {
        match e {
            UpgradeError::Io(e) => e,
            UpgradeError::StreamClosed => {
                io::Error::new(io::ErrorKind::ConnectionAborted, UpgradeError::StreamClosed)
            }
        }
    }
}
