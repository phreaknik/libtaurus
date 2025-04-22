#![feature(assert_matches)]

use cordelia::{
    consensus::{block, vertex},
    hash::Hash,
    p2p::consensus_rpc::{Request, Response},
    params::MIN_DIFFICULTY,
    Block, BlockHash, Txo, TxoHash, Vertex, VertexHash, WireFormat,
};
use quick_protobuf::{MessageRead, MessageWrite};
use std::{assert_matches::assert_matches, fmt::Debug, sync::Arc};

/// Tests that encoded data matches decoded after encoding and decoding each. Returns the
/// encode and decode errors, if any.
pub fn test_wire_format<'a, P, D>(
    decoded: D,
    encoded: &'a [u8],
) -> (
    Option<<D as WireFormat<'a, P>>::Error>,
    Option<<D as WireFormat<'a, P>>::Error>,
)
where
    P: MessageWrite + MessageRead<'a>,
    D: WireFormat<'a, P> + Debug + Eq,
{
    let encode_err = match decoded.to_wire(true) {
        Ok(bytes) => {
            assert_eq!(bytes, encoded);
            None
        }
        Err(e) => Some(e),
    };
    let decode_err = match <D as WireFormat<'a, P>>::from_wire(&encoded, true) {
        Ok(block) => {
            assert_eq!(block, decoded);
            None
        }
        Err(e) => Some(e),
    };
    (encode_err, decode_err)
}

#[test]
pub fn slim_vertex_with_no_parents() {
    let decoded = Vertex::default();
    let encoded = &[
        0, 1, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(vertex::Error::EmptyParents));
    assert_matches!(decode_err, Some(vertex::Error::EmptyParents));
}

#[test]
pub fn slim_vertex_with_one_parent() {
    let decoded = Vertex::new_slim(BlockHash::default(), vec![VertexHash::default()]);
    let encoded = &[
        0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn slim_vertex_with_multiple_parents() {
    let decoded = Vertex::new_slim(
        BlockHash::default(),
        vec![
            Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ]),
            Hash::with_bytes([
                128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
            ]),
            Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1,
            ]),
            Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            Hash::with_bytes([
                127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254,
            ]),
        ],
    );
    let encoded = &[
        0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 10, 34, 2, 32, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 34, 2, 32, 127, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 10, 34, 2, 32, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn slim_vertex_with_repeated_parents() {
    let decoded = Vertex::new_slim(
        BlockHash::default(),
        vec![BlockHash::default(), BlockHash::default()],
    );
    let encoded = &[
        0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(vertex::Error::RepeatedParents));
    assert_matches!(decode_err, Some(vertex::Error::RepeatedParents));
}

#[test]
pub fn basic_full_vertex() {
    let decoded = Vertex::new_full(Arc::new(
        Block::default().with_parents(vec![VertexHash::default()]),
    ));
    let encoded = &[
        0, 1, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45,
        48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn full_vertex_with_redundant_parents() {
    let decoded = {
        let mut v = Vertex::new_full(Arc::new(
            Block::default().with_parents(vec![VertexHash::default()]),
        ));
        v.parents = Some(vec![
            Hash::with_bytes([
                127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254,
            ]),
        ]);
        v
    };
    let encoded = &[
        0, 1, 10, 34, 2, 32, 127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        10, 34, 2, 32, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
        255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 26,
        72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45, 48, 49,
        45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(vertex::Error::RedundantParents));
    assert_matches!(decode_err, Some(vertex::Error::RedundantParents));
}

#[test]
pub fn full_vertex_with_bad_block_hash() {
    let decoded = {
        let mut v = Vertex::new_full(Arc::new(Block::default().with_parents(vec![
            Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1,
            ]),
        ])));
        v.bhash = BlockHash::default();
        v
    };
    let encoded = &[
        0, 1, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 50, 25,
        49, 57, 55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58,
        48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(vertex::Error::BadBlockHash));
    assert_matches!(decode_err, Some(vertex::Error::RedundantBlockHash));
}

#[test]
pub fn minimal_block() {
    let decoded = Block::default().with_parents(vec![VertexHash::default()]);
    let encoded = &[
        0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45, 48, 49, 45,
        48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn incomplete_block() {
    let decoded = Block::default();
    let encoded = &[
        0, 1, 8, 232, 7, 18, 2, 0, 0, 50, 25, 49, 57, 55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48,
        58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(block::Error::MissingParents));
    assert_matches!(decode_err, Some(block::Error::MissingParents));
}

#[test]
pub fn unsupported_block() {
    let decoded = {
        let mut b = Block::default().with_parents(vec![VertexHash::default()]);
        b.version = u32::MAX;
        b
    };
    let encoded = &[
        0, 255, 255, 255, 255, 15, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55,
        48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(block::Error::UnsupportedVersion));
    assert_matches!(decode_err, Some(block::Error::UnsupportedVersion));
}

#[test]
pub fn block_with_illegal_difficulty() {
    let decoded = {
        let mut b = Block::default().with_parents(vec![VertexHash::default()]);
        b.difficulty = MIN_DIFFICULTY - 1;
        b
    };
    let encoded = &[
        0, 1, 8, 231, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45, 48, 49, 45,
        48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(block::Error::InvalidDifficulty));
    assert_matches!(decode_err, Some(block::Error::InvalidDifficulty));
}

#[test]
pub fn block_with_repeated_parents() {
    let decoded = Block::default().with_parents(vec![VertexHash::default(), VertexHash::default()]);
    let encoded = &[
        0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55,
        48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(block::Error::RepeatedParents));
    assert_matches!(decode_err, Some(block::Error::RepeatedParents));
}

#[test]
pub fn block_with_repeated_inputs() {
    let decoded = {
        let mut b = Block::default().with_parents(vec![VertexHash::default()]);
        b.inputs = vec![TxoHash::default(), TxoHash::default()];
        b
    };
    let encoded = &[
        0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 34, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 34, 34, 2, 32, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 50, 25, 49, 57, 55, 48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43,
        48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(block::Error::RepeatedInputs));
    assert_matches!(decode_err, Some(block::Error::RepeatedInputs));
}

#[test]
pub fn block_with_repeated_outputs() {
    let decoded = {
        let mut b = Block::default().with_parents(vec![VertexHash::default()]);
        b.outputs = vec![Txo::default(), Txo::default()];
        b
    };
    let encoded = &[
        0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 42, 0, 42, 0, 50, 25, 49, 57, 55, 48,
        45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, Some(block::Error::RepeatedOutputs));
    assert_matches!(decode_err, Some(block::Error::RepeatedOutputs));
}

#[test]
pub fn get_block_request() {
    let decoded = Request::GetBlock(BlockHash::default());
    let encoded = &[
        2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn get_vertex_request() {
    let decoded = Request::GetVertex(BlockHash::default());
    let encoded = &[
        10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn get_preference_request() {
    let decoded = Request::GetPreference(BlockHash::default());
    let encoded = &[
        18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn block_response() {
    let decoded = Response::Block(Block::default().with_parents(vec![VertexHash::default()]));
    let encoded = &[
        2, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55, 48, 45, 48,
        49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
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
        10, 76, 0, 1, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55,
        48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
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
        10, 74, 0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
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
        18, 36, 2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);

    // Positive preference
    let decoded = Response::Preference(VertexHash::default(), true);
    let encoded = &[
        18, 38, 2, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 1,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
fn hash_corner_cases() {
    pub struct HashTestCase<'a> {
        pub decoded: Hash,
        pub long_hex: &'a str,
        pub short_hex: &'a str,
    }
    for test in [
        HashTestCase {
            decoded: Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0,
            ]),
            long_hex: "0000000000000000000000000000000000000000000000000000000000000000",
            short_hex: "00000000..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
            ]),
            long_hex: "8000000000000000000000000000000000000000000000000000000000000000",
            short_hex: "80000000..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 1,
            ]),
            long_hex: "0000000000000000000000000000000000000000000000000000000000000001",
            short_hex: "00000000..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            long_hex: "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            short_hex: "ffffffff..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                127, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
            ]),
            long_hex: "7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
            short_hex: "7fffffff..",
        },
        HashTestCase {
            decoded: Hash::with_bytes([
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
                255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254,
            ]),
            long_hex: "fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
            short_hex: "ffffffff..",
        },
    ] {
        assert_eq!(test.decoded.to_hex(), test.long_hex);
        assert_eq!(test.decoded.to_short_hex(), test.short_hex);
    }
}
