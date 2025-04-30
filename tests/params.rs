use libtaurus::params::*;

#[test]
fn valid_quorum_size() {
    assert!(AVALANCHE_QUORUM <= AVALANCHE_QUERY_COUNT);
}

#[test]
fn valid_confidence_threshold() {
    assert!(AVALANCHE_CONFIDENCE_THRESHOLD > 0);
}

#[test]
fn valid_counter_threshold() {
    assert!(AVALANCHE_COUNTER_THRESHOLD > 0);
}
