use cordelia::{p2p::BroadcastData, wire::test_wire_format, Block, BlockHash, Vertex, VertexHash};
use std::{assert_matches::assert_matches, sync::Arc};

#[test]
pub fn broadcast_slim_vertex() {
    let decoded = BroadcastData::Vertex(Vertex::new_slim(
        BlockHash::default(),
        vec![VertexHash::default()],
    ));
    let encoded = &[
        2, 74, 0, 1, 10, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 18, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn broadcast_full_vertex() {
    let decoded = BroadcastData::Vertex(Vertex::new_full(Arc::new(
        Block::default().with_parents(vec![VertexHash::default()]),
    )));
    let encoded = &[
        2, 76, 0, 1, 26, 72, 0, 1, 8, 232, 7, 18, 2, 0, 0, 26, 34, 2, 32, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 50, 25, 49, 57, 55,
        48, 45, 48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}
