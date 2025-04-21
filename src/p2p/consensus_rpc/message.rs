use super::Error;
use crate::consensus::{Block, BlockHash, Vertex};
use crate::wire::{proto, WireFormat};
use crate::VertexHash;
use std::fmt;
use std::result;
use std::sync::Arc;
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
mod tests {
    use super::*;
    use crate::{
        consensus::block::tests::TEST_CASES as BLOCK_CASES, hash::tests::TEST_CASES as HASH_CASES,
        wire::WireFormat,
    };
    use quick_protobuf::{BytesReader, MessageRead, MessageWrite, Writer};

    pub struct RequestTestCase<'a> {
        pub decoded: Request,
        pub encoded: &'a [u8],
    }

    pub fn request_cases<'a>() -> impl Iterator<Item = RequestTestCase<'a>> {
        // GetBlock requests
        let encoded = [
            &[
                2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                2, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 1,
            ],
            &[
                2, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255,
            ],
            &[
                2, 34, 2, 32, 127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255,
            ],
            &[
                2, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 254,
            ],
        ];
        assert_eq!(encoded.len(), HASH_CASES.len());
        let get_block_cases = HASH_CASES
            .iter()
            .map(|tc| Request::GetBlock(tc.decoded))
            .zip(encoded.into_iter())
            .map(|(decoded, encoded)| RequestTestCase { decoded, encoded });
        // GetVertex requests
        let encoded = [
            &[
                10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                10, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 1,
            ],
            &[
                10, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255,
            ],
            &[
                10, 34, 2, 32, 127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255,
            ],
            &[
                10, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 254,
            ],
        ];
        assert_eq!(encoded.len(), HASH_CASES.len());
        let get_vertex_cases = HASH_CASES
            .iter()
            .map(|tc| Request::GetVertex(tc.decoded))
            .zip(encoded.into_iter())
            .map(|(decoded, encoded)| RequestTestCase { decoded, encoded });
        // GetPreference requests
        let encoded = [
            &[
                18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                18, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 1,
            ],
            &[
                18, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255,
            ],
            &[
                18, 34, 2, 32, 127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255,
            ],
            &[
                18, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 254,
            ],
        ];
        assert_eq!(encoded.len(), HASH_CASES.len());
        let get_preference_cases = HASH_CASES
            .iter()
            .map(|tc| Request::GetPreference(tc.decoded))
            .zip(encoded.into_iter())
            .map(|(decoded, encoded)| RequestTestCase { decoded, encoded });

        // Return the full set of test cases
        get_block_cases
            .chain(get_vertex_cases)
            .chain(get_preference_cases)
    }

    pub struct ResponseTestCase<'a> {
        pub decoded: Response,
        pub encoded: &'a [u8],
    }

    pub fn response_cases<'a>() -> impl Iterator<Item = ResponseTestCase<'a>> {
        // GetBlock requests
        let encoded = [
            &[
                2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                2, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            &[
                2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 1,
            ],
            &[
                2, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255,
            ],
            &[
                2, 34, 2, 32, 127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255,
            ],
            &[
                2, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 254,
            ],
        ];
        assert_eq!(encoded.len(), BLOCK_CASES.len());
        let block_cases = BLOCK_CASES
            .iter()
            .map(|tc| Response::Block(tc.decoded.clone()))
            .zip(encoded.into_iter())
            .map(|(decoded, encoded)| ResponseTestCase { decoded, encoded });

        // Return the full set of test cases
        block_cases
    }

    #[test]
    fn encoding_requests() {
        // Attempt to serialize and deserialize each request test case
        for case in request_cases() {
            // Encode the request
            let mut bytes = Vec::new();
            let mut writer = Writer::new(&mut bytes);
            let protobuf = case.decoded.to_protobuf(true).unwrap();
            protobuf.write_message(&mut writer).unwrap();
            assert_eq!(case.encoded, bytes);

            // Decode the request
            let mut reader = BytesReader::from_bytes(&case.encoded);
            let protobuf = proto::Request::from_reader(&mut reader, &bytes).unwrap();
            let decoded = Request::from_protobuf(&protobuf, true).unwrap();
            assert_eq!(case.decoded, decoded);
        }
    }

    #[test]
    fn encoding_responses() {
        // Attempt to serialize and deserialize each request test case
        for case in response_cases() {
            // Encode the request
            let mut bytes = Vec::new();
            let mut writer = Writer::new(&mut bytes);
            let protobuf = case.decoded.to_protobuf(true).unwrap();
            protobuf.write_message(&mut writer).unwrap();
            assert_eq!(case.encoded, bytes);

            // Decode the request
            let mut reader = BytesReader::from_bytes(&case.encoded);
            let protobuf = proto::Response::from_reader(&mut reader, &bytes).unwrap();
            let decoded = Response::from_protobuf(&protobuf, true).unwrap();
            assert_eq!(case.decoded, decoded);
        }
    }
}
