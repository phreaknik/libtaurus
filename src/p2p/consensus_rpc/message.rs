use super::Error;
use crate::consensus::{Block, BlockHash};
use crate::wire::{self, proto, WireFormat};
use crate::VertexHash;
use std::fmt;
use std::result;
use std::sync::Arc;
use strum_macros::EnumIter;

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
pub enum Request {
    GetBlock(BlockHash),
    GetVertex(VertexHash),
    GetPreference(VertexHash),
}

impl<'a> WireFormat<'a, proto::Request> for Request {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> result::Result<proto::Request, Error> {
        Ok(proto::Request {
            RequestData: match self {
                Request::GetBlock(hash) => {
                    proto::mod_Request::OneOfRequestData::get_block(hash.to_protobuf(check)?)
                }
                Request::GetVertex(hash) => {
                    proto::mod_Request::OneOfRequestData::get_vertex(hash.to_protobuf(check)?)
                }
                Request::GetPreference(hash) => {
                    proto::mod_Request::OneOfRequestData::get_preference(hash.to_protobuf(check)?)
                }
            },
        })
    }

    fn from_protobuf(req: &proto::Request, check: bool) -> result::Result<Self, Error> {
        match &req.RequestData {
            proto::mod_Request::OneOfRequestData::get_block(message) => Ok(Request::GetBlock(
                BlockHash::from_protobuf(&message, check)?,
            )),
            proto::mod_Request::OneOfRequestData::get_vertex(message) => Ok(Request::GetVertex(
                VertexHash::from_protobuf(&message, check)?,
            )),
            proto::mod_Request::OneOfRequestData::get_preference(message) => Ok(
                Request::GetPreference(VertexHash::from_protobuf(&message, check)?),
            ),
            proto::mod_Request::OneOfRequestData::None => Err(super::Error::IncompleteRequest),
        }
    }
}

impl fmt::Display for Request {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Request::GetBlock(hash) => write!(f, "GetBlock({})", hash.to_short_hex()),
            Request::GetVertex(hash) => write!(f, "GetVertex({})", hash.to_short_hex()),
            Request::GetPreference(hash) => write!(f, "GetPreference({})", hash.to_short_hex()),
        }
    }
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone)]
pub enum Response {
    Block(Block),
    Vertex(Arc<wire::Vertex>),
    Preference(VertexHash, bool),
}

impl<'a> WireFormat<'a, proto::Response> for Response {
    type Error = Error;

    fn to_protobuf(&self, check: bool) -> Result<proto::Response, Error> {
        Ok(proto::Response {
            ResponseData: match self {
                Response::Block(b) => {
                    proto::mod_Response::OneOfResponseData::block(b.to_protobuf(check)?)
                }
                Response::Vertex(v) => {
                    proto::mod_Response::OneOfResponseData::vertex(v.to_protobuf(check)?)
                }
                Response::Preference(hash, preferred) => {
                    proto::mod_Response::OneOfResponseData::preference(proto::Preference {
                        hash: Some(hash.to_protobuf(check)?),
                        preferred: *preferred,
                    })
                }
            },
        })
    }

    fn from_protobuf(resp: &proto::Response, check: bool) -> Result<Self, Error> {
        match &resp.ResponseData {
            proto::mod_Response::OneOfResponseData::block(b) => {
                Ok(Response::Block(Block::from_protobuf(&b, check)?))
            }
            proto::mod_Response::OneOfResponseData::vertex(v) => Ok(Response::Vertex(Arc::new(
                wire::Vertex::from_protobuf(&v, check)?,
            ))),
            proto::mod_Response::OneOfResponseData::preference(h) => Ok(Response::Preference(
                VertexHash::from_protobuf(
                    h.hash.as_ref().ok_or(Error::IncompleteResponse)?,
                    check,
                )?,
                h.preferred,
            )),
            proto::mod_Response::OneOfResponseData::None => Err(super::Error::IncompleteResponse),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::wire::WireFormat;

    use super::proto;
    use super::Request;
    use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};
    use strum::IntoEnumIterator;

    #[test]
    fn protobuf_requests() {
        // Attempt to serialize and deserialize each request type
        for request_in in Request::iter() {
            // Encode the message
            let mut bytes = Vec::new();
            let mut writer = Writer::new(&mut bytes);
            let protobuf = request_in.clone().to_protobuf(true).unwrap();
            protobuf.write_message(&mut writer).unwrap();

            // Decode the request
            let mut reader = BytesReader::from_bytes(&bytes);
            let protobuf = proto::Request::from_reader(&mut reader, &bytes).unwrap();
            let request_out = Request::from_protobuf(&protobuf, true).unwrap();
            assert_eq!(request_in, request_out);
        }
    }
}
