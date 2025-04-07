use crate::consensus::{Block, SerdeHash};

use super::proto;

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone)]
pub enum Request {
    GetBlock(u64, SerdeHash),
}

impl TryFrom<proto::Request> for Request {
    type Error = super::Error;

    fn try_from(req: proto::Request) -> Result<Self, super::Error> {
        match req.RequestData {
            proto::mod_Request::OneOfRequestData::get_block(message) => Ok(Request::GetBlock(
                message.height,
                serde_cbor::from_slice(message.hash.as_slice())?,
            )),
            proto::mod_Request::OneOfRequestData::None => Err(super::Error::IncompleteRequest),
        }
    }
}

impl TryInto<proto::Request> for Request {
    type Error = super::Error;

    fn try_into(self) -> Result<proto::Request, Self::Error> {
        Ok(proto::Request {
            RequestData: match self {
                Request::GetBlock(height, hash) => {
                    proto::mod_Request::OneOfRequestData::get_block(proto::GetBlock {
                        height,
                        hash: serde_cbor::to_vec(&hash)?,
                    })
                }
            },
        })
    }
}

/// Message type defining the peer RPC response messages
#[derive(Debug, Clone)]
pub enum Response {
    Error(proto::mod_Response::Error),
    Block(Block),
}

impl TryFrom<proto::Response> for Response {
    type Error = super::Error;

    fn try_from(resp: proto::Response) -> Result<Self, super::Error> {
        match resp.ResponseData {
            proto::mod_Response::OneOfResponseData::error(e) => Ok(Response::Error(e)),
            proto::mod_Response::OneOfResponseData::block(b) => Ok(Response::Block(b.try_into()?)),
            proto::mod_Response::OneOfResponseData::None => Err(super::Error::IncompleteResponse),
        }
    }
}

impl TryInto<proto::Response> for Response {
    type Error = super::Error;

    fn try_into(self) -> Result<proto::Response, super::Error> {
        Ok(proto::Response {
            ResponseData: match self {
                Response::Error(e) => proto::mod_Response::OneOfResponseData::error(e),
                Response::Block(b) => proto::mod_Response::OneOfResponseData::block(b.try_into()?),
            },
        })
    }
}
