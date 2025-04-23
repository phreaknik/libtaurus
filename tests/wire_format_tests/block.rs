use cordelia::{
    consensus::block, params::MIN_DIFFICULTY, wire::test_wire_format, Block, Txo, TxoHash,
    VertexHash,
};
use std::assert_matches::assert_matches;

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
