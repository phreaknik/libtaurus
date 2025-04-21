use super::Error;
use crate::{
    consensus::{Block, BlockHash, Vertex},
    wire::{proto, WireFormat},
    VertexHash,
};
use std::{fmt, result, sync::Arc};
use strum_macros::{EnumCount, EnumIter};

/// Message type defining the peer RPC request messages
#[derive(Debug, Clone, PartialEq, Eq, EnumIter, EnumCount)]
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
#[derive(Debug, Clone, PartialEq, Eq, EnumIter)]
pub enum Response {
    Block(Block),
    Vertex(Arc<Vertex>),
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
                Vertex::from_protobuf(&v, check)?,
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
mod wire_format_tests {
    use super::*;
    use crate::wire::tests::test_wire_format;
    use std::assert_matches::assert_matches;

    #[test]
    pub fn get_block_request() {
        let decoded = Request::GetBlock(BlockHash::default());
        let encoded = &[
            2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);
    }

    #[test]
    pub fn get_vertex_request() {
        let decoded = Request::GetVertex(BlockHash::default());
        let encoded = &[
            10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);
    }

    #[test]
    pub fn get_preference_request() {
        let decoded = Request::GetPreference(BlockHash::default());
        let encoded = &[
            18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);
    }

    #[test]
    pub fn block_response() {
        let decoded = Response::Block(Block::default().with_parents(vec![VertexHash::default()]));
        let encoded = &[
            2, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45,
            48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);
    }

    #[test]
    pub fn vertex_response() {
        // Full vertex
        let decoded = Response::Vertex(Arc::new(Vertex::new_full(Arc::new(
            Block::default().with_parents(vec![VertexHash::default()]),
        ))));
        let encoded = &[
            10, 76, 0, 1, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49,
            57, 55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58,
            48, 48,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);

        // Slim vertex
        let decoded = Response::Vertex(Arc::new(Vertex::new_slim(
            VertexHash::default(),
            vec![VertexHash::default()],
        )));
        let encoded = &[
            10, 74, 0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);
    }

    #[test]
    pub fn preference_response() {
        // Negative preference
        let decoded = Response::Preference(VertexHash::default(), false);
        let encoded = &[
            18, 36, 2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);

        // Positive preference
        let decoded = Response::Preference(VertexHash::default(), true);
        let encoded = &[
            18, 38, 2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 1,
        ];
        let (encode_err, decode_err) = test_wire_format(decoded, encoded);
        assert_matches!(encode_err, None);
        assert_matches!(decode_err, None);
    }
}
