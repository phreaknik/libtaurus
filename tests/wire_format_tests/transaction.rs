use crate::wire_format_tests::test_wire_format;
use cordelia::Transaction;
use std::assert_matches::assert_matches;

#[test]
pub fn basic_transaction() {
    let decoded = Transaction::default();
    let encoded = &[];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}

#[test]
pub fn basic_txo() {
    let decoded = Transaction::default();
    let encoded = &[];
    let (encode_err, decode_err) = test_wire_format(decoded, encoded);
    assert_matches!(encode_err, None);
    assert_matches!(decode_err, None);
}
