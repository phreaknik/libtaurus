use chrono::Utc;
use cordelia::{consensus, params::MIN_DIFFICULTY, Block, GenesisConfig, Vertex};
use libp2p::{multihash::Multihash, PeerId};
use std::{assert_matches::assert_matches, sync::Arc};

#[test]
fn genesis_cfg_to_vertex() {
    let cfg = GenesisConfig {
        difficulty: MIN_DIFFICULTY,
        time: Utc::now(),
    };
    let expected = Arc::new(Vertex::genesis(Block {
        version: 1,
        difficulty: cfg.difficulty,
        miner: PeerId::from_multihash(Multihash::default()).unwrap(),
        parents: Vec::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        time: cfg.time,
        nonce: 0,
    }));
    assert_eq!(expected, cfg.to_vertex());
}

#[test]
#[should_panic]
fn invalid_difficulty() {
    GenesisConfig {
        difficulty: MIN_DIFFICULTY - 1,
        time: Utc::now(),
    }
    .to_vertex();
}

#[test]
fn sanity_checks() {
    let mut cfg = GenesisConfig {
        difficulty: MIN_DIFFICULTY - 1,
        time: Utc::now(),
    };
    assert_matches!(cfg.sanity_checks(), Err(consensus::Error::BadDifficulty(_)));
    cfg.difficulty = MIN_DIFFICULTY;
    assert!(cfg.sanity_checks().is_ok());
}
