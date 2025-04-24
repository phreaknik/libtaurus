use cordelia::{p2p::BroadcastData, wire::test_wire_format, Block, BlockHash, Vertex, VertexHash};
use std::{assert_matches::assert_matches, sync::Arc};

#[test]
pub fn broadcast_slim_vertex() {
    let decoded = BroadcastData::Vertex(Vertex::new_slim(
        BlockHash::default(),
        vec![VertexHash::default()],
    ));
    let encoded = &[
        10, 72, 18, 34, 10, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 26, 34, 10, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
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
        10, 72, 34, 70, 16, 232, 7, 26, 2, 0, 0, 34, 34, 10, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 58, 25, 49, 57, 55, 48, 45,
        48, 49, 45, 48, 49, 84, 48, 48, 58, 48, 48, 58, 48, 48, 43, 48, 48, 58, 48, 48,
    ];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}
