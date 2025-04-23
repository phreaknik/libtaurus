use chrono::Utc;
use cordelia::{params::MIN_DIFFICULTY, Block, GenesisConfig, Vertex};
use libp2p::{multihash::Multihash, PeerId};
use std::sync::Arc;

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
