use super::proto;
use crate::consensus::{Block, SerdeHash};
use strum_macros::EnumIter;

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
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
            let protobuf = TryInto::<proto::Request>::try_into(request_in.clone()).unwrap();
            protobuf.write_message(&mut writer).unwrap();

            // Decode the request
            let mut reader = BytesReader::from_bytes(&bytes);
            let request_out: Request = proto::Request::from_reader(&mut reader, &bytes)
                .unwrap()
                .try_into()
                .unwrap();
            assert_eq!(request_in, request_out);
        }
    }
}
