use super::Error;
use crate::{
    consensus::Vertex,
    wire::{generated::proto, WireFormat},
    VertexHash,
};
use std::{fmt, result, sync::Arc};
use strum_macros::{EnumCount, EnumIter};

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
            None => Err(super::Error::IncompleteRequest),
        }
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Request::GetVertex(hash) => write!(f, "GetVertex({})", hash.to_short_hex()),
            Request::GetPreference(hash) => write!(f, "GetPreference({})", hash.to_short_hex()),
        }
    }
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
pub enum Response {
    Vertex(Arc<Vertex>),
    Preference(VertexHash, bool),
}

impl<'a> WireFormat<'a, proto::Response> for Response {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Response, Error> {
        Ok(proto::Response {
            response_data: match self {
                Response::Vertex(v) => {
                    Some(proto::response::ResponseData::Vertex(v.to_protobuf(check)?))
                }
                Response::Preference(hash, preferred) => Some(
                    proto::response::ResponseData::Preference(proto::Preference {
                        hash: Some(hash.to_protobuf(check)?),
                        preferred: *preferred,
                    }),
                ),
            },
        })
    }

    fn from_protobuf(resp: &proto::Response, check: bool) -> Result<Self, Error> {
        match &resp.response_data {
            Some(proto::response::ResponseData::Vertex(v)) => Ok(Response::Vertex(Arc::new(
                Vertex::from_protobuf(&v, check)?,
            ))),
            Some(proto::response::ResponseData::Preference(h)) => Ok(Response::Preference(
                VertexHash::from_protobuf(
                    h.hash.as_ref().ok_or(Error::IncompleteResponse)?,
                    check,
                )?,
                h.preferred,
            )),
            None => Err(super::Error::IncompleteResponse),
        }
    }
}
