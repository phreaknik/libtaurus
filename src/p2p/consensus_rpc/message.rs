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
mod tests {
    use super::*;
    use crate::{hash::tests::generate_test_hashes, wire::WireFormat};
    use itertools::Itertools;
    use std::iter;

    pub struct RequestTestCase<'a> {
        pub decoded: Request,
        pub encoded: &'a [u8],
    }

    pub fn generate_test_requests<'a>() -> impl Iterator<Item = RequestTestCase<'a>> {
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
        let get_block_cases = generate_test_hashes()
            .map(|tc| Request::GetBlock(tc.decoded))
            .zip_eq(encoded.into_iter())
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
        let get_vertex_cases = generate_test_hashes()
            .map(|tc| Request::GetVertex(tc.decoded))
            .zip_eq(encoded.into_iter())
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
        let get_preference_cases = generate_test_hashes()
            .map(|tc| Request::GetPreference(tc.decoded))
            .zip_eq(encoded.into_iter())
            .map(|(decoded, encoded)| RequestTestCase { decoded, encoded });

        // Return the full set of test cases
        get_block_cases
            .chain(get_vertex_cases)
            .chain(get_preference_cases)
    }

    pub struct ResponseTestCase {
        pub decoded: Response,
        pub encoded: Vec<u8>,
    }

    pub fn generate_test_responses() -> impl Iterator<Item = ResponseTestCase> {
        // GetBlock responses
        let block_cases = iter::once(ResponseTestCase {
            decoded: Response::Block(Block::default().with_parents(vec![VertexHash::default()])),
            encoded: vec![
                2, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57,
                55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58,
                48, 48,
            ],
        });

        // GetVertex responses
        let vertex_cases = iter::once(ResponseTestCase {
            decoded: Response::Vertex(Arc::new(Vertex::new_full(Arc::new(
                Block::default().with_parents(vec![VertexHash::default()]),
            )))),
            encoded: vec![
                10, 74, 0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
        });

        // GetPreference responses
        let encoded = [
            vec![
                18, 36, 2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            ],
            vec![
                18, 38, 2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 1,
            ],
        ];
        let preference_cases = generate_test_hashes()
            .map(|tc| {
                [
                    Response::Preference(tc.decoded, false),
                    Response::Preference(tc.decoded, true),
                ]
                .into_iter()
            })
            .flatten()
            .zip_eq(encoded.into_iter())
            .map(|(decoded, encoded)| ResponseTestCase { decoded, encoded })
            .take(1); // Don't need to iterate all the test hashes for this

        // Return the full set of test cases
        block_cases.chain(vertex_cases).chain(preference_cases)
    }

    /// Attempt to serialize and deserialize each request test case
    #[test]
    fn encoding_requests() {
        for case in generate_test_requests() {
            // Encode the request
            let encoded = case.decoded.to_wire(true).unwrap();
            assert_eq!(case.encoded, encoded);

            // Decode the request
            let decoded = Request::from_wire(&case.encoded, true).unwrap();
            assert_eq!(case.decoded, decoded);
        }
    }

    /// Attempt to serialize and deserialize each response test case
    #[test]
    fn encoding_responses() {
        for case in generate_test_responses() {
            // Encode the response
            let encoded = case.decoded.to_wire(true).unwrap();
            assert_eq!(case.encoded, encoded);

            // Decode the response
            let decoded = Response::from_wire(&case.encoded, true).unwrap();
            assert_eq!(case.decoded, decoded);
        }
    }
}
