use super::Config;
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::request_response;
use libp2p::request_response::ProtocolName;
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
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        serde_cbor::from_slice(buf.as_slice()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "received invalid request message",
            )
        })
    }

    async fn read_response<T>(
        &mut self,
        _: &AvalancheRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        serde_cbor::from_slice(buf.as_slice()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "received invalid response message",
            )
        })
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
        io.write_all(
            &serde_cbor::to_vec(&data)
                .map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "error encoding request message")
                })?
                .as_slice(),
        )
        .await?;
        io.close().await?;
        Ok(())
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
        io.write_all(
            &serde_cbor::to_vec(&data)
                .map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "error encoding response message",
                    )
                })?
                .as_slice(),
        )
        .await?;
        io.close().await?;
        Ok(())
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
