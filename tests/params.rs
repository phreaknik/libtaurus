use taurus::params::*;

#[test]
fn valid_quorum_size() {
    assert!(AVALANCHE_QUORUM <= AVALANCHE_QUERY_COUNT);
}

#[test]
fn valid_acceptance_threshold() {
    assert!(AVALANCHE_ACCEPTANCE_THRESHOLD > 0);
}

#[test]
fn valid_registration_window() {
    assert!(VOTER_REGISTRATION_WINDOW_HRS <= VOTER_EXPERATION_HRS);
}
