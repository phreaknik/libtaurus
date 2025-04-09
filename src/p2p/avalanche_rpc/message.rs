use super::proto;
use super::Error;
use crate::consensus::{Block, BlockHash};
use std::fmt;
use strum_macros::EnumIter;

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
pub enum Request {
    GetBlock(BlockHash),
    GetPreference(BlockHash),
}

impl Request {
    /// Convert a Request to into protobuf format
    pub fn to_protobuf(self) -> Result<proto::Request, Error> {
        Ok(proto::Request {
            RequestData: match self {
                Request::GetBlock(hash) => {
                    proto::mod_Request::OneOfRequestData::get_block(proto::BlockID {
                        hash: rmp_serde::to_vec(&hash)?,
                    })
                }
                Request::GetPreference(hash) => {
                    proto::mod_Request::OneOfRequestData::get_preference(proto::BlockID {
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
            Request::GetBlock(serde_hash) => {
                write!(f, "GetBlock({})", Into::<blake3::Hash>::into(serde_hash))
            }
            // TODO: Need to get rid of old references to serde_hash
            Request::GetPreference(serde_hash) => {
                write!(
                    f,
                    "GetPreference({})",
                    Into::<blake3::Hash>::into(serde_hash)
                )
            }
        }
    }
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone)]
pub enum Response {
    Error(proto::mod_Response::Error),
    Block(Block),
    Preferred(BlockHash),
}

impl Response {
    /// Convert a Response to protobuf format
    pub fn to_protobuf(self) -> Result<proto::Response, Error> {
        Ok(proto::Response {
            ResponseData: match self {
                Response::Error(e) => proto::mod_Response::OneOfResponseData::error(e),
                Response::Block(b) => proto::mod_Response::OneOfResponseData::block(b.try_into()?),
                Response::Preferred(h) => {
                    proto::mod_Response::OneOfResponseData::preferred(proto::BlockID {
                        hash: rmp_serde::to_vec(&h)?,
                    })
                }
            },
        })
    }

    /// Convert a Response from protobuf format
    pub fn from_protobuf(resp: proto::Response) -> Result<Self, Error> {
        match resp.ResponseData {
            proto::mod_Response::OneOfResponseData::error(e) => Ok(Response::Error(e)),
            proto::mod_Response::OneOfResponseData::block(b) => Ok(Response::Block(b.try_into()?)),
            proto::mod_Response::OneOfResponseData::preferred(h) => {
                Ok(Response::Preferred(rmp_serde::from_slice(&h.hash)?))
            }
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
