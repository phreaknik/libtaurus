use super::proto::rpc_messages;
use super::Config;
use async_trait::async_trait;
use futures::prelude::*;
use libp2p::request_response;
use libp2p::request_response::ProtocolName;
use quick_protobuf::BytesReader;
use quick_protobuf::MessageRead;
use quick_protobuf::MessageWrite;
use quick_protobuf::Writer;
use std::io;
use thiserror::Error;
use tracing::error;

pub const PROTOCOL_NAME: &[u8] = b"/cordelia/peer_rpc/0.1.0";

#[derive(Clone, Debug)]
pub struct PeerRpcProtocol {
    _config: Config,
}

impl PeerRpcProtocol {
    pub fn new(_config: Config) -> Self {
        PeerRpcProtocol { _config }
    }
}

impl ProtocolName for PeerRpcProtocol {
    fn protocol_name(&self) -> &[u8] {
        PROTOCOL_NAME
    }
}

#[derive(Clone)]
pub struct PeerRpcCodec;

#[async_trait]
impl request_response::Codec for PeerRpcCodec {
    type Protocol = PeerRpcProtocol;
    type Request = rpc_messages::Request;
    type Response = rpc_messages::Response;

    async fn read_request<T>(
        &mut self,
        _: &PeerRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        let mut reader = BytesReader::from_bytes(&buf);
        rpc_messages::Request::from_reader(&mut reader, &buf).or(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid request message",
        )))
    }

    async fn read_response<T>(
        &mut self,
        _: &PeerRpcProtocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut buf = Vec::new();
        io.read_to_end(&mut buf).await?;
        rpc_messages::Response::try_from(&buf[..]).or(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid response message",
        )))
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &PeerRpcProtocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut buf = Vec::with_capacity(data.get_size());
        let mut writer = Writer::new(&mut buf);
        data.write_message(&mut writer).or(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Error encoding request message",
        )))?;
        io.write_all(&buf).await?;
        io.close().await?;
        Ok(())
    }

    async fn write_response<T>(
        &mut self,
        _: &PeerRpcProtocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        let mut buf = Vec::with_capacity(data.get_size());
        let mut writer = Writer::new(&mut buf);
        data.write_message(&mut writer).or(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Error encoding request message",
        )))?;
        io.write_all(&buf).await?;
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
