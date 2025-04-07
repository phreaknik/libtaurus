use super::proto;
use crate::consensus::{Block, SerdeHash};

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

#[cfg(test)]
mod tests {
    use crate::consensus::SerdeHash;

    use super::proto;
    use super::Request;
    use quick_protobuf::{BytesReader, MessageRead, Writer};
    use std::io;

    #[test]
    fn protobuf_requests() {
        let request_in = Request::GetBlock(10, SerdeHash::default());
        println!("encoded={request_in:?}");
        let mut bytes = Vec::new();
        let mut writer = Writer::new(&mut bytes);
        // Encode the message
        writer.write_message(&TryInto::<proto::Request>::try_into(request_in).unwrap());
        println!("raw={bytes:?}");

        // Decode the request
        let mut reader = BytesReader::from_bytes(&bytes);
        let request_out: Request = proto::Request::from_reader(&mut reader, &bytes)
            .unwrap()
            .try_into()
            .unwrap();
        println!("encoded={request_out:?}");
        assert_eq!(0, 4);
    }
}
