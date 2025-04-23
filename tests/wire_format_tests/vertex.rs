use cordelia::{
    consensus::vertex, hash::Hash, wire::test_wire_format, Block, BlockHash, Vertex, VertexHash,
};
use std::{assert_matches::assert_matches, sync::Arc};

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
