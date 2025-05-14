use crate::wire::{proto, WireFormat};
use crate::{Vertex, VertexHash};
use async_trait::async_trait;
use core::fmt;
use futures::prelude::*;
use libp2p::request_response;
use std::sync::Arc;
use std::{io, result};
use strum::{Display, EnumCount, EnumIter};

pub const PROTOCOL_NAME: &str = "/taurus_request/0.1.0";

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Hash(#[from] crate::hash::Error),
    #[error("incomplete response")]
    IncompleteResponse,
    #[error("incomplete request")]
    IncompleteRequest,
    #[error("invalid error code (found {0})")]
    InvalidErrorCode(i32),
    #[error(transparent)]
    ProstDecode(#[from] prost::DecodeError),
    #[error(transparent)]
    ProstEncode(#[from] prost::EncodeError),
    #[error(transparent)]
    Vertex(#[from] crate::vertex::Error),
}
type Result<T> = result::Result<T, Error>;

#[derive(Clone, Debug, Default)]
pub struct Protocol {}

impl AsRef<str> for Protocol {
    fn as_ref(&self) -> &str {
        PROTOCOL_NAME
    }
}

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, EnumCount)]
pub enum Request {
    GetVertex(VertexHash),
    GetPreference(VertexHash),
}

impl<'a> WireFormat<'a, proto::Request> for Request {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> result::Result<proto::Request, Error> {
        Ok(proto::Request {
            request_data: match self {
                Request::GetVertex(hash) => Some(proto::request::RequestData::GetVertex(
                    hash.to_protobuf(check)?,
                )),
                Request::GetPreference(hash) => Some(proto::request::RequestData::GetPreference(
                    hash.to_protobuf(check)?,
                )),
            },
        })
    }

    fn from_protobuf(req: &proto::Request, check: bool) -> result::Result<Self, Error> {
        match &req.request_data {
            Some(proto::request::RequestData::GetVertex(message)) => Ok(Request::GetVertex(
                VertexHash::from_protobuf(&message, check)?,
            )),
            Some(proto::request::RequestData::GetPreference(message)) => Ok(
                Request::GetPreference(VertexHash::from_protobuf(&message, check)?),
            ),
            None => Err(Error::IncompleteRequest),
        }
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Request::GetVertex(hash) => write!(f, "GetVertex({hash})"),
            Request::GetPreference(hash) => write!(f, "GetPreference({hash})"),
        }
    }
}

/// Error codes
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, Default, Display)]
pub enum ErrorCode {
    #[default]
    Unknown,
    NotFound,
}

impl From<&ErrorCode> for i32 {
    fn from(value: &ErrorCode) -> Self {
        match value {
            ErrorCode::Unknown => 0,
            ErrorCode::NotFound => 1,
        }
    }
}

impl TryFrom<i32> for ErrorCode {
    type Error = Error;

    fn try_from(value: i32) -> result::Result<Self, Self::Error> {
        match value {
            0 => Ok(ErrorCode::Unknown),
            1 => Ok(ErrorCode::NotFound),
            _ => Err(Error::InvalidErrorCode(value)),
        }
    }
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
pub enum Response {
    Error(ErrorCode),
    Vertex(Arc<Vertex>),
    Preference(bool),
}

impl<'a> WireFormat<'a, proto::Response> for Response {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Response> {
        Ok(proto::Response {
            response_data: match self {
                Response::Error(code) => Some(proto::response::ResponseData::Error(code.into())),
                Response::Vertex(vx) => Some(proto::response::ResponseData::Vertex(
                    vx.to_protobuf(check)?,
                )),
                Response::Preference(preferred) => Some(proto::response::ResponseData::Preference(
                    proto::Preference {
                        preferred: *preferred,
                    },
                )),
            },
        })
    }

    fn from_protobuf(resp: &proto::Response, check: bool) -> Result<Self> {
        match &resp.response_data {
            Some(proto::response::ResponseData::Error(code)) => {
                Ok(Response::Error((*code).try_into()?))
            }
            Some(proto::response::ResponseData::Vertex(v)) => Ok(Response::Vertex(Arc::new(
                Vertex::from_protobuf(&v, check)?,
            ))),
            Some(proto::response::ResponseData::Preference(h)) => {
                Ok(Response::Preference(h.preferred))
            }
            None => Err(Error::IncompleteResponse),
        }
    }
}

#[derive(Clone, Default)]
pub struct Codec;

#[async_trait]
impl request_response::Codec for Codec {
    type Protocol = Protocol;
    type Request = Request;
    type Response = Response;

    async fn read_request<T>(&mut self, _: &Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut bytes = Vec::new();
        io.read_to_end(&mut bytes).await?;
        Self::Request::from_wire(&bytes, true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to read request message: {e}"),
            )
        })
    }

    async fn read_response<T>(&mut self, _: &Protocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: AsyncRead + Send + Unpin,
    {
        let mut bytes = Vec::new();
        io.read_to_end(&mut bytes).await?;
        Self::Response::from_wire(&bytes, true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to read response message: {e}"),
            )
        })
    }

    async fn write_request<T>(
        &mut self,
        _protocol: &Protocol,
        io: &mut T,
        data: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        //TODO: Do we need to check before writing? maybe not?
        let bytes = data.to_wire(true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to write request message: {e}"),
            )
        })?;
        io.write_all(bytes.as_slice()).await?;
        io.close().await
    }

    async fn write_response<T>(
        &mut self,
        _: &Protocol,
        io: &mut T,
        data: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Send + Unpin,
    {
        //TODO: Do we need to check before writing? maybe not?
        let bytes = data.to_wire(true).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unable to write response message: {e}"),
            )
        })?;
        io.write_all(bytes.as_slice()).await?;
        io.close().await
    }
}
