use chrono::{Duration, Utc};
use cordelia::{
    consensus::block,
    params::{FUTURE_BLOCK_LIMIT_SECS, MIN_DIFFICULTY},
    randomx::RandomXVMInstance,
    util::Randomizer,
    Block, Txo, TxoHash, VertexHash,
};
use libp2p::PeerId;
use num::{BigUint, FromPrimitive};
use randomx_rs::RandomXFlag;
use std::assert_matches::assert_matches;

#[test]
fn mining_target() {
    let mut block = Block::random();
    block.difficulty = MIN_DIFFICULTY - 1;
    assert_matches!(block.mining_target(), Err(block::Error::InvalidDifficulty));
    block.difficulty = MIN_DIFFICULTY + rand::random::<u64>();
    assert_eq!(
        block.mining_target().unwrap(),
        BigUint::from_u64(2).unwrap().pow(256) / BigUint::from_u64(block.difficulty).unwrap()
    );
}

#[test]
fn verify_pow() {
    let randomx_vm = RandomXVMInstance::new(
        b"cordelia-randomx-test",
        RandomXFlag::get_recommended_flags(),
    )
    .unwrap();
    let mut block = Block::default();
    block.nonce = 469;
    assert!(block.verify_pow(&randomx_vm).is_ok());
    block.nonce += 1;
    assert!(block.verify_pow(&randomx_vm).is_err());
}

#[test]
fn with_timestamp() {
    let new_time = Utc::now();
    assert_eq!(new_time, Block::random().with_timestamp(new_time).time);
}

#[test]
fn with_miner() {
    let new_miner = PeerId::random();
    assert_eq!(new_miner, Block::random().with_miner(new_miner).miner);
}

#[test]
fn with_parents() {
    let new_parents = vec![
        VertexHash::random(),
        VertexHash::random(),
        VertexHash::random(),
    ];
    assert_eq!(
        new_parents,
        Block::random().with_parents(new_parents.clone()).parents
    );
}

#[test]
fn sanity_checks() {
    let mut block = Block {
        version: block::VERSION + 1,
        difficulty: MIN_DIFFICULTY - 1,
        time: Utc::now() + Duration::seconds(2 * FUTURE_BLOCK_LIMIT_SECS),
        parents: Vec::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        miner: PeerId::random(),
        nonce: rand::random(),
    };
    assert_matches!(block.sanity_checks(), Err(block::Error::BadVersion(1)));
    block.version = block::VERSION;
    assert_matches!(block.sanity_checks(), Err(block::Error::InvalidDifficulty));
    block.difficulty = MIN_DIFFICULTY;
    assert_matches!(block.sanity_checks(), Err(block::Error::FutureTime));
    block.time = Utc::now();
    assert_matches!(block.sanity_checks(), Err(block::Error::MissingParents));
    let parent = VertexHash::random();
    block.parents = vec![parent, parent];
    assert_matches!(block.sanity_checks(), Err(block::Error::RepeatedParents));
    block.parents = vec![VertexHash::random(), VertexHash::random()];
    let input = TxoHash::random();
    block.inputs = vec![input, input];
    assert_matches!(block.sanity_checks(), Err(block::Error::RepeatedInputs));
    block.inputs = vec![TxoHash::random(), TxoHash::random()];
    let output = Txo::random();
    block.outputs = vec![output, output];
    assert_matches!(block.sanity_checks(), Err(block::Error::RepeatedOutputs));
    block.outputs = vec![Txo::random(), Txo::random()];
    assert_matches!(block.sanity_checks(), Ok(_));
}
