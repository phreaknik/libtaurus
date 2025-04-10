use super::proto;
use super::Error;
use crate::consensus::{Block, BlockHash};
use crate::VertexHash;
use crate::WireVertex;
use std::fmt;
use std::sync::Arc;
use strum_macros::EnumIter;

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
pub enum Request {
    GetBlock(BlockHash),
    GetVertex(VertexHash),
    GetPreference(VertexHash),
}

impl Request {
    /// Convert a Request to into protobuf format
    pub fn to_protobuf(self) -> Result<proto::Request, Error> {
        Ok(proto::Request {
            RequestData: match self {
                Request::GetBlock(hash) => {
                    proto::mod_Request::OneOfRequestData::get_block(proto::Hash {
                        hash: rmp_serde::to_vec(&hash)?,
                    })
                }
                Request::GetVertex(hash) => {
                    proto::mod_Request::OneOfRequestData::get_vertex(proto::Hash {
                        hash: rmp_serde::to_vec(&hash)?,
                    })
                }
                Request::GetPreference(hash) => {
                    proto::mod_Request::OneOfRequestData::get_preference(proto::Hash {
                        hash: rmp_serde::to_vec(&hash)?,
                    })
                }
            },
        })
    }

    /// Convert a Request from protobuf format
    pub fn from_protobuf(req: proto::Request) -> Result<Self, Error> {
        match req.RequestData {
            proto::mod_Request::OneOfRequestData::get_block(message) => Ok(Request::GetBlock(
                rmp_serde::from_slice(message.hash.as_slice())?,
            )),
            proto::mod_Request::OneOfRequestData::get_vertex(message) => Ok(Request::GetVertex(
                rmp_serde::from_slice(message.hash.as_slice())?,
            )),
            proto::mod_Request::OneOfRequestData::get_preference(message) => Ok(
                Request::GetPreference(rmp_serde::from_slice(message.hash.as_slice())?),
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
    Error(proto::mod_Response::Error),
    Block(Arc<Block>),
    Vertex(Arc<WireVertex>),
    Preference(VertexHash, bool),
}

impl Response {
    /// Convert a Response to protobuf format
    pub fn to_protobuf(self) -> Result<proto::Response, Error> {
        Ok(proto::Response {
            ResponseData: match self {
                Response::Error(e) => proto::mod_Response::OneOfResponseData::error(e),
                Response::Block(b) => {
                    proto::mod_Response::OneOfResponseData::block(b.to_protobuf()?)
                }
                Response::Vertex(v) => {
                    proto::mod_Response::OneOfResponseData::vertex(v.to_protobuf()?)
                }
                Response::Preference(hash, preferred) => {
                    proto::mod_Response::OneOfResponseData::preference(proto::Preference {
                        hash: rmp_serde::to_vec(&hash)?,
                        preferred,
                    })
                }
            },
        })
    }

    /// Convert a Response from protobuf format
    pub fn from_protobuf(resp: proto::Response) -> Result<Self, Error> {
        match resp.ResponseData {
            proto::mod_Response::OneOfResponseData::error(e) => Ok(Response::Error(e)),
            proto::mod_Response::OneOfResponseData::block(b) => {
                Ok(Response::Block(Arc::new(Block::from_protobuf(b)?)))
            }
            proto::mod_Response::OneOfResponseData::vertex(v) => {
                Ok(Response::Vertex(Arc::new(WireVertex::from_protobuf(v)?)))
            }
            proto::mod_Response::OneOfResponseData::preference(h) => Ok(Response::Preference(
                rmp_serde::from_slice(&h.hash)?,
                h.preferred,
            )),
            proto::mod_Response::OneOfResponseData::None => Err(super::Error::IncompleteResponse),
        }
    }
}

#[cfg(test)]
mod tests {
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
            let protobuf = request_in.clone().to_protobuf().unwrap();
            protobuf.write_message(&mut writer).unwrap();

            // Decode the request
            let mut reader = BytesReader::from_bytes(&bytes);
            let protobuf = proto::Request::from_reader(&mut reader, &bytes).unwrap();
            let request_out = Request::from_protobuf(protobuf).unwrap();
            assert_eq!(request_in, request_out);
        }
    }
}
